use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use devup_mcp_devup_ui::codegen::RootLayout;
use devup_mcp_figma::{
    CollectedParts, CollectionStats, CollectorSession, CollectorStep, DevupError, ErrorCode,
    UpstreamResult,
};
use rand::Rng;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::{artifacts::ArtifactRequestKey, delivery::DeliveryMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingOperation {
    Collect,
    Artifact {
        operation: Box<PendingOperation>,
        artifact_key: ArtifactRequestKey,
    },
    ToUi {
        component_name: Option<String>,
        include_diagnostics: bool,
        root_layout: RootLayout,
        output_path: Option<String>,
        delivery: DeliveryMode,
    },
    ToJson {
        scope: String,
        include_diagnostics: bool,
        output_path: Option<String>,
        delivery: DeliveryMode,
    },
    Export {
        outputs: Vec<String>,
        component_name: Option<String>,
        include_diagnostics: bool,
        root_layout: RootLayout,
        scope: String,
        strict: bool,
        output_paths: BTreeMap<String, String>,
        frame_ids: Vec<String>,
        all_screens: bool,
        asset_captures: Vec<devup_mcp_figma::AssetSelection>,
        asset_output_paths: BTreeMap<String, String>,
        delivery: DeliveryMode,
    },
    Search {
        query: String,
        node_types: Vec<String>,
        match_kind: String,
        limit: usize,
    },
    Explore {
        limit: usize,
        target: devup_mcp_figma::FigmaTarget,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffCall {
    pub call_id: String,
    pub server: &'static str,
    pub tool: &'static str,
    pub arguments: Value,
    /// The Figma node this call targets, tracked outside `arguments` because
    /// the official `use_figma` schema forbids a `nodeId` argument
    /// (`additionalProperties: false`). Absent for calls with no single
    /// target node (e.g. the file-wide page catalog).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug)]
pub enum HandoffStep {
    NeedsFigma {
        session_id: String,
        expires_at_epoch_seconds: u64,
        calls: Vec<HandoffCall>,
        collection: CollectionStats,
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
    consumed: BTreeSet<String>,
}

#[derive(Default)]
struct StoreState {
    sessions: BTreeMap<String, Session>,
    tombstones: BTreeMap<String, SessionTombstone>,
    total_result_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
struct SessionTombstone {
    expires_at: u64,
}

const MAX_TOMBSTONES: usize = 64;
const GET_METADATA_RESULT_TAIL: &str = "IMPORTANT: After you call this tool, you MUST call get_design_context if trying to implement the design, since this tool only returns metadata. If you do not call get_design_context, the agent will not be able to implement the design.";

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
        self.begin_with_artifact(operation, collector, None).await
    }

    pub async fn begin_with_artifact(
        &self,
        operation: PendingOperation,
        collector: CollectorSession,
        artifact_key: Option<ArtifactRequestKey>,
    ) -> Result<String, DevupError> {
        let operation = artifact_key.map_or(operation.clone(), |artifact_key| {
            PendingOperation::Artifact {
                operation: Box::new(operation),
                artifact_key,
            }
        });
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        prune_expired(&mut state, now, self.limits.ttl.as_secs());
        if state.sessions.len() >= self.limits.max_sessions {
            return Err(too_large(
                "Exceeded the number of Figma handoff sessions that can be held at once.",
            ));
        }
        let session_id = unique_id(&state.sessions, &state.tombstones);
        state.sessions.insert(
            session_id.clone(),
            Session {
                operation,
                collector,
                expires_at: now.saturating_add(self.limits.ttl.as_secs()),
                result_bytes: 0,
                pending: BTreeMap::new(),
                consumed: BTreeSet::new(),
            },
        );
        Ok(session_id)
    }

    pub async fn next(&self, session_id: &str) -> Result<HandoffStep, DevupError> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        let mut session = take_session(&mut state, session_id, now, self.limits.ttl.as_secs())?;

        loop {
            match session.collector.advance() {
                Ok(CollectorStep::Call(planned)) => {
                    let call_id = random_id();
                    let handoff_call = HandoffCall {
                        call_id: call_id.clone(),
                        server: "figma",
                        tool: planned.call.tool_name(),
                        arguments: Value::Object(planned.call.arguments()),
                        node_id: planned.expected_node_id.clone(),
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
                    let collection = session.collector.stats().clone();
                    put_session(&mut state, session_id.to_owned(), session);
                    return Ok(HandoffStep::NeedsFigma {
                        session_id: session_id.to_owned(),
                        expires_at_epoch_seconds,
                        calls,
                        collection,
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
        let result = normalize_handoff_result(result)?;
        let encoded_len = serde_json::to_vec(&result)
            .map_err(|_| invalid("Cannot read the Figma handoff result as JSON."))?
            .len();
        if encoded_len > self.limits.max_result_bytes {
            self.remove(session_id).await;
            return Err(too_large(
                "The Figma handoff result exceeded the allowed size.",
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
                "Figma handoff results exceeded the total memory limit.",
            ));
        }
        let mut session = take_session(&mut state, session_id, now, self.limits.ttl.as_secs())?;
        let Some((collector_call_id, handoff_call)) = session.pending.get(call_id) else {
            let reason = if session.consumed.contains(call_id) {
                "consumed"
            } else {
                "unknown_call"
            };
            put_session(&mut state, session_id.to_owned(), session);
            return Err(invalid_reason(
                "Unknown or already-consumed Figma handoff call ID.",
                reason,
            ));
        };
        let collector_call_id = collector_call_id.clone();
        let requested_tool = handoff_call.tool;
        if let Some(error) = detect_tool_mismatch(requested_tool, call_id, &result) {
            put_session(&mut state, session_id.to_owned(), session);
            return Err(error);
        }
        if is_section_error_result(&result) {
            let mut rejected_collector = session.collector.clone();
            let error = DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "DEVUP_TARGET_IS_SECTION",
                false,
            );
            if rejected_collector.reject(&collector_call_id, &error)? {
                session.collector = rejected_collector;
                session.pending.remove(call_id);
                session.consumed.insert(call_id.to_owned());
                session.result_bytes = session.result_bytes.saturating_add(encoded_len);
                session.expires_at = now.saturating_add(self.limits.ttl.as_secs());
                put_session(&mut state, session_id.to_owned(), session);
                return Ok(());
            }
        }
        let mut result = result;
        strip_get_metadata_tail(&mut result);
        let mut accepted_collector = session.collector.clone();
        if let Err(error) =
            accepted_collector.accept(&collector_call_id, UpstreamResult { raw: result })
        {
            put_session(&mut state, session_id.to_owned(), session);
            return Err(error);
        }
        session.collector = accepted_collector;
        session.pending.remove(call_id);
        session.consumed.insert(call_id.to_owned());
        session.result_bytes = session.result_bytes.saturating_add(encoded_len);
        session.expires_at = now.saturating_add(self.limits.ttl.as_secs());
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

/// Whether an upstream result is the fast snapshot script reporting that its
/// target is a Section.
///
/// MCP reports a thrown script error as a *successful* tool call whose result
/// carries `isError`, so this cannot be spotted by matching on `Err`. Both the
/// handoff path and the direct path in `server::mod` need the same test: a
/// Section has no single screen to convert, and the collector answers it by
/// switching to the section index and offering selectable screens instead.
pub(crate) fn is_section_error_result(value: &Value) -> bool {
    value.get("isError").and_then(Value::as_bool) == Some(true)
        && value.to_string().contains("DEVUP_TARGET_IS_SECTION")
}

fn take_session(
    state: &mut StoreState,
    session_id: &str,
    now: u64,
    tombstone_ttl: u64,
) -> Result<Session, DevupError> {
    prune_tombstones(state, now);
    let Some(session) = state.sessions.remove(session_id) else {
        return if state.tombstones.contains_key(session_id) {
            Err(expired())
        } else {
            Err(invalid_reason(
                "No such Figma handoff session.",
                "unknown_session",
            ))
        };
    };
    state.total_result_bytes = state
        .total_result_bytes
        .saturating_sub(session.result_bytes);
    if session.expires_at <= now {
        remember_expired(state, session_id.to_owned(), now, tombstone_ttl);
        return Err(expired());
    }
    Ok(session)
}

fn put_session(state: &mut StoreState, session_id: String, session: Session) {
    state.total_result_bytes = state
        .total_result_bytes
        .saturating_add(session.result_bytes);
    state.sessions.insert(session_id, session);
}

fn prune_expired(state: &mut StoreState, now: u64, tombstone_ttl: u64) {
    prune_tombstones(state, now);
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
            remember_expired(state, id, now, tombstone_ttl);
        }
    }
}

fn remember_expired(state: &mut StoreState, id: String, now: u64, ttl: u64) {
    if state.tombstones.len() >= MAX_TOMBSTONES
        && let Some(oldest) = state
            .tombstones
            .iter()
            .min_by_key(|(_, tombstone)| tombstone.expires_at)
            .map(|(id, _)| id.clone())
    {
        state.tombstones.remove(&oldest);
    }
    state.tombstones.insert(
        id,
        SessionTombstone {
            expires_at: now.saturating_add(ttl),
        },
    );
}

fn prune_tombstones(state: &mut StoreState, now: u64) {
    state
        .tombstones
        .retain(|_, tombstone| tombstone.expires_at > now);
}

fn unique_id(
    sessions: &BTreeMap<String, Session>,
    tombstones: &BTreeMap<String, SessionTombstone>,
) -> String {
    loop {
        let id = random_id();
        if !sessions.contains_key(&id) && !tombstones.contains_key(&id) {
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

fn invalid_reason(message: &str, reason: &str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaHandoffInvalid,
        message,
        false,
        json!({"source": "host", "reason": reason}),
    )
}

fn expired() -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaHandoffExpired,
        "The Figma handoff session has expired.",
        true,
        json!({"source": "host", "reason": "expired"}),
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

/// Rejects the WQUW-156 wrong-tool handoff before the collector interprets
/// a `get_metadata` response using another call's recorded kind.
///
/// Detection is deliberately conservative: Figma's complete fixed reminder
/// is the only signature recognized today, and it is always legitimate when
/// the recorded request itself was `get_metadata`. Other result shapes are
/// left to the collector rather than guessed from design content.
fn detect_tool_mismatch(requested_tool: &str, call_id: &str, value: &Value) -> Option<DevupError> {
    if requested_tool == "get_metadata" || !contains_get_metadata_tail(value) {
        return None;
    }
    Some(DevupError::with_details(
        ErrorCode::DevupFigmaHandoffInvalid,
        "This looks like the result of a different Figma tool than the one requested.",
        false,
        json!({
            "reason": "tool_mismatch",
            "requested": { "tool": requested_tool, "callId": call_id },
            "hint": "This looks like the result of a tool other than the one requested. Run calls[].tool exactly as given.",
            "doNot": "Do not substitute another Figma tool, and do not reshape the result to fit the expected format."
        }),
    ))
}

/// Finds only Figma's complete fixed `get_metadata` reminder, recursively,
/// so official results remain detectable through host-added JSON wrappers.
/// It deliberately ignores every other metadata-looking string.
fn contains_get_metadata_tail(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains(GET_METADATA_RESULT_TAIL),
        Value::Object(object) => object.values().any(contains_get_metadata_tail),
        Value::Array(values) => values.iter().any(contains_get_metadata_tail),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Removes Figma's fixed reminder from every top-level `content[].text`
/// block before XML or text fallback parsing.
///
/// This addresses clients that discard `structuredContent` and expose only
/// official Figma text. It truncates at the exact Figma-authored marker and
/// trims whitespace immediately before it; all other fields and all text
/// before the marker remain unchanged. It never creates envelope fields or
/// attempts to infer metadata.
fn strip_get_metadata_tail(value: &mut Value) {
    let Some(content) = value.get_mut("content").and_then(Value::as_array_mut) else {
        return;
    };
    for item in content {
        let Some(Value::String(text)) = item.get_mut("text") else {
            continue;
        };
        let Some(marker_start) = text.find(GET_METADATA_RESULT_TAIL) else {
            continue;
        };
        text.truncate(marker_start);
        text.truncate(text.trim_end().len());
    }
}

/// Normalizes a `devup_figma_continue` `result` payload before it reaches
/// the collector. This is the fix for a real observed failure: opencode's
/// host handoff flattens an official Figma MCP `CallToolResult` down to
/// plain text before the agent ever sees it, so the agent has no envelope
/// to "pass through unchanged" — only a bare string. An agent that has
/// nothing but that string has previously invented a plausible-looking
/// `{"content":[{"type":"text","text":...}]}` wrapper by hand rather than
/// submit the string directly, which is exactly the kind of fabrication
/// this module exists to make unnecessary.
///
/// Two things happen here, and nothing else:
///
/// - A bare [`Value::String`] is promoted to the minimal MCP content-block
///   envelope `{"content": [{"type": "text", "text": <string>}]}`. This is
///   shape promotion only — the string itself is carried through
///   byte-for-byte, never modified, parsed, or re-interpreted.
/// - An object that has a `content` array but no `structuredContent` is
///   passed through unchanged *as long as at least one content item is
///   actually usable* (non-empty text, or image data). Every extraction
///   path in this codebase's collector already tolerates content-only
///   envelopes by design (`get_metadata`'s XML-text fallback,
///   variable/snapshot JSON encoded as `content[].text`, image content for
///   screenshots, ...), so rejecting these here would be a regression, not
///   a fix.
///
/// The only case rejected outright: a `content` array with nothing usable
/// in it and no `structuredContent` either. That shape gives every
/// downstream extraction path nothing to work with regardless of which
/// Figma tool the call was for, so failing fast here — with a
/// schema-shaped, non-design-leaking error — is strictly better than
/// letting the agent discover that after the collector's own, more
/// generic rejection.
///
/// Never fabricates data: this function only ever promotes or rejects
/// based on *shape*. It never invents a `structuredContent` value or edits
/// the content the caller actually sent.
fn normalize_handoff_result(result: Value) -> Result<Value, DevupError> {
    let promoted = match result {
        Value::String(text) => json!({ "content": [{ "type": "text", "text": text }] }),
        other => other,
    };
    if let Value::Object(object) = &promoted
        && let Some(Value::Array(content)) = object.get("content")
        && !object.contains_key("structuredContent")
        && !content.iter().any(has_usable_content_item)
    {
        return Err(missing_structured_content_error(&promoted));
    }
    Ok(promoted)
}

/// A content block counts as usable if it carries non-empty text, or
/// non-empty image data — the two shapes this codebase's collector
/// actually extracts from `content[]` today.
fn has_usable_content_item(item: &Value) -> bool {
    let has_text = item
        .get("text")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty());
    let has_image_data = item.get("type").and_then(Value::as_str) == Some("image")
        && item
            .get("data")
            .and_then(Value::as_str)
            .is_some_and(|data| !data.is_empty());
    has_text || has_image_data
}

/// Builds the `DEVUP_FIGMA_HANDOFF_INVALID` / `missing_structured_content`
/// rejection: the shape devup-mcp actually expects, the shape it received
/// (key names and content block `type`s only — see [`received_shape`]),
/// and explicit next-step guidance that forbids guessing the envelope.
fn missing_structured_content_error(value: &Value) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaHandoffInvalid,
        "Found no usable content or structuredContent in the Figma handoff result.",
        false,
        json!({
            "reason": "missing_structured_content",
            "expectedSchema": {
                "content": [{ "type": "text", "text": "<string>" }],
                "structuredContent": { "devupMetadata": "<object, required for this call kind>" }
            },
            "receivedShape": received_shape(value),
            "howToFix": "Pass the official Figma MCP response through verbatim, without processing it. If the host exposes text only, change sourcePolicy or the collection path.",
            "doNot": "Do not guess and fabricate envelope fields."
        }),
    )
}

/// Only key names and content-block `type` strings — never a value that
/// could carry design text, tokens, or credentials. This is deliberate:
/// the whole point of this error is to tell the agent what shape it sent
/// without ever echoing anything from the design or the upstream response
/// back into an error message.
fn received_shape(value: &Value) -> Value {
    let top_level_keys = match value {
        Value::Object(object) => object.keys().cloned().collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let mut content_types = value
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("type").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    content_types.sort();
    content_types.dedup();
    json!({ "topLevelKeys": top_level_keys, "contentTypes": content_types })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{GET_METADATA_RESULT_TAIL, strip_get_metadata_tail};

    /// Every text content block loses only the fixed reminder while values
    /// outside `content[].text` remain byte-for-byte unchanged.
    #[test]
    fn strips_get_metadata_reminder_from_every_text_content_item_only() {
        let mut value = json!({
            "content": [
                {"type": "text", "text": format!("first\n\n{GET_METADATA_RESULT_TAIL}")},
                {"type": "image", "data": "image-bytes"},
                {"type": "text", "text": format!("second  \n{GET_METADATA_RESULT_TAIL}")},
                {"type": "text", "text": "unchanged"}
            ],
            "structuredContent": {"reminder": GET_METADATA_RESULT_TAIL}
        });

        strip_get_metadata_tail(&mut value);

        assert_eq!(value["content"][0]["text"], "first");
        assert_eq!(value["content"][1]["data"], "image-bytes");
        assert_eq!(value["content"][2]["text"], "second");
        assert_eq!(value["content"][3]["text"], "unchanged");
        assert_eq!(
            value["structuredContent"]["reminder"],
            GET_METADATA_RESULT_TAIL
        );
    }
}
