use devup_mcp_figma::{
    ErrorCode, UpstreamFailureContext, UpstreamFailureKind, classify_upstream_failure,
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
fn error_codes_have_stable_json_values() {
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
            ErrorCode::DevupFigmaHandoffExpired,
            "DEVUP_FIGMA_HANDOFF_EXPIRED",
        ),
        (
            ErrorCode::DevupFigmaHandoffInvalid,
            "DEVUP_FIGMA_HANDOFF_INVALID",
        ),
        (ErrorCode::DevupInvalidInput, "DEVUP_INVALID_INPUT"),
    ];
    for (code, expected) in codes {
        assert_eq!(serde_json::to_value(code).unwrap(), expected);
    }
}

/// This split is what decides whether the caller sees INVALID_PARAMS or
/// INTERNAL_ERROR, which for an agent is the difference between fixing its
/// arguments and giving up. Getting a code onto the wrong side is silent —
/// the response still looks well formed — so both sides are pinned here.
///
/// A variant added later and left unclassified falls to the `false` side and
/// is reported as INTERNAL_ERROR, which is the safe direction: the caller
/// stops instead of retrying a call that will never succeed.
#[test]
fn caller_mistakes_are_separated_from_failures_behind_the_call() {
    for code in [
        ErrorCode::DevupInvalidInput,
        ErrorCode::DevupFigmaNodeNotFound,
        ErrorCode::DevupFigmaUnsupportedFile,
        ErrorCode::DevupProjectRootNotFound,
        ErrorCode::DevupFigmaHandoffInvalid,
        ErrorCode::DevupFigmaHandoffExpired,
    ] {
        assert!(
            code.is_caller_mistake(),
            "{code:?} is fixable by the caller"
        );
    }

    for code in [
        // Authentication, the network, and Figma itself: nothing the caller
        // can repair by changing an argument.
        ErrorCode::DevupAuthRequired,
        ErrorCode::DevupAuthCallbackTimeout,
        ErrorCode::DevupAuthStateMismatch,
        ErrorCode::DevupFigmaCallbackPortInUse,
        ErrorCode::DevupFigmaPermissionDenied,
        ErrorCode::DevupFigmaRateLimited,
        ErrorCode::DevupFigmaDirectUnavailable,
        ErrorCode::DevupFigmaCatalogRejected,
        ErrorCode::DevupFigmaResponseTooLarge,
        ErrorCode::DevupFigmaVersionChanged,
        // Conditions found in the design or the generated code, not in the
        // arguments: a theme whose collections disagree is a real conflict,
        // and reporting it as a bad parameter would send the caller looking
        // for a typo it will not find.
        ErrorCode::DevupSnapshotUnsupported,
        ErrorCode::DevupCodegenFailed,
        ErrorCode::DevupThemeConflict,
    ] {
        assert!(
            !code.is_caller_mistake(),
            "{code:?} is not fixable by changing the call"
        );
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
