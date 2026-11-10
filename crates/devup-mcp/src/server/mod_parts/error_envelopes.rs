/// Maps a [`DevupError`] onto the JSON-RPC error the caller actually sees.
///
/// The protocol code is not decoration here: the caller is usually an agent
/// choosing between "fix the arguments and call again" and "stop and report",
/// and every error used to arrive as INTERNAL_ERROR, which says the second
/// thing about both. A mistake in the call itself is INVALID_PARAMS, so that
/// decision can be made without parsing the message. `data` keeps carrying
/// the exact `code` and `retryable`, unchanged.
fn to_mcp_error(mut error: DevupError) -> ErrorData {
    if error.code == ErrorCode::DevupFigmaHandoffExpired
        && error.details.get("exportJob").is_none()
        && error.details.get("assetJob").is_none()
    {
        if !error.details.is_object() {
            error.details = json!({"upstreamDetails":error.details});
        }
        if error.details.get("stage").is_none() {
            error.details["stage"] = json!("artifact-lookup");
        }
        if error.details.get("artifactState").is_none() {
            error.details["artifactState"] = json!("unknown");
        }
        let recovery = error.details.get("recoveryArguments").cloned();
        error.details["recoveryState"] = json!(if recovery.is_some() {
            "available"
        } else {
            "unrecoverable"
        });
        let another_process = error.details["artifactState"] == "issued-by-another-process";
        if recovery.is_none() {
            error.details["recoveryReason"] = json!(if another_process {
                include_str!("../text/mod-to_mcp_error-784239dd.md")
            } else {
                "Original URL/selection cannot be recovered from this server. The ID may be unknown, evicted beyond metadata retention, or lost on restart; the cause cannot be determined."
            });
        }
        error.details["recovery"] = json!({
            "mode": "recollect",
            "cause": match error.details["artifactState"].as_str() {
                Some("expired") => "artifact-expired",
                Some("evicted") => "artifact-evicted",
                Some("issued-by-another-process") => "issued-by-another-process",
                _ => "artifact-unknown",
            },
            "reprojectable": false,
            "requiresOriginalUrl": recovery.is_none(),
        });
        error.details["nextAction"] = json!({"tool":"devup_figma_export",
            "arguments":{"refresh":true},"requiredArguments":["url"],"omitArguments":["artifactId"],
            "how":include_str!("../text/mod-to_mcp_error-20941350.md")});
        if let Some(arguments) = recovery {
            error.details["nextAction"]["arguments"] = arguments;
        }
    }
    if error.details["recoveryArguments"]["nodeIds"].is_array() {
        error.details["nextAction"] = json!({"tool":"devup_figma_measure","arguments":error.details["recoveryArguments"],
            "how":"Repeat the explicit measured node list; a measurement receipt never recovers by exporting the current selection."});
    }
    let mcp_code = if validation::is_caller_mistake(&error) {
        McpErrorCode::INVALID_PARAMS
    } else {
        McpErrorCode::INTERNAL_ERROR
    };
    ErrorData::new(
        mcp_code,
        error.message,
        Some(
            json!({ "code": error.code, "retryable": error.retryable, "details": error.details, "server":delivery::server_identity() }),
        ),
    )
}

fn structured_tool_error(error: ErrorData) -> rmcp::model::CallToolResult {
    let mut data = error.data.unwrap_or_else(|| json!({}));
    if !data.is_object() {
        data = json!({"details":data});
    }
    data.as_object_mut().unwrap().remove("server");
    if data.get("code").is_none() {
        data["code"] = json!("DEVUP_INVALID_INPUT");
    }
    if data.get("retryable").is_none() {
        data["retryable"] = json!(false);
    }
    if data.get("details").is_none() {
        data["details"] = json!({});
    }
    data["message"] = json!(error.message);
    data["rpcCode"] = json!(error.code);
    let mut result = tool_result(json!({"error":data}));
    result.is_error = Some(true);
    result
}

