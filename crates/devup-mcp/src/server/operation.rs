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
    Export {
        outputs: Vec<String>,
        component_name: Option<String>,
        include_diagnostics: bool,
        root_layout: RootLayout,
        asset_names_per_node: bool,
        scope: String,
        strict: bool,
        output_paths: BTreeMap<String, String>,
        page_scaffold: Option<super::projection::page_scaffold::PageScaffoldOptions>,
        frame_ids: Vec<String>,
        all_screens: bool,
        asset_captures: Vec<devup_mcp_figma::AssetSelection>,
        asset_output_paths: BTreeMap<String, String>,
        asset_public_root: Option<std::path::PathBuf>,
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
    let flagged = value.get("isError").and_then(Value::as_bool) == Some(true);
    let message = first_text(value).unwrap_or_else(|| "Figma reported an error.".to_owned());
    let lowered = message.to_lowercase();

    // A quota refusal is the one upstream failure that clears on its own,
    // so it must not be reported as a permanent one.
    //
    // It also need not be flagged. One arrived carrying no `isError` at all
    // and so was handed to the collector, which looked for the data it did not
    // contain and reported "variable/style batch not found in the Figma MCP
    // response" — the parser that happened to be next, named as the cause,
    // after twenty-four minutes of collecting. So the refusal is recognised by
    // what it says as well as by how it is flagged.
    //
    // Only when it is short, though. A healthy response carries its payload in
    // the same field, and a design is free to contain a layer named "rate
    // limit"; reading that as a refusal would fail a collection that worked. A
    // refusal is a sentence, a payload is a document, and the two are never
    // close in length.
    const LONGEST_REFUSAL: usize = 2000;
    let refuses = (lowered.contains("tool call limit") || lowered.contains("rate limit"))
        && (flagged || message.len() <= LONGEST_REFUSAL);
    if !flagged && !refuses {
        return None;
    }
    if refuses {
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
    if message.contains("DEVUP_FIGMA_HANDOFF_EXPIRED") {
        return Some(DevupError::with_details(
            ErrorCode::DevupFigmaHandoffExpired,
            message,
            true,
            json!({"stage":"plugin-execution","pluginCode":"DEVUP_FIGMA_HANDOFF_EXPIRED"}),
        ));
    }
    let mut details = json!({"stage":"plugin-execution","pluginCode":null,
        "nextAction":{"how":"Inspect the original upstream message and retry the original URL after correcting the reported plugin error."}});
    if message.contains("DEVUP_SECTION_REQUIRED") {
        details = json!({"pluginCode":"DEVUP_SECTION_REQUIRED","stage":"section-index","sectionId":null,
            "nextAction":{"tool":"devup_figma_export","requiredArguments":["url"],
                "how":"The url must point to a SECTION, while frameIds selects frames inside it. Open the requested frame's containing SECTION in Figma and copy its link. This legacy response did not include its ancestor SECTION ID."}});
        if let Some(start) = message.find('{')
            && let Some(Ok(structured)) = serde_json::Deserializer::from_str(&message[start..])
                .into_iter::<Value>()
                .next()
            && structured["pluginCode"] == "DEVUP_SECTION_REQUIRED"
            && structured.is_object()
        {
            details = structured;
        }
    }
    Some(DevupError::with_details(
        ErrorCode::DevupSnapshotUnsupported,
        message,
        false,
        details,
    ))
}

#[cfg(test)]
mod r7_tests {
    use super::*;
    #[test]
    fn r7_plugin_section_error_preserves_structured_recovery() {
        let detail = json!({"pluginCode":"DEVUP_SECTION_REQUIRED","stage":"section-index","nodeId":"3997:46333","nodeType":"FRAME","sectionId":"4279:7806","nextAction":{"tool":"devup_figma_export","arguments":{"url":"https://www.figma.com/design/test?node-id=4279-7806","frameIds":["3997:46333"]}}});
        let value = json!({"isError":true,"content":[{"type":"text","text":format!("Error: DEVUP_SECTION_REQUIRED {}\n at <anonymous> (PLUGIN_17_SOURCE:3:48)",detail)}]});
        let error = upstream_error(&value).unwrap();
        assert_eq!(error.details["sectionId"], "4279:7806");
        assert_eq!(
            error.details["nextAction"]["arguments"]["frameIds"],
            json!(["3997:46333"])
        );
    }
    #[test]
    fn r7_legacy_section_error_has_action_without_invented_ancestor() {
        let value = json!({"isError":true,"content":[{"type":"text","text":"Error: DEVUP_SECTION_REQUIRED\n at <anonymous> (PLUGIN_17_SOURCE:3:48)"}]});
        let error = upstream_error(&value).unwrap();
        assert_eq!(error.details["pluginCode"], "DEVUP_SECTION_REQUIRED");
        assert!(error.details["sectionId"].is_null());
        assert!(
            error.details["nextAction"]["how"]
                .as_str()
                .unwrap()
                .contains("SECTION")
        );
    }
}
