//! Integration tests for the three ground-truth tools
//! (`devup_project_context`, `devup_ui_validate`, `devup_stack_diff`) added
//! to prevent the exact failure documented in `README.md`'s brief: three
//! agents independently inventing a `$gray100` color token, a 16px bubble
//! radius, and a 36px avatar size that did not exist in the project's real
//! `devup.json`.
//!
//! These tools never call Figma, so the auth/upstream mocks here are
//! trivial stubs (unlike `source_orchestration.rs`'s fixtures, which
//! simulate real collection flows) — they exist only because `DevupServer`
//! requires a `Services` value to construct.

use std::sync::Arc;

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{AuthStatus, DevupError, FigmaUpstream, ReadToolCall, UpstreamResult};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
};
use serde_json::{Map, Value, json};

struct NeverCalledAuth;

#[async_trait]
impl DevupAuth for NeverCalledAuth {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        unreachable!("ground-truth tools never touch Figma auth")
    }

    async fn login(&self) -> Result<AuthStatus, DevupError> {
        unreachable!("ground-truth tools never touch Figma auth")
    }

    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        unreachable!("ground-truth tools never touch Figma auth")
    }
}

struct NeverCalledUpstream;

#[async_trait]
impl FigmaUpstream for NeverCalledUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        unreachable!("ground-truth tools never touch Figma upstream")
    }

    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        unreachable!("ground-truth tools never touch Figma upstream")
    }
}

async fn call_tool(tool: &str, arguments: Value) -> anyhow::Result<CallToolResult> {
    let server = DevupServer::new(Services::new(
        Arc::new(NeverCalledAuth),
        Arc::new(NeverCalledUpstream),
    ));
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap_or_default();
    let result = client
        .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments))
        .await?;
    client.cancel().await?;
    task.await??;
    Ok(result)
}

/// Absolute path to `tests/fixtures/ground-truth-project`, a minimal
/// synthetic project (not real girok-space data, per the brief's "저장소에
/// 남기는 건 최소한의 합성 데이터로 하라") with a real `devup.json`,
/// `openapi.json`, and a Vespertide `models/message.json`.
fn fixture_project_root() -> String {
    format!(
        "{}/tests/fixtures/ground-truth-project",
        env!("CARGO_MANIFEST_DIR")
    )
}

// ---------------------------------------------------------------------
// devup_project_context
// ---------------------------------------------------------------------

#[tokio::test]
async fn project_context_theme_scope_reads_exact_tokens_from_the_fixture_devup_json()
-> anyhow::Result<()> {
    let result = call_tool(
        "devup_project_context",
        json!({ "scope": "theme", "projectRoot": fixture_project_root() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["found"], true);
    let file = &output["files"][0];
    assert_eq!(
        file["colors"]["default"]["captionLight"], "#8a8a8a",
        "must report the real fixture value, not an invented one: {output}"
    );
    assert_eq!(file["colors"]["default"]["primaryColor"], "#3366ff");
    assert_eq!(file["length"]["default"]["md"], "16px");
    // The exact fabricated token from the brief's incident must NOT exist
    // in this fixture's real devup.json.
    assert!(file["colors"]["default"].get("gray100").is_none());
    assert!(file["colors"]["dark"].get("gray100").is_none());
    Ok(())
}

#[tokio::test]
async fn project_context_returns_stop_and_report_guardrail_when_devup_json_is_absent()
-> anyhow::Result<()> {
    let empty_root = std::env::temp_dir().join(format!(
        "devup-mcp-ground-truth-no-devup-json-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&empty_root)?;
    std::fs::write(empty_root.join("package.json"), "{}")?;

    let result = call_tool(
        "devup_project_context",
        json!({ "scope": "theme", "projectRoot": empty_root.to_string_lossy() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["found"], false);
    assert_eq!(output["guardrail"]["action"], "stop-and-report");
    assert!(
        output["guardrail"]["message"]
            .as_str()
            .unwrap()
            .contains("추측")
    );
    assert!(
        !output["guardrail"]["searchedPaths"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    std::fs::remove_dir_all(&empty_root)?;
    Ok(())
}

#[tokio::test]
async fn project_context_missing_project_root_also_reports_stop_and_report_guardrail()
-> anyhow::Result<()> {
    let orphan = std::env::temp_dir().join(format!(
        "devup-mcp-ground-truth-orphan-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    // No package.json/devup.json/Cargo.toml/.git anywhere in this leaf.
    std::fs::create_dir_all(&orphan)?;
    let result = call_tool(
        "devup_project_context",
        json!({ "scope": "theme", "projectRoot": orphan.to_string_lossy() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["found"], false);
    assert_eq!(output["guardrail"]["action"], "stop-and-report");
    std::fs::remove_dir_all(&orphan)?;
    Ok(())
}

#[tokio::test]
async fn project_context_api_scope_lists_real_endpoints_and_required_fields() -> anyhow::Result<()>
{
    let result = call_tool(
        "devup_project_context",
        json!({ "scope": "api", "projectRoot": fixture_project_root() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["found"], true);
    let spec = &output["specs"][0];
    let operation_ids = spec["endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|endpoint| endpoint["operationId"].as_str())
        .collect::<Vec<_>>();
    assert!(operation_ids.contains(&"listMessages"));
    assert!(operation_ids.contains(&"getMessage"));
    let message_schema = spec["schemas"]
        .as_array()
        .unwrap()
        .iter()
        .find(|schema| schema["name"] == "Message")
        .expect("Message schema present");
    let required = message_schema["requiredFields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(required.contains(&"authorId"));
    Ok(())
}

#[tokio::test]
async fn project_context_db_scope_lists_real_columns_and_enum_values() -> anyhow::Result<()> {
    let result = call_tool(
        "devup_project_context",
        json!({ "scope": "db", "projectRoot": fixture_project_root() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["found"], true);
    let table = &output["tables"][0];
    assert_eq!(table["table"], "message");
    let column_names = table["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| column["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(column_names.contains(&"author_id"));
    assert!(column_names.contains(&"status"));
    let enum_def = &table["enums"][0];
    assert_eq!(enum_def["values"][0], "draft");
    Ok(())
}

// ---------------------------------------------------------------------
// devup_ui_validate — the $gray100 regression case is the core deliverable
// ---------------------------------------------------------------------

#[tokio::test]
async fn ui_validate_catches_the_exact_gray100_regression_from_the_incident() -> anyhow::Result<()>
{
    // This TSX is exactly the shape of the fabricated failure documented
    // in the brief: an agent using a plausible-looking but nonexistent
    // color token instead of one of the real tokens in devup.json.
    let tsx = r##"
        import { Box } from "@devup-ui/react";

        export const ChatBubble = () => (
            <Box color="$gray100" borderRadius="16px" />
        );
    "##;
    let result = call_tool(
        "devup_ui_validate",
        json!({ "tsx": tsx, "projectRoot": fixture_project_root() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["themeAvailable"], true);
    assert_eq!(output["ok"], false, "must fail: {output}");
    let violations = output["violations"].as_array().unwrap();
    let token_violation = violations
        .iter()
        .find(|violation| violation["rule"] == "unknown-token")
        .expect("unknown-token violation for $gray100");
    assert_eq!(token_violation["severity"], "error");
    assert!(
        token_violation["message"]
            .as_str()
            .unwrap()
            .contains("gray100"),
        "{token_violation}"
    );
    // The tool must not silently accept the same input's hardcoded 16px
    // radius either — devup.json has a real "md": "16px" length token.
    assert!(
        violations
            .iter()
            .any(|violation| violation["rule"] == "hardcoded-length"),
        "{violations:?}"
    );
    Ok(())
}

#[tokio::test]
async fn ui_validate_accepts_tsx_using_only_real_project_tokens() -> anyhow::Result<()> {
    let tsx = r##"
        import { Box } from "@devup-ui/react";

        export const ChatBubble = () => (
            <Box color="$captionLight" borderRadius="$md" />
        );
    "##;
    let result = call_tool(
        "devup_ui_validate",
        json!({ "tsx": tsx, "projectRoot": fixture_project_root() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["ok"], true, "{output}");
    assert_eq!(output["checkedTokens"], 2);
    Ok(())
}

#[tokio::test]
async fn ui_validate_suggests_the_matching_real_token_for_a_hardcoded_hex_color()
-> anyhow::Result<()> {
    let tsx = r##"<Box color="#8a8a8a" />"##;
    let result = call_tool(
        "devup_ui_validate",
        json!({ "tsx": tsx, "projectRoot": fixture_project_root() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    let violation = output["violations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|violation| violation["rule"] == "hardcoded-color")
        .expect("hardcoded-color violation");
    assert_eq!(violation["severity"], "warning");
    assert!(
        violation["suggestion"]
            .as_str()
            .unwrap()
            .contains("captionLight"),
        "{violation}"
    );
    Ok(())
}

#[tokio::test]
async fn ui_validate_suggests_the_matching_real_token_for_a_hardcoded_px_length()
-> anyhow::Result<()> {
    let tsx = r##"<Box borderRadius="16px" />"##;
    let result = call_tool(
        "devup_ui_validate",
        json!({ "tsx": tsx, "projectRoot": fixture_project_root() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    let violation = output["violations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|violation| violation["rule"] == "hardcoded-length")
        .expect("hardcoded-length violation");
    assert!(violation["suggestion"].as_str().unwrap().contains("md"));
    Ok(())
}

#[tokio::test]
async fn ui_validate_catches_runtime_value_inside_css_call() -> anyhow::Result<()> {
    let tsx = r##"
        import { css } from "@devup-ui/react";
        const dynamicWidth = getWidth();
        const cls = css({ width: dynamicWidth });
    "##;
    let result = call_tool(
        "devup_ui_validate",
        json!({ "tsx": tsx, "projectRoot": fixture_project_root() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["ok"], false);
    assert!(
        output["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|violation| violation["rule"] == "runtime-value"),
        "{output}"
    );
    Ok(())
}

#[tokio::test]
async fn ui_validate_does_not_flag_dynamic_jsx_props_as_runtime_value() -> anyhow::Result<()> {
    // Verified against @devup-ui/react's own docs: `<Box bg={dynamic} />`
    // compiles to a CSS custom property, it is not a runtime-value error.
    let tsx = r##"export const X = ({ color }) => <Box bg={color} />;"##;
    let result = call_tool(
        "devup_ui_validate",
        json!({ "tsx": tsx, "projectRoot": fixture_project_root() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert!(
        output["violations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|violation| violation["rule"] != "runtime-value"),
        "{output}"
    );
    Ok(())
}

#[tokio::test]
async fn ui_validate_reports_missing_theme_without_crashing_and_skips_token_checks()
-> anyhow::Result<()> {
    let empty_root = std::env::temp_dir().join(format!(
        "devup-mcp-ground-truth-ui-validate-no-theme-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&empty_root)?;
    std::fs::write(empty_root.join("package.json"), "{}")?;

    let result = call_tool(
        "devup_ui_validate",
        json!({ "tsx": r##"<Box color="$whateverToken" />"##, "projectRoot": empty_root.to_string_lossy() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["themeAvailable"], false);
    assert_eq!(output["themeGuardrail"]["action"], "stop-and-report");
    assert!(
        output["violations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|violation| violation["rule"] != "unknown-token"),
        "without a theme, unknown-token must be skipped, not guessed at: {output}"
    );

    std::fs::remove_dir_all(&empty_root)?;
    Ok(())
}

#[tokio::test]
async fn ui_validate_strict_mode_fails_on_warning_severity_violations() -> anyhow::Result<()> {
    let tsx = r##"<Box color="#8a8a8a" />"##;
    let lenient = call_tool(
        "devup_ui_validate",
        json!({ "tsx": tsx, "projectRoot": fixture_project_root(), "strict": false }),
    )
    .await?
    .structured_content
    .unwrap();
    let strict = call_tool(
        "devup_ui_validate",
        json!({ "tsx": tsx, "projectRoot": fixture_project_root(), "strict": true }),
    )
    .await?
    .structured_content
    .unwrap();
    assert_eq!(lenient["ok"], true);
    assert_eq!(strict["ok"], false);
    Ok(())
}

// ---------------------------------------------------------------------
// devup_stack_diff
// ---------------------------------------------------------------------

#[tokio::test]
async fn stack_diff_reports_every_requested_layer_with_explicit_confidence() -> anyhow::Result<()> {
    let result = call_tool(
        "devup_stack_diff",
        json!({ "projectRoot": fixture_project_root() }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["found"], true);
    for layer in [
        "db-entity",
        "entity-route",
        "route-openapi",
        "openapi-client",
    ] {
        assert!(
            output["layers"].get(layer).is_some(),
            "missing layer {layer} in {output}"
        );
        assert!(
            output["layers"][layer].get("checked").is_some(),
            "layer {layer} must report whether it could run"
        );
    }
    Ok(())
}

#[tokio::test]
async fn stack_diff_rejects_unknown_layer_names() -> anyhow::Result<()> {
    let result = call_tool(
        "devup_stack_diff",
        json!({ "projectRoot": fixture_project_root(), "layers": ["not-a-real-layer"] }),
    )
    .await;
    assert!(result.is_err());
    Ok(())
}
