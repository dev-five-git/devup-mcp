//! 포트를 잡지 못한 devup-mcp 가 잡은 devup-mcp 를 통해 플러그인을 쓰는 길.
//!
//! # 왜 이렇게 하나
//!
//! 플러그인은 바꿀 수 없다. 이미 설치된 플러그인은 manifest 의 `allowedDomains`
//! 에 적힌 `ws://localhost:1993` 하나에만 붙고, Figma 는 그 목록을 실행 중에 바꾸지
//! 못한다. 붙으면 `hello` 로 파일·페이지·선택을 알리고, 선택이 바뀔 때마다
//! `context` 를 보내고, 받은 `devup-job` 을 하나씩 순서대로 실행해 `devup-result`
//! 로 답한다. 소켓이 닫히면 2초 뒤 같은 주소로 다시 붙는다(`plugin/src/ui.ts`).
//!
//! 그래서 포트를 잡은 프로세스(호스트)가 플러그인을 받고, 나머지는 호스트에 붙어
//! 읽기를 맡긴다(중계). 호스트가 떠나면 남은 프로세스 가운데 하나가 포트를
//! 이어받고, 플러그인은 제 재시도로 새 호스트에 붙는다.
//!
//! 버린 대안:
//!
//! - **상주 프로세스(데몬)가 포트를 쥔다.** 누가 띄우고 누가 끄는지가 남는다.
//!   마지막 세션이 끝나도 데몬이 남으면 고아가 되고, 클라이언트가 자기 프로세스
//!   트리를 정리하면(Windows job object 등) 그 세션이 띄운 데몬도 함께 죽어 다른
//!   세션들이 한꺼번에 끊긴다. 여기서는 호스트도 그저 한 세션의 devup-mcp 라서,
//!   떠나면 남은 쪽이 이어받는다. 누구도 자기 수명보다 오래 살지 않는다.
//! - **프로세스마다 다른 포트를 쓴다.** 플러그인이 붙을 수 있는 포트는 하나다.
//! - **중계를 다른 포트에 연다.** 호스트를 찾을 곳이 따로 필요해진다. 플러그인이
//!   붙는 그 포트에 문을 하나 더 여는 편이, 이어받기 뒤에도 찾을 곳이 바뀌지 않는다.
//!
//! # 규약
//!
//! 중계는 호스트의 `/relay` 에 WebSocket 으로 붙는다. 호스트가 먼저
//! `relay-hello {protocol, nonce, host}` 로 자신을 밝히고, 중계가
//! `relay-auth {protocol, nonce, proof, peer}` 로 답하고, 호스트가
//! `relay-welcome {proof, files}` 로 받아들인다. 증명은 [`super::secret`] 의 HMAC
//! 이다 — 양쪽이 서로의 증명을 확인해야 이어진다. 판이 다르면(`protocol`) 어느
//! 쪽이든 `relay-refused` 로 거절한다. 틀린 답을 내느니 잇지 않는다.
//!
//! 이어진 뒤에는 중계가 `relay-job {id, fileKey, script, params}` 를 보내고 호스트가
//! `relay-result {id, data | error, served}` 로 답한다. `id` 는 중계가 매기고 그
//! 연결 안에서만 뜻이 있다. 플러그인이 보는 `requestId` 는 호스트가 따로 매기므로
//! 여러 프로세스의 번호가 섞이지 않고, 답은 요청이 온 연결로만 간다. 플러그인이
//! 붙거나 떠나거나 선택이 바뀌면 호스트가 `relay-files` 로 모두에게 같은 목록을
//! 보낸다.
//!
//! 중계가 끊기면 호스트는 그 중계의 읽기를 거둔다. 플러그인에는 취소를 보낼 길이
//! 없어(플러그인은 바꾸지 않는다) 이미 넘긴 작업은 끝까지 돌지만, 그 답은 버려진다.
//! 호스트가 끊기면 중계의 읽기는 기다리지 않고 곧장 실패하고, 중계는 곧바로 포트를
//! 잡아 본다. OS 가 포트를 한 소켓에만 주므로 둘이 동시에 호스트가 되지 않는다 —
//! 진 쪽은 이긴 쪽에 중계로 붙는다.
//!
//! # 누가 쥐었는지
//!
//! 포트를 잡지 못했을 때 그 포트에서 듣는 쪽은 셋 중 하나다.
//!
//! - `/relay` 가 `relay-hello` 로 답하면 이 기능을 아는 devup-mcp 다.
//! - `/relay` 는 404 인데 `/plugin` 이 WebSocket 을 받으면 이 기능 이전의
//!   devup-mcp 다. 고칠 수는 없으므로 그 사실과 할 일(그 클라이언트를 재시작하거나
//!   갱신한다)을 말하고, 포트가 풀리면 이어받는다. `hello` 를 보내지 않으므로 그
//!   프로세스에 플러그인으로 등록되지 않는다.
//! - 그 밖은 devup-mcp 가 아닌 프로그램이다.
//!
//! 어느 쪽이든 답을 기다리는 데 한도가 있다. 대답하지 않는 상대도 몇 초 안에
//! 판정되고, 그 뒤로는 주기적으로 다시 확인한다.

use std::{
    collections::HashMap,
    io,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    extract::{
        State,
        ws::{Message as ServerMessage, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header::ORIGIN},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
    task::JoinSet,
    time::timeout,
};
use tokio_tungstenite::{
    WebSocketStream, client_async,
    tungstenite::{self, Message, client::IntoClientRequest},
};

use super::{
    AttachedFile, BridgeIssue, BridgePeer, BridgeServed, BridgeState, Connected, JOB_TIMEOUT,
    LinkState, PluginContext, Waiting, attached_file,
    secret::{self, RelaySecret, Side},
    shut_down, unavailable,
};
use crate::errors::{DevupError, ErrorCode};

/// 중계 규약의 판. 호스트와 중계는 같은 판일 때만 이어진다.
pub(super) const PROTOCOL: u64 = 1;

/// 포트를 쥔 쪽에 닿고 핸드셰이크를 마치기까지 한 단계마다 기다리는 한도.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// 중계로 붙은 쪽이 증명을 내기까지 호스트가 기다리는 한도. 말없이 붙어 있는
/// 연결은 이 뒤에 끊는다.
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);

/// 브리지를 쓸 수 없는 동안 포트를 다시 확인하는 간격.
const RETRY_INTERVAL: Duration = Duration::from_secs(2);

/// 호스트가 떠난 직후에는 이 동안 촘촘히 다시 시도하고, 실패를 보고하지 않는다.
/// 떠나는 프로세스의 소켓이 모두 닫히기까지의 짧은 틈을 "다른 프로그램"으로
/// 오판하지 않기 위해서다.
const TAKEOVER_WINDOW: Duration = Duration::from_secs(3);
const QUICK_RETRY: Duration = Duration::from_millis(50);

/// 포트를 잡을 수 없는데 아무도 듣지 않는 상태를 몇 번까지 경합으로 볼지.
const VACANT_RETRIES: u32 = 10;

/// 호스트의 90초 제한이 먼저 끝나 그 이유가 전해지도록 중계는 조금 더 기다린다.
const RELAY_SLACK: Duration = Duration::from_secs(5);

type ClientSocket = WebSocketStream<TcpStream>;

/// 호스트가 중계에 보내는 플러그인 하나.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireFile {
    connection: u64,
    #[serde(default)]
    file_key: Option<String>,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(flatten)]
    context: PluginContext,
}

fn wire_files(plugins: &HashMap<u64, Connected>) -> Vec<WireFile> {
    let mut ids: Vec<u64> = plugins.keys().copied().collect();
    ids.sort_unstable();
    ids.into_iter()
        .map(|id| {
            let plugin = &plugins[&id];
            WireFile {
                connection: id,
                file_key: plugin.reported_key(),
                file_name: plugin.file_name.clone(),
                context: plugin.context.clone(),
            }
        })
        .collect()
}

pub(super) fn files_message(plugins: &HashMap<u64, Connected>) -> String {
    json!({ "kind": "relay-files", "files": wire_files(plugins) }).to_string()
}

fn files_from(message: &Value) -> Vec<(u64, AttachedFile)> {
    message
        .get("files")
        .cloned()
        .and_then(|files| serde_json::from_value::<Vec<WireFile>>(files).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|file| {
            (
                file.connection,
                attached_file(file.connection, file.file_key, file.file_name, file.context),
            )
        })
        .collect()
}

fn kind(message: &Value) -> Option<&str> {
    message.get("kind").and_then(Value::as_str)
}

fn text_field<'a>(message: &'a Value, field: &str) -> Option<&'a str> {
    message.get(field).and_then(Value::as_str)
}

// ---------------------------------------------------------------------------
// 호스트 쪽
// ---------------------------------------------------------------------------

/// 중계 문.
///
/// 브라우저는 WebSocket 핸드셰이크에 늘 `Origin` 을 싣고, 페이지는 그 헤더를 지울 수
/// 없다. devup-mcp 는 싣지 않는다. 그래서 `Origin` 이 있으면 업그레이드 전에
/// 거절한다 — 로컬 웹 페이지가 MCP 클라이언트인 척 열린 문서를 읽는 길을 막는다.
pub(super) async fn relay_socket(
    State(state): State<BridgeState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if headers.contains_key(ORIGIN) {
        return StatusCode::FORBIDDEN.into_response();
    }
    upgrade.on_upgrade(move |socket| serve_relay(state, socket))
}

async fn next_server_json(socket: &mut WebSocket) -> Option<Value> {
    while let Some(message) = socket.recv().await {
        match message.ok()? {
            ServerMessage::Text(text) => return serde_json::from_str(&text).ok(),
            ServerMessage::Close(_) => return None,
            _ => {}
        }
    }
    None
}

async fn send_server(socket: &mut WebSocket, message: &Value) -> bool {
    socket
        .send(ServerMessage::Text(message.to_string().into()))
        .await
        .is_ok()
}

/// 중계로 들일지. 들이면 상대의 nonce 와 이쪽의 증명을, 아니면 거절 사유를 준다.
///
/// 사유는 상대에게 가는 짧은 낱말이다. 비밀값이나 증명은 싣지 않는다.
async fn admit(
    state: &BridgeState,
    host_nonce: &str,
    answer: &Value,
) -> Result<String, &'static str> {
    if kind(answer) != Some("relay-auth") {
        return Err("auth");
    }
    if answer.get("protocol").and_then(Value::as_u64) != Some(PROTOCOL) {
        return Err("protocol");
    }
    let (Some(relay_nonce), Some(proof)) =
        (text_field(answer, "nonce"), text_field(answer, "proof"))
    else {
        return Err("auth");
    };
    let Some(path) = state.link.secret_path.as_deref() else {
        return Err("secret");
    };
    let Ok(secret) = secret::load_or_create(path).await else {
        return Err("secret");
    };
    if !secret.verifies(Side::Relay, host_nonce, relay_nonce, proof) {
        return Err("auth");
    }
    Ok(secret.proof(Side::Host, host_nonce, relay_nonce))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayJob {
    id: u64,
    file_key: String,
    script: String,
    #[serde(default)]
    params: Value,
}

async fn serve_relay(state: BridgeState, mut socket: WebSocket) {
    let mut shutdown = state.link.shutdown_signal();
    let nonce = secret::nonce();
    let hello = json!({
        "kind": "relay-hello",
        "protocol": PROTOCOL,
        "nonce": nonce,
        "host": state.link.me,
    });
    if !send_server(&mut socket, &hello).await {
        return;
    }
    let answer = tokio::select! {
        answer = timeout(AUTH_TIMEOUT, next_server_json(&mut socket)) => answer.ok().flatten(),
        () = shut_down(&mut shutdown) => return,
    };
    // 말이 없거나 알아볼 수 없는 말을 한 상대는 그냥 끊는다.
    let Some(answer) = answer else { return };
    let proof = match admit(&state, &nonce, &answer).await {
        Ok(proof) => proof,
        Err(reason) => {
            let refused =
                json!({ "kind": "relay-refused", "reason": reason, "protocol": PROTOCOL });
            let _ = send_server(&mut socket, &refused).await;
            let _ = socket.send(ServerMessage::Close(None)).await;
            return;
        }
    };

    let (outbox, mut outbox_rx) = mpsc::unbounded_channel::<String>();
    let relay = state.counter.fetch_add(1, Ordering::Relaxed);
    {
        // 환영과 등록을 한 잠금 안에서 한다. 그 사이에 플러그인이 바뀌어도 이 중계는
        // 바뀐 목록을 환영 뒤에 받는다.
        let mut inner = state.inner.lock().await;
        let welcome =
            json!({ "kind": "relay-welcome", "proof": proof, "files": wire_files(&inner.plugins) });
        let _ = outbox.send(welcome.to_string());
        inner.relays.insert(relay, outbox.clone());
    }

    let mut reads = JoinSet::new();
    loop {
        tokio::select! {
            outgoing = outbox_rx.recv() => {
                let Some(text) = outgoing else { break };
                if socket.send(ServerMessage::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                let ServerMessage::Text(text) = message else { continue };
                let Ok(message) = serde_json::from_str::<Value>(&text) else { continue };
                if kind(&message) != Some("relay-job") {
                    continue;
                }
                let Ok(job) = serde_json::from_value::<RelayJob>(message) else { continue };
                let (state, outbox) = (state.clone(), outbox.clone());
                reads.spawn(async move {
                    let answer = match state.dispatch_local(&job.file_key, &job.script, job.params).await {
                        Ok((data, served)) => json!({
                            "kind": "relay-result", "id": job.id, "data": data, "served": served,
                        }),
                        Err(error) => json!({
                            "kind": "relay-result", "id": job.id, "error": error.message,
                        }),
                    };
                    let _ = outbox.send(answer.to_string());
                });
            }
            Some(_) = reads.join_next(), if !reads.is_empty() => {}
            () = shut_down(&mut shutdown) => break,
        }
    }
    state.inner.lock().await.relays.remove(&relay);
    // 요청한 프로세스가 떠났다. 그 프로세스의 읽기를 모두 거둔다 — 취소된 읽기의
    // 대기표는 그 Drop 에서 걷힌다.
    reads.abort_all();
}

// ---------------------------------------------------------------------------
// 중계 쪽
// ---------------------------------------------------------------------------

enum Answer {
    Data(Value, BridgeServed),
    Failed(String),
}

#[derive(Deserialize)]
struct RelayResult {
    id: u64,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    served: Option<BridgeServed>,
}

/// 이 프로세스가 통해 읽는 호스트 하나와의 연결.
pub(crate) struct RelayConnection {
    pub(crate) host: BridgePeer,
    outbox: mpsc::UnboundedSender<String>,
    pending: StdMutex<HashMap<u64, oneshot::Sender<Answer>>>,
    next: AtomicU64,
}

impl RelayConnection {
    fn gone(&self) -> DevupError {
        unavailable(format!(
            "the devup-mcp holding the Devup Bridge port (pid {}) went away mid-read. Another \
             process takes the port over and the plugin reconnects within seconds; repeat the call.",
            self.host.pid
        ))
    }

    pub(crate) async fn dispatch(
        &self,
        file_key: &str,
        script: &str,
        params: Value,
    ) -> Result<(Value, BridgeServed), DevupError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .expect("the pending table is never poisoned")
            .insert(id, tx);
        let _waiting = Waiting {
            pending: &self.pending,
            key: id,
        };
        let job = json!({
            "kind": "relay-job", "id": id, "fileKey": file_key, "script": script, "params": params,
        });
        if self.outbox.send(job.to_string()).is_err() {
            return Err(self.gone());
        }
        match timeout(JOB_TIMEOUT + RELAY_SLACK, rx).await {
            Ok(Ok(Answer::Data(data, served))) => Ok((data, served)),
            // 호스트가 붙인 문구를 그대로 올린다. 스크립트가 던진 DEVUP_* 코드도
            // 그 안에 있고, 위쪽 분기는 그 문자열로 판정한다.
            Ok(Ok(Answer::Failed(message))) => Err(DevupError::new(
                ErrorCode::DevupFigmaDirectUnavailable,
                message,
                false,
            )),
            Ok(Err(_)) => Err(self.gone()),
            Err(_) => Err(unavailable(
                "the Devup Bridge plugin did not answer in time",
            )),
        }
    }

    fn answer(&self, result: RelayResult) {
        let Some(waiting) = self
            .pending
            .lock()
            .expect("the pending table is never poisoned")
            .remove(&result.id)
        else {
            return;
        };
        let answer = match (result.data, result.error) {
            (_, Some(error)) => Answer::Failed(error),
            (Some(data), None) => Answer::Data(data, result.served.unwrap_or_default()),
            (None, None) => Answer::Failed("bridge returned neither data nor error".to_owned()),
        };
        let _ = waiting.send(answer);
    }
}

struct Joined {
    connection: Arc<RelayConnection>,
    files: Vec<(u64, AttachedFile)>,
    socket: ClientSocket,
    outbox: mpsc::UnboundedReceiver<String>,
}

enum Probe {
    Joined(Box<Joined>),
    /// 아무도 듣고 있지 않다 — 포트를 쥔 쪽이 막 떠났다.
    Vacant,
    /// 누군가 쥐었는데 그를 통해 읽을 수 없다.
    Refused {
        issue: BridgeIssue,
        holder: Option<BridgePeer>,
    },
}

fn foreign(detail: impl Into<String>) -> Probe {
    Probe::Refused {
        issue: BridgeIssue::ForeignProgram {
            detail: detail.into(),
        },
        holder: None,
    }
}

enum Unopened {
    Vacant,
    NotFound,
    Other(String),
}

/// 포트에서 듣는 쪽의 `path` 에 WebSocket 을 연다.
async fn open(port: u16, path: &str) -> Result<ClientSocket, Unopened> {
    let seconds = PROBE_TIMEOUT.as_secs();
    let stream = match timeout(PROBE_TIMEOUT, TcpStream::connect(("127.0.0.1", port))).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionRefused => {
            return Err(Unopened::Vacant);
        }
        Ok(Err(error)) => {
            return Err(Unopened::Other(format!(
                "connecting to it failed ({error})"
            )));
        }
        Err(_) => {
            return Err(Unopened::Other(format!(
                "it did not accept a connection within {seconds}s"
            )));
        }
    };
    let request = format!("ws://127.0.0.1:{port}{path}")
        .into_client_request()
        .map_err(|error| Unopened::Other(error.to_string()))?;
    match timeout(PROBE_TIMEOUT, client_async(request, stream)).await {
        Ok(Ok((socket, _))) => Ok(socket),
        Ok(Err(tungstenite::Error::Http(response))) if response.status().as_u16() == 404 => {
            Err(Unopened::NotFound)
        }
        Ok(Err(tungstenite::Error::Http(response))) => Err(Unopened::Other(format!(
            "it answered a WebSocket handshake on {path} with HTTP {}",
            response.status().as_u16()
        ))),
        Ok(Err(error)) => Err(Unopened::Other(format!(
            "it does not speak WebSocket on {path} ({error})"
        ))),
        Err(_) => Err(Unopened::Other(format!(
            "it did not answer a WebSocket handshake within {seconds}s"
        ))),
    }
}

async fn next_client_json(socket: &mut ClientSocket) -> Option<Value> {
    while let Some(message) = socket.next().await {
        match message.ok()? {
            Message::Text(text) => return serde_json::from_str(&text).ok(),
            Message::Close(_) => return None,
            _ => {}
        }
    }
    None
}

async fn send_client(socket: &mut ClientSocket, message: &Value) -> bool {
    socket
        .send(Message::Text(message.to_string().into()))
        .await
        .is_ok()
}

/// `/relay` 가 없는 쪽이 예전 devup-mcp 인지. `/plugin` 이 WebSocket 을 받으면 그렇다.
///
/// `hello` 를 보내지 않으니 그 프로세스에 플러그인으로 등록되지 않는다. 곧장 닫는다.
async fn legacy_or_foreign(port: u16) -> Probe {
    match open(port, "/plugin").await {
        Ok(mut socket) => {
            let _ = socket.close(None).await;
            Probe::Refused {
                issue: BridgeIssue::LegacyHost,
                holder: None,
            }
        }
        Err(Unopened::Vacant) => Probe::Vacant,
        Err(_) => {
            foreign("it answers HTTP on the port but has neither of devup-mcp's bridge endpoints")
        }
    }
}

/// 포트를 쥔 쪽에 중계로 붙는다.
async fn join(state: &BridgeState, port: u16) -> Probe {
    let mut socket = match open(port, "/relay").await {
        Ok(socket) => socket,
        Err(Unopened::Vacant) => return Probe::Vacant,
        Err(Unopened::NotFound) => return legacy_or_foreign(port).await,
        Err(Unopened::Other(detail)) => return foreign(detail),
    };
    let Some(hello) = timeout(PROBE_TIMEOUT, next_client_json(&mut socket))
        .await
        .ok()
        .flatten()
    else {
        return foreign("it accepted a relay connection but never introduced itself");
    };
    if kind(&hello) != Some("relay-hello") {
        return foreign("it answered the relay handshake with something else");
    }
    let holder = hello
        .get("host")
        .cloned()
        .and_then(|host| serde_json::from_value::<BridgePeer>(host).ok());
    let theirs = hello.get("protocol").and_then(Value::as_u64);
    if theirs != Some(PROTOCOL) {
        let refused =
            json!({ "kind": "relay-refused", "reason": "protocol", "protocol": PROTOCOL });
        let _ = send_client(&mut socket, &refused).await;
        let _ = socket.close(None).await;
        return Probe::Refused {
            issue: BridgeIssue::IncompatibleProtocol {
                theirs,
                ours: PROTOCOL,
            },
            holder,
        };
    }
    let Some(host_nonce) = text_field(&hello, "nonce").map(str::to_owned) else {
        return foreign("its relay greeting carried no challenge");
    };
    let secret = match load_secret(state).await {
        Ok(secret) => secret,
        Err(detail) => {
            return Probe::Refused {
                issue: BridgeIssue::SecretUnavailable { detail },
                holder,
            };
        }
    };
    let nonce = secret::nonce();
    let auth = json!({
        "kind": "relay-auth",
        "protocol": PROTOCOL,
        "nonce": nonce,
        "proof": secret.proof(Side::Relay, &host_nonce, &nonce),
        "peer": state.link.me,
    });
    if !send_client(&mut socket, &auth).await {
        return foreign("it closed the relay connection during the handshake");
    }
    let Some(reply) = timeout(PROBE_TIMEOUT, next_client_json(&mut socket))
        .await
        .ok()
        .flatten()
    else {
        return Probe::Refused {
            issue: BridgeIssue::AuthenticationFailed {
                detail: "it did not answer this process's proof".to_owned(),
            },
            holder,
        };
    };
    match kind(&reply) {
        Some("relay-welcome") => {
            let proven = text_field(&reply, "proof")
                .is_some_and(|proof| secret.verifies(Side::Host, &host_nonce, &nonce, proof));
            if !proven {
                let _ = socket.close(None).await;
                return Probe::Refused {
                    issue: BridgeIssue::AuthenticationFailed {
                        detail: "it could not prove it holds this user's relay secret".to_owned(),
                    },
                    holder,
                };
            }
            let Some(host) = holder else {
                return foreign("it did not say which process it is");
            };
            let (outbox, outbox_rx) = mpsc::unbounded_channel();
            Probe::Joined(Box::new(Joined {
                connection: Arc::new(RelayConnection {
                    host,
                    outbox,
                    pending: StdMutex::default(),
                    next: AtomicU64::new(1),
                }),
                files: files_from(&reply),
                socket,
                outbox: outbox_rx,
            }))
        }
        Some("relay-refused") => {
            let issue = match text_field(&reply, "reason") {
                Some("protocol") => BridgeIssue::IncompatibleProtocol {
                    theirs: reply.get("protocol").and_then(Value::as_u64),
                    ours: PROTOCOL,
                },
                Some("secret") => BridgeIssue::AuthenticationFailed {
                    detail: "it could not read its own relay secret".to_owned(),
                },
                _ => BridgeIssue::AuthenticationFailed {
                    detail: "it did not accept this process's proof, so the two do not share \
                             this user's relay secret"
                        .to_owned(),
                },
            };
            Probe::Refused { issue, holder }
        }
        _ => foreign("it answered the relay handshake with something else"),
    }
}

async fn load_secret(state: &BridgeState) -> Result<RelaySecret, String> {
    let Some(path) = state.link.secret_path.as_deref() else {
        return Err(
            "there is no per-user directory to keep it in (USERPROFILE or HOME is not set)"
                .to_owned(),
        );
    };
    secret::load_or_create(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// 호스트와의 연결을 끝날 때까지 돌린다. 끝나면 이 프로세스는 다시 포트를 찾는다.
///
/// 끝날 때 플러그인이 보였는지를 돌려준다. 보였다면 그 플러그인은 곧 새 호스트에
/// 다시 붙는다.
async fn drive(state: &BridgeState, joined: Joined, expecting: Option<BridgePeer>) -> bool {
    let Joined {
        connection,
        files,
        mut socket,
        mut outbox,
    } = joined;
    state.connected.store(files.len(), Ordering::Relaxed);
    state.link.set(LinkState::Relay {
        connection: connection.clone(),
        files,
        expecting,
        since: Instant::now(),
    });
    let mut shutdown = state.link.shutdown_signal();
    loop {
        tokio::select! {
            outgoing = outbox.recv() => {
                let Some(text) = outgoing else { break };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.next() => {
                let Some(Ok(message)) = incoming else { break };
                let text = match message {
                    Message::Text(text) => text,
                    Message::Close(_) => break,
                    _ => continue,
                };
                let Ok(message) = serde_json::from_str::<Value>(&text) else { continue };
                match kind(&message) {
                    Some("relay-result") => {
                        if let Ok(result) = serde_json::from_value::<RelayResult>(message) {
                            connection.answer(result);
                        }
                    }
                    Some("relay-files") => {
                        let files = files_from(&message);
                        state.connected.store(files.len(), Ordering::Relaxed);
                        state.link.state.send_modify(|link| {
                            if let LinkState::Relay { files: mirror, .. } = link {
                                *mirror = files;
                            }
                        });
                    }
                    _ => {}
                }
            }
            () = shut_down(&mut shutdown) => break,
        }
    }
    let had_plugins = state.connected.swap(0, Ordering::Relaxed) > 0;
    state.link.set(LinkState::Connecting {
        after: Some(connection.host.clone()),
    });
    // 기다리던 읽기는 모두 "호스트가 떠났다"로 곧장 끝난다. 90초를 기다리지 않는다.
    connection
        .pending
        .lock()
        .expect("the pending table is never poisoned")
        .clear();
    had_plugins
}

/// 포트를 잡지 못한 프로세스의 일: 포트를 쥔 쪽을 통해 읽고, 그가 떠나면 이어받는다.
pub(super) async fn supervise(state: BridgeState, port: u16) {
    let mut shutdown = state.link.shutdown_signal();
    // 방금 떠난 호스트에 플러그인이 붙어 있었으면, 그 플러그인은 곧 다시 붙는다.
    let mut expecting: Option<BridgePeer> = None;
    let mut takeover_until: Option<Instant> = None;
    let mut vacant = 0_u32;
    loop {
        if state.link.is_shut_down() {
            return;
        }
        // 포트가 풀렸으면 잡는다. OS 가 한 소켓에만 주므로 경합에서 둘이 함께 이기지 않는다.
        let bind_error = match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => match state.serve(listener, expecting.clone()) {
                Ok(()) => return,
                Err(error) => error,
            },
            Err(error) => error,
        };
        let probe = tokio::select! {
            probe = join(&state, port) => probe,
            () = shut_down(&mut shutdown) => return,
        };
        let taking_over = takeover_until.is_some_and(|until| Instant::now() < until);
        let pause = match probe {
            Probe::Joined(joined) => {
                vacant = 0;
                let host = joined.connection.host.clone();
                let had_plugins = drive(&state, *joined, expecting.take()).await;
                expecting = had_plugins.then_some(host);
                takeover_until = Some(Instant::now() + TAKEOVER_WINDOW);
                continue;
            }
            Probe::Vacant if taking_over || vacant < VACANT_RETRIES => {
                vacant += 1;
                QUICK_RETRY
            }
            Probe::Vacant => {
                state.link.set(LinkState::Unavailable {
                    issue: BridgeIssue::BindFailed {
                        detail: bind_error.to_string(),
                    },
                    holder: None,
                });
                RETRY_INTERVAL
            }
            Probe::Refused { .. } if taking_over => QUICK_RETRY,
            Probe::Refused { issue, holder } => {
                vacant = 0;
                state.link.set(LinkState::Unavailable { issue, holder });
                RETRY_INTERVAL
            }
        };
        tokio::select! {
            () = tokio::time::sleep(pause) => {}
            () = shut_down(&mut shutdown) => return,
        }
    }
}
