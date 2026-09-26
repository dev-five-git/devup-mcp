// 플러그인 UI(iframe) — 네트워크를 쓸 수 있는 유일한 쪽.
//
// devup-mcp 가 연 로컬 WebSocket 에 붙어, 들어온 작업을 메인 스레드로 넘기고
// 결과를 돌려보낸다. 문서는 만지지 않는다(만질 수도 없다).
//
// 연결은 끊어지는 것이 정상이다 — devup-mcp 는 MCP 클라이언트가 뜰 때마다 새로
// 시작한다. 그래서 실패를 예외가 아니라 상태로 다루고 계속 재시도한다.

interface NodeRef {
  id: string
  name: string
  type?: string
}

/** 보고 있는 페이지와 선택. 메인 스레드가 바뀔 때마다 보낸다. */
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

interface ResultMessage {
  kind: 'devup-result'
  requestId: string
  data?: unknown
  error?: string
}

const RETRY_MS = 2000

/**
 * 이 플러그인 창 하나를 가리키는 이름. 창이 열려 있는 동안 바뀌지 않는다.
 *
 * devup-mcp 는 파일 키를 보고하지 못하는 플러그인(Dev Mode)을 이 이름으로 부른다.
 * 소켓이 끊겨 다시 붙어도 — 포트를 쥔 devup-mcp 가 바뀌었어도 — 같은 창이면 같은
 * 이름이라, 수집 도중에 연결이 바뀌어도 남은 읽기가 이 창을 다시 찾는다.
 */
const SESSION_ID = (() => {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
})()

let socket: WebSocket | null = null
// `status` 는 window 전역(문자열)과 겹친다.
let bridgeStatus: StatusMessage | null = null
let served = 0

const dot = document.getElementById('dot') as HTMLElement
const line = document.getElementById('line') as HTMLElement
const detail = document.getElementById('detail') as HTMLElement

function render(state: 'on' | 'off', text: string) {
  dot.className = `dot ${state}`
  line.textContent = text
  detail.textContent = bridgeStatus
    ? `${bridgeStatus.fileName} · 처리 ${served}건`
    : '파일 정보를 읽는 중…'
}

function connect() {
  if (!bridgeStatus) return
  // 반드시 `localhost` 여야 한다. Figma 는 manifest 의 allowedDomains 에 적힌
  // 주소로만 나가게 하는데, `ws://127.0.0.1:...` 은 "유효한 URL 이 아니다"라며
  // 매니페스트 자체를 거부한다 — 플러그인이 아예 실행되지 않는다.
  const url = `ws://localhost:${bridgeStatus.port}/plugin`
  let ws: WebSocket
  try {
    ws = new WebSocket(url)
  } catch {
    render('off', 'devup-mcp 를 찾지 못했습니다')
    setTimeout(connect, RETRY_MS)
    return
  }
  socket = ws

  ws.onopen = () => {
    render('on', 'devup-mcp 에 연결됨')
    // 어느 파일인지 먼저 알려야 devup-mcp 가 요청을 이 소켓으로 보낼 수 있다.
    // 페이지와 선택도 함께 보낸다 — url 없는 요청은 그것으로 대상을 고른다.
    ws.send(
      JSON.stringify({
        kind: 'hello',
        sessionId: SESSION_ID,
        fileKey: bridgeStatus?.fileKey ?? null,
        fileName: bridgeStatus?.fileName ?? '',
        currentPage: bridgeStatus?.currentPage ?? null,
        selection: bridgeStatus?.selection ?? [],
        selectionCount: bridgeStatus?.selectionCount ?? 0,
      }),
    )
  }

  ws.onmessage = (event) => {
    let job: unknown
    try {
      job = JSON.parse(String(event.data))
    } catch {
      return
    }
    // 작업은 그대로 메인 스레드로 넘긴다. UI 는 내용을 해석하지 않는다.
    parent.postMessage({ pluginMessage: job }, '*')
  }

  ws.onclose = () => {
    socket = null
    render('off', 'devup-mcp 연결 대기 중…')
    setTimeout(connect, RETRY_MS)
  }

  // onclose 가 뒤따르므로 여기서 재시도를 걸면 두 번 걸린다.
  ws.onerror = () => {}
}

window.onmessage = (event: MessageEvent) => {
  const msg = event.data?.pluginMessage
  if (!msg || typeof msg !== 'object') return

  if (msg.kind === 'devup-status') {
    bridgeStatus = msg as StatusMessage
    render('off', 'devup-mcp 연결 대기 중…')
    connect()
    return
  }

  if (msg.kind === 'devup-context') {
    const context = msg as ContextMessage
    // 상태를 받기 전이면 버린다 — 상태가 그때의 값을 싣고 온다. 연결 전이면
    // 보관만 해서 hello 가 싣고 가게 한다.
    if (!bridgeStatus) return
    bridgeStatus = {
      ...bridgeStatus,
      currentPage: context.currentPage,
      selection: context.selection,
      selectionCount: context.selectionCount,
    }
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(
        JSON.stringify({
          kind: 'context',
          currentPage: context.currentPage,
          selection: context.selection,
          selectionCount: context.selectionCount,
        }),
      )
    }
    return
  }

  if (msg.kind === 'devup-result') {
    const result = msg as ResultMessage
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify(result))
      served += 1
      render('on', 'devup-mcp 에 연결됨')
    }
    // 소켓이 끊긴 사이의 결과는 버린다. devup-mcp 가 그 요청을 이미 타임아웃
    // 처리했고, 재연결 후 다시 보낼 것이다.
  }
}

/**
 * 창이 뒤로 가도 이 iframe 이 잠들지 않게 붙잡는다.
 *
 * Chromium 은 보이지 않는 페이지의 작업 큐를 분당 한 번 수준으로 묶는다. 그래서
 * 사용자가 다른 창을 누르는 순간 읽기 하나가 60초씩 걸렸고, 화면 하나를 받는 데
 * 필요한 수십 번의 왕복이 사실상 멈췄다. 실측한 값이다 — 앞의 8번은 206ms 에
 * 끝나고 9번째가 59초였다.
 *
 * 소리를 내고 있는 페이지는 그 대상에서 빠진다. 그래서 들리지 않는 소리를 낸다:
 * 게인을 0 으로 둔 오실레이터라 스피커로는 아무것도 나가지 않지만, 브라우저에는
 * 재생 중인 페이지로 보인다.
 *
 * 실패해도 브리지는 그대로 동작한다. 느려질 뿐이므로 조용히 넘어간다.
 */
function keepAwake() {
  try {
    const Ctor =
      window.AudioContext ??
      (window as unknown as { webkitAudioContext?: typeof AudioContext })
        .webkitAudioContext
    if (!Ctor) return
    const ctx = new Ctor()
    const gain = ctx.createGain()
    gain.gain.value = 0
    const osc = ctx.createOscillator()
    osc.connect(gain)
    gain.connect(ctx.destination)
    osc.start()
    // 사용자 제스처 없이 시작하면 suspended 로 태어난다. 플러그인 창을 연 것
    // 자체가 제스처로 잡히는 경우가 많아 대개 여기서 풀린다.
    if (ctx.state === 'suspended') void ctx.resume()
  } catch {
    // 오디오를 못 쓰는 환경. 포그라운드에서는 여전히 제 속도가 난다.
  }
}

keepAwake()

// 메인 스레드에 파일 정보를 요청하는 것으로 시작한다.
parent.postMessage({ pluginMessage: { kind: 'devup-ready' } }, '*')
