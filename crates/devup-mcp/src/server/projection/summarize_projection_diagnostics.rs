use super::*;

pub(super) struct ProjectionDiagnostics<'a> {
    pub(super) payload: &'a CollectedPayload,
    pub(super) outputs: &'a Vec<String>,
    pub(super) include_diagnostics: bool,
    pub(super) root_layout: devup_mcp_devup_ui::codegen::RootLayout,
    pub(super) asset_names_per_node: bool,
    pub(super) scope: &'a String,
    pub(super) strict: bool,
    pub(super) frame_ids: &'a Vec<String>,
    pub(super) all_screens: bool,
    pub(super) asset_captures: &'a Vec<devup_mcp_figma::AssetSelection>,
    pub(super) delivery: DeliveryMode,
    pub(super) result: Map<String, Value>,
    pub(super) failures: Vec<Value>,
    pub(super) warnings: Vec<Value>,
    pub(super) completeness_report: &'a devup_mcp_figma::PayloadCompletenessReport,
    pub(super) section_tsx_projected: bool,
    pub(super) projection_diagnostics: Vec<Diagnostic>,
    pub(super) fidelity_reports: Vec<devup_mcp_devup_ui::provenance::FidelityReport>,
    pub(super) theme_conflict_count: usize,
    pub(super) theme_unresolved_count: usize,
    pub(super) component_name_for_components: &'a Option<String>,
    pub(super) target_kind: TargetKind,
}

pub(super) fn summarize_projection_diagnostics(
    input: ProjectionDiagnostics<'_>,
) -> Result<(Map<String, Value>,), DevupError> {
    let ProjectionDiagnostics {
        payload,
        outputs,
        include_diagnostics,
        root_layout,
        asset_names_per_node,
        scope,
        strict,
        frame_ids,
        all_screens,
        asset_captures,
        delivery,
        mut result,
        failures,
        warnings,
        completeness_report,
        section_tsx_projected,
        mut projection_diagnostics,
        fidelity_reports,
        theme_conflict_count,
        theme_unresolved_count,
        component_name_for_components,
        target_kind,
    } = input;
    for failure in failures.iter().filter(|f| f.get("output").is_some()) {
        let issue = failure_issue(failure);
        projection_diagnostics.push(issue.clone());
        if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
            for frame in frames
                .iter_mut()
                .filter(|f| f["nodeId"] == failure["nodeId"])
            {
                frame["projectionIssues"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(issue));
            }
        }
    }
    responsive_groups::completion(&mut result, frame_ids, all_screens);
    result.insert(
        "placementContracts".into(),
        json!(
            projection_diagnostics
                .iter()
                .filter(|d| d.code == "DEVUP_CODEGEN_PLACEMENT_CONTRACT")
                .collect::<Vec<_>>()
        ),
    );
    result.insert(
        "projectionIssues".into(),
        json!(
            projection_diagnostics
                .iter()
                .filter(
                    |d| d.fidelity_impact() != devup_mcp_figma::FidelityImpact::None
                        || d.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"
                )
                .collect::<Vec<_>>()
        ),
    );
    result.insert(
        "projectionEvidence".into(),
        json!(
            projection_diagnostics
                .iter()
                .filter(|d| {
                    d.fidelity_impact() == devup_mcp_figma::FidelityImpact::None
                        && matches!(
                            d.code.as_str(),
                            "DEVUP_CODEGEN_NON_RENDERING_ASSET"
                                | "DEVUP_CODEGEN_ABSOLUTE_VERIFIED"
                                | "DEVUP_CODEGEN_LAYOUT_ACCOUNTED_FOR"
                                | "DEVUP_CODEGEN_PROPERTY_EVIDENCE"
                        )
                })
                .collect::<Vec<_>>()
        ),
    );
    let mut quality = OutputQuality {
        acquisition: acquisition_quality(completeness_report, false),
        projection: projection_quality(
            outputs.iter().any(|output| {
                matches!(
                    output.as_str(),
                    "tsx" | "componentTsx" | "responsiveTsx" | "pageScaffold"
                )
            }),
            &projection_diagnostics,
        ),
        theme: theme_quality(
            outputs.iter().any(|output| output == "devupJson"),
            theme_conflict_count,
            theme_unresolved_count,
        ),
        assets: assets_quality(
            outputs.iter().any(|output| output == "assetManifest"),
            &asset_captures
                .iter()
                .map(|capture| capture.asset_id.clone())
                .collect::<Vec<_>>(),
            &payload.assets,
        ),
    };
    if asset_captures.is_empty() && let Some(summary) = result.get("assetSummary") {
        quality.assets = asset_collection_quality(summary);
    }
    if failures
        .iter()
        .any(|failure| failure.get("assetId").is_some())
    {
        quality.assets = super::super::quality::AssetsQuality::Partial;
    }
    if failures
        .iter()
        .any(|failure| failure.get("output").is_some())
    {
        quality.projection = super::super::quality::ProjectionQuality::Lossy;
    }
    if !warnings.is_empty() {
        result.insert("warnings".to_owned(), json!(warnings));
    }
    if target_kind == TargetKind::Section && !failures.is_empty() {
        let failed_frames = failures
            .iter()
            .filter(|failure| failure.get("assetId").is_none())
            .filter_map(|failure| failure["nodeId"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if !failed_frames.is_empty() {
            let mut arguments = json!({"url":payload.target.link(payload.target.node_id.as_deref()),
            "frameIds":failed_frames,"outputs":outputs,"scope":scope,"delivery":delivery,
            "rootLayout":match root_layout { devup_mcp_devup_ui::codegen::RootLayout::Standalone => "standalone", devup_mcp_devup_ui::codegen::RootLayout::Embedded => "embedded" },
            "assetNamesPerNode":asset_names_per_node,"strict":strict,"includeDiagnostics":include_diagnostics});
            if let Some(name) = &component_name_for_components {
                arguments["componentName"] = json!(name);
            }
            result.insert("resume".into(), json!({"tool":"devup_figma_export", "arguments":arguments,
            "note":"Retry only failed frames. Missing collected nodes require a fresh URL acquisition; component master failures may require collecting the master or requesting tsx."}));
        }
    }
    result.insert("failures".to_owned(), json!(failures));
    let fidelity_violation = fidelity_reports
        .iter()
        .any(|report| !report.strict_compatible());
    if strict && (quality.strict_violation() || fidelity_violation || !failures.is_empty()) {
        return Err(DevupError::with_details(
            ErrorCode::DevupSnapshotUnsupported,
            format!(
                "strict export only allows exact/complete output: status={}, quality={}",
                quality.status(),
                serde_json::to_string(&quality).unwrap_or_default()
            ),
            false,
            json!({
                "quality": quality,
                "fidelity": fidelity_reports,
                "failures": failures,
                "completenessReport": completeness_report,
                "projectionIssues": result.get("projectionIssues")
            }),
        ));
    }
    let final_status = if failures.is_empty() {
        quality.status()
    } else {
        "partial"
    };
    result.insert("status".to_owned(), json!(final_status));
    result.insert("quality".to_owned(), json!(quality));
    if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames {
            let id = frame["nodeId"].as_str().unwrap_or_default().to_owned();
            let frame_failures = failures
                .iter()
                .filter(|f| f["nodeId"] == id)
                .cloned()
                .collect::<Vec<_>>();
            attach_review_and_deliverable(
                frame.as_object_mut().unwrap(),
                outputs,
                &frame_failures,
                Some(&id),
            );
            if !frame_failures.is_empty() {
                frame["failures"] = json!(frame_failures);
            }
        }
    }
    attach_review_and_deliverable(
        &mut result,
        outputs,
        &failures,
        payload.target.node_id.as_deref(),
    );
    // Only the whole-node tsx is missing its fidelity report at this
    // point; each Section frame already carries its own.
    if !section_tsx_projected && let Some(report) = fidelity_reports.first() {
        let mut carrier = Value::Object(Map::new());
        attach_fidelity(&mut carrier, report, include_diagnostics);
        if let Some(fidelity) = carrier.get("fidelity") {
            result.insert("fidelity".to_owned(), fidelity.clone());
        }
    }
    let mut carrier = Value::Object(Map::new());
    attach_completeness_report(
        &mut carrier,
        quality,
        completeness_report,
        include_diagnostics,
    );
    if let Some(report) = carrier.get("completenessReport") {
        result.insert("completenessReport".to_owned(), report.clone());
    }
    Ok((result,))
}
