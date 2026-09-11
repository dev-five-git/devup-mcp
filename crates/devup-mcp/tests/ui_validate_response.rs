//! What `devup_ui_validate`'s response says about itself.
//!
//! The MCP keys are assembled by hand in the tool handler rather than
//! serialized from `UiValidation`, so what that struct's documentation
//! explains reaches nobody unless the response answers it too. Two
//! readings went wrong in practice: `ok: true` beside ten violations read
//! as a contradiction, and `checkedTokens: 4` beside
//! `availableTokenCount: 81` read as "4 of 81 checked". These lock the
//! response to the meaning `UiValidation` documents.

use std::sync::Arc;

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, DevupError, ErrorCode, FigmaUpstream, ReadToolCall, UpstreamResult,
};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Map, Value, json};

struct Auth;

#[async_trait]
impl DevupAuth for Auth {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }
    async fn login(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }
    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

struct NoFigma;

#[async_trait]
impl FigmaUpstream for NoFigma {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec![])
    }
    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        Err(DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "this tool does not reach Figma",
            false,
        ))
    }
}

fn fixture_project_root() -> String {
    format!(
        "{}/tests/fixtures/ground-truth-project",
        env!("CARGO_MANIFEST_DIR")
    )
}

async fn validate(arguments: Value) -> anyhow::Result<Value> {
    let server = DevupServer::new(Services::new(Arc::new(Auth), Arc::new(NoFigma)));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("devup_ui_validate").with_arguments(arguments))
        .await?;
    let content = serde_json::to_value(&result.content[0])?;
    assert_eq!(
        serde_json::from_str::<Value>(content["text"].as_str().unwrap())?,
        result.structured_content.clone().unwrap()
    );
    client.cancel().await?;
    task.await??;
    Ok(result.structured_content.unwrap())
}

#[tokio::test]
async fn r12_failed_validation_explains_why_source_cannot_be_corrected_automatically()
-> anyhow::Result<()> {
    for tsx in [
        r#"<Text maxLength={50} />"#,
        r#"<Box bg="$missing" />"#,
        "<Box bg=",
        "css({width: dynamic})",
    ] {
        let output = validate(json!({"tsx":tsx,"projectRoot":fixture_project_root()})).await?;
        assert_eq!(output["ok"], false);
        assert_eq!(output["recoveryState"], "manual-fix-required");
        assert!(output["nextAction"].is_null());
        assert!(
            output["recoveryReason"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        );
        assert!(
            output["nextActionReason"]
                .as_str()
                .is_some_and(|s| s.contains("source"))
        );
    }
    Ok(())
}

#[tokio::test]
async fn r12_unique_exact_token_warning_supplies_callable_corrected_arguments() -> anyhow::Result<()>
{
    // The original fixture's primaryColor differs in dark mode. R17 forbids
    // auto-correcting that partial match; exercise R12's safe correction with
    // an otherwise identical fixture that matches in every mode.
    let root =
        std::env::temp_dir().join(format!("devup-r17-safe-correction-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let mut theme: Value = serde_json::from_str(&std::fs::read_to_string(
        std::path::Path::new(&fixture_project_root()).join("devup.json"),
    )?)?;
    theme["theme"]["colors"]["dark"]["primaryColor"] = json!("#3366ff");
    std::fs::write(root.join("devup.json"), serde_json::to_vec(&theme)?)?;
    let output = validate(json!({"tsx":r##"<><Text>한글</Text><Box bg="#3366ff" p="16px" /></>"##,"strict":true,"projectRoot":root.to_string_lossy()})).await?;
    assert_eq!(output["ok"], false);
    assert_eq!(output["recoveryState"], "available");
    assert_eq!(output["nextAction"]["tool"], "devup_ui_validate");
    let arguments = output["nextAction"]["arguments"].clone();
    assert_eq!(arguments["strict"], true);
    assert_eq!(arguments["projectRoot"], root.to_string_lossy().as_ref());
    assert_eq!(
        arguments["tsx"],
        r##"<><Text>한글</Text><Box bg="$primaryColor" p="$md" /></>"##
    );
    assert_eq!(validate(arguments).await?["ok"], true);
    std::fs::remove_file(root.join("devup.json"))?;
    std::fs::remove_dir(root)?;
    Ok(())
}

#[tokio::test]
async fn r12_strict_warning_with_error_requires_source_edit_not_partial_retry() -> anyhow::Result<()>
{
    let output = validate(json!({"tsx":r##"<Box bg="#3366ff" notARealProp="x" />"##,"strict":true,"projectRoot":fixture_project_root()})).await?;
    assert_eq!(output["okReason"], "error-violations");
    assert_eq!(output["recoveryState"], "manual-fix-required");
    assert!(output["nextAction"].is_null());
    assert!(output["recoveryReason"].is_string());
    Ok(())
}

#[tokio::test]
async fn r12_ambiguous_exact_tokens_require_a_source_decision() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!(
        "devup-r12-ambiguous-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&root)?;
    std::fs::write(
        root.join("devup.json"),
        r##"{"theme":{"colors":{"default":{"primary":"#3366ff","accent":"#3366ff"}}}}"##,
    )?;
    let result = validate(json!({"tsx":r##"<Box bg="#3366ff" />"##,"strict":true,"projectRoot":root.to_string_lossy()})).await;
    std::fs::remove_file(root.join("devup.json"))?;
    std::fs::remove_dir(&root)?;
    let output = result?;
    assert_eq!(output["ok"], false);
    assert_eq!(output["okReason"], "strict-warnings");
    assert_eq!(output["strict"], true);
    assert_eq!(output["recoveryState"], "manual-fix-required");
    assert!(output["nextAction"].is_null());
    assert!(
        output["recoveryReason"]
            .as_str()
            .unwrap()
            .contains("matching theme tokens")
    );
    Ok(())
}

/// `ok: true` next to a non-empty `violations` is correct output, and the
/// response now carries the reason it is correct: none of them is an
/// error. Reading the two fields alone, without the struct's rustdoc, has
/// to reach that conclusion.
#[tokio::test]
async fn exact_token_matches_are_actionable_warnings() -> anyhow::Result<()> {
    let output = validate(json!({
        "tsx": r##"export const S = () => <Box bg="#3366ff" p="16px" gap="24px" />;"##,
        "projectRoot": fixture_project_root()
    }))
    .await?;

    assert_eq!(output["ok"], true);
    assert_eq!(output["okReason"], "warnings-only");
    assert_eq!(
        output["violationCounts"],
        json!({"error":0,"warning":3,"info":0})
    );
    assert_eq!(output["strict"], false);
    assert_eq!(output["violationCounts"]["error"], 0);
    assert!(
        output["violationCounts"]["warning"].as_u64().unwrap() > 0,
        "the fixture TSX is meant to produce warnings: {output}"
    );
    assert_eq!(
        output["violationCounts"]["warning"].as_u64().unwrap() as usize,
        output["violations"].as_array().unwrap().len(),
        "every violation here is a warning, so the counts must add up"
    );

    // The `bg` color is one of them. Through this tool - which is where it
    // was observed missing - a generated `bg="#752E2E"` used to produce no
    // finding at all, and the silence was mistaken for a rule.
    assert!(
        output["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|violation| violation["rule"] == "hardcoded-color"
                && violation["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("bg") && message.contains("#3366ff"))),
        "the hardcoded bg color must be reported: {output}"
    );
    Ok(())
}

/// The same TSX under `strict` fails, and says that strictness is why -
/// not that new problems appeared.
#[tokio::test]
async fn strict_mode_fails_on_actionable_warnings() -> anyhow::Result<()> {
    let output = validate(json!({
        "tsx": r##"export const S = () => <Box bg="#3366ff" />;"##,
        "projectRoot": fixture_project_root(),
        "strict": true
    }))
    .await?;

    assert_eq!(output["ok"], false);
    assert_eq!(output["okReason"], "strict-warnings");
    assert_eq!(output["strict"], true);
    assert_eq!(output["violationCounts"]["error"], 0);
    Ok(())
}

/// A real error names itself as one, so `ok: false` is never ambiguous
/// between "this is broken" and "you asked for strict".
#[tokio::test]
async fn an_error_violation_is_distinguished_from_a_strict_failure() -> anyhow::Result<()> {
    let output = validate(json!({
        "tsx": r##"export const S = () => <Box bg="$gray100" />;"##,
        "projectRoot": fixture_project_root()
    }))
    .await?;

    assert_eq!(output["ok"], false);
    assert_eq!(output["okReason"], "error-violations");
    assert!(output["violationCounts"]["error"].as_u64().unwrap() > 0);
    Ok(())
}

/// Clean TSX is distinguishable from TSX that merely has no errors.
#[tokio::test]
async fn clean_tsx_reports_clean() -> anyhow::Result<()> {
    let output = validate(json!({
        "tsx": r##"export const S = () => <Box bg="$primaryColor" />;"##,
        "projectRoot": fixture_project_root()
    }))
    .await?;

    assert_eq!(output["ok"], true);
    assert_eq!(output["okReason"], "clean");
    assert_eq!(output["violationCounts"]["info"], 0);
    assert_eq!(output["violations"].as_array().unwrap().len(), 0);
    assert_eq!(output["violationCounts"]["error"], 0);
    assert_eq!(output["violationCounts"]["warning"], 0);
    Ok(())
}

/// `checkedTokens` counts references the code makes; `availableTokenCount`
/// counts definitions the theme holds. They are not a ratio, and `tokens`
/// says so by naming each one after what it counts.
#[tokio::test]
async fn the_two_token_counts_name_what_they_count() -> anyhow::Result<()> {
    let output = validate(json!({
        "tsx": r##"export const S = () => <Box bg="$primaryColor" color="$captionLight" />;"##,
        "projectRoot": fixture_project_root()
    }))
    .await?;

    assert_eq!(
        output["checkedTokens"], 2,
        "two $token references: {output}"
    );
    assert_eq!(output["tokens"]["referencedByTsx"], output["checkedTokens"]);
    assert_eq!(
        output["tokens"]["definedByTheme"],
        output["availableTokenCount"]
    );
    assert!(
        output["availableTokenCount"].as_u64().unwrap() > 2,
        "the fixture theme defines more tokens than this TSX references, \
         which is exactly why the two numbers must not read as a ratio: {output}"
    );
    assert_eq!(output["tokens"]["unknownTokenCheckRan"], true);
    assert_eq!(output["themeAvailable"], true);
    Ok(())
}

/// With no theme the token check is skipped, not passed. `checkedTokens`
/// still counts the references, so it alone cannot tell the two apart -
/// `unknownTokenCheckRan` is what does.
#[tokio::test]
async fn a_skipped_token_check_does_not_read_as_a_passed_one() -> anyhow::Result<()> {
    let empty_root = std::env::temp_dir().join(format!(
        "devup-mcp-ui-validate-no-theme-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&empty_root)?;

    let output = validate(json!({
        "tsx": r##"export const S = () => <Box bg="$whateverToken" />;"##,
        "projectRoot": empty_root.to_string_lossy()
    }))
    .await?;

    assert_eq!(output["checkedTokens"], 1, "the reference is still counted");
    assert_eq!(output["tokens"]["unknownTokenCheckRan"], false);
    assert_eq!(output["themeAvailable"], false);
    assert_eq!(output["availableTokenCount"], 0);
    assert_eq!(output["tokens"]["definedByTheme"], 0);

    std::fs::remove_dir(&empty_root)?;
    Ok(())
}

/// The description is read before the call, so the `ok` rule belongs there
/// too - a caller who plans around "ok means no findings" has already
/// chosen wrong by the time they see the response.
#[tokio::test]
async fn the_tool_description_states_the_ok_rule() -> anyhow::Result<()> {
    let server = DevupServer::new(Services::new(Arc::new(Auth), Arc::new(NoFigma)));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let tools = client.list_all_tools().await?;
    let validate = tools
        .iter()
        .find(|tool| tool.name == "devup_ui_validate")
        .expect("devup_ui_validate is published");
    let description = validate.description.clone().unwrap_or_default();

    assert!(
        description.contains("severity"),
        "the description must say severity decides ok: {description}"
    );
    assert!(
        description.contains("checkedTokens") && description.contains("availableTokenCount"),
        "the description must say the two token counts are different things: {description}"
    );
    client.cancel().await?;
    task.await??;
    Ok(())
}

/// Nonmatching values stay visible; info never becomes a strict failure.
#[tokio::test]
async fn unmatched_literals_are_info_only_in_both_modes() -> anyhow::Result<()> {
    for strict in [false, true] {
        let output = validate(json!({"tsx": r##"export const S = () => <Box bg="#752E2E" p="20px" gap="40px" />;"##, "projectRoot": fixture_project_root(), "strict": strict})).await?;
        assert_eq!(output["ok"], true);
        assert_eq!(output["okReason"], "info-only");
        assert_eq!(
            output["violationCounts"],
            json!({"error":0,"warning":0,"info":3})
        );
        assert_eq!(output["violations"].as_array().unwrap().len(), 3);
        assert_eq!(output["themeNotes"], json!([]));
        for finding in output["violations"].as_array().unwrap() {
            assert_eq!(finding["severity"], "info");
            assert!(finding.get("suggestion").is_none());
            assert!(
                finding["message"]
                    .as_str()
                    .unwrap()
                    .contains("no matching token")
            );
        }
    }
    Ok(())
}

/// Info must not inflate warning counts or override errors/strict warnings.
#[tokio::test]
async fn mixed_severities_keep_counts_and_failure_reasons_consistent() -> anyhow::Result<()> {
    for (strict, token, ok, reason, errors) in [
        (false, "$primaryColor", true, "warnings-and-info", 0),
        (true, "$primaryColor", false, "strict-warnings", 0),
        (false, "$missingToken", false, "error-violations", 1),
        (true, "$missingToken", false, "error-violations", 1),
    ] {
        let output = validate(json!({"tsx":format!(r##"export const S = () => <Box bg="#3366ff" p="20px" color="{token}" />;"##),"projectRoot":fixture_project_root(),"strict":strict})).await?;
        assert_eq!(output["ok"], ok);
        assert_eq!(output["okReason"], reason);
        assert_eq!(
            output["violationCounts"],
            json!({"error":errors,"warning":1,"info":1})
        );
    }
    Ok(())
}

/// A sparse theme reports each literal while explaining the missing category once.
#[tokio::test]
async fn empty_length_category_is_summarized_once_in_the_response() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!(
        "devup-mcp-ui-empty-length-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&root)?;
    std::fs::write(
        root.join("devup.json"),
        r##"{"theme":{"colors":{"default":{"primary":"#3366ff"}},"length":{}}}"##,
    )?;
    let result = validate(json!({
        "tsx": r##"export const S = () => <Box p="20px" gap="40px" />;"##,
        "projectRoot": root.to_string_lossy(), "strict":true
    }))
    .await;
    std::fs::remove_file(root.join("devup.json"))?;
    std::fs::remove_dir(&root)?;
    let output = result?;
    assert_eq!(output["ok"], true);
    assert_eq!(output["okReason"], "info-only");
    assert_eq!(
        output["violationCounts"],
        json!({"error":0,"warning":0,"info":2})
    );
    assert_eq!(
        output["themeNotes"],
        json!(["The theme defines no length tokens."])
    );
    for finding in output["violations"].as_array().unwrap() {
        assert!(finding.get("suggestion").is_none());
        assert!(
            !finding["message"]
                .as_str()
                .unwrap()
                .contains("defines no length")
        );
    }
    Ok(())
}

#[tokio::test]
async fn r17_build_identity_and_manual_source_recovery() -> anyhow::Result<()> {
    let out = validate(json!({"tsx":"<Text maxLength={50}/>","projectRoot":fixture_project_root(),"sourceName":"Modal.tsx"})).await?;
    assert_eq!(out["recoveryState"], "manual-fix-required");
    assert_eq!(out["violations"][0]["sourceName"], "Modal.tsx");
    assert!(
        out["server"]["displayVersion"]
            .as_str()
            .unwrap_or("")
            .contains('+')
    );
    assert!(
        out["server"]["identityGuidance"]
            .as_str()
            .unwrap_or("")
            .contains("commit")
    );
    Ok(())
}

#[tokio::test]
async fn r17_reference_png_recovery_has_frame_url() -> anyhow::Result<()> {
    let server = DevupServer::new(Services::new(Arc::new(Auth), Arc::new(NoFigma)));
    let (st, ct) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(st).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(ct).await?;
    let result = client.call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(json!({"url":"https://www.figma.com/design/Fixture/Test?node-id=1-1","frameIds":["2:3"],"outputs":["referencePng"]}).as_object().unwrap().clone())).await?;
    let out = result.structured_content.unwrap();
    let text = out.to_string();
    assert!(text.contains("node-id=2-3"), "{out}");
    client.cancel().await?;
    task.await??;
    Ok(())
}
#[tokio::test]
async fn r17_readable_deployment_identity() -> anyhow::Result<()> {
    let out = validate(json!({"tsx":"<Box/>"})).await?;
    assert!(
        out["server"]["displayVersion"]
            .as_str()
            .unwrap_or("")
            .contains('+')
    );
    assert!(
        out["server"]["identityGuidance"]
            .as_str()
            .unwrap_or("")
            .contains("commit")
    );
    Ok(())
}
#[tokio::test]
async fn r17_source_name_reaches_diagnostics() -> anyhow::Result<()> {
    let out = validate(json!({"tsx":"<Text maxLength={50}/>","sourceName":"Modal.tsx"})).await?;
    assert_eq!(out["violations"][0]["sourceName"], "Modal.tsx");
    Ok(())
}
#[tokio::test]
async fn r17_partial_mode_match_never_auto_replaces_literal() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("devup-r17-mode-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    std::fs::write(
        root.join("devup.json"),
        r##"{"theme":{"colors":{"light":{"base":"#FFF"},"dark":{"base":"#000"}}}}"##,
    )?;
    let result = validate(
        json!({"tsx":r##"<Box bg="#FFF"/>"##,"strict":true,"projectRoot":root.to_string_lossy()}),
    )
    .await;
    std::fs::remove_file(root.join("devup.json"))?;
    std::fs::remove_dir(root)?;
    let out = result?;
    assert!(out["nextAction"].is_null(), "{out}");
    Ok(())
}
