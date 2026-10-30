use super::*;

#[test]
fn p3_expired_artifact_is_state_failure_with_original_message() {
    let error = to_mcp_error(DevupError::new(
        ErrorCode::DevupFigmaHandoffExpired,
        "The Figma artifact is missing or expired.",
        true,
    ));
    assert_eq!(error.code, McpErrorCode::INTERNAL_ERROR);
    assert_eq!(error.message, "The Figma artifact is missing or expired.");
}

#[test]
fn p3_output_root_violation_is_invalid_params() {
    let policy = OutputPolicy::from_roots(vec![std::env::current_dir().unwrap()]).unwrap();
    let error = policy.resolve("../outside.svg").err().unwrap();
    assert_eq!(to_mcp_error(error).code, McpErrorCode::INVALID_PARAMS);
}

#[test]
fn r7_errors_have_the_same_build_identity_as_success() {
    let success = serde_json::to_value(tool_result(json!({"status":"complete"}))).unwrap();
    let success: Value =
        serde_json::from_str(success["content"][0]["text"].as_str().unwrap()).unwrap();
    for code in [
        ErrorCode::DevupInvalidInput,
        ErrorCode::DevupSnapshotUnsupported,
        ErrorCode::DevupFigmaHandoffExpired,
    ] {
        let error = to_mcp_error(DevupError::new(code, "failure", false));
        assert_eq!(error.data.as_ref().unwrap()["server"], success["server"]);
    }
}

#[test]
fn r7_plugin_expiry_has_recovery_and_keeps_its_stage() {
    let raw = json!({"isError":true,"content":[{"type":"text","text":"DEVUP_FIGMA_HANDOFF_EXPIRED"}]});
    let error = to_mcp_error(operation::upstream_error(&raw).unwrap());
    let data = error.data.unwrap();
    assert_eq!(data["code"], "DEVUP_FIGMA_HANDOFF_EXPIRED");
    assert_eq!(data["details"]["stage"], "plugin-execution");
    assert_eq!(data["details"]["nextAction"]["arguments"]["refresh"], true);
}

#[test]
fn r7_expired_artifact_explains_refresh_arguments() {
    let error = to_mcp_error(DevupError::new(
        ErrorCode::DevupFigmaHandoffExpired,
        "expired",
        true,
    ));
    let d = &error.data.as_ref().unwrap()["details"];
    assert_eq!(d["stage"], "artifact-lookup");
    assert_eq!(d["nextAction"]["tool"], "devup_figma_export");
    assert_eq!(d["nextAction"]["arguments"]["refresh"], true);
    assert!(
        d["nextAction"]["omitArguments"]
            .as_array()
            .unwrap()
            .contains(&json!("artifactId"))
    );
}

#[test]
fn r8_expired_artifact_returns_known_url_or_explicit_unrecoverable_state() {
    for known in [true, false] {
        let details = if known {
            json!({"artifactState":"expired","recoveryArguments":{"url":"https://www.figma.com/design/test?node-id=1-2","frameIds":["2:1"],"refresh":true}})
        } else {
            json!({})
        };
        let e = to_mcp_error(DevupError::with_details(
            ErrorCode::DevupFigmaHandoffExpired,
            "expired",
            true,
            details,
        ));
        let d = &e.data.unwrap()["details"];
        if known {
            assert_eq!(
                d["nextAction"]["arguments"]["url"],
                "https://www.figma.com/design/test?node-id=1-2"
            );
        } else {
            assert_eq!(d["artifactState"], "unknown");
            assert_eq!(d["recoveryState"], "unrecoverable");
        }
    }
}
