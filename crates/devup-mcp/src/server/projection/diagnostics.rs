/// Adds `completenessReport` only when it has something to say.
///
/// `quality.acquisition` already grades the capture, and on a clean one the
/// report underneath it is six empty arrays and a row of counters that agree
/// with the grade. Measured on one export it was 360 of the 1,911 bytes every
/// response carried before this, none of it actionable. It is attached
/// whenever the capture is not clean, and whenever the caller asked for
/// diagnostics and therefore wants the detail regardless.
/// Adds `fidelity` whenever the report has something `quality` does not say.
///
/// Keyed on the report itself, not on `quality.projection`. That was the
/// first attempt and it hid a real defect: `projection_quality` is computed
/// from diagnostics alone, so a coverage shortfall that raises no diagnostic
/// leaves the grade reading `exact` and took the report away with it.
/// Measured on a real 50-node screen, `variables` was 19 of 20 - a `$primary`
/// binding frozen into an exported SVG - while `quality.projection` said
/// `exact`, so the response went from showing the shortfall to hiding it.
///
/// `strict_compatible` is the same predicate `strict: true` refuses on, which
/// keeps the two from disagreeing about whether anything was lost. On the ten
/// real captured screens it is true - nothing sent - for every screen with no
/// shortfall and no approximation, which is what this is for.
fn attach_fidelity(
    response: &mut Value,
    report: &devup_mcp_devup_ui::provenance::FidelityReport,
    include_diagnostics: bool,
) {
    if include_diagnostics || !report.strict_compatible() {
        response["fidelity"] = json!(report);
    }
}

fn attach_completeness_report(
    response: &mut Value,
    quality: OutputQuality,
    report: &devup_mcp_figma::PayloadCompletenessReport,
    include_diagnostics: bool,
) {
    let clean = matches!(
        quality.acquisition,
        AcquisitionQuality::Complete | AcquisitionQuality::ExpectedProjection
    );
    if include_diagnostics || !clean {
        response["completenessReport"] = json!(report);
    }
}

fn unavailable_output(payload: &CollectedPayload, output: &str, reason: &str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupInvalidInput,
        format!("{output} cannot be produced. {reason}"),
        false,
        json!({"output":output,"nodeId":payload.target.node_id,"snapshotRootIds":payload.snapshot.roots,
            "collectedTargetIds":payload.snapshot.roots}),
    )
}

fn projection_failure(
    payload: &CollectedPayload,
    node_id: &str,
    output: &str,
    error: &DevupError,
) -> Value {
    let mut details = error.details.as_object().cloned().unwrap_or_default();
    details.insert("nodeId".into(), json!(node_id));
    details.insert("snapshotRootIds".into(), json!(payload.snapshot.roots));
    details.insert("collectedTargetIds".into(), json!(payload.snapshot.roots));
    json!({"nodeId":node_id,"output":output,"errorCode":error.code,"message":error.message,
        "retryable":error.retryable,"details":details})
}
