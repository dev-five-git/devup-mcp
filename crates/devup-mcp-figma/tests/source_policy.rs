use devup_mcp_figma::{
    ErrorCode, SourcePolicy, UpstreamFailureContext, UpstreamFailureKind, classify_upstream_failure,
};

#[test]
fn classifies_upstream_failures_from_boundary_metadata() {
    let cases = [
        (
            UpstreamFailureContext::RegisterClient,
            Some(403),
            "client is not approved for the Figma MCP Catalog",
            UpstreamFailureKind::CatalogRejected,
        ),
        (
            UpstreamFailureContext::Connect,
            Some(401),
            "unauthorized",
            UpstreamFailureKind::AuthUnavailable,
        ),
        (
            UpstreamFailureContext::ListTools,
            None,
            "required tool use_figma is unavailable",
            UpstreamFailureKind::CapabilityUnavailable,
        ),
        (
            UpstreamFailureContext::CallTool,
            Some(403),
            "forbidden",
            UpstreamFailureKind::PermissionDenied,
        ),
        (
            UpstreamFailureContext::CallTool,
            Some(429),
            "too many requests",
            UpstreamFailureKind::RateLimited,
        ),
        (
            UpstreamFailureContext::CallTool,
            Some(404),
            "node not found",
            UpstreamFailureKind::NodeNotFound,
        ),
        (
            UpstreamFailureContext::Decode,
            None,
            "invalid json",
            UpstreamFailureKind::InvalidResponse,
        ),
        (
            UpstreamFailureContext::Connect,
            None,
            "connection reset",
            UpstreamFailureKind::Transport,
        ),
    ];

    for (context, status, message, expected) in cases {
        assert_eq!(
            classify_upstream_failure(context, status, message),
            expected,
            "{context:?}: {message}"
        );
    }
}

#[test]
fn public_policy_and_error_codes_have_stable_json_values() {
    assert_eq!(serde_json::to_value(SourcePolicy::Auto).unwrap(), "auto");
    assert_eq!(
        serde_json::to_value(SourcePolicy::Direct).unwrap(),
        "direct"
    );
    let codes = [
        (
            ErrorCode::DevupFigmaDirectUnavailable,
            "DEVUP_FIGMA_DIRECT_UNAVAILABLE",
        ),
        (
            ErrorCode::DevupFigmaCatalogRejected,
            "DEVUP_FIGMA_CATALOG_REJECTED",
        ),
        (
            ErrorCode::DevupFigmaHostRequired,
            "DEVUP_FIGMA_HOST_REQUIRED",
        ),
        (
            ErrorCode::DevupFigmaHandoffExpired,
            "DEVUP_FIGMA_HANDOFF_EXPIRED",
        ),
        (
            ErrorCode::DevupFigmaHandoffInvalid,
            "DEVUP_FIGMA_HANDOFF_INVALID",
        ),
        (
            ErrorCode::DevupCompatCorpusDrift,
            "DEVUP_COMPAT_CORPUS_DRIFT",
        ),
    ];
    for (code, expected) in codes {
        assert_eq!(serde_json::to_value(code).unwrap(), expected);
    }
}

#[test]
fn classified_errors_never_copy_the_raw_upstream_message() {
    let raw = "catalog rejected Authorization: Bearer figma-secret-token";
    let kind = classify_upstream_failure(UpstreamFailureContext::RegisterClient, Some(403), raw);
    let error = kind.into_devup_error(Some(403));
    let serialized = serde_json::to_string(&error).unwrap();

    assert_eq!(error.code, ErrorCode::DevupFigmaCatalogRejected);
    assert!(serialized.contains("\"source\":\"direct\""));
    assert!(serialized.contains("\"status\":403"));
    assert!(!serialized.contains("figma-secret-token"));
    assert!(!serialized.contains("Authorization"));
}
