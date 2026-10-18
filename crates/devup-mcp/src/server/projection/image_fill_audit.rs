use super::*;

pub(super) fn attach_image_fill_audit(
    result: &mut Map<String, Value>,
    payload: &CollectedPayload,
    completeness_report: &devup_mcp_figma::PayloadCompletenessReport,
    outputs: &[String],
    asset_names_per_node: bool,
) {
    result.insert(
        "imageFillAudit".into(),
        devup_mcp_figma::image_fill_audit(&payload.snapshot, &payload.assets),
    );
    let mut audit_assets = devup_mcp_figma::discover_image_fill_assets(&payload.snapshot);
    for captured in &payload.assets {
        if let Some(existing) = audit_assets
            .iter_mut()
            .find(|a| a.asset_id == captured.asset_id)
        {
            *existing = captured.clone();
        }
    }
    let audit_css = asset_policy::css_map(&payload.snapshot, &audit_assets, asset_names_per_node);
    asset_policy::enrich_audit(result.get_mut("imageFillAudit").unwrap(), &audit_css);
    result.get_mut("imageFillAudit").unwrap()["collectionCompleteness"] = json!({"state":completeness_report.state,"snapshotState":completeness_report.snapshot.state,"resourcesState":completeness_report.resources.state,"unresolvedResources":completeness_report.resources.unresolved_count,"failedScreens":payload.failures.len()});
    result.get_mut("imageFillAudit").unwrap()["resourceCompleteness"] = json!(payload.completeness);
    if payload.metadata.get("sectionIndex").is_some()
        && payload.metadata.get("selectedRootIds").is_none()
    {
        let audit = result.get_mut("imageFillAudit").unwrap();
        audit["scope"] = json!("selection-index-only; descendant image paints not acquired");
        audit["completeness"] = json!("selection-required");
        audit["inScopeImageFills"] = Value::Null;
        audit["summary"] = json!(
            "Section index only: select frameIds to acquire and audit descendant image fills."
        );
    }
    if outputs.iter().any(|o| o == "imageAudit") {
        result.insert("imageAudit".into(), result["imageFillAudit"].clone());
    }
}
