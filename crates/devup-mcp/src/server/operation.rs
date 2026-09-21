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
        previous_design_fingerprints: Option<String>,
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
    // The plugin serializes its own structured payload after the code.
    // Reading it for every code, rather than only for DEVUP_SECTION_REQUIRED,
    // is what lets a caller act on the others. DEVUP_SECTION_INDEX_TOO_LARGE
    // measured the bytes it produced and the transport ceiling it hit, and
    // every bit of that was dropped in favour of a generic "retry the
    // original URL" — the one move that cannot work, because the ceiling is
    // a property of the Section's size and not of the attempt.
    let structured = message
        .find('{')
        .and_then(|start| {
            serde_json::Deserializer::from_str(&message[start..])
                .into_iter::<Value>()
                .next()
        })
        .and_then(Result::ok)
        .filter(|value| value.is_object() && value["pluginCode"].is_string());
    // The code the script threw, whether or not it serialized a payload with
    // it. Only two of the scripts' twenty-five codes carry JSON, so reading
    // `pluginCode` off the message is what stops the other twenty-three from
    // arriving as `pluginCode: null` with nothing to act on.
    let code = plugin_code(&message).map(str::to_owned);
    if let Some(mut structured) = structured {
        if structured["nextAction"].is_null()
            && let Some(action) =
                plugin_next_action(structured["pluginCode"].as_str().unwrap_or_default())
        {
            structured["nextAction"] = action;
        }
        details = structured;
    } else if message.contains("DEVUP_SECTION_REQUIRED") {
        details = json!({"pluginCode":"DEVUP_SECTION_REQUIRED","stage":"section-index","sectionId":null,
            "nextAction":{"tool":"devup_figma_export","requiredArguments":["url"],
                "how":"The url must point to a SECTION, while frameIds selects frames inside it. Open the requested frame's containing SECTION in Figma and copy its link. This legacy response did not include its ancestor SECTION ID."}});
    }
    if let Some(code) = &code {
        if details["pluginCode"].is_null() {
            details["pluginCode"] = json!(code);
        }
        if details["nextAction"]["tool"].is_null()
            && let Some(action) = plugin_next_action(code)
        {
            details["nextAction"] = action;
        }
    }
    Some(DevupError::with_details(
        code.as_deref()
            .and_then(plugin_error_code)
            .unwrap_or(ErrorCode::DevupSnapshotUnsupported),
        message,
        false,
        details,
    ))
}

/// The `DEVUP_…` token the script threw, read out of the message text.
fn plugin_code(message: &str) -> Option<&str> {
    message
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| !(c.is_ascii_uppercase() || c == '_')))
        .find(|token| token.len() > "DEVUP_".len() && token.starts_with("DEVUP_"))
}

/// The codes that mean something specific to the caller rather than to the
/// collector.
///
/// Everything else stays `DevupSnapshotUnsupported`, because an internal
/// acquisition condition is not a tool argument anyone can fix. But a node or
/// a page that is not there *is* a wrong argument, and without this it
/// reached the caller as `-32603 INTERNAL_ERROR` — breaking the README's
/// promise that the JSON-RPC code alone separates "fix the arguments and call
/// again" from "stop and report", on the most common mistake there is.
fn plugin_error_code(code: &str) -> Option<ErrorCode> {
    match code {
        "DEVUP_NODE_NOT_FOUND" | "DEVUP_PAGE_NOT_FOUND" => Some(ErrorCode::DevupFigmaNodeNotFound),
        "DEVUP_ENVELOPE_TOO_LARGE" | "DEVUP_EXPLORE_PROJECTION_TOO_LARGE" => {
            Some(ErrorCode::DevupFigmaResponseTooLarge)
        }
        _ => None,
    }
}

/// Recovery for plugin codes whose payload measures the failure but says
/// nothing about what to do with it.
fn plugin_next_action(code: &str) -> Option<Value> {
    match code {
        "DEVUP_NODE_NOT_FOUND" => Some(json!({
            "tool": "devup_figma_search",
            "how": "The node-id in this url does not exist in this file. Figma links can outlive the node they pointed at, and a link copied from a comment or a requirement often names a node that has since been renamed or deleted. Search this file by name for the screen you meant and use the canonicalUrl the search returns.",
            "requiredArguments": ["url", "query"]
        })),
        "DEVUP_PAGE_NOT_FOUND" => Some(json!({
            "tool": "devup_figma_search",
            "how": "The page this url names is not in this file. Search the file without a node-id to list what it actually contains.",
            "requiredArguments": ["url", "query"]
        })),
        "DEVUP_ENVELOPE_TOO_LARGE" | "DEVUP_EXPLORE_PROJECTION_TOO_LARGE" => Some(json!({
            "tool": "devup_figma_export",
            "how": "The selection produced more than the plugin transport carries. Narrow it: export one frame at a time rather than a Section or a whole page, and leave debug outputs off.",
            "doNot": "Do not raise limit to recover this; limit bounds the answer, not the collection."
        })),
        "DEVUP_SECTION_INDEX_TOO_LARGE" => Some(json!({
            "tool": "devup_figma_search",
            "how": "This Section's candidate list does not fit the plugin transport. The list is never truncated to fit, because dropping a candidate would hide a screen the caller can select; text previews are already surrendered first. Narrow the target instead: search this Section by name for the screen you want, or open a nested Section and export from there.",
            "doNot": "Do not retry the same Section URL. The ceiling is a property of this Section's size, so the same request produces the same refusal.",
            "requiredArguments": ["url", "query"]
        })),
        _ => None,
    }
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
    fn plugin_failure(text: &str) -> DevupError {
        upstream_error(&json!({"isError":true,"content":[{"type":"text",
            "text":format!("Error: {text}\n at <anonymous> (PLUGIN_17_SOURCE:3:48)")}]}))
        .unwrap()
    }

    /// Every code the scripts in `devup-mcp-figma/src/scripts/` throw, in
    /// both shapes they arrive in: bare, and followed by a JSON payload. The
    /// point of reading the code out of the message is that it works for all
    /// of them — only two scripts serialize a payload, so a rule that only
    /// covered those would leave the other eleven reporting `pluginCode:
    /// null` exactly as before.
    #[test]
    fn every_plugin_code_is_named_in_both_message_shapes() {
        for code in [
            "DEVUP_NODE_NOT_FOUND",
            "DEVUP_PAGE_NOT_FOUND",
            "DEVUP_NODE_BOUNDS_UNAVAILABLE",
            "DEVUP_EXPLORE_PROJECTION_TOO_LARGE",
            "DEVUP_ENVELOPE_TOO_LARGE",
            "DEVUP_ENVELOPE_LENGTH_UNSTABLE",
            "DEVUP_ROOTS_INVALID",
            "DEVUP_SNAPSHOT_RANGE_INVALID",
            "DEVUP_TARGET_IS_SECTION",
            "DEVUP_LARGE_VALUE_CHANGED",
            "DEVUP_LARGE_VALUE_RANGE_INVALID",
            "DEVUP_SECTION_INDEX_TOO_LARGE",
            "DEVUP_SECTION_REQUIRED",
        ] {
            assert_eq!(plugin_code(code), Some(code), "bare: {code}");
            assert_eq!(
                plugin_code(&format!(
                    "Error: {code}\n at <anonymous> (PLUGIN_17_SOURCE:3:48)"
                )),
                Some(code),
                "thrown: {code}"
            );
            assert_eq!(
                plugin_code(&format!(r#"Error: {code} {{"pluginCode":"{code}"}}"#)),
                Some(code),
                "with payload: {code}"
            );
            assert_eq!(
                plugin_failure(code).details["pluginCode"],
                code,
                "reported: {code}"
            );
        }
        // A script failure that names no code must not invent one.
        assert_eq!(plugin_code("Error: unsupported"), None);
        assert_eq!(plugin_code("Error: something went wrong"), None);
    }

    /// A node that is not there is a wrong argument, not a server fault.
    /// Every script code except two carries no JSON payload, so this arrived
    /// as `pluginCode: null` on `DevupSnapshotUnsupported` — which
    /// `is_caller_mistake` reads as an internal failure, making it
    /// `-32603 INTERNAL_ERROR`. The README promises the JSON-RPC code alone
    /// separates "fix the arguments" from "stop and report"; on the most
    /// common mistake there is, it did the opposite.
    #[test]
    fn a_missing_node_is_a_caller_mistake_not_an_internal_failure() {
        for code in ["DEVUP_NODE_NOT_FOUND", "DEVUP_PAGE_NOT_FOUND"] {
            let error = plugin_failure(code);
            assert_eq!(error.code, ErrorCode::DevupFigmaNodeNotFound, "{code}");
            assert!(
                super::super::validation::is_caller_mistake(&error),
                "{code} must map to INVALID_PARAMS"
            );
            assert_eq!(error.details["pluginCode"], code);
            assert_eq!(error.details["nextAction"]["tool"], "devup_figma_search");
        }
    }

    #[test]
    fn an_oversized_envelope_reports_a_size_code_rather_than_an_unsupported_snapshot() {
        for code in [
            "DEVUP_ENVELOPE_TOO_LARGE",
            "DEVUP_EXPLORE_PROJECTION_TOO_LARGE",
        ] {
            let error = plugin_failure(code);
            assert_eq!(error.code, ErrorCode::DevupFigmaResponseTooLarge, "{code}");
            assert_eq!(error.details["pluginCode"], code);
            assert!(error.details["nextAction"]["doNot"].is_string(), "{code}");
        }
    }

    /// Only the codes that name a caller-fixable condition are remapped. An
    /// internal acquisition failure is not a tool argument anyone can edit,
    /// and calling it one would send the caller to change something that was
    /// never wrong.
    #[test]
    fn an_internal_plugin_condition_stays_an_internal_failure() {
        for code in [
            "DEVUP_ENVELOPE_LENGTH_UNSTABLE",
            "DEVUP_SNAPSHOT_RANGE_INVALID",
            "DEVUP_NODE_BOUNDS_UNAVAILABLE",
        ] {
            let error = plugin_failure(code);
            assert_eq!(error.code, ErrorCode::DevupSnapshotUnsupported, "{code}");
            // Still named, so the caller is not left with `pluginCode: null`.
            assert_eq!(error.details["pluginCode"], code);
            assert!(
                !super::super::validation::is_caller_mistake(&error),
                "{code}"
            );
        }
    }

    /// The plugin measured the refusal and named the ceiling. That payload
    /// used to be dropped for a generic "retry the original URL", which is
    /// the one move guaranteed to fail again.
    #[test]
    fn r7_oversized_section_index_reports_its_measurements_and_a_way_out() {
        let detail = json!({"pluginCode":"DEVUP_SECTION_INDEX_TOO_LARGE","stage":"section-index",
            "responseBytes":19613,"maxResponseBytes":19456});
        let value = json!({"isError":true,"content":[{"type":"text","text":format!("Error: DEVUP_SECTION_INDEX_TOO_LARGE {}\n at <anonymous> (PLUGIN_17_SOURCE:3:48)",detail)}]});
        let error = upstream_error(&value).unwrap();
        assert_eq!(error.details["pluginCode"], "DEVUP_SECTION_INDEX_TOO_LARGE");
        assert_eq!(error.details["responseBytes"], 19613);
        assert_eq!(error.details["maxResponseBytes"], 19456);
        assert_eq!(error.details["nextAction"]["tool"], "devup_figma_search");
        assert!(
            error.details["nextAction"]["doNot"]
                .as_str()
                .unwrap()
                .contains("retry")
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
