
#[path = "holder.rs"]
mod holder;
#[path = "relay.rs"]
mod relay;
#[path = "secret.rs"]
mod secret;

pub use holder::PortOwner;

use std::{
    collections::HashMap,
    hash::Hash,
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header::ORIGIN},
    response::{IntoResponse, Response},
    routing::any,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc, oneshot, watch},
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

/// 참조 PNG 결과의 `_meta` 에서 플러그인이 그 그림에 단 말을 싣는 자리.
pub(crate) const REFERENCE_PNG_NOTE_META_KEY: &str = "devup/referencePngNote";

/// 플러그인이 끊긴 뒤 다시 붙기를 시도하는 간격(`plugin/src/ui.ts` 의 `RETRY_MS`).
const PLUGIN_RETRY: Duration = Duration::from_secs(2);

/// 호스트가 바뀐 직후, 붙어 있던 플러그인이 새 호스트에 다시 붙기를 기다리는 시간.
/// 플러그인의 재시도 간격에 연결·인사에 걸리는 시간을 얹었다. 이 사이의 읽기는
/// 플러그인이 없다고 단정해 요금이 드는 경로로 넘기지 않고, 붙기를 기다린다.
const REATTACH_GRACE: Duration = Duration::from_secs(PLUGIN_RETRY.as_secs() + 3);

/// 누가 포트를 쥐었는지 아직 모를 때 status 와 읽기가 답을 기다리는 한도. 확인은
/// 몇 밀리초면 끝나고, 대답하지 않는 상대도 [`relay`] 의 제한 시간 안에 판정된다.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(5);

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<serde_json::Number>,
    /// Pixel ratio of the plugin UI iframe, not a verified canvas capture scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_device_pixel_ratio: Option<serde_json::Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_known_selection: Option<Vec<NodeRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_known_page: Option<NodeRef>,
    #[serde(default)]
    pub current_page: Option<NodeRef>,
    /// 선택한 노드의 앞부분. 플러그인이 보내는 만큼만 있고, 전체 수는
    /// `selection_count` 다.
    #[serde(default)]
    pub selection: Option<Vec<NodeRef>>,
    #[serde(default)]
    pub selection_count: Option<usize>,
    /// 플러그인 창이 화면에 보이는지. Figma 는 보이지 않는 파일이 아직 불러오지 않은
    /// 데이터를 읽지 못하게 하므로, 수집을 시작하기 전에 알 수 있어야 한다. 이것을
    /// 보고하기 전에 빌드된 플러그인은 `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_view: Option<bool>,
    /// 플러그인 빌드의 버전. `hello` 에만 실려 오며, 이것을 보고하기 전에 빌드된
    /// 플러그인은 `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u32>,
}

impl PluginContext {
    pub fn plugin_build(&self) -> Value {
        let unknown = self.scripts_hash.as_deref().is_none_or(str::is_empty) || self.protocol.is_none();
        let stale = self.scripts_hash.as_deref() != Some(crate::PLUGIN_SCRIPTS_HASH)
            || self.protocol != Some(crate::PLUGIN_PROTOCOL);
        json!({"pluginStale":stale,"state":if unknown {"stale-unknown"} else if stale {"stale"} else {"current"},
            "expected":crate::PLUGIN_SCRIPTS_HASH,"actual":self.scripts_hash,
            "expectedProtocol":crate::PLUGIN_PROTOCOL,"actualProtocol":self.protocol})
    }

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
    #[serde(default)]
    pub scripts_hash: Option<String>,
    #[serde(default)]
    pub protocol: Option<u32>,
}

impl BridgeServed {
    /// 브리지가 답한 결과면 그 출처, 아니면 `None`.
    pub fn from_result(raw: &Value) -> Option<Self> {
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
///
/// `requestId` 는 늘 호스트가 매긴다. 중계로 온 읽기도 마찬가지라, 여러 프로세스의
/// 읽기가 한 플러그인에 모여도 번호가 겹치지 않고, 답은 번호로 물은 쪽에만 간다.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Job<'a> {
    kind: &'static str,
    request_id: String,
    script: &'a str,
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
    /// 읽기가 이 플러그인에 닿으려면 부를 이름. [`routing_key`] 가 정한다.
    routing_key: String,
    file_name: Option<String>,
    context: PluginContext,
}

impl Connected {
    fn reported_key(&self) -> Option<String> {
        (!self.file_key.is_empty()).then(|| self.file_key.clone())
    }

    fn attached(&self) -> AttachedFile {
        AttachedFile {
            target_key: self.routing_key.clone(),
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
            scripts_hash: self.context.scripts_hash.clone(),
            protocol: self.context.protocol,
        }
    }
}

/// 읽기가 이 플러그인에 닿으려면 부를 이름.
///
/// 파일 키를 보고했으면 그 키다. 보고하지 못했으면(Dev Mode) 브리지 전용 키인데,
/// 플러그인이 창마다 한 번 정해 `hello` 에 싣는 `sessionId` 가 있으면 그것에서
/// 만든다 — 소켓이 끊겨 다시 붙어도, 포트를 쥔 devup-mcp 가 바뀌어도 같은 창이면
/// 같은 이름이라, 수집 도중에 연결이 바뀌어도 남은 읽기가 그 창을 다시 찾는다.
/// `sessionId` 를 보내지 않는 예전 빌드는 연결마다 매긴 번호로 부른다.
fn routing_key(file_key: &str, session: Option<&str>, connection: u64) -> String {
    if !file_key.is_empty() {
        return file_key.to_owned();
    }
    match session {
        Some(session) => format!("{BRIDGE_KEY_PREFIX}{session}"),
        None => connection_key(connection),
    }
}

/// 플러그인이 보낸 `sessionId` 를 이름에 쓸 수 있을 때만 받는다. 브리지 전용 키의
/// 모양을 지키고(`bridge:current` 같은 예약된 이름과 겹치지 않게) 길이를 묶는다.
fn session_of(hello: &Value) -> Option<String> {
    hello
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|session| {
            (16..=64).contains(&session.len())
                && session
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        })
        .map(|session| format!("s-{session}"))
}

/// 플러그인이 답하기를 기다리는 읽기 하나.
struct Pending {
    /// 작업을 받은 플러그인의 연결 번호. 그 연결이 끊기면 이 읽기도 끝난다.
    plugin: u64,
    /// 그 플러그인에 보낼 곳. 기다리는 쪽이 떠나면 여기로 거둬 달라고 알린다.
    outbox: mpsc::UnboundedSender<String>,
    waiter: oneshot::Sender<PluginResult>,
}

type PendingTable = StdMutex<HashMap<String, Pending>>;

/// 플러그인의 답을 기다리는 동안 쥐고 있는 대기표.
///
/// 답이 오면 받는 쪽이 대기표를 이미 걷었으므로 아무 일도 하지 않는다. 답을 받지
/// 못한 채 버려지면 — 시간이 다 됐거나, 읽기를 맡긴 프로세스가 떠나 호스트가 그
/// 읽기를 취소했거나 — 대기표를 걷고 플러그인에 `devup-cancel` 을 보낸다. 아직
/// 차례를 기다리던 작업이면 플러그인은 돌리지 않는다.
struct Awaiting<'a> {
    pending: &'a PendingTable,
    request_id: String,
}

impl Drop for Awaiting<'_> {
    fn drop(&mut self) {
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        if let Some(abandoned) = pending.remove(&self.request_id) {
            let cancel = json!({ "kind": "devup-cancel", "requestId": self.request_id });
            let _ = abandoned.outbox.send(cancel.to_string());
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
    /// 연결 번호 → 이 호스트를 통해 읽는 다른 devup-mcp. 플러그인이 붙거나 떠나거나
    /// 선택이 바뀔 때마다 모두에게 새 목록을 보낸다.
    relays: HashMap<u64, mpsc::UnboundedSender<String>>,
}

/// 결과를 기다리는 쪽이 사라지면 그 대기표를 걷는다.
///
/// 기다리던 future 가 버려지는 일은 흔하다 — 중계를 요청한 프로세스가 끊기면 호스트는
/// 그 읽기들을 취소한다. 걷지 않으면 플러그인이 끝내 답하지 않는 한 대기표가 남는다.
struct Waiting<'a, K: Eq + Hash, V> {
    pending: &'a StdMutex<HashMap<K, V>>,
    key: K,
}

impl<K: Eq + Hash, V> Drop for Waiting<'_, K, V> {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&self.key);
        }
    }
}

/// 이 프로세스가 브리지에서 맡은 역할.
#[derive(Clone)]
enum LinkState {
    /// 누가 포트를 쥐었는지 아직 모른다 — 시작할 때, 또는 쥐고 있던 호스트가 떠난 뒤.
    Connecting { after: Option<BridgePeer> },
    /// 이 프로세스가 포트를 쥐었다.
    ///
    /// `expecting` 은 방금 떠난 호스트다. 그때 플러그인이 붙어 있었으면 곧 이리로
    /// 다시 붙는다.
    Host {
        expecting: Option<BridgePeer>,
        since: Instant,
    },
    /// 다른 devup-mcp 가 포트를 쥐었고, 이 프로세스는 그를 통해 읽는다. `files` 는
    /// 그 호스트가 보낸 플러그인 목록이다.
    Relay {
        connection: Arc<relay::RelayConnection>,
        files: Vec<(u64, AttachedFile)>,
        expecting: Option<BridgePeer>,
        since: Instant,
    },
    /// 지금은 브리지를 쓸 수 없다. `holder` 는 포트를 쥔 쪽이 스스로 밝힌 것이다.
    Unavailable {
        issue: BridgeIssue,
        holder: Option<BridgePeer>,
    },
}

struct Link {
    state: watch::Sender<LinkState>,
    /// 중계 핸드셰이크에서 상대에게 밝히는 이 프로세스.
    me: BridgePeer,
    secret_path: Option<PathBuf>,
    shutdown: watch::Sender<bool>,
}

impl Link {
    fn current(&self) -> LinkState {
        self.state.borrow().clone()
    }

    fn set(&self, state: LinkState) {
        self.state.send_replace(state);
    }

    /// 역할은 그대로지만 보이는 것이 바뀌었다(플러그인이 붙거나 떠났다).
    fn touch(&self) {
        self.state.send_modify(|_| {});
    }

    fn shutdown_signal(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }

    fn is_shut_down(&self) -> bool {
        *self.shutdown.borrow()
    }
}

/// 닫으라는 신호가 올 때까지 기다린다.
async fn shut_down(signal: &mut watch::Receiver<bool>) {
    let _ = signal.wait_for(|down| *down).await;
}

/// 플러그인 연결과 대기 중인 요청, 이 프로세스의 역할을 함께 들고 있는 공유 상태.
#[derive(Clone)]
pub struct BridgeState {
    inner: Arc<Mutex<Inner>>,
    /// requestId → 결과를 기다리는 쪽. 잠금을 쥔 채 기다리지 않으므로 동기 잠금이면
    /// 되고, 그래야 기다리던 쪽이 사라질 때 [`Awaiting`] 이 곧바로 걷을 수 있다.
    pending: Arc<PendingTable>,
    /// 요청 번호와 연결 번호를 함께 매긴다. 둘 다 유일하기만 하면 된다.
    counter: Arc<AtomicU64>,
    /// 이 프로세스에서 보이는 플러그인 수. 배치 크기를 정할 때는 잠금을 기다릴 수 없어
    /// (그 자리가 async 가 아니다) 따로 센다.
    connected: Arc<AtomicUsize>,
    link: Arc<Link>,
}

fn unavailable(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupFigmaDirectUnavailable, message, false)
}

/// 플러그인이 답한 실패 — 스크립트가 던진 `DEVUP_*` 코드나 Figma 런타임의 문구.
/// 전송이 끊긴 것과 달리 읽기는 플러그인에 닿았으므로, [`BridgeFigmaClient`] 가
/// 원격 `use_figma` 와 같은 `isError` 결과로 바꿔 올린다. 그러지 않으면 Figma 가
/// 페이지를 불러오지 못했다는 말까지 "direct 경로를 쓸 수 없다"로 읽혔다.
fn script_failure(message: impl Into<String>) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaDirectUnavailable,
        message,
        false,
        json!({ "source": "bridge", "stage": "plugin-execution" }),
    )
}

pub(crate) fn is_script_failure(error: &DevupError) -> bool {
    error.details["source"] == "bridge" && error.details["stage"] == "plugin-execution"
}

/// 플러그인이 답한 실패를 원격 `use_figma` 가 주는 모양으로 쓴다. 위쪽은 두 경로의
/// 실패를 한 가지로 분류한다 — 없는 노드는 호출자 오류로, 나머지는 플러그인 실행
/// 오류로.
fn script_failure_result(message: &str) -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "content": [{ "type": "text", "text": format!("Error: {message}") }],
            "isError": true,
        }),
    }
}

/// 이 연결 하나만 가리키는 브리지 전용 키.
fn connection_key(id: u64) -> String {
    format!("{BRIDGE_KEY_PREFIX}{id}")
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
/// 브리지 전용 키는 그 창 하나만 가리킨다. 몇 개가 붙어 있든 모호하지 않고, 그
/// 창이 끊기면 아무도 맡지 않는다 — 다른 파일의 플러그인이 대신 답하면 안 된다.
/// 같은 창이 다시 붙으면(같은 `sessionId`) 나중 연결이 맡는다.
///
/// `plugins` 는 (연결 번호, 부를 이름, 보고한 파일 키 — 없으면 빈 문자열) 이다.
/// 호스트는 제 연결로, 중계는 호스트가 보낸 목록으로 같은 규칙을 쓴다.
fn resolve<'a>(
    plugins: impl IntoIterator<Item = (u64, &'a str, &'a str)>,
    file_key: &str,
) -> Option<u64> {
    let plugins: Vec<(u64, &str, &str)> = plugins.into_iter().collect();
    if is_bridge_only_key(file_key) {
        return plugins
            .iter()
            .filter(|(_, routing, _)| *routing == file_key)
            .map(|(id, ..)| *id)
            .max();
    }
    let holding = plugins
        .iter()
        .filter(|(_, _, reported)| *reported == file_key)
        .map(|(id, ..)| *id)
        .max();
    if holding.is_some() {
        return holding;
    }
    match plugins.as_slice() {
        [(id, _, "")] => Some(*id),
        _ => None,
    }
}

fn keys_of(plugins: &HashMap<u64, Connected>) -> impl Iterator<Item = (u64, &str, &str)> {
    plugins
        .iter()
        .map(|(id, plugin)| (*id, plugin.routing_key.as_str(), plugin.file_key.as_str()))
}

fn keys_in(files: &[(u64, AttachedFile)]) -> impl Iterator<Item = (u64, &str, &str)> {
    files.iter().map(|(id, file)| {
        (
            *id,
            file.target_key.as_str(),
            file.file_key.as_deref().unwrap_or_default(),
        )
    })
}

/// 읽기 한 번이 끝난 모양. 중간에 끊긴 것은 다시 보낼 수 있다 — 읽기는 문서를
/// 바꾸지 않는다.
pub(crate) enum Failure {
    /// 플러그인에 닿는 길이 도중에 끊겼다: 플러그인이 떠났거나, 포트를 쥔 호스트가
    /// 떠났다. 플러그인이 돌아오면 다시 보낸다.
    Interrupted(DevupError),
    /// 답이 왔거나(스크립트 오류 포함) 더 기다릴 수 없다. 그대로 올린다.
    Final(DevupError),
}

/// 끊긴 읽기를 다시 보내는 횟수의 한도.
const INTERRUPTIONS: usize = 3;

/// 끊긴 읽기가 플러그인이 돌아오기를 기다리는 한도. 호스트가 바뀌어도 이 안에
/// 포트가 넘어가고, 플러그인은 2초마다 다시 붙는다.
const RETURN_WAIT: Duration = Duration::from_secs(10);

/// 중계가 다룰 수 없는 상태에서 읽기가 왔을 때의 거절.
fn not_reachable(link: &LinkState) -> DevupError {
    match link {
        LinkState::Unavailable { issue, .. } => unavailable(format!(
            "this devup-mcp cannot use the Devup Bridge right now ({}); devup_figma_auth \
             {{ action: \"status\" }} says why and what to do",
            issue.code()
        )),
        _ => unavailable(include_str!("../text/bridge-not_reachable-fa7318ce.md")),
    }
}

impl BridgeState {
    fn new(options: BridgeOptions) -> Self {
        Self {
            inner: Arc::default(),
            pending: Arc::default(),
            // 연결 번호가 키 없는 플러그인의 라우팅 키(`bridge:<번호>`)가 된다. 호스트가
            // 바뀐 뒤 옛 호스트가 매긴 번호가 새 호스트의 다른 플러그인을 가리키면, 읽기가
            // 엉뚱한 파일로 조용히 간다. 프로세스마다 멀리 떨어진 자리에서 센다.
            counter: Arc::new(AtomicU64::new(rand::random::<u64>() >> 16)),
            connected: Arc::default(),
            link: Arc::new(Link {
                state: watch::channel(LinkState::Connecting { after: None }).0,
                me: BridgePeer {
                    pid: std::process::id(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                    build_id: options.build_id,
                },
                secret_path: options.secret_path.or_else(secret::default_path),
                shutdown: watch::channel(false).0,
            }),
        }
    }

    fn next_request_id(&self) -> String {
        format!("job-{}", self.counter.fetch_add(1, Ordering::Relaxed))
    }

    /// 포트를 쥔 이 프로세스가 플러그인과 중계를 받기 시작한다.
    fn serve(
        &self,
        listener: std::net::TcpListener,
        expecting: Option<BridgePeer>,
    ) -> io::Result<()> {
        listener.set_nonblocking(true)?;
        let listener = TcpListener::from_std(listener)?;
        let app = Router::new()
            .route("/plugin", any(plugin_socket))
            .route("/relay", any(relay::relay_socket))
            .with_state(self.clone());
        let mut shutdown = self.link.shutdown_signal();
        self.connected.store(0, Ordering::Relaxed);
        self.link.set(LinkState::Host {
            expecting,
            since: Instant::now(),
        });
        tokio::spawn(async move {
            // 닫으라는 신호가 오면 듣기를 멈추고 포트를 놓는다. 이어받을 프로세스가 곧장
            // bind 할 수 있어야 한다.
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move { shut_down(&mut shutdown).await })
                .await;
        });
        Ok(())
    }

    /// 플러그인이 어느 devup-mcp 에 붙었는지 알 수 있게 먼저 밝힌다.
    /// 모르는 메시지는 예전 플러그인이 무시한다.
    fn announce_server(&self, outbox: &tokio::sync::mpsc::UnboundedSender<String>) {
        let _ = outbox.send(
            json!({
                "kind": "server-info",
                "version": self.link.me.version,
                "buildId": self.link.me.build_id,
            })
            .to_string(),
        );
    }

    /// 플러그인 목록이 바뀌었다. 세고, 중계들에게 알리고, 기다리는 쪽을 깨운다.
    fn plugins_changed(&self, inner: &mut Inner) {
        self.connected.store(inner.plugins.len(), Ordering::Relaxed);
        if !inner.relays.is_empty() {
            let message = relay::files_message(&inner.plugins);
            inner
                .relays
                .retain(|_, outbox| outbox.send(message.clone()).is_ok());
        }
        self.link.touch();
    }

    /// 역할이 정해질 때까지, 그리고 방금 호스트가 바뀌었다면 플러그인이 다시 붙을
    /// 때까지 기다린다. 둘 다 짧고, 한도가 있다.
    ///
    /// 그 사이에 판정하면 "플러그인이 없다"가 된다. 그러면 읽기는 요금이 드는 경로로
    /// 넘어가고, status 는 로그인하라고 한다 — 플러그인이 2초 뒤면 다시 붙는데도.
    async fn settle(&self) {
        let deadline = tokio::time::Instant::now() + SETTLE_TIMEOUT;
        let mut changes = self.link.state.subscribe();
        loop {
            let until = match &*changes.borrow_and_update() {
                LinkState::Connecting { .. } => Some(deadline),
                LinkState::Host {
                    expecting: Some(_),
                    since,
                }
                | LinkState::Relay {
                    expecting: Some(_),
                    since,
                    ..
                } if self.connected.load(Ordering::Relaxed) == 0 => {
                    Some(deadline.min(tokio::time::Instant::from_std(*since + REATTACH_GRACE)))
                }
                _ => None,
            };
            let Some(until) = until else { return };
            if !matches!(
                tokio::time::timeout_at(until, changes.changed()).await,
                Ok(Ok(()))
            ) {
                return;
            }
        }
    }

    /// 이 프로세스에서 보이는 플러그인, 연결 번호와 함께 붙은 순서대로.
    async fn view(&self, link: &LinkState) -> Vec<(u64, AttachedFile)> {
        match link {
            LinkState::Host { .. } => {
                let inner = self.inner.lock().await;
                let mut plugins: Vec<_> = inner.plugins.iter().collect();
                plugins.sort_unstable_by_key(|(id, _)| **id);
                plugins
                    .into_iter()
                    .map(|(id, plugin)| (*id, plugin.attached()))
                    .collect()
            }
            LinkState::Relay { files, .. } => files.clone(),
            LinkState::Connecting { .. } | LinkState::Unavailable { .. } => Vec::new(),
        }
    }

    /// 지금 이 파일을 맡을 플러그인이 보이는지. 기다리지 않는다.
    async fn sees(&self, file_key: &str) -> bool {
        let link = self.link.current();
        let view = self.view(&link).await;
        resolve(keys_in(&view), file_key).is_some()
    }

    /// 이 파일을 열어 둔 플러그인이 있는지. 포트를 쥔 프로세스를 통해 보이는 것도 센다.
    pub async fn has_plugin(&self, file_key: &str) -> bool {
        self.settle().await;
        self.sees(file_key).await
    }

    /// 끊긴 읽기의 플러그인이 돌아오기를 기다린다. 돌아왔으면 `true`.
    async fn plugin_returns(&self, file_key: &str) -> bool {
        let until = tokio::time::Instant::now() + RETURN_WAIT;
        let mut changes = self.link.state.subscribe();
        loop {
            changes.borrow_and_update();
            if self.sees(file_key).await {
                return true;
            }
            if !matches!(
                tokio::time::timeout_at(until, changes.changed()).await,
                Ok(Ok(()))
            ) {
                return self.sees(file_key).await;
            }
        }
    }

    /// 붙어 있는 플러그인이 보고한 파일 키. 보고하지 못한 플러그인은 빈 문자열이다.
    pub async fn connected_files(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .attached_files()
            .await
            .into_iter()
            .map(|file| file.file_key.unwrap_or_default())
            .collect();
        keys.sort();
        keys
    }

    /// 붙어 있는 플러그인 전부를 붙은 순서대로. 어느 프로세스에서 물어도 같다 —
    /// 포트를 쥐지 못한 프로세스는 쥔 프로세스가 보낸 목록을 그대로 보인다.
    pub async fn attached_files(&self) -> Vec<AttachedFile> {
        self.settle().await;
        let link = self.link.current();
        self.view(&link)
            .await
            .into_iter()
            .map(|(_, file)| file)
            .collect()
    }

    /// 아직 플러그인의 답을 기다리는 읽기 수. 요청한 쪽이 사라지면 그 읽기가 걷히는지
    /// 테스트가 확인한다.
    #[doc(hidden)]
    pub fn pending_reads(&self) -> usize {
        self.pending.lock().map_or(0, |pending| pending.len())
    }

    async fn path_snapshot(&self, port: Option<u16>) -> BridgePathSnapshot {
        self.settle().await;
        let link = self.link.current();
        let attached_files: Vec<AttachedFile> = self
            .view(&link)
            .await
            .into_iter()
            .map(|(_, file)| file)
            .collect();
        let recently = |expecting: &Option<BridgePeer>, since: &Instant| {
            expecting
                .clone()
                .filter(|_| attached_files.is_empty() && since.elapsed() < REATTACH_GRACE)
        };
        let (role, host, issue, handover_from) = match &link {
            LinkState::Host { expecting, since } => (
                BridgeRole::Host,
                Some(self.link.me.clone()),
                None,
                recently(expecting, since),
            ),
            LinkState::Relay {
                connection,
                expecting,
                since,
                ..
            } => (
                BridgeRole::Relay,
                Some(connection.host.clone()),
                None,
                recently(expecting, since),
            ),
            LinkState::Connecting { after } => (BridgeRole::Connecting, None, None, after.clone()),
            LinkState::Unavailable { issue, holder } => (
                BridgeRole::Unavailable,
                holder.clone(),
                Some(issue.clone()),
                None,
            ),
        };
        BridgePathSnapshot {
            port,
            attached_files,
            role,
            host,
            issue,
            handover_from,
        }
    }

    /// 읽기 하나를 플러그인에 보낸다. 포트를 쥐었으면 곧장, 아니면 쥔 프로세스를 통해.
    ///
    /// 도중에 길이 끊기면 — 플러그인 창이 다시 붙었거나, 포트를 쥔 호스트가 떠나
    /// 다른 프로세스가 이어받았으면 — 플러그인이 돌아오기를 잠깐 기다렸다가 다시
    /// 보낸다. 읽기는 문서를 바꾸지 않으므로 두 번 돌아도 해가 없고, 호출자는 인계가
    /// 있었는지 모른 채 답을 받는다. 수집 한가운데서 호스트 세션이 끝나도 그 수집은
    /// 이어진다.
    async fn dispatch(
        &self,
        file_key: &str,
        script: &str,
        params: Value,
    ) -> Result<(Value, BridgeServed), DevupError> {
        let mut interruptions = 0;
        loop {
            self.settle().await;
            let attempt = match self.link.current() {
                LinkState::Host { .. } => {
                    self.dispatch_local(file_key, script, params.clone()).await
                }
                LinkState::Relay { connection, .. } => {
                    connection.dispatch(file_key, script, params.clone()).await
                }
                other => return Err(not_reachable(&other)),
            };
            match attempt {
                Ok(done) => return Ok(done),
                Err(Failure::Final(error)) => return Err(error),
                Err(Failure::Interrupted(error)) => {
                    interruptions += 1;
                    if interruptions > INTERRUPTIONS || !self.plugin_returns(file_key).await {
                        return Err(error);
                    }
                }
            }
        }
    }

    /// 이 프로세스에 붙은 플러그인으로 한 번 보낸다.
    async fn dispatch_local(
        &self,
        file_key: &str,
        script: &str,
        params: Value,
    ) -> Result<(Value, BridgeServed), Failure> {
        let request_id = self.next_request_id();
        let budget = job_timeout(script, &params);
        let is_asset = job_is_asset_read(script, &params);
        let requested_node = params["nodeId"].as_str().unwrap_or("unknown").to_owned();
        let (tx, rx) = oneshot::channel();

        let served = {
            let mut inner = self.inner.lock().await;
            let Some((id, plugin)) = resolve(keys_of(&inner.plugins), file_key)
                .and_then(|id| inner.plugins.get(&id).map(|plugin| (id, plugin)))
            else {
                return Err(Failure::Final(unavailable(format!(
                    "no Devup Bridge plugin is open for {}",
                    describe_file(file_key)
                ))));
            };
            let job = Job {
                kind: "devup-job",
                request_id: request_id.clone(),
                script,
                params,
            };
            let encoded = serde_json::to_string(&job).map_err(|error| {
                Failure::Final(unavailable(format!("bridge job encode failed: {error}")))
            })?;
            // 답이 보내기보다 먼저 올 수는 없지만, 대기표는 보내기 전에 둔다.
            self.pending
                .lock()
                .expect("the pending table is never poisoned")
                .insert(
                    request_id.clone(),
                    Pending {
                        plugin: id,
                        outbox: plugin.outbox.clone(),
                        waiter: tx,
                    },
                );
            let sent = plugin.outbox.send(encoded).is_ok();
            let served = plugin.served();
            if !sent {
                // 소켓이 막 닫혔다. 등록을 지워 다음 호출이 곧장 폴백하도록 한다.
                inner.plugins.remove(&id);
                self.plugin_left(&mut inner, id);
                return Err(Failure::Interrupted(unavailable(format!(
                    "the Devup Bridge plugin for {} disconnected",
                    describe_file(file_key)
                ))));
            }
            served
        };

        // 성공이든 실패든, 기다리던 쪽이 도중에 사라지든 대기표는 반드시 걷는다.
        // 답을 받지 못한 채 걷으면 플러그인에도 그 작업을 거두라고 알린다.
        let _awaiting = Awaiting {
            pending: &self.pending,
            request_id,
        };
        match timeout(budget, rx).await {
            Ok(Ok(result)) => match (result.data, result.error) {
                // 스크립트가 던진 DEVUP_* 코드는 그대로 올린다. 원격 경로와 같은
                // 문자열이어야 위쪽 분기가 동일하게 동작한다.
                (_, Some(message)) => Err(Failure::Final(script_failure(message))),
                (Some(data), None) => Ok((data, served)),
                (None, None) => Err(Failure::Final(unavailable(
                    "bridge returned neither data nor error",
                ))),
            },
            // 작업을 받은 플러그인 창이 끊겼다.
            Ok(Err(_)) => Err(Failure::Interrupted(unavailable(
                "the Devup Bridge plugin disconnected mid-read",
            ))),
            Err(_) => Err(Failure::Final(unavailable(format!(
                "{}Devup Bridge {script} did not answer after {} ms (node: {requested_node})",
                if is_asset {
                    "DEVUP_ASSET_TIMEOUT: "
                } else {
                    ""
                },
                budget.as_millis()
            )))),
        }
    }

    /// 플러그인 하나가 떠났다(등록은 이미 지웠다). 그 플러그인이 받아 둔 읽기는 더
    /// 답이 오지 않으니 곧장 끝낸다 — 90초를 기다리게 하지 않는다.
    fn plugin_left(&self, inner: &mut Inner, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.retain(|_, waiting| waiting.plugin != id);
        }
        self.plugins_changed(inner);
    }
}
