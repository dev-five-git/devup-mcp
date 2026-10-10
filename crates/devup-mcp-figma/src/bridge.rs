//! 열려 있는 Figma 파일을 우리 플러그인으로 직접 읽는 전송 계층.
//!
//! 공식 MCP 경로(`RemoteFigmaClient`)는 읽기 스크립트를 `use_figma` 로 보내는데,
//! 그 도구가 요금 한도를 쓴다. 스크립트가 건드리는 것은 문서화된 Plugin API 뿐이라
//! 우리가 돌리는 플러그인이 같은 일을 할 수 있고, 그 경로에는 한도가 없다.
//!
//! 방향이 거꾸로인 점이 설계를 결정한다. Figma 플러그인은 들어오는 연결을 받지
//! 못하고 나가는 WebSocket 만 열 수 있어서, **서버는 이쪽**이고 플러그인이 붙는다.
//!
//! 한 기기에 devup-mcp 가 여럿 떠 있는 것은 정상이다 — MCP 클라이언트마다, 세션마다
//! 하나씩 뜬다. 플러그인이 붙을 수 있는 포트는 manifest 에 적힌 하나뿐이라, 그 포트를
//! 잡은 프로세스(호스트)만 플러그인을 받는다. 잡지 못한 프로세스는 호스트의 `/relay`
//! 에 붙어 그를 통해 읽고, 호스트가 떠나면 그중 하나가 포트를 이어받는다. 어떻게,
//! 왜 그렇게 하는지는 [`relay`] 에 있다.
//!
//! 응답은 원격이 주는 모양 그대로 감싼다. 디코더들은 `content[].text` 안의 JSON
//! 을 찾도록 쓰여 있으므로, 여기서 모양을 바꾸면 두 경로가 조용히 갈라진다. 중계를
//! 거친 답도 이 모양이다 — 감싸는 일은 요청한 프로세스가 한다.

mod holder;
mod relay;
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
        _ => unavailable(
            "the Devup Bridge port is changing hands: the devup-mcp that held it went away and \
             this process is taking it over or reconnecting. Repeat the call in a few seconds.",
        ),
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
        match timeout(JOB_TIMEOUT, rx).await {
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
            Err(_) => Err(Failure::Final(unavailable(
                "the Devup Bridge plugin did not answer in time",
            ))),
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

/// 한 번에 담을 수 있는 양. 로컬 소켓이라 공식 MCP 의 자름을 따를 이유가 없다.
///
/// 기본값(봉투 19KB · 필드 4KB)은 공식 MCP 가 텍스트 결과를 20,480 바이트에서
/// 자르기 때문에 생긴 것이다. 그 탓에 화면 하나가 30번 넘는 왕복으로 쪼개지고,
/// 창이 뒤로 가 있으면 왕복 하나가 60초까지 걸린다 — 실측한 값이다.
pub(crate) const BRIDGE_ENVELOPE_BYTES: usize = 8 * 1024 * 1024;
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
///
/// 빠른 스냅샷·테마 봉투는 이 키를 `source` 와 `snapshot` 안에 싣는다. 거기가 빈
/// 채로 가면 디코더가 대상 불일치로 보고 빠른 경로를 통째로 버린다 — 실기기에서
/// 화면마다 그렇게 노드마다 읽는 예전 경로로 떨어졌고, breakpoint 형제 폭도 잃었다.
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
    for nested in ["source", "snapshot"] {
        if let Some(inner) = object.get_mut(nested).and_then(Value::as_object_mut)
            && inner
                .get("fileKey")
                .is_some_and(|key| key.as_str().is_none_or(str::is_empty))
        {
            inner.insert("fileKey".to_owned(), Value::String(file_key.to_owned()));
        }
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

/// 노드 없는 metadata 읽기를 원격 `get_metadata` 가 주는 모양 — 최상위 페이지 목록 —
/// 으로 쓴다. 수집기는 두 경로의 답을 같은 파서(`parse_top_level_pages`)로 받는다.
pub(crate) fn wrap_as_top_level_pages(
    data: &Value,
    served: &BridgeServed,
) -> Result<Value, DevupError> {
    let pages = data["nodes"]
        .as_array()
        .ok_or_else(|| unavailable("the plugin's page catalog has no nodes"))?;
    let mut text = String::from("Top-level pages of the document:\n");
    for page in pages {
        let name = page["fields"]["name"].as_str().unwrap_or_default();
        text.push_str(&format!(
            "- {}: {}\n",
            page["id"].as_str().unwrap_or_default(),
            name.replace(['\r', '\n'], " ")
        ));
    }
    let mut result = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "_meta": {},
    });
    result["_meta"][SERVED_META_KEY] = serde_json::to_value(served)
        .map_err(|error| unavailable(format!("bridge result encode failed: {error}")))?;
    Ok(result)
}

/// 참조 PNG 의 답이 브리지 답 하나에 들어가지 않으면 플러그인은 그것을 붙들고 조각으로
/// 내준다. 이어 받아 하나로 되돌린다 — 원격 `get_screenshot` 이 16 MiB 까지 받는 것을
/// 브리지도 받는다.
async fn whole_reference_answer(
    state: &BridgeState,
    file_key: &str,
    data: Value,
) -> Result<Value, DevupError> {
    if data["kind"] != "devupReferencePngChunked" {
        return Ok(data);
    }
    let id = data["id"].as_str().unwrap_or_default().to_owned();
    let length = data["length"].as_u64().unwrap_or(0) as usize;
    if id.is_empty() || length == 0 || length > REFERENCE_ANSWER_LIMIT {
        return Err(unavailable(
            "the plugin's reference PNG answer is too large or unnamed",
        ));
    }
    let mut text = String::with_capacity(length);
    while text.len() < length {
        let (piece, _) = state
            .dispatch(
                file_key,
                "referencePngChunk",
                json!({ "largeValue": { "id": id, "offset": text.len() } }),
            )
            .await?;
        let part = piece["text"].as_str().unwrap_or_default();
        // 붙든 글은 ASCII 라 오프셋이 두 쪽에서 같은 자리를 가리킨다. 아니면 어긋난 것이다.
        if piece["kind"] != "devupReferencePngChunk"
            || piece["id"] != id.as_str()
            || piece["offset"].as_u64() != Some(text.len() as u64)
            || part.is_empty()
            || !part.is_ascii()
            || text.len() + part.len() > length
        {
            return Err(unavailable(
                "the plugin's reference PNG pieces do not line up",
            ));
        }
        text.push_str(part);
    }
    serde_json::from_str(&text)
        .map_err(|error| unavailable(format!("the plugin's reference PNG answer: {error}")))
}

/// 참조 PNG 의 답 전체.
///
/// 플러그인은 큰 답을 읽혀 갈 때까지 붙들어 두고, 그것들이 상한만큼 쌓여 있으면 새 그림을
/// 그리지 않고 바쁘다고 답한다(`DEVUP_REFERENCE_PNG_BUSY`). 붙든 답을 밀어내면 그 답을 읽던
/// 요청이 실패하기 때문이다. 그동안에도 다른 요청의 조각 읽기는 처리되므로 잠시 뒤 다시
/// 묻는다. 붙들어 둔 답이 읽기 전에 사라졌으면(플러그인이 다시 실행되는 등) 한 번 다시 받는다.
async fn reference_answer(
    state: &BridgeState,
    file_key: &str,
    script: &'static str,
    params: Value,
) -> Result<(Value, BridgeServed), DevupError> {
    let deadline = tokio::time::Instant::now() + REFERENCE_BUSY_WAIT;
    let mut fetched_again = false;
    loop {
        let (data, served) = match state.dispatch(file_key, script, params.clone()).await {
            Err(error)
                if plugin_says(&error, "DEVUP_REFERENCE_PNG_BUSY")
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(REFERENCE_BUSY_RETRY).await;
                continue;
            }
            answered => answered?,
        };
        match whole_reference_answer(state, file_key, data).await {
            Err(error)
                if plugin_says(&error, "DEVUP_REFERENCE_PNG_ANSWER_GONE") && !fetched_again =>
            {
                fetched_again = true;
            }
            whole => return whole.map(|data| (data, served)),
        }
    }
}

fn plugin_says(error: &DevupError, code: &str) -> bool {
    is_script_failure(error) && error.message.contains(code)
}

/// 바쁜 플러그인에게 참조 PNG 를 다시 묻는 간격과 그 상한. 상한은 export 작업이 읽기 하나에
/// 주는 90초 안에 그림과 조각 읽기까지 마치도록 잡았다.
const REFERENCE_BUSY_RETRY: std::time::Duration = std::time::Duration::from_millis(500);
const REFERENCE_BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(45);

/// 조각으로 받는 참조 PNG 답의 상한. 이미지 채움의 원본까지 실으므로 PNG 상한(16 MiB)
/// 보다 크다.
const REFERENCE_ANSWER_LIMIT: usize = 96 * 1024 * 1024;

/// 참조 PNG 결과의 `_meta` 에서, 플러그인이 `get_screenshot` 과 같은 그림을 다 그리지
/// 못했다는 이유를 싣는 자리. 원격 경로가 열려 있으면 그쪽이 대신 답한다.
const REFERENCE_PNG_APPROXIMATE_META_KEY: &str = "devup/referencePngApproximate";

/// 원격 경로가 브리지 대신 답했을 때 그 이유를 결과의 `_meta` 에 싣는 자리.
pub(crate) const DIRECT_FALLBACK_META_KEY: &str = "devup/directFallback";

/// 플러그인이 export 한 참조 PNG 를 공식 `get_screenshot` 이 주는 모양 — 이미지
/// 블록 하나 — 으로 쓴다. 수집기는 두 경로의 답을 같은 디코더로 받는다.
///
/// 그림도 같게 맞춘다. 다른 레이어가 닿지 않으면 플러그인은 캔버스가 보여 주는 그
/// 자리를 export 하고 페이지 배경을 알려 주며, 여기서 그 위로 투명한 자리를 덮는다.
/// 닿으면 노드만 export 하고 조상들을 설명하며, 여기서 나머지를 그린다
/// (`reference_compose`). 다 그리지 못한 것이 있으면 그 이유를 `_meta` 에 실어
/// 원격 경로가 대신 답하게 한다.
fn wrap_as_image_result(data: &Value, served: &BridgeServed) -> Result<Value, DevupError> {
    if data["kind"] != "devupReferencePng" || data["mimeType"] != "image/png" {
        return Err(unavailable(
            "the plugin's reference PNG answer is not a devupReferencePng",
        ));
    }
    // 그리다 멈추는 일이 생겨도 함께 온 캔버스 그림이 답이 된다.
    let composed = data["compose"].is_object().then(|| {
        std::panic::catch_unwind(|| crate::reference_compose::compose(&data["compose"]))
            .unwrap_or_else(|_| Err(unavailable("composing it stopped unexpectedly")))
    });
    let mut failure = None;
    let png = match composed {
        Some(Ok(png)) => png,
        failed => {
            failure = failed.and_then(Result::err);
            let png = data["data"].as_str().ok_or_else(|| {
                unavailable("the plugin's reference PNG answer carries no picture")
            })?;
            match named_backdrop(data) {
                Some(backdrop) => crate::collector::flatten_reference_png(png, backdrop)?,
                None => png.to_owned(),
            }
        }
    };
    let mut result = json!({
        "content": [{ "type": "image", "data": png, "mimeType": "image/png" }],
        "isError": false,
        "_meta": {},
    });
    result["_meta"][SERVED_META_KEY] = serde_json::to_value(served)
        .map_err(|error| unavailable(format!("bridge result encode failed: {error}")))?;
    if let Some(note) = data.get("note").and_then(Value::as_str) {
        result["_meta"][REFERENCE_PNG_NOTE_META_KEY] = json!(note);
    }
    if let Some(reason) = data.get("approximate").and_then(Value::as_str) {
        result["_meta"][REFERENCE_PNG_APPROXIMATE_META_KEY] = json!(reason);
    }
    if let Some(error) = failure {
        result["_meta"][REFERENCE_PNG_APPROXIMATE_META_KEY] =
            json!(format!("devup-mcp could not compose it: {}", error.message));
        result["_meta"][REFERENCE_PNG_NOTE_META_KEY] = json!(format!(
            "Drawn as the canvas shows this area: devup-mcp could not compose what get_screenshot draws here ({}). Layers reaching into it may show here: {}; get_screenshot leaves other layers out.",
            error.message,
            data["reaching"].as_str().unwrap_or("other layers")
        ));
    }
    Ok(result)
}

/// 플러그인이 export 한 노드 아래의 색, `[r, g, b]`. 이 값을 보내기 전에 빌드된
/// 플러그인은 보내지 않으며, 그 export 는 받은 그대로 넘긴다.
fn named_backdrop(data: &Value) -> Option<[u8; 3]> {
    let channels = data
        .get("backdrop")?
        .as_array()?
        .iter()
        .map(|value| value.as_u64().and_then(|value| u8::try_from(value).ok()))
        .collect::<Option<Vec<_>>>()?;
    <[u8; 3]>::try_from(channels).ok()
}

/// 플러그인이 붙는 문에 들이는 `Origin`.
///
/// 브라우저 페이지도 `ws://localhost` 에 붙을 수 있다. 그 페이지가 여기서 `hello`
/// 를 보내 플러그인인 척하면 읽기 요청을 받고 지어낸 디자인을 돌려줄 수 있다 — 같은
/// 파일 키로 나중에 붙은 연결이 그 파일의 읽기를 맡으므로 진짜 플러그인을 가로챌
/// 수도 있다. 브라우저는 WebSocket 핸드셰이크에 늘 `Origin` 을 싣고 페이지는 그것을
/// 바꾸지 못한다.
///
/// Figma 플러그인의 UI 는 origin 이 `null` 인 iframe 에서 돈다(Figma 개발자 문서
/// "Making Network Requests": "Plugin iframes have a null origin"). 이 플러그인은
/// iframe 을 다른 주소로 옮기지 않는다. 혹시 Figma 가 제 origin 을 싣는 판이 있어도
/// 끊기지 않도록 figma.com 도 들인다 — 다른 페이지는 그 값을 지어낼 수 없다.
/// `Origin` 이 없는 요청은 브라우저가 아닌 프로그램(테스트, 프로브)이다.
///
/// 한계가 있다: 샌드박스 iframe 이나 `file://` 페이지도 `null` 을 싣는다. 흔한 웹
/// 페이지는 막지만 그런 페이지까지 막지는 못한다.
fn a_plugin_origin(headers: &HeaderMap) -> bool {
    headers.get(ORIGIN).is_none_or(|origin| {
        matches!(
            origin.as_bytes(),
            b"null" | b"https://www.figma.com" | b"https://figma.com"
        )
    })
}

/// 플러그인이 붙는 문.
async fn plugin_socket(
    State(state): State<BridgeState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !a_plugin_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    upgrade.on_upgrade(move |socket| handle_plugin(state, socket))
}

async fn handle_plugin(state: BridgeState, mut socket: WebSocket) {
    let (outbox, mut outbox_rx) = mpsc::unbounded_channel::<String>();
    let id = state.counter.fetch_add(1, Ordering::Relaxed);
    let mut shutdown = state.link.shutdown_signal();

    // 보내기와 받기를 한 루프에서 번갈아 본다. 소켓을 쪼개려면 Stream/Sink 트레이트
    // 의존성이 필요한데, 그것을 들이는 것보다 select 가 싸다.
    loop {
        tokio::select! {
            // 이 프로세스가 브리지를 내려놓는다. 소켓을 닫아야 플러그인이 다음 호스트를 찾는다.
            () = shut_down(&mut shutdown) => break,
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
                        let file_key = value
                            .get("fileKey")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        let plugin = Connected {
                            outbox: outbox.clone(),
                            routing_key: routing_key(&file_key, session_of(&value).as_deref(), id),
                            file_key,
                            file_name: value
                                .get("fileName")
                                .and_then(Value::as_str)
                                .filter(|name| !name.is_empty())
                                .map(str::to_owned),
                            context: serde_json::from_value(value).unwrap_or_default(),
                        };
                        let mut inner = state.inner.lock().await;
                        inner.plugins.insert(id, plugin);
                        state.plugins_changed(&mut inner);
                    }
                    // 페이지를 옮기거나 선택을 바꿀 때마다 온다. url 없는 요청은
                    // 이 값으로 대상을 고르므로, 늦게라도 최신이어야 한다. 중계들도
                    // 같은 선택을 보도록 함께 알린다.
                    Some("context") => {
                        let Ok(context) = serde_json::from_value::<PluginContext>(value) else {
                            continue;
                        };
                        let mut inner = state.inner.lock().await;
                        if let Some(plugin) = inner.plugins.get_mut(&id) {
                            plugin.context = context;
                            state.plugins_changed(&mut inner);
                        }
                    }
                    Some("devup-result") => {
                        let Ok(result) = serde_json::from_value::<PluginResult>(value) else {
                            continue;
                        };
                        let waiting = state
                            .pending
                            .lock()
                            .expect("the pending table is never poisoned")
                            .remove(&result.request_id);
                        // 받는 쪽이 이미 타임아웃했거나 떠났으면 버린다.
                        if let Some(waiting) = waiting {
                            let _ = waiting.waiter.send(result);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let mut inner = state.inner.lock().await;
    if inner.plugins.remove(&id).is_some() {
        state.plugin_left(&mut inner, id);
    }
}

/// 브리지를 여는 데 쓰는 것.
#[derive(Debug, Clone, Default)]
pub struct BridgeOptions {
    /// 이 빌드의 식별자(`devup-mcp --version` 의 괄호 안). 중계 핸드셰이크가 상대에게
    /// 알리고, status 가 포트를 쥔 프로세스를 밝힐 때 쓴다.
    pub build_id: Option<String>,
    /// 중계 비밀값 파일. 비우면 그 사용자만 쓰는 기본 자리다.
    pub secret_path: Option<PathBuf>,
}

/// 중계 핸드셰이크에서 밝히는 devup-mcp 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgePeer {
    pub pid: u32,
    pub version: String,
    #[serde(default)]
    pub build_id: Option<String>,
}

/// 이 프로세스가 브리지에서 맡은 역할.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeRole {
    /// 포트를 쥐었다. 플러그인은 이 프로세스에 붙는다.
    Host,
    /// 다른 devup-mcp 가 쥔 포트를 통해 읽는다.
    Relay,
    /// 누가 포트를 쥐었는지 확인하는 중이다 — 시작할 때, 또는 호스트가 떠난 뒤.
    Connecting,
    /// 지금은 브리지를 쓸 수 없다. 이유는 [`BridgeIssue`] 다.
    Unavailable,
}

/// 브리지를 쓸 수 없는 이유. 고치는 방법이 서로 다르다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeIssue {
    /// 중계를 모르는 예전 devup-mcp 가 포트를 쥐었다. `/plugin` 만 있고 `/relay` 가 없다.
    /// 자신을 밝히지 않으므로 `owner` 는 OS 에 물어 안 것이다.
    LegacyHost { owner: Option<PortOwner> },
    /// devup-mcp 가 아닌 프로그램이 포트를 쥐었다. `owner` 는 OS 에 물어 안 것이다.
    ForeignProgram {
        detail: String,
        owner: Option<PortOwner>,
    },
    /// 다른 판의 중계 규약을 쓰는 devup-mcp 가 쥐었다. 틀린 답을 내느니 잇지 않는다.
    IncompatibleProtocol { theirs: Option<u64>, ours: u64 },
    /// 서로 같은 사용자의 devup-mcp 임을 증명하지 못했다.
    AuthenticationFailed { detail: String },
    /// 중계 비밀값 파일을 읽거나 만들 수 없다.
    SecretUnavailable { detail: String },
    /// 포트를 잡을 수 없는데 그 포트에서 듣는 쪽도 없다.
    BindFailed { detail: String },
}

impl BridgeIssue {
    /// status 가 싣는 짧은 이름.
    pub fn code(&self) -> &'static str {
        match self {
            Self::LegacyHost { .. } => "legacy-host",
            Self::ForeignProgram { .. } => "foreign-program",
            Self::IncompatibleProtocol { .. } => "incompatible-protocol",
            Self::AuthenticationFailed { .. } => "authentication-failed",
            Self::SecretUnavailable { .. } => "secret-unavailable",
            Self::BindFailed { .. } => "bind-failed",
        }
    }
}

/// 브리지 서버.
///
/// 포트를 잡으면 호스트가 되고, 잡지 못하면 그 포트를 쥔 devup-mcp 를 통해 읽는다.
/// 어느 쪽이든 [`BridgeFigmaClient`] 는 같은 방식으로 쓴다.
pub struct BridgeServer {
    state: BridgeState,
    port: u16,
}

impl BridgeServer {
    /// 포트를 잡아 서버를 띄운다.
    ///
    /// 이미 다른 프로세스가 같은 포트를 쓰고 있으면 그쪽을 통해 읽는다. 한 기기에서
    /// MCP 클라이언트를 여러 개 띄우는 것은 정상이고, 플러그인이 붙을 수 있는 포트는
    /// 하나뿐이다. 그 포트를 쥔 쪽이 떠나면 이 프로세스가 이어받을 수 있다.
    ///
    /// 서버가 만들어지는 자리가 동기 함수라 bind 도 동기로 한다 — 듣기 소켓을
    /// 여는 것은 어차피 기다리지 않는 일이고, 기다리는 부분만 따로 띄운다.
    pub fn start(port: u16) -> Option<Self> {
        Self::start_with(port, BridgeOptions::default())
    }

    pub fn start_with(port: u16, options: BridgeOptions) -> Option<Self> {
        // 런타임 밖에서 만들어졌다면 붙일 곳이 없다. 그때도 원격 경로는 멀쩡하다.
        // bind 보다 먼저 본다 — 쓰지도 못할 포트를 잠깐이라도 쥐면 안 된다.
        let runtime = tokio::runtime::Handle::try_current().ok()?;
        let state = BridgeState::new(options);
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                // 0 을 주면 커널이 빈 포트를 고른다. 실제로 잡힌 번호를 알아야 붙을 수 있다.
                let bound = listener.local_addr().ok()?.port();
                state.serve(listener, None).ok()?;
                Some(Self { state, port: bound })
            }
            // 0 은 빈 포트를 달라는 뜻이라, 누가 쥐고 있을 포트가 없다.
            Err(_) if port == 0 => None,
            Err(_) => {
                runtime.spawn(relay::supervise(state.clone(), port));
                Some(Self { state, port })
            }
        }
    }

    /// 환경 설정을 읽어 띄운다.
    ///
    /// `DEVUP_FIGMA_BRIDGE_PORT` 가 포트를 정하며, `0` 이나 `off` 는 끈다. 값이
    /// 없으면 기본 포트로 켠다 — 플러그인이 안 떠 있으면 어차피 원격으로 가므로
    /// 켜 두는 쪽이 손해가 없다.
    pub fn from_env() -> Option<Self> {
        Self::from_env_with(BridgeOptions::default())
    }

    pub fn from_env_with(options: BridgeOptions) -> Option<Self> {
        let setting = std::env::var("DEVUP_FIGMA_BRIDGE_PORT").ok();
        let port = match setting.as_deref().map(str::trim) {
            None | Some("") => DEFAULT_BRIDGE_PORT,
            Some("off") | Some("0") => return None,
            Some(value) => value.parse().ok()?,
        };
        Self::start_with(port, options)
    }

    pub fn state(&self) -> BridgeState {
        self.state.clone()
    }

    /// 실제로 잡은 포트, 또는 이 프로세스가 통해 읽는 포트. `start(0)` 으로 띄웠을 때
    /// 어디에 붙어야 하는지 알려 준다.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// 브리지를 내려놓는다 — 프로세스가 끝날 때처럼 포트와 모든 연결을 닫는다.
    ///
    /// 서버를 버리는 것만으로는 닫지 않는다. 운영에서는 프로세스가 끝날 때까지
    /// 살아 있어야 하고, 이것은 한 프로세스 안에서 호스트가 떠나는 일을 재현한다.
    pub fn shutdown(self) {
        self.state.link.shutdown.send_replace(true);
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
/// 이 값이 있다는 것 자체가 "이 프로세스가 브리지를 쓰려 한다"는 뜻이다.
/// `DEVUP_FIGMA_BRIDGE_PORT=off` 면 브리지 상류가 아예 만들어지지 않으므로 `None`
/// 이 된다. 포트를 잡지 못한 것은 이제 `None` 이 아니다 — 그 포트를 쥔 쪽을 통해
/// 읽거나([`BridgeRole::Relay`]), 왜 그럴 수 없는지를 말한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgePathSnapshot {
    /// 플러그인이 붙는 포트 — 이 프로세스가 잡았든 다른 프로세스가 잡았든.
    /// 플러그인 manifest 의 `allowedDomains` 와 같아야 붙는다.
    pub port: Option<u16>,
    /// 지금 붙어 있는 플러그인, 붙은 순서대로. 포트를 쥔 프로세스 기준이라 어느
    /// 프로세스에서 보든 같다. 파일 키를 보고하지 못한 플러그인은 혼자 붙어 있을
    /// 때만 다른 키의 읽기를 받는다.
    pub attached_files: Vec<AttachedFile>,
    pub role: BridgeRole,
    /// 포트를 쥔 devup-mcp. 호스트면 이 프로세스 자신이다. 모르면 `None` 이다 —
    /// 예전 devup-mcp 는 자신을 밝히지 않는다.
    pub host: Option<BridgePeer>,
    /// [`BridgeRole::Unavailable`] 의 이유.
    pub issue: Option<BridgeIssue>,
    /// 방금 떠난 호스트. 포트가 넘어가는 중이거나, 넘어갔는데 그때 붙어 있던
    /// 플러그인이 아직 다시 붙지 않았다.
    pub handover_from: Option<BridgePeer>,
}

impl BridgePathSnapshot {
    /// 지금 이 경로로 읽기를 보낼 수 있는지 — 플러그인이 보이고, 그것에 닿는 길이 있다.
    pub fn available(&self) -> bool {
        matches!(self.role, BridgeRole::Host | BridgeRole::Relay) && !self.attached_files.is_empty()
    }
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
        let answered = if matches!(call, ReadToolCall::Screenshot { .. }) {
            reference_answer(&self.state, call.file_key(), job.script, params).await
        } else {
            self.state
                .dispatch(call.file_key(), job.script, params)
                .await
        };
        let (mut data, served) = match answered {
            Ok(answered) => answered,
            Err(error) if is_script_failure(&error) => {
                return Ok(script_failure_result(&error.message));
            }
            Err(error) => return Err(error),
        };
        let served = BridgeServed {
            port: self.port,
            ..served
        };
        if matches!(call, ReadToolCall::Screenshot { .. }) {
            // 그리는 일은 픽셀마다 도는 계산이라 비동기 작업자를 붙들지 않게 넘긴다.
            let raw = tokio::task::spawn_blocking(move || wrap_as_image_result(&data, &served))
                .await
                .map_err(|error| {
                    unavailable(format!("composing the reference PNG stopped: {error}"))
                })??;
            return Ok(UpstreamResult { raw });
        }
        if matches!(call, ReadToolCall::Metadata { node_id: None, .. }) {
            return Ok(UpstreamResult {
                raw: wrap_as_top_level_pages(&data, &served)?,
            });
        }
        stamp_file_key(&mut data, call.file_key());
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
        Some(self.state.path_snapshot(self.port).await)
    }

    async fn is_metered(&self, _call: &ReadToolCall) -> bool {
        false
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
/// 물으면 아낀 요금을 도로 쓰게 된다. 예외는 원격이라면 해낼 수 있는 거절
/// (`remote_may_answer`)이다. 원격이 읽을 수 있는 키면 원격에 묻되, 원격도 다시 해 볼
/// 여지 없이 거절하면(로그인이 없다 등) 플러그인의 거절을 올린다 — 한도를 쓰지 않는
/// 처방(플러그인 갱신)을 말하는 쪽이 그것이다.
pub struct FallbackUpstream<P, S> {
    preferred: P,
    secondary: S,
    meter: Option<Arc<dyn Meter>>,
}

/// 한도를 쓰는 경로로 보낼 읽기를 한도 안에 들 때까지 붙든다.
///
/// 원격으로 가는 읽기는 부르는 쪽이 스스로 늦춘다. 플러그인이 답한 읽기를 원격으로 다시
/// 읽을지는 그 판단 뒤 여기서 정해지므로 여기서 이것을 기다린다 — 다만 정해진 시간 안에
/// 자리가 나지 않으면 기다리지 않는다. 기다리는 시간도 읽기 하나에 주어진 시간 안에 들어
/// 있어서, 오래 기다리면 이미 받은 플러그인의 답까지 시간 초과로 잃는다.
#[async_trait]
pub trait Meter: Send + Sync {
    /// `budget` 안에 한도 자리를 얻었으면 `true`. 얻지 못했으면 자리를 쓰지 않는다.
    async fn acquire_within(&self, budget: std::time::Duration) -> bool;
}

/// 읽기 하나에 주어진 시간. export 작업은 이 시간이 지난 읽기를 멈추고 다시 하게 한다.
pub const READ_TIME_LIMIT: std::time::Duration = std::time::Duration::from_secs(90);

/// 원격으로 다시 읽는 일 — 한도 자리를 기다리는 것과 원격의 답까지 — 에 주는 시간의 상한.
const DIRECT_FALLBACK_BUDGET: std::time::Duration = std::time::Duration::from_secs(40);

/// 다시 읽는 일은 플러그인의 읽기와 합쳐 `READ_TIME_LIMIT` 보다 이만큼 일찍 끝낸다. 그래야
/// 시간이 다 됐을 때 이미 받은 플러그인의 답을 돌려줄 수 있다.
const FALLBACK_MARGIN: std::time::Duration = std::time::Duration::from_secs(5);

impl<P, S> FallbackUpstream<P, S> {
    pub fn new(preferred: P, secondary: S) -> Self {
        Self {
            preferred,
            secondary,
            meter: None,
        }
    }

    pub fn with_meter(mut self, meter: Arc<dyn Meter>) -> Self {
        self.meter = Some(meter);
        self
    }

    /// 원격 읽기를 한도 자리를 얻어 `deadline` 안에 끝낸다. 자리를 얻지 못했거나 시간 안에
    /// 끝나지 않았으면 `None`.
    async fn metered<T>(
        &self,
        deadline: tokio::time::Instant,
        read: impl std::future::Future<Output = T>,
    ) -> Option<T> {
        let budget = deadline.saturating_duration_since(tokio::time::Instant::now());
        if budget.is_zero() {
            return None;
        }
        if let Some(meter) = &self.meter
            && !tokio::time::timeout_at(deadline, meter.acquire_within(budget))
                .await
                .unwrap_or(false)
        {
            return None;
        }
        tokio::time::timeout_at(deadline, read).await.ok()
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
        let started = tokio::time::Instant::now();
        if self.preferred.can_serve(&call).await {
            let answered = self.preferred.call_read_tool(call.clone()).await?;
            if is_bridge_only_key(call.file_key()) {
                return Ok(answered);
            }
            let better = remote_does_better(&answered.raw);
            let Some(reason) = better.or_else(|| remote_may_answer(&answered.raw)) else {
                return Ok(answered);
            };
            // 로그인 확인, 한도 자리, 원격의 답이 모두 이 시간 안이다 — `DIRECT_FALLBACK_BUDGET`
            // 안에, 그리고 이 읽기가 시작된 때부터 `READ_TIME_LIMIT - FALLBACK_MARGIN` 안에.
            let deadline = (tokio::time::Instant::now() + DIRECT_FALLBACK_BUDGET)
                .min(started + READ_TIME_LIMIT - FALLBACK_MARGIN);
            let direct =
                match tokio::time::timeout_at(deadline, self.secondary.has_credentials()).await {
                    Ok(false) => return Ok(answered),
                    Ok(true) => {
                        self.metered(deadline, self.secondary.call_read_tool(call))
                            .await
                    }
                    Err(_) => None,
                };
            return match direct {
                // 플러그인의 답을 쓸 수 있으면 그것을 쓴다. 쓸 수 없는 답(거절)이면 한도가
                // 비는 대로 다시 해 볼 일이다.
                None if better.is_some() => Ok(answered),
                None => Err(no_room_on_the_metered_path()),
                Some(Ok(direct)) if better.is_some() && direct_unusable(&direct.raw) => {
                    Ok(answered)
                }
                Some(Ok(mut direct)) => {
                    direct.raw["_meta"][DIRECT_FALLBACK_META_KEY] = json!(reason);
                    Ok(direct)
                }
                Some(Err(error)) if better.is_none() && error.retryable => Err(error),
                Some(Err(_)) => Ok(answered),
            };
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

    /// 브리지가 맡는 읽기는 한도를 쓰지 않는다. 브리지만 읽을 수 있는 키는 원격으로
    /// 가지 않으므로(거절된다) 역시 세지 않는다.
    async fn is_metered(&self, call: &ReadToolCall) -> bool {
        !(is_bridge_only_key(call.file_key()) || self.preferred.can_serve(call).await)
    }
}

/// 플러그인이 답은 했지만 원격 경로가 더 낫게 답할 수 있는 경우와 그 이유. 원격이
/// 어떤 이유로든 답하지 못하면 플러그인의 답을 그대로 쓴다 — 쓸 수 있는 답을 버리고
/// 실패를 올리지 않는다.
///
/// - Figma 가 화면에 보이지 않는 파일의 데이터를 가져오지 않아 읽기의 일부나 전부가
///   "Unable to establish connection to Figma" 로 빠졌다(`figma_unreachable_hint`).
///   원격 경로는 그 상태와 무관하다.
/// - 참조 PNG 에서 플러그인이 `get_screenshot` 과 같은 그림을 다 그리지 못했다.
fn remote_does_better(raw: &Value) -> Option<&'static str> {
    if raw["_meta"][REFERENCE_PNG_APPROXIMATE_META_KEY].is_string() {
        return Some("reference-png-approximate");
    }
    texts(raw)
        .any(|text| {
            crate::figma_unreachable_hint(text).is_some()
                && (raw["isError"] == true || reports_unreachable(text))
        })
        .then_some("file-out-of-sight")
}

/// 플러그인 답의 JSON 에서 오류를 싣는 자리 — 값을 읽지 못한 `$error`, 필드 읽기 실패
/// `fieldErrors`, 미해석 리소스의 `error`, 자산 export 의 `errorDetail` — 에 그 말이 있는지.
/// 디자인의 글(`characters`, 이름, 설명)에 같은 문장이 있어도 한도를 써서 다시 읽지 않는다.
fn reports_unreachable(text: &str) -> bool {
    fn within(value: &Value, in_error: bool) -> bool {
        match value {
            Value::String(text) => in_error && crate::figma_unreachable_hint(text).is_some(),
            Value::Array(items) => items.iter().any(|item| within(item, in_error)),
            Value::Object(fields) => fields.iter().any(|(key, value)| {
                let error_field = matches!(
                    key.as_str(),
                    "$error" | "error" | "errors" | "fieldErrors" | "errorDetail" | "lookupError"
                );
                within(value, in_error || error_field)
            }),
            _ => false,
        }
    }
    serde_json::from_str::<Value>(text).is_ok_and(|value| within(&value, false))
}

/// 플러그인이 답하지 못한 읽기를 원격으로 넘기려 했지만 정해진 시간 안에 한도 자리가 나지
/// 않았다. 한도가 찬 것과 같으므로 그렇게 알려, 수집이 한도가 비기를 기다렸다 다시 하게 한다.
fn no_room_on_the_metered_path() -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaRateLimited,
        "The Devup Bridge plugin could not answer this read, and the metered direct path had no \
         room in its allowance to read it in time. It is retried once the allowance frees up.",
        true,
        json!({ "source": "direct", "stage": "bridge-fallback" }),
    )
}

/// 원격 경로라면 해낼 수 있는 플러그인의 거절과 그 이유:
///
/// - 이 devup-mcp 보다 먼저 빌드돼 스크립트를 모른다 — 읽기는 시도되지 않았다.
/// - 참조 PNG 가 브리지 답 하나(8 MiB)를 넘는다 — 조각으로 내주기 전의 플러그인이다.
/// - 큰 값의 바이트가 읽기가 가리키는 해시와 다르다 — 그 값은 원격 경로의 렌더러가
///   만든 것이라(플러그인이 답하지 못해 원격이 읽은 자산의 이어 읽기), 거기서만 같은
///   바이트가 나온다.
fn remote_may_answer(raw: &Value) -> Option<&'static str> {
    if raw["isError"] != true {
        return None;
    }
    let text = raw["content"][0]["text"].as_str()?;
    [
        ("DEVUP_BRIDGE_UNKNOWN_SCRIPT", "plugin-older-than-server"),
        ("DEVUP_REFERENCE_PNG_TOO_LARGE", "plugin-answer-too-large"),
        ("DEVUP_REFERENCE_PNG_BUSY", "plugin-busy"),
        ("DEVUP_LARGE_VALUE_CHANGED", "value-from-direct-path"),
    ]
    .into_iter()
    .find_map(|(code, reason)| text.contains(code).then_some(reason))
}

/// 원격의 답을 쓸 수 없는지. 그때는 플러그인의 답이 낫다.
///
/// - 오류로 표시됐다.
/// - 한도에 걸렸다는 문장 하나다 — `isError` 없이 오기도 한다(`is_quota_refusal`).
/// - 공식 MCP 의 텍스트 상한(20 KB)에서 잘렸다. 플러그인의 크기로 묶은 읽기를 원격이 다시
///   읽으면 생기고, 그 답은 디코딩되지 않는다.
fn direct_unusable(raw: &Value) -> bool {
    raw["isError"] == true
        || texts(raw).any(|text| {
            crate::is_quota_refusal(text, false)
                || text.trim_end().ends_with("// truncated to 20kb")
        })
}

fn texts(raw: &Value) -> impl Iterator<Item = &str> {
    raw["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
}

/// 브리지만 읽을 수 있는 키의 읽기를 브리지가 맡지 못할 때의 거절.
///
/// 두 경우는 고치는 방법이 다르다. 스크립트 읽기면 플러그인이 끊긴 것이니 다시
/// 띄우면 되고, 그 밖의 읽기(수집이 쓰지 않는 공식 도구 — 변수 정의, 디자인 컨텍스트,
/// Code Connect)는 브리지가 애초에 못 하는 일이라 그 파일의 진짜 Figma 링크가 있어야
/// direct 경로로 읽을 수 있다.
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
