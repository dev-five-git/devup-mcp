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
    url::{BRIDGE_KEY_PREFIX, is_bridge_only_key},
};

/// 플러그인 manifest 의 `allowedDomains` 와 같은 값이어야 한다. 바꾸려면 양쪽을
/// 함께 고쳐야 하며, 플러그인은 재빌드·재설치가 필요하다.
pub const DEFAULT_BRIDGE_PORT: u16 = 1993;

/// 한 번의 읽기에 허용하는 시간. 스크립트는 문서를 한 번 훑는 정도라 원격보다
/// 훨씬 짧아도 되지만, 아주 큰 페이지의 첫 스냅샷은 몇 초가 걸린다.
const JOB_TIMEOUT: Duration = Duration::from_secs(90);

/// 브리지가 답한 결과의 `_meta` 에서 어느 플러그인이 답했는지를 싣는 자리.
const SERVED_META_KEY: &str = "devup/bridge";

/// 플러그인이 가리키는 노드 하나 — 보고 있는 페이지, 또는 거기서 선택한 것.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRef {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
}

/// Figma 를 쓰는 사람이 지금 있는 곳: 보고 있는 페이지와 거기서 선택한 것.
///
/// 모두 선택 사항이다. 이것을 보고하기 전에 빌드된 플러그인은 아무것도 보내지
/// 않고, "이 플러그인은 말하지 않는다"와 "아무것도 선택하지 않았다"는 계속
/// 구분되어야 한다 — 앞의 것은 플러그인을 다시 띄울 일이고, 뒤의 것은 Figma 에서
/// 고를 일이다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginContext {
    #[serde(default)]
    pub current_page: Option<NodeRef>,
    /// 선택한 노드의 앞부분. 플러그인이 보내는 만큼만 있고, 전체 수는
    /// `selection_count` 다.
    #[serde(default)]
    pub selection: Option<Vec<NodeRef>>,
    #[serde(default)]
    pub selection_count: Option<usize>,
}

impl PluginContext {
    /// 정확히 하나가 선택돼 있으면 그 노드.
    pub fn single_selection(&self) -> Option<&NodeRef> {
        match self.selection.as_deref()? {
            [node] if self.selection_count.is_none_or(|count| count == 1) => Some(node),
            _ => None,
        }
    }
}

/// 붙어 있는 플러그인 하나. `status` 가 보고하는 것이자, url 없는 요청이
/// 가리키게 되는 것이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedFile {
    /// 읽기가 이 플러그인에 닿으려면 부를 이름. 플러그인이 보고한 파일 키이고,
    /// 보고하지 못했으면 이 연결만 가리키는 브리지 전용 키다. 호출자에게는
    /// 보이지 않는다 — 플러그인이 보고하지 않은 키는 그 파일의 키가 아니다.
    pub target_key: String,
    /// 플러그인이 보고한 파일 키. 보고하지 못했으면 `None`.
    pub file_key: Option<String>,
    pub file_name: Option<String>,
    pub context: PluginContext,
}

/// 수집의 읽기를 어느 플러그인이 답했는지.
///
/// 수집 결과와 함께 보관되어, 새로 투영하든 재사용하든 같은 출처를 말한다.
/// 파일 키는 플러그인이 보고한 것뿐이다 — 보고하지 못했으면 키를 말하지 않으며,
/// 요청에 실려 온 키를 그 파일의 키로 내세우지 않는다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeServed {
    /// 수집의 읽기 가운데 플러그인이 답한 수.
    pub reads: usize,
    pub port: Option<u16>,
    pub file_key: Option<String>,
    pub file_name: Option<String>,
    pub page_name: Option<String>,
}

impl BridgeServed {
    /// 브리지가 답한 결과면 그 출처, 아니면 `None`.
    pub(crate) fn from_result(raw: &Value) -> Option<Self> {
        serde_json::from_value(raw.get("_meta")?.get(SERVED_META_KEY)?.clone()).ok()
    }
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
    /// 플러그인이 보고한 파일 키. 보고하지 못했으면 빈 문자열이다.
    file_key: String,
    file_name: Option<String>,
    context: PluginContext,
}

impl Connected {
    fn reported_key(&self) -> Option<String> {
        (!self.file_key.is_empty()).then(|| self.file_key.clone())
    }

    fn attached(&self, id: u64) -> AttachedFile {
        AttachedFile {
            target_key: self.reported_key().unwrap_or_else(|| connection_key(id)),
            file_key: self.reported_key(),
            file_name: self.file_name.clone(),
            context: self.context.clone(),
        }
    }

    fn served(&self) -> BridgeServed {
        BridgeServed {
            reads: 1,
            port: None,
            file_key: self.reported_key(),
            file_name: self.file_name.clone(),
            page_name: self
                .context
                .current_page
                .as_ref()
                .map(|page| page.name.clone()),
        }
    }
}

#[derive(Default)]
struct Inner {
    /// 연결 번호 → 플러그인.
    ///
    /// 파일 키로 묶지 않는다. 키를 보고하지 못한 플러그인들은 모두 빈 키라서,
    /// 그렇게 묶으면 둘째가 첫째를 덮어쓰고 첫째가 끊길 때 둘째의 등록까지
    /// 지운다. 그러면 "플러그인이 정확히 하나"를 셀 수도 없다.
    plugins: HashMap<u64, Connected>,
    /// requestId → 결과를 기다리는 쪽.
    pending: HashMap<String, oneshot::Sender<PluginResult>>,
}

/// 플러그인 연결과 대기 중인 요청을 함께 들고 있는 공유 상태.
#[derive(Clone, Default)]
pub struct BridgeState {
    inner: Arc<Mutex<Inner>>,
    /// 요청 번호와 연결 번호를 함께 매긴다. 둘 다 유일하기만 하면 된다.
    counter: Arc<AtomicU64>,
    /// 붙어 있는 플러그인 수. 배치 크기를 정할 때는 잠금을 기다릴 수 없어
    /// (그 자리가 async 가 아니다) 따로 센다.
    connected: Arc<AtomicUsize>,
}

fn unavailable(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupFigmaDirectUnavailable, message, false)
}

/// 이 연결 하나만 가리키는 브리지 전용 키.
fn connection_key(id: u64) -> String {
    format!("{BRIDGE_KEY_PREFIX}{id}")
}

fn connection_id(file_key: &str) -> Option<u64> {
    file_key.strip_prefix(BRIDGE_KEY_PREFIX)?.parse().ok()
}

/// 오류 문구에 쓸 파일 이름. 브리지 전용 키는 파일의 키가 아니므로 드러내지 않는다.
fn describe_file(file_key: &str) -> String {
    if is_bridge_only_key(file_key) {
        "the file this request was addressed to".to_owned()
    } else {
        format!("file {file_key}")
    }
}

/// 어느 연결이 이 파일을 맡을지 고른다.
///
/// 보통은 파일 키가 그대로 맞는다. 같은 파일을 두 창에서 열었으면 나중에 붙은
/// 쪽이 맡는다 — 둘 다 같은 문서를 보므로 어느 쪽이든 답은 같다.
///
/// 다만 `figma.fileKey` 는 늘 있는 값이 아니어서 (Dev Mode 에서 비어 오는 것을
/// 실제로 봤다) 키 없이 붙는 플러그인이 생긴다. 그때 등록을 건너뛰면 창은
/// "연결됨"이라고 하는데 어떤 읽기도 오지 않는 — 원인을 찾기 가장 어려운 — 상태가
/// 된다. 그래서 키 없는 플러그인은 **혼자 붙어 있을 때만** 아무 키나 맡는다.
/// 여럿이면 어느 파일을 보고 있는지 알 수 없고, 엉뚱한 파일을 읽어 주는 것보다
/// 원격으로 넘기는 편이 낫다.
///
/// 연결 키는 그 연결 하나만 가리킨다. 몇 개가 붙어 있든 모호하지 않고, 그 연결이
/// 끊기면 아무도 맡지 않는다 — 다른 파일의 플러그인이 대신 답하면 안 된다.
fn resolve(plugins: &HashMap<u64, Connected>, file_key: &str) -> Option<u64> {
    if is_bridge_only_key(file_key) {
        return connection_id(file_key).filter(|id| plugins.contains_key(id));
    }
    let holding = plugins
        .iter()
        .filter(|(_, plugin)| plugin.file_key == file_key)
        .map(|(id, _)| *id)
        .max();
    if holding.is_some() {
        return holding;
    }
    match plugins.iter().next() {
        Some((id, plugin)) if plugins.len() == 1 && plugin.file_key.is_empty() => Some(*id),
        _ => None,
    }
}

impl BridgeState {
    fn next_request_id(&self) -> String {
        format!("job-{}", self.counter.fetch_add(1, Ordering::Relaxed))
    }

    /// 이 파일을 열어 둔 플러그인이 있는지.
    pub async fn has_plugin(&self, file_key: &str) -> bool {
        resolve(&self.inner.lock().await.plugins, file_key).is_some()
    }

    /// 붙어 있는 플러그인이 보고한 파일 키. 보고하지 못한 플러그인은 빈 문자열이다.
    pub async fn connected_files(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .inner
            .lock()
            .await
            .plugins
            .values()
            .map(|plugin| plugin.file_key.clone())
            .collect();
        keys.sort();
        keys
    }

    /// 붙어 있는 플러그인 전부를 붙은 순서대로.
    pub async fn attached_files(&self) -> Vec<AttachedFile> {
        let inner = self.inner.lock().await;
        let mut plugins: Vec<_> = inner.plugins.iter().collect();
        plugins.sort_unstable_by_key(|(id, _)| **id);
        plugins
            .into_iter()
            .map(|(id, plugin)| plugin.attached(*id))
            .collect()
    }

    async fn dispatch(
        &self,
        file_key: &str,
        script: &'static str,
        params: Value,
    ) -> Result<(Value, BridgeServed), DevupError> {
        let request_id = self.next_request_id();
        let (tx, rx) = oneshot::channel();

        let served = {
            let mut inner = self.inner.lock().await;
            let Some((id, plugin)) = resolve(&inner.plugins, file_key)
                .and_then(|id| inner.plugins.get(&id).map(|plugin| (id, plugin)))
            else {
                return Err(unavailable(format!(
                    "no Devup Bridge plugin is open for {}",
                    describe_file(file_key)
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
            let sent = plugin.outbox.send(encoded).is_ok();
            let served = plugin.served();
            if !sent {
                // 소켓이 막 닫혔다. 등록을 지워 다음 호출이 곧장 폴백하도록 한다.
                inner.plugins.remove(&id);
                self.connected.store(inner.plugins.len(), Ordering::Relaxed);
                return Err(unavailable(format!(
                    "the Devup Bridge plugin for {} disconnected",
                    describe_file(file_key)
                )));
            }
            inner.pending.insert(request_id.clone(), tx);
            served
        };

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
                (Some(data), None) => Ok((data, served)),
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
/// 그 파일을 이 플러그인이 맡는다는 판단은 이미 `resolve` 가 내렸다. 이 값은
/// 디코더의 대조에만 쓰이고, 파일 키로 보고되는 것은 [`BridgeServed`] 가 싣는
/// 플러그인 자신의 보고뿐이다.
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
///
/// 어느 플러그인이 답했는지는 `_meta` 에 싣는다. 디코더는 그 자리를 읽지 않으므로
/// 두 경로의 모양은 그대로이고, 수집기는 이것으로 응답의 출처를 브리지로 적는다.
fn wrap_as_tool_result(data: &Value, served: &BridgeServed) -> Result<Value, DevupError> {
    let encode =
        |error: serde_json::Error| unavailable(format!("bridge result encode failed: {error}"));
    let text = serde_json::to_string(data).map_err(encode)?;
    let mut result = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "_meta": {},
    });
    result["_meta"][SERVED_META_KEY] = serde_json::to_value(served).map_err(encode)?;
    Ok(result)
}

async fn plugin_socket(State(state): State<BridgeState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| handle_plugin(state, socket))
}

async fn handle_plugin(state: BridgeState, mut socket: WebSocket) {
    let (outbox, mut outbox_rx) = mpsc::unbounded_channel::<String>();
    let id = state.counter.fetch_add(1, Ordering::Relaxed);

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
                        // 어디까지 믿을지는 resolve 가 정한다.
                        //
                        // 필드마다 따로 읽는다. 페이지나 선택이 어긋난 모양으로 와도
                        // 파일 키까지 잃으면 안 된다.
                        let plugin = Connected {
                            outbox: outbox.clone(),
                            file_key: value
                                .get("fileKey")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            file_name: value
                                .get("fileName")
                                .and_then(Value::as_str)
                                .filter(|name| !name.is_empty())
                                .map(str::to_owned),
                            context: serde_json::from_value(value).unwrap_or_default(),
                        };
                        let mut inner = state.inner.lock().await;
                        inner.plugins.insert(id, plugin);
                        state.connected.store(inner.plugins.len(), Ordering::Relaxed);
                    }
                    // 페이지를 옮기거나 선택을 바꿀 때마다 온다. url 없는 요청은
                    // 이 값으로 대상을 고르므로, 늦게라도 최신이어야 한다.
                    Some("context") => {
                        let Ok(context) = serde_json::from_value::<PluginContext>(value) else {
                            continue;
                        };
                        if let Some(plugin) = state.inner.lock().await.plugins.get_mut(&id) {
                            plugin.context = context;
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

    let mut inner = state.inner.lock().await;
    inner.plugins.remove(&id);
    state
        .connected
        .store(inner.plugins.len(), Ordering::Relaxed);
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
    /// 지금 붙어 있는 플러그인, 붙은 순서대로. 파일 키를 보고하지 못한
    /// 플러그인은 혼자 붙어 있을 때만 다른 키의 읽기를 받는다.
    pub attached_files: Vec<AttachedFile>,
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
        let (mut data, served) = self
            .state
            .dispatch(call.file_key(), job.script, params)
            .await?;
        stamp_file_key(&mut data, call.file_key());
        let served = BridgeServed {
            port: self.port,
            ..served
        };
        Ok(UpstreamResult {
            raw: wrap_as_tool_result(&data, &served)?,
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
            attached_files: self.state.attached_files().await,
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
        // 브리지만 읽을 수 있는 키다. 원격으로 넘기면 Figma 에 없는 키를 묻게 되고,
        // 돌아오는 것은 원인과 무관한 거절 — 로그인하라거나 파일이 없다는 — 뿐이다.
        if is_bridge_only_key(call.file_key()) {
            return Err(bridge_only_refusal(&call));
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
    ///
    /// 브리지만 읽을 수 있는 키는 로그인을 기다리지 않는다. direct 경로는 그 키로는
    /// 무엇을 치러도 읽을 수 없으므로, 로그인을 요구하는 것은 엉뚱한 처방이다.
    /// 플러그인이 그새 끊겼다면 그 거절은 첫 읽기가 제 이유와 함께 한다.
    async fn serves_without_credentials(&self, file_key: &str) -> bool {
        is_bridge_only_key(file_key) || self.preferred.serves_without_credentials(file_key).await
    }

    async fn bridge_path_snapshot(&self) -> Option<BridgePathSnapshot> {
        self.preferred.bridge_path_snapshot().await
    }
}

/// 브리지만 읽을 수 있는 키의 읽기를 브리지가 맡지 못할 때의 거절.
///
/// 두 경우는 고치는 방법이 다르다. 스크립트 읽기면 플러그인이 끊긴 것이니 다시
/// 띄우면 되고, 그 밖의 읽기(`get_screenshot` 등)는 브리지가 애초에 못 하는 일이라
/// 그 파일의 진짜 Figma 링크가 있어야 direct 경로로 읽을 수 있다.
fn bridge_only_refusal(call: &ReadToolCall) -> DevupError {
    let message = if call.bridge_job().is_some() {
        "The Devup Bridge plugin this request was addressed to is no longer attached. Run it \
         again on the file in the Figma desktop app, then repeat the call."
    } else {
        "This read cannot go through the Devup Bridge plugin, and the plugin could not report \
         the file's Figma key, so the direct path cannot make it either. Repeat the call with the \
         file's own Figma link (Share -> Copy link) to read it through the direct path."
    };
    DevupError::with_details(
        ErrorCode::DevupFigmaDirectUnavailable,
        message,
        false,
        json!({ "source": "bridge", "stage": "bridge-routing", "tool": call.tool_name() }),
    )
}
