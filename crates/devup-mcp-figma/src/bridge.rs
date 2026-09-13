//! 열려 있는 Figma 파일을 우리 플러그인으로 직접 읽는 전송 계층.
//!
//! 공식 MCP 경로(`RemoteFigmaClient`)는 읽기 스크립트를 `use_figma` 로 보내는데,
//! 그 도구가 요금 한도를 쓴다. 스크립트가 건드리는 것은 문서화된 Plugin API 뿐이라
//! 우리가 돌리는 플러그인이 같은 일을 할 수 있고, 그 경로에는 한도가 없다.
//!
//! 방향이 거꾸로인 점이 설계를 결정한다. Figma 플러그인은 들어오는 연결을 받지
//! 못하고 나가는 WebSocket 만 열 수 있어서, **서버는 이쪽**이고 플러그인이 붙는다.
//!
//! 응답은 원격이 주는 모양 그대로 감싼다. 디코더들은 `content[].text` 안의 JSON
//! 을 찾도록 쓰여 있으므로, 여기서 모양을 바꾸면 두 경로가 조용히 갈라진다.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::any,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc, oneshot},
    time::timeout,
};

use crate::{
    errors::{DevupError, ErrorCode},
    upstream::{FigmaUpstream, ReadToolCall, UpstreamResult},
};

/// 플러그인 manifest 의 `allowedDomains` 와 같은 값이어야 한다. 바꾸려면 양쪽을
/// 함께 고쳐야 하며, 플러그인은 재빌드·재설치가 필요하다.
pub const DEFAULT_BRIDGE_PORT: u16 = 1993;

/// 한 번의 읽기에 허용하는 시간. 스크립트는 문서를 한 번 훑는 정도라 원격보다
/// 훨씬 짧아도 되지만, 아주 큰 페이지의 첫 스냅샷은 몇 초가 걸린다.
const JOB_TIMEOUT: Duration = Duration::from_secs(90);

/// 플러그인이 붙을 때 보내는 첫 메시지.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hello {
    #[serde(default)]
    file_key: Option<String>,
}

/// 플러그인이 작업을 마치고 보내는 메시지.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginResult {
    request_id: String,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

/// 서버가 플러그인에 보내는 작업.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    kind: &'static str,
    request_id: String,
    script: &'static str,
    params: Value,
}

/// 브리지가 맡을 수 있는 읽기 하나. `ReadToolCall::bridge_job` 이 만든다.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeJob {
    /// 플러그인이 아는 이름. 코드젠이 파일명에서 만든 것과 같아야 한다.
    pub script: &'static str,
    /// 스크립트가 읽는 값. Rust 의 치환 자리와 1:1 이다.
    pub params: Value,
}

struct Connected {
    outbox: mpsc::UnboundedSender<String>,
}

#[derive(Default)]
struct Inner {
    /// fileKey → 그 파일을 열어 둔 플러그인. 같은 파일을 두 창에서 열면 나중에
    /// 붙은 쪽이 이긴다. 둘 다 같은 문서를 보므로 어느 쪽이든 답은 같다.
    plugins: HashMap<String, Connected>,
    /// requestId → 결과를 기다리는 쪽.
    pending: HashMap<String, oneshot::Sender<PluginResult>>,
}

/// 플러그인 연결과 대기 중인 요청을 함께 들고 있는 공유 상태.
#[derive(Clone, Default)]
pub struct BridgeState {
    inner: Arc<Mutex<Inner>>,
    counter: Arc<AtomicU64>,
}

fn unavailable(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupFigmaDirectUnavailable, message, false)
}

impl BridgeState {
    fn next_request_id(&self) -> String {
        format!("job-{}", self.counter.fetch_add(1, Ordering::Relaxed))
    }

    /// 이 파일을 열어 둔 플러그인이 있는지.
    pub async fn has_plugin(&self, file_key: &str) -> bool {
        self.inner.lock().await.plugins.contains_key(file_key)
    }

    /// 붙어 있는 파일 키 목록. 진단용.
    pub async fn connected_files(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.inner.lock().await.plugins.keys().cloned().collect();
        keys.sort();
        keys
    }

    async fn dispatch(
        &self,
        file_key: &str,
        script: &'static str,
        params: Value,
    ) -> Result<Value, DevupError> {
        let request_id = self.next_request_id();
        let (tx, rx) = oneshot::channel();

        {
            let mut inner = self.inner.lock().await;
            let Some(plugin) = inner.plugins.get(file_key) else {
                return Err(unavailable(format!(
                    "no Devup Bridge plugin is open for file {file_key}"
                )));
            };
            let job = Job {
                kind: "devup-job",
                request_id: request_id.clone(),
                script,
                params,
            };
            let encoded = serde_json::to_string(&job)
                .map_err(|error| unavailable(format!("bridge job encode failed: {error}")))?;
            if plugin.outbox.send(encoded).is_err() {
                // 소켓이 막 닫혔다. 등록을 지워 다음 호출이 곧장 폴백하도록 한다.
                inner.plugins.remove(file_key);
                return Err(unavailable(format!(
                    "the Devup Bridge plugin for file {file_key} disconnected"
                )));
            }
            inner.pending.insert(request_id.clone(), tx);
        }

        let received = timeout(JOB_TIMEOUT, rx).await;
        // 성공이든 실패든 대기표는 반드시 걷는다. 남겨 두면 연결이 오래 살아 있는
        // 동안 계속 쌓인다.
        self.inner.lock().await.pending.remove(&request_id);

        match received {
            Ok(Ok(result)) => match (result.data, result.error) {
                // 스크립트가 던진 DEVUP_* 코드는 그대로 올린다. 원격 경로와 같은
                // 문자열이어야 위쪽 분기가 동일하게 동작한다.
                (_, Some(message)) => Err(DevupError::new(
                    ErrorCode::DevupFigmaDirectUnavailable,
                    message,
                    false,
                )),
                (Some(data), None) => Ok(data),
                (None, None) => Err(unavailable("bridge returned neither data nor error")),
            },
            // 플러그인 창이 닫혔다.
            Ok(Err(_)) => Err(unavailable("the Devup Bridge plugin disconnected mid-read")),
            Err(_) => Err(unavailable(
                "the Devup Bridge plugin did not answer in time",
            )),
        }
    }
}

/// 플러그인이 돌려준 값을 원격 `use_figma` 가 주는 모양으로 감싼다.
///
/// 디코더들은 `content[].text` 안의 JSON 문자열을 찾도록 쓰여 있다. 값을 그대로
/// 올리면 일부 디코더는 통과하고 일부는 실패해, 두 경로가 화면 단위로 갈라진다.
fn wrap_as_tool_result(data: &Value) -> Result<Value, DevupError> {
    let text = serde_json::to_string(data)
        .map_err(|error| unavailable(format!("bridge result encode failed: {error}")))?;
    Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": false }))
}

async fn plugin_socket(State(state): State<BridgeState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| handle_plugin(state, socket))
}

async fn handle_plugin(state: BridgeState, mut socket: WebSocket) {
    let (outbox, mut outbox_rx) = mpsc::unbounded_channel::<String>();
    let mut registered: Option<String> = None;

    // 보내기와 받기를 한 루프에서 번갈아 본다. 소켓을 쪼개려면 Stream/Sink 트레이트
    // 의존성이 필요한데, 그것을 들이는 것보다 select 가 싸다.
    loop {
        tokio::select! {
            outgoing = outbox_rx.recv() => {
                let Some(text) = outgoing else { break };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                let Message::Text(text) = message else { continue };
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };

                match value.get("kind").and_then(Value::as_str) {
                    Some("hello") => {
                        let file_key = serde_json::from_value::<Hello>(value)
                            .ok()
                            .and_then(|hello| hello.file_key)
                            .filter(|key| !key.is_empty());
                        // fileKey 가 없는 파일(저장 전 초안)은 요청 대상이 될 수 없다.
                        if let Some(key) = file_key {
                            state.inner.lock().await.plugins.insert(
                                key.clone(),
                                Connected { outbox: outbox.clone() },
                            );
                            registered = Some(key);
                        }
                    }
                    Some("devup-result") => {
                        let Ok(result) = serde_json::from_value::<PluginResult>(value) else {
                            continue;
                        };
                        let waiting = state.inner.lock().await.pending.remove(&result.request_id);
                        // 받는 쪽이 이미 타임아웃했으면 버린다.
                        if let Some(waiting) = waiting {
                            let _ = waiting.send(result);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(key) = registered {
        state.inner.lock().await.plugins.remove(&key);
    }
}

/// 브리지 서버. 포트를 잡지 못하면 열지 않으며, 그 경우 호출자는 원격 경로만 쓴다.
pub struct BridgeServer {
    state: BridgeState,
}

impl BridgeServer {
    /// 포트를 잡아 서버를 띄운다.
    ///
    /// 이미 다른 devup-mcp 가 같은 포트를 쓰고 있으면 `None` 을 준다. 한 대의
    /// 기기에서 MCP 클라이언트를 여러 개 띄우는 것은 정상이므로, 이는 오류가
    /// 아니라 "이 프로세스는 브리지를 쓰지 않는다"는 뜻이다.
    ///
    /// 서버가 만들어지는 자리가 동기 함수라 bind 도 동기로 한다 — 듣기 소켓을
    /// 여는 것은 어차피 기다리지 않는 일이고, 기다리는 부분만 따로 띄운다.
    pub fn start(port: u16) -> Option<Self> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", port)).ok()?;
        listener.set_nonblocking(true).ok()?;
        // 런타임 밖에서 만들어졌다면 붙일 곳이 없다. 그때도 원격 경로는 멀쩡하다.
        let runtime = tokio::runtime::Handle::try_current().ok()?;

        let state = BridgeState::default();
        let app = Router::new()
            .route("/plugin", any(plugin_socket))
            .with_state(state.clone());
        runtime.spawn(async move {
            let Ok(listener) = TcpListener::from_std(listener) else {
                return;
            };
            let _ = axum::serve(listener, app).await;
        });
        Some(Self { state })
    }

    /// 환경 설정을 읽어 띄운다.
    ///
    /// `DEVUP_FIGMA_BRIDGE_PORT` 가 포트를 정하며, `0` 이나 `off` 는 끈다. 값이
    /// 없으면 기본 포트로 켠다 — 플러그인이 안 떠 있으면 어차피 원격으로 가므로
    /// 켜 두는 쪽이 손해가 없다.
    pub fn from_env() -> Option<Self> {
        let setting = std::env::var("DEVUP_FIGMA_BRIDGE_PORT").ok();
        let port = match setting.as_deref().map(str::trim) {
            None | Some("") => DEFAULT_BRIDGE_PORT,
            Some("off") | Some("0") => return None,
            Some(value) => value.parse().ok()?,
        };
        Self::start(port)
    }

    pub fn state(&self) -> BridgeState {
        self.state.clone()
    }
}

/// 이 요청을 맡을 수 있는지 **부르기 전에** 답하는 상류.
///
/// 실패한 뒤에 폴백하면 안 되기 때문에 따로 둔다. `CapabilityUnavailable` 과
/// `Transport` 는 같은 `ErrorCode` 로 접히므로, 에러만 보고는 "브리지가 못 하는
/// 일"과 "브리지가 하다가 실패한 일"을 구분할 수 없다. 뒤엣것까지 원격으로 넘기면
/// 아끼려던 요금을 그대로 쓰게 된다.
#[async_trait]
pub trait PreferredUpstream: FigmaUpstream {
    async fn can_serve(&self, call: &ReadToolCall) -> bool;
}

/// 플러그인을 통해 Figma 를 읽는 `FigmaUpstream`.
#[derive(Clone)]
pub struct BridgeFigmaClient {
    state: BridgeState,
}

impl BridgeFigmaClient {
    pub fn new(state: BridgeState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl FigmaUpstream for BridgeFigmaClient {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        // 스크립트 경로만 대신하므로 광고하는 것도 그 하나다.
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        let Some(job) = call.bridge_job() else {
            // 공식 도구 이름으로 가는 읽기(get_metadata 등)는 응답 모양이 달라
            // 여기서 흉내 내지 않는다.
            return Err(unavailable("the bridge serves script reads only"));
        };
        let data = self
            .state
            .dispatch(call.file_key(), job.script, job.params)
            .await?;
        Ok(UpstreamResult {
            raw: wrap_as_tool_result(&data)?,
        })
    }
}

#[async_trait]
impl PreferredUpstream for BridgeFigmaClient {
    async fn can_serve(&self, call: &ReadToolCall) -> bool {
        call.bridge_job().is_some() && self.state.has_plugin(call.file_key()).await
    }
}

/// 브리지가 맡을 수 있으면 브리지로, 아니면 원격으로 보낸다.
///
/// 판정은 부르기 전에 끝난다. 브리지가 맡은 요청이 실패하면 그 실패를 그대로
/// 올린다 — 노드가 없거나 봉투가 너무 큰 것은 원격에서도 같은 결과이고, 다시
/// 물으면 아낀 요금을 도로 쓰게 된다.
pub struct FallbackUpstream<P, S> {
    preferred: P,
    secondary: S,
}

impl<P, S> FallbackUpstream<P, S> {
    pub fn new(preferred: P, secondary: S) -> Self {
        Self {
            preferred,
            secondary,
        }
    }
}

#[async_trait]
impl<P, S> FigmaUpstream for FallbackUpstream<P, S>
where
    P: PreferredUpstream,
    S: FigmaUpstream,
{
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        // 능력 목록은 원격이 정본이다. 브리지는 그중 하나를 대신할 뿐이다.
        self.secondary.list_tools().await
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        if self.preferred.can_serve(&call).await {
            return self.preferred.call_read_tool(call).await;
        }
        self.secondary.call_read_tool(call).await
    }
}
