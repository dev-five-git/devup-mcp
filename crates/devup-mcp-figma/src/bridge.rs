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
        atomic::{AtomicU64, AtomicUsize, Ordering},
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
    upstream::{BatchBudget, FigmaUpstream, ReadToolCall, UpstreamResult},
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
    /// 붙어 있는 플러그인 수. 배치 크기를 정할 때는 잠금을 기다릴 수 없어
    /// (그 자리가 async 가 아니다) 따로 센다.
    connected: Arc<AtomicUsize>,
}

fn unavailable(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupFigmaDirectUnavailable, message, false)
}

/// 어느 플러그인이 이 파일을 맡을지 고른다.
///
/// 보통은 파일 키가 그대로 맞는다. 다만 `figma.fileKey` 는 늘 있는 값이 아니어서
/// (Dev Mode 에서 비어 오는 것을 실제로 봤다) 키 없이 붙는 플러그인이 생긴다.
/// 그때 등록을 건너뛰면 창은 "연결됨"이라고 하는데 어떤 읽기도 오지 않는 —
/// 원인을 찾기 가장 어려운 — 상태가 된다.
///
/// 그래서 키 없는 플러그인은 **혼자 붙어 있을 때만** 맡는다. 여럿이면 어느 파일을
/// 보고 있는지 알 수 없고, 엉뚱한 파일을 읽어 주는 것보다 원격으로 넘기는 편이 낫다.
fn resolve_key(plugins: &HashMap<String, Connected>, file_key: &str) -> Option<String> {
    if plugins.contains_key(file_key) {
        return Some(file_key.to_owned());
    }
    if plugins.len() == 1
        && let Some(key) = plugins.keys().next()
        && key.is_empty()
    {
        return Some(key.clone());
    }
    None
}

impl BridgeState {
    fn next_request_id(&self) -> String {
        format!("job-{}", self.counter.fetch_add(1, Ordering::Relaxed))
    }

    /// 이 파일을 열어 둔 플러그인이 있는지.
    pub async fn has_plugin(&self, file_key: &str) -> bool {
        resolve_key(&self.inner.lock().await.plugins, file_key).is_some()
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
            let Some(resolved) = resolve_key(&inner.plugins, file_key) else {
                return Err(unavailable(format!(
                    "no Devup Bridge plugin is open for file {file_key}"
                )));
            };
            let file_key = resolved.as_str();
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

/// 한 번에 담을 수 있는 양. 로컬 소켓이라 공식 MCP 의 자름을 따를 이유가 없다.
///
/// 기본값(봉투 19KB · 필드 4KB)은 공식 MCP 가 텍스트 결과를 20,480 바이트에서
/// 자르기 때문에 생긴 것이다. 그 탓에 화면 하나가 30번 넘는 왕복으로 쪼개지고,
/// 창이 뒤로 가 있으면 왕복 하나가 60초까지 걸린다 — 실측한 값이다.
const BRIDGE_ENVELOPE_BYTES: usize = 8 * 1024 * 1024;
const BRIDGE_FIELD_BYTES: usize = 4 * 1024 * 1024;

/// 스냅샷 읽기의 한도를 브리지 전송에 맞게 올린다.
///
/// 값만 바꿀 뿐 무엇을 읽을지는 그대로다. 스크립트는 한 페이지에 다 담기면
/// `complete` 로 표시하고, 큰 필드도 잘리지 않으므로 수집기가 그 필드를 따로
/// 받으러 가지 않는다. 두 왕복이 한 왕복이 되는 것이 아니라 서른이 하나가 된다.
fn widen_budgets(params: &mut Value) {
    let Some(object) = params.as_object_mut() else {
        return;
    };
    let Some(snapshot) = object.get_mut("snapshot").and_then(Value::as_object_mut) else {
        return;
    };
    snapshot.insert("maxEnvelopeBytes".to_owned(), BRIDGE_ENVELOPE_BYTES.into());
    snapshot.insert("maxPayloadBytes".to_owned(), BRIDGE_ENVELOPE_BYTES.into());
    snapshot.insert("maxFieldBytes".to_owned(), BRIDGE_FIELD_BYTES.into());
}

/// 봉투의 `fileKey` 가 비어 있으면 요청한 값으로 채운다.
///
/// 스크립트는 `figma.fileKey` 를 그대로 싣는데, 그 값이 늘 오지는 않는다 —
/// 비어 오는 것을 실기기에서 확인했다. 디코더는 비어 있지 않은 `fileKey` 를 요구하므로
/// 그대로 두면 노드를 다 받고도 "스냅샷을 찾지 못했다"며 버린다.
///
/// 지어내는 값이 아니다. 이 읽기가 어느 파일을 향했는지는 호출자가 알고 있고,
/// 그 파일을 이 플러그인이 맡는다는 판단은 이미 `resolve_key` 가 내렸다.
fn stamp_file_key(data: &mut Value, file_key: &str) {
    let Some(object) = data.as_object_mut() else {
        return;
    };
    let empty = object
        .get("fileKey")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty);
    if empty {
        object.insert("fileKey".to_owned(), Value::String(file_key.to_owned()));
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
                        // 키가 없어도 등록한다. 건너뛰면 창은 "연결됨"이라고 하는데
                        // 어떤 읽기도 오지 않아 원인을 찾을 수 없다. 키 없는 연결을
                        // 어디까지 믿을지는 resolve_key 가 정한다.
                        let key = serde_json::from_value::<Hello>(value)
                            .ok()
                            .and_then(|hello| hello.file_key)
                            .unwrap_or_default();
                        state.inner.lock().await.plugins.insert(
                            key.clone(),
                            Connected { outbox: outbox.clone() },
                        );
                        state.connected.store(
                            state.inner.lock().await.plugins.len(),
                            Ordering::Relaxed,
                        );
                        registered = Some(key);
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
    state
        .connected
        .store(state.inner.lock().await.plugins.len(), Ordering::Relaxed);
}

/// 브리지 서버. 포트를 잡지 못하면 열지 않으며, 그 경우 호출자는 원격 경로만 쓴다.
pub struct BridgeServer {
    state: BridgeState,
    port: u16,
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
        // 0 을 주면 커널이 빈 포트를 고른다. 실제로 잡힌 번호를 알아야 붙을 수 있다.
        let bound = listener.local_addr().ok()?.port();
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
        Some(Self { state, port: bound })
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

    /// 실제로 잡은 포트. `start(0)` 으로 띄웠을 때 어디에 붙어야 하는지 알려 준다.
    pub fn port(&self) -> u16 {
        self.port
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

    /// 지금 이 상류가 쓸 수 있는 상태인지. 배치 크기를 정하는 자리는 async 가
    /// 아니어서 `can_serve` 를 부를 수 없다.
    fn is_live(&self) -> bool;
}

/// 브리지 경로의 실측 상태. `devup_figma_auth doctor` 의 `paths.bridge` 가 된다.
///
/// 이 값이 있다는 것 자체가 "이 프로세스가 브리지를 열었다"는 뜻이다. 포트를
/// 잡지 못했거나 `DEVUP_FIGMA_BRIDGE_PORT=off` 면 브리지 상류가 아예 만들어지지
/// 않으므로 `None` 이 된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgePathSnapshot {
    /// 실제로 잡은 포트. 플러그인 manifest 의 `allowedDomains` 와 같아야 붙는다.
    pub port: Option<u16>,
    /// 지금 붙어 있는 플러그인이 열어 둔 파일 키. 빈 문자열은 자기 파일 키를
    /// 보고하지 못한 플러그인이며, 혼자 붙어 있을 때만 읽기를 받는다.
    pub attached_files: Vec<String>,
}

/// 플러그인을 통해 Figma 를 읽는 `FigmaUpstream`.
#[derive(Clone)]
pub struct BridgeFigmaClient {
    state: BridgeState,
    /// 진단에만 쓴다. 읽기 경로는 포트를 알 필요가 없다.
    port: Option<u16>,
}

impl BridgeFigmaClient {
    pub fn new(state: BridgeState) -> Self {
        Self { state, port: None }
    }

    /// 잡은 포트를 함께 들고 있게 한다. "문이 어디에 열려 있는가"는 붙지 않는
    /// 플러그인을 진단할 때 가장 먼저 확인할 값이다.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
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
            return Err(unavailable("the bridge serves script reads only"));
        };
        let mut params = job.params;
        widen_budgets(&mut params);
        let mut data = self
            .state
            .dispatch(call.file_key(), job.script, params)
            .await?;
        stamp_file_key(&mut data, call.file_key());
        Ok(UpstreamResult {
            raw: wrap_as_tool_result(&data)?,
        })
    }

    fn batch_budget(&self) -> BatchBudget {
        bridge_batch_budget()
    }

    async fn serves_without_credentials(&self, file_key: &str) -> bool {
        self.state.has_plugin(file_key).await
    }

    async fn bridge_path_snapshot(&self) -> Option<BridgePathSnapshot> {
        Some(BridgePathSnapshot {
            port: self.port,
            attached_files: self.state.connected_files().await,
        })
    }
}

#[async_trait]
impl PreferredUpstream for BridgeFigmaClient {
    async fn can_serve(&self, call: &ReadToolCall) -> bool {
        call.bridge_job().is_some() && self.state.has_plugin(call.file_key()).await
    }

    fn is_live(&self) -> bool {
        self.state.connected.load(Ordering::Relaxed) > 0
    }
}

/// 자르지 않는 전송이므로 쪼갤 이유가 없다. 변수와 스타일을 한 번에 다 묻는다.
const BRIDGE_BATCH: BatchBudget = BatchBudget {
    variable_items: 4096,
    style_items: 4096,
    used_resource_items: 4096,
    used_resource_bytes: BRIDGE_ENVELOPE_BYTES,
};

/// 한 번에 물을 리소스 수를 밖에서 낮춰 보기 위한 손잡이.
///
/// 리소스가 하나도 해석되지 않는 것을 실기기에서 봤고, 원인이 이 크기인지
/// (플러그인이 그만한 동시 조회를 감당하지 못하는지) 아니면 애초에 그 변수를
/// 읽을 권한이 없는지 가려야 했다. 둘은 고치는 곳이 다르다.
const BRIDGE_USED_RESOURCE_ITEMS_ENV: &str = "DEVUP_FIGMA_BRIDGE_USED_RESOURCE_ITEMS";

fn bridge_batch_budget() -> BatchBudget {
    let mut budget = BRIDGE_BATCH;
    if let Some(items) = std::env::var(BRIDGE_USED_RESOURCE_ITEMS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|items| *items > 0)
    {
        budget.used_resource_items = items;
    }
    budget
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

    /// 붙어 있는 플러그인이 있으면 그쪽 크기로 묶는다.
    ///
    /// 배치는 부르기 한참 전에 나뉘므로 어느 전송이 받을지 그때는 알 수 없다.
    /// 다만 크게 묶어도 되는 읽기는 전부 스크립트 경로이고, 플러그인이 붙어 있는
    /// 한 그것은 브리지가 받는다. 수집 도중 플러그인이 닫히면 남은 배치가 원격에
    /// 너무 커서 거절되는데, 그때는 수집 자체가 이어질 수 없으므로 실패가 드러나는
    /// 편이 맞다.
    fn batch_budget(&self) -> BatchBudget {
        if self.preferred.is_live() {
            self.preferred.batch_budget()
        } else {
            self.secondary.batch_budget()
        }
    }

    /// 이 파일을 맡은 플러그인이 있으면 원격 자격증명 없이도 수집이 성립한다.
    /// 뒤엣것은 원격이므로 물을 것이 없다.
    async fn serves_without_credentials(&self, file_key: &str) -> bool {
        self.preferred.serves_without_credentials(file_key).await
    }

    async fn bridge_path_snapshot(&self) -> Option<BridgePathSnapshot> {
        self.preferred.bridge_path_snapshot().await
    }
}
