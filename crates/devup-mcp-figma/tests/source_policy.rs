use devup_mcp_figma::{
    ErrorCode, SourcePolicy, UpstreamFailureContext, UpstreamFailureKind,
    classify_upstream_failure, fallback_allowed, fallback_allowed_for_error,
    upstream_failure_error,
};

#[test]
fn auto_falls_back_only_for_identity_or_capability_failures() {
    use UpstreamFailureKind::{
        AuthUnavailable, CapabilityUnavailable, CatalogRejected, NodeNotFound, PermissionDenied,
        RateLimited, VersionChanged,
    };

    for kind in [
        CatalogRejected,
        AuthUnavailable,
        CapabilityUnavailable,
        PermissionDenied,
    ] {
        assert!(fallback_allowed(SourcePolicy::Auto, kind), "{kind:?}");
        assert!(!fallback_allowed(SourcePolicy::Direct, kind), "{kind:?}");
        assert!(!fallback_allowed(SourcePolicy::Host, kind), "{kind:?}");
    }

    for kind in [RateLimited, NodeNotFound, VersionChanged] {
        assert!(!fallback_allowed(SourcePolicy::Auto, kind), "{kind:?}");
    }
}

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
    assert_eq!(serde_json::to_value(SourcePolicy::Host).unwrap(), "host");
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

#[test]
fn auto_can_decide_fallback_from_the_safe_public_error() {
    let catalog = upstream_failure_error(
        UpstreamFailureContext::Connect,
        Some(403),
        "Figma MCP Catalog rejected bearer-secret",
    );
    assert!(fallback_allowed_for_error(SourcePolicy::Auto, &catalog));
    assert!(!fallback_allowed_for_error(SourcePolicy::Direct, &catalog));

    let rate_limited =
        upstream_failure_error(UpstreamFailureContext::CallTool, Some(429), "bearer-secret");
    assert!(!fallback_allowed_for_error(
        SourcePolicy::Auto,
        &rate_limited
    ));
    assert_eq!(rate_limited.code, ErrorCode::DevupFigmaRateLimited);
    assert!(
        !serde_json::to_string(&rate_limited)
            .unwrap()
            .contains("bearer-secret")
    );
}
