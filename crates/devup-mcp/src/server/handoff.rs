use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use devup_mcp_devup_ui::codegen::RootLayout;
use devup_mcp_figma::{
    CollectedParts, CollectorSession, CollectorStep, DevupError, ErrorCode, UpstreamResult,
};
use rand::Rng;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingOperation {
    Collect,
    ToUi {
        component_name: Option<String>,
        include_diagnostics: bool,
        root_layout: RootLayout,
        output_path: Option<String>,
    },
    ToJson {
        scope: String,
        include_diagnostics: bool,
        output_path: Option<String>,
    },
    Search {
        query: String,
        node_types: Vec<String>,
        match_kind: String,
        limit: usize,
    },
    Explore {
        limit: usize,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffCall {
    pub call_id: String,
    pub server: &'static str,
    pub tool: &'static str,
    pub arguments: Value,
}

#[derive(Debug)]
pub enum HandoffStep {
    NeedsFigma {
        session_id: String,
        expires_at_epoch_seconds: u64,
        calls: Vec<HandoffCall>,
    },
    Complete {
        operation: PendingOperation,
        parts: Box<CollectedParts>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct HandoffLimits {
    pub ttl: Duration,
    pub max_sessions: usize,
    pub max_result_bytes: usize,
    pub max_total_bytes: usize,
}

impl Default for HandoffLimits {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(10 * 60),
            max_sessions: 8,
            max_result_bytes: 16 * 1024 * 1024,
            max_total_bytes: 64 * 1024 * 1024,
        }
    }
}

pub trait Clock: Send + Sync {
    fn now_epoch_seconds(&self) -> u64;
}

#[derive(Debug)]
struct SystemClock;

impl Clock for SystemClock {
    fn now_epoch_seconds(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

struct Session {
    operation: PendingOperation,
    collector: CollectorSession,
    expires_at: u64,
    result_bytes: usize,
    pending: BTreeMap<String, (String, HandoffCall)>,
}

#[derive(Default)]
struct StoreState {
    sessions: BTreeMap<String, Session>,
    total_result_bytes: usize,
}

#[derive(Clone)]
pub struct HandoffStore {
    state: Arc<Mutex<StoreState>>,
    clock: Arc<dyn Clock>,
    limits: HandoffLimits,
}

impl Default for HandoffStore {
    fn default() -> Self {
        Self::with_limits(HandoffLimits::default())
    }
}

impl HandoffStore {
    pub fn with_limits(limits: HandoffLimits) -> Self {
        Self::with_clock(Arc::new(SystemClock), limits)
    }

    pub fn with_clock(clock: Arc<dyn Clock>, limits: HandoffLimits) -> Self {
        Self {
            state: Arc::new(Mutex::new(StoreState::default())),
            clock,
            limits,
        }
    }

    pub async fn begin(
        &self,
        operation: PendingOperation,
        collector: CollectorSession,
    ) -> Result<String, DevupError> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        prune_expired(&mut state, now);
        if state.sessions.len() >= self.limits.max_sessions {
            return Err(too_large(
                "동시에 유지할 수 있는 Figma handoff session 수를 초과했습니다.",
            ));
        }
        let session_id = unique_id(&state.sessions);
        state.sessions.insert(
            session_id.clone(),
            Session {
                operation,
                collector,
                expires_at: now.saturating_add(self.limits.ttl.as_secs()),
                result_bytes: 0,
                pending: BTreeMap::new(),
            },
        );
        Ok(session_id)
    }

    pub async fn next(&self, session_id: &str) -> Result<HandoffStep, DevupError> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        let mut session = take_session(&mut state, session_id, now)?;

        loop {
            match session.collector.advance() {
                Ok(CollectorStep::Call(planned)) => {
                    let call_id = random_id();
                    let handoff_call = HandoffCall {
                        call_id: call_id.clone(),
                        server: "figma",
                        tool: planned.call.tool_name(),
                        arguments: Value::Object(planned.call.arguments()),
                    };
                    session.pending.insert(call_id, (planned.id, handoff_call));
                }
                Ok(CollectorStep::AwaitingResults) => {
                    let calls = session
                        .pending
                        .values()
                        .map(|(_, call)| call.clone())
                        .collect();
                    let expires_at_epoch_seconds = session.expires_at;
                    put_session(&mut state, session_id.to_owned(), session);
                    return Ok(HandoffStep::NeedsFigma {
                        session_id: session_id.to_owned(),
                        expires_at_epoch_seconds,
                        calls,
                    });
                }
                Ok(CollectorStep::Complete(parts)) => {
                    return Ok(HandoffStep::Complete {
                        operation: session.operation,
                        parts,
                    });
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub async fn accept(
        &self,
        session_id: &str,
        call_id: &str,
        result: Value,
    ) -> Result<(), DevupError> {
        let encoded_len = serde_json::to_vec(&result)
            .map_err(|_| invalid("Figma handoff result를 JSON으로 읽을 수 없습니다."))?
            .len();
        if encoded_len > self.limits.max_result_bytes {
            self.remove(session_id).await;
            return Err(too_large(
                "Figma handoff result의 허용 크기를 초과했습니다.",
            ));
        }

        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        if state.total_result_bytes.saturating_add(encoded_len) > self.limits.max_total_bytes {
            if let Some(session) = state.sessions.remove(session_id) {
                state.total_result_bytes = state
                    .total_result_bytes
                    .saturating_sub(session.result_bytes);
            }
            return Err(too_large(
                "Figma handoff result의 전체 메모리 한도를 초과했습니다.",
            ));
        }
        let mut session = take_session(&mut state, session_id, now)?;
        let Some((collector_call_id, _)) = session.pending.remove(call_id) else {
            return Err(invalid(
                "알 수 없거나 이미 처리한 Figma handoff call ID입니다.",
            ));
        };
        session
            .collector
            .accept(&collector_call_id, UpstreamResult { raw: result })?;
        session.result_bytes = session.result_bytes.saturating_add(encoded_len);
        put_session(&mut state, session_id.to_owned(), session);
        Ok(())
    }

    pub async fn remove(&self, session_id: &str) {
        let mut state = self.state.lock().await;
        if let Some(session) = state.sessions.remove(session_id) {
            state.total_result_bytes = state
                .total_result_bytes
                .saturating_sub(session.result_bytes);
        }
    }
}

fn take_session(state: &mut StoreState, session_id: &str, now: u64) -> Result<Session, DevupError> {
    let Some(session) = state.sessions.remove(session_id) else {
        return Err(invalid("존재하지 않는 Figma handoff session입니다."));
    };
    state.total_result_bytes = state
        .total_result_bytes
        .saturating_sub(session.result_bytes);
    if session.expires_at <= now {
        return Err(DevupError::new(
            ErrorCode::DevupFigmaHandoffExpired,
            "Figma handoff session이 만료되었습니다.",
            true,
        ));
    }
    Ok(session)
}

fn put_session(state: &mut StoreState, session_id: String, session: Session) {
    state.total_result_bytes = state
        .total_result_bytes
        .saturating_add(session.result_bytes);
    state.sessions.insert(session_id, session);
}

fn prune_expired(state: &mut StoreState, now: u64) {
    let expired = state
        .sessions
        .iter()
        .filter_map(|(id, session)| (session.expires_at <= now).then_some(id.clone()))
        .collect::<Vec<_>>();
    for id in expired {
        if let Some(session) = state.sessions.remove(&id) {
            state.total_result_bytes = state
                .total_result_bytes
                .saturating_sub(session.result_bytes);
        }
    }
}

fn unique_id(sessions: &BTreeMap<String, Session>) -> String {
    loop {
        let id = random_id();
        if !sessions.contains_key(&id) {
            return id;
        }
    }
}

fn random_id() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn invalid(message: &str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaHandoffInvalid,
        message,
        false,
        json!({"source": "host"}),
    )
}

fn too_large(message: &str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaResponseTooLarge,
        message,
        false,
        json!({"source": "host"}),
    )
}
