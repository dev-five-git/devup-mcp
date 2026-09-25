// 플러그인 메인 스레드 — Figma Plugin API 를 쓸 수 있는 유일한 쪽.
//
// 하는 일은 하나다. UI(iframe)가 WebSocket 으로 받아 넘겨준 작업을 이름으로 찾아
// 실행하고 결과를 돌려준다. 네트워크는 UI 만, 문서는 메인만 만질 수 있어서
// 둘을 postMessage 로 잇는 구조가 강제된다.
//
// 읽기 전용이다. SCRIPTS 는 devup-mcp 의 읽기 스크립트에서 생성되며 문서를
// 바꾸는 호출을 포함하지 않는다.

import { SCRIPTS, type ScriptParams } from './generated/scripts'

/** UI → 메인. devup-mcp 가 보낸 작업 그대로다. */
interface JobMessage {
  kind: 'devup-job'
  requestId: string
  script: string
  params: Partial<ScriptParams>
}

/** 메인 → UI. 성공이면 data, 실패면 error 중 하나만 채운다. */
interface ResultMessage {
  kind: 'devup-result'
  requestId: string
  data?: unknown
  error?: string
}

/** 페이지나 선택한 노드 하나. */
interface NodeRef {
  id: string
  name: string
  type?: string
}

/**
 * 지금 보고 있는 페이지와 거기서 선택한 것.
 *
 * devup-mcp 는 url 없는 요청을 이 값으로 해석한다 — 연 파일의, 선택한 노드.
 * 선택이 바뀔 때마다 보내므로 늦게 도착해도 최신이다.
 */
interface Context {
  currentPage: NodeRef
  selection: NodeRef[]
  selectionCount: number
}

interface StatusMessage extends Context {
  kind: 'devup-status'
  fileKey: string | null
  fileName: string
  port: number
}

interface ContextMessage extends Context {
  kind: 'devup-context'
}

const PORT = 1993

/**
 * 선택은 앞의 이만큼만 보낸다. 레이어 수천 개를 한꺼번에 고르면 메시지가
 * 그만큼 커지는데, 요청을 해석하는 데 필요한 것은 "정확히 하나인가"와 그 하나다.
 * 전체 수는 selectionCount 로 따로 간다.
 */
const SELECTION_LIMIT = 20

function context(): Context {
  const page = figma.currentPage
  const selection = page.selection
  return {
    currentPage: { id: page.id, name: page.name },
    selection: selection
      .slice(0, SELECTION_LIMIT)
      .map((node) => ({ id: node.id, name: node.name, type: node.type })),
    selectionCount: selection.length,
  }
}

function postContext() {
  const message: ContextMessage = { kind: 'devup-context', ...context() }
  figma.ui.postMessage(message)
}

/**
 * 스크립트가 읽는 자리를 모두 채운 파라미터.
 *
 * 빠진 값을 `undefined` 로 두면 스크립트가 `undefined.length` 같은 데서 죽어
 * 원인을 찾기 어려워진다. Rust 쪽 `ScriptInputs::default()` 와 같은 빈 값으로
 * 맞춰 둔다.
 */
function fillParams(partial: Partial<ScriptParams>): ScriptParams {
  return {
    nodeId: partial.nodeId ?? '',
    rootIds: partial.rootIds ?? [],
    snapshot: partial.snapshot ?? {},
    search: partial.search ?? {},
    explore: partial.explore ?? {},
    resources: partial.resources ?? { variableIds: [], styles: [] },
    largeValue: partial.largeValue ?? {},
    asset: partial.asset ?? {},
    theme: partial.theme ?? { offset: 0 },
  }
}

async function runJob(job: JobMessage): Promise<ResultMessage> {
  const script = SCRIPTS[job.script]
  if (!script) {
    // devup-mcp 가 이 플러그인보다 새 스크립트를 아는 상태. 어느 쪽이 오래됐는지
    // 알 수 있도록 아는 목록을 함께 돌려준다.
    return {
      kind: 'devup-result',
      requestId: job.requestId,
      error: `DEVUP_BRIDGE_UNKNOWN_SCRIPT: ${job.script} (아는 스크립트: ${Object.keys(SCRIPTS).join(', ')})`,
    }
  }
  try {
    const data = await script(fillParams(job.params))
    return { kind: 'devup-result', requestId: job.requestId, data }
  } catch (error) {
    // 스크립트는 DEVUP_* 코드를 던진다. 그 문자열을 그대로 올려야 Rust 가
    // 분기할 수 있으므로 감싸지 않는다.
    return {
      kind: 'devup-result',
      requestId: job.requestId,
      error: error instanceof Error ? error.message : String(error),
    }
  }
}

/**
 * 들어온 작업을 순서대로 하나씩 처리한다.
 *
 * 여러 건이 한꺼번에 와도 받아 두되 동시에 실행하지는 않는다. 스냅샷 스크립트는
 * `figma.setCurrentPageAsync` 로 현재 페이지를 옮기는데, 두 작업이 겹쳐 돌면
 * 서로의 페이지를 갈아치워 엉뚱한 화면을 읽은 결과가 섞여 나온다. 그 오염은
 * 오류로 드러나지 않고 그럴듯한 노드로 돌아오므로 가장 위험하다.
 *
 * 큐에 쌓아 두는 것만으로도 왕복 지연은 사라진다 — 다음 작업이 이미 도착해 있어
 * 앞 작업이 끝나는 즉시 시작한다.
 */
const queue: JobMessage[] = []
let draining = false

async function drain() {
  if (draining) return
  draining = true
  try {
    while (queue.length > 0) {
      // biome-ignore lint/style/noNonNullAssertion: length 를 확인하고 꺼낸다
      const job = queue.shift()!
      const result = await runJob(job).catch(
        (error: unknown) =>
          ({
            kind: 'devup-result',
            requestId: job.requestId,
            error: error instanceof Error ? error.message : String(error),
          }) satisfies ResultMessage,
      )
      figma.ui.postMessage(result)
    }
  } finally {
    draining = false
  }
}

figma.showUI(__html__, { width: 320, height: 220 })

// 읽기 스크립트도 페이지를 옮기므로 사람이 옮긴 것과 함께 이 이벤트로 온다.
figma.on('selectionchange', postContext)
figma.on('currentpagechange', postContext)

figma.ui.onmessage = (message: unknown) => {
  if (typeof message !== 'object' || message === null) return
  const msg = message as { kind?: string }

  if (msg.kind === 'devup-ready') {
    const status: StatusMessage = {
      kind: 'devup-status',
      fileKey: figma.fileKey ?? null,
      fileName: figma.root.name,
      port: PORT,
      ...context(),
    }
    figma.ui.postMessage(status)
    return
  }

  if (msg.kind === 'devup-job') {
    queue.push(message as JobMessage)
    // onmessage 를 async 로 만들면 Figma 가 반환값을 기다리지 않아 예외가 조용히
    // 사라진다. 큐에 넣고 배수는 따로 돌린다.
    void drain()
  }
}
