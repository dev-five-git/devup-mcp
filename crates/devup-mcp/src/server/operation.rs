//! What a caller asked for, and how to read a refusal that arrived dressed as
//! success.
//!
//! [`PendingOperation`] carries the request's own shape — which outputs, which
//! paths, which delivery — from the tool boundary through collection to
//! projection, so a completed collection can be answered in the terms it was
//! asked in.
//!
//! The rest reads upstream results. MCP reports a thrown script error as a
//! *successful* call whose result carries `isError`, so a refusal cannot be
//! found by matching on `Err`; it has to be read out of the body.
use std::collections::BTreeMap;

use devup_mcp_devup_ui::codegen::RootLayout;
use devup_mcp_figma::{DevupError, ErrorCode};
use serde_json::{Value, json};

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

/// Whether an upstream result is the fast snapshot script reporting that its
/// target is a Section.
///
/// MCP reports a thrown script error as a *successful* tool call whose result
/// carries `isError`, so this cannot be spotted by matching on `Err`. A Section
/// has no single screen to convert, and the collector answers it by
/// switching to the section index and offering selectable screens instead.
pub(crate) fn is_section_error_result(value: &Value) -> bool {
    value.get("isError").and_then(Value::as_bool) == Some(true)
        && value.to_string().contains("DEVUP_TARGET_IS_SECTION")
}

/// The message carried by an upstream result that reports a failure.
///
/// A Section target was only the first error delivered this way. Anything
/// upstream refuses — a tool-call rate limit above all — arrives as a
/// *successful* MCP call carrying `isError`, and handing that to the
/// collector made it hunt for data the response never contained. It then
/// blamed the parser: "metadata not found in the Figma MCP response", or
/// the same for snapshot data, variable batches and asset descriptors,
/// depending only on which step happened to receive it. The real reason
/// was in the response the whole time, so return it and let the caller
/// read it.
/// The wait Figma asked for, in seconds, wherever it appears.
///
/// Figma's REST API answers a 429 with `Retry-After`. The MCP relay does
/// not forward response headers today, so this usually finds nothing — but
/// reading it costs nothing and is the only authoritative answer to "when
/// can I retry", which otherwise has to be guessed.
fn retry_after_seconds(value: &Value) -> Option<u64> {
    match value {
        Value::Object(object) => object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("retry-after") || *key == "retryAfter")
            .and_then(|(_, found)| {
                found
                    .as_u64()
                    .or_else(|| found.as_str().and_then(|text| text.parse().ok()))
            })
            .or_else(|| object.values().find_map(retry_after_seconds)),
        Value::Array(values) => values.iter().find_map(retry_after_seconds),
        _ => None,
    }
}

pub(crate) fn upstream_error(value: &Value) -> Option<DevupError> {
    if value.get("isError").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    fn first_text(value: &Value) -> Option<String> {
        match value {
            Value::Object(object) => object
                .get("text")
                .and_then(Value::as_str)
                .filter(|text| text.len() > 16)
                .map(str::to_owned)
                .or_else(|| object.values().find_map(first_text)),
            Value::Array(values) => values.iter().find_map(first_text),
            _ => None,
        }
    }
    let message = first_text(value).unwrap_or_else(|| "Figma reported an error.".to_owned());

    // A quota refusal is the one upstream failure that clears on its own,
    // so it must not be reported as a permanent one.
    let lowered = message.to_lowercase();
    if lowered.contains("tool call limit") || lowered.contains("rate limit") {
        let mut details = json!({
            // Figma meters reads with a leaky bucket, so there is no reset
            // hour to wait for: capacity drains back continuously. Saying
            // an allowance "resets tomorrow" would invite waiting for a
            // rollover that never happens, and it explains why small
            // requests slip through while a large one still fails.
            "recovery": "Figma meters reads with a leaky bucket, so capacity returns gradually rather than resetting at a fixed time. Retry after a short wait; a small request may succeed while a large one is still refused.",
            "costHint": "A refreshed export spends about 15 Figma tool calls, so prefer a cached artifact over refresh.",
        });
        // The REST API states the exact wait in `Retry-After`, and names
        // the ceiling in `X-Figma-Rate-Limit-Type`. The MCP relay does not
        // forward either today, so read them when present rather than
        // guessing, and say plainly when they are absent.
        match retry_after_seconds(value) {
            Some(seconds) => {
                details["retryAfterSeconds"] = json!(seconds);
            }
            None => {
                details["whichLimit"] = json!(
                    "Not stated. Figma applies a per-minute ceiling alongside a daily or monthly allowance, and the MCP response does not say which was reached."
                );
            }
        }
        return Some(DevupError::with_details(
            ErrorCode::DevupFigmaRateLimited,
            message,
            true,
            details,
        ));
    }
    Some(DevupError::new(
        ErrorCode::DevupSnapshotUnsupported,
        message,
        false,
    ))
}
