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

/**
 * UI → 메인. devup-mcp 가 더는 기다리지 않는 작업 — 요청한 쪽이 떠났거나 시간이
 * 다 됐다. 이 메시지를 모르는 예전 빌드는 그냥 무시한다.
 */
interface CancelMessage {
  kind: 'devup-cancel'
  requestId: string
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

/** UI → 메인. `devup-decode` 의 답 — 디코딩하지 못했으면 png 가 null 이다. */
interface DecodedMessage {
  kind: 'devup-decoded'
  id: string
  png: Uint8Array | null
}

const PORT = 1993

/**
 * 사진을 브라우저로 디코딩해 PNG 로 받는 자리. 참조 PNG 스크립트가 devup-mcp 가
 * 디코딩하지 않을 사진(PNG·JPEG·GIF 가 아니거나, 너무 크거나, 대부분 메타데이터인 것)을
 * 넘긴다. 디코더는 브라우저가 있는 UI 에만 있어 거기에 맡긴다. 답이 없으면 null 이고,
 * 스크립트는 원본을 그대로 보낸다. 기다리는 시간(`timeoutMs`)은 스크립트가 정한다 —
 * 장면 하나의 사진들이 한 몫을 나눠 쓴다. 사진의 크기(`width`·`height`)는 Figma 가 아는
 * 값이고, UI 는 디코딩한 크기가 그와 다르면 버린다.
 */
const decoding = new Map<string, (png: Uint8Array | null) => void>()
let decodes = 0
;(
  globalThis as unknown as {
    __devupNormalizeImage: (
      bytes: Uint8Array,
      maxEdge: number,
      timeoutMs: number,
      width: number,
      height: number,
    ) => Promise<Uint8Array | null>
  }
).__devupNormalizeImage = (bytes, maxEdge, timeoutMs, width, height) =>
  new Promise((resolve) => {
    decodes += 1
    const id = `decode-${decodes}`
    const timer = setTimeout(() => {
      decoding.delete(id)
      resolve(null)
    }, timeoutMs)
    decoding.set(id, (png) => {
      clearTimeout(timer)
      decoding.delete(id)
      resolve(png)
    })
    figma.ui.postMessage({ kind: 'devup-decode', id, bytes, maxEdge, width, height })
  })

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

/** 지금 돌고 있는 작업, 그리고 돌던 중에 거둬진 작업. */
let running: string | null = null
const withdrawn = new Set<string>()

async function drain() {
  if (draining) return
  draining = true
  try {
    while (queue.length > 0) {
      // biome-ignore lint/style/noNonNullAssertion: length 를 확인하고 꺼낸다
      const job = queue.shift()!
      running = job.requestId
      const result = await runJob(job).catch(
        (error: unknown) =>
          ({
            kind: 'devup-result',
            requestId: job.requestId,
            error: error instanceof Error ? error.message : String(error),
          }) satisfies ResultMessage,
      )
      running = null
      // 기다리는 쪽이 없는 답은 보내지 않는다. 큰 스냅샷이면 그만큼 소켓이 빈다.
      if (withdrawn.delete(job.requestId)) continue
      figma.ui.postMessage(result)
    }
  } finally {
    draining = false
  }
}

/**
 * 거둬진 작업을 뺀다. 아직 차례를 기다리던 것이면 돌리지 않는다. 이미 돌고 있는
 * 것은 스크립트를 멈출 수 없으니 답만 보내지 않는다.
 */
function withdraw(requestId: string) {
  const waiting = queue.findIndex((job) => job.requestId === requestId)
  if (waiting >= 0) {
    queue.splice(waiting, 1)
    return
  }
  if (running === requestId) withdrawn.add(requestId)
}

/**
 * 문서를 미리 불러 둔다. Figma 는 파일이 화면에서 1~3분 사라지면 아직 불러오지 않은
 * 데이터를 읽는 플러그인에 10초 뒤 "Unable to establish connection" 으로 답하지만,
 * 이미 불러온 것은 계속 답한다(페이지를 불러 둔 뒤에는 같은 실패가 재현되지 않았다).
 * 그래서 보일 때 — 시작할 때와 다시 보이게 될 때 — 모든 페이지와 로컬 변수·스타일을
 * 불러 둔다. 실패해도 읽기는 그때그때 불러온다.
 */
let preloading: Promise<void> | null = null
function preload() {
  preloading ??= (async () => {
    try {
      await figma.loadAllPagesAsync()
      await Promise.all([
        figma.variables.getLocalVariableCollectionsAsync(),
        figma.variables.getLocalVariablesAsync(),
        figma.getLocalPaintStylesAsync(),
        figma.getLocalTextStylesAsync(),
        figma.getLocalEffectStylesAsync(),
        figma.getLocalGridStylesAsync(),
      ])
    } catch {
      // 불러 두지 못한 것은 읽을 때 불러온다.
    } finally {
      preloading = null
    }
  })()
}

figma.showUI(__html__, { width: 320, height: 220 })
preload()

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
    return
  }

  if (msg.kind === 'devup-cancel') {
    withdraw((message as CancelMessage).requestId)
    return
  }

  if (msg.kind === 'devup-decoded') {
    const decoded = message as DecodedMessage
    decoding.get(decoded.id)?.(decoded.png ?? null)
    return
  }

  // 다시 보이게 됐다. 가려진 사이에 바뀐 것까지 불러 둔다.
  if (msg.kind === 'devup-in-view') {
    preload()
  }
}
