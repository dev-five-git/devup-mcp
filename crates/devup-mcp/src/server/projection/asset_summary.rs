/// This describes collection, even when the caller did not request a manifest.
/// Missing evidence is never reported as proof that the design has no assets.
fn asset_summary(
    payload: &CollectedPayload,
    artifact: &ArtifactLookup,
    roots: &[String],
    manifest_requested: bool,
) -> Value {
    let mut snapshot = payload.snapshot.clone();
    snapshot.roots = roots.to_vec();
    let mut reachable = std::collections::BTreeSet::new();
    let mut pending = roots.to_vec();
    while let Some(id) = pending.pop() {
        if !reachable.insert(id.clone()) {
            continue;
        }
        if let Some(node) = snapshot.nodes.get(&id) {
            pending.extend(node.typed_view().child_ids().map(str::to_owned));
        }
    }
    snapshot.nodes.retain(|id, _| reachable.contains(id));
    let mut assets = asset_policy::discovery(
        &snapshot,
        &payload.assets,
        artifact.capabilities.web_ready_images,
    )
    .assets;
    for captured in payload
        .assets
        .iter()
        .filter(|asset| reachable.contains(&asset.node_id))
    {
        if let Some(existing) = assets
            .iter_mut()
            .find(|asset| asset.asset_id == captured.asset_id)
        {
            *existing = captured.clone();
        } else {
            assets.push(captured.clone());
        }
    }
    let index_only = artifact.capabilities.kind == super::artifacts::ArtifactKind::SectionIndex;
    let incomplete = index_only
        || snapshot.audit().state != devup_mcp_figma::CompletenessState::Complete
        || snapshot
            .nodes
            .values()
            .any(|node| node.typed_view().bool("projectionTruncated") == Some(true));
    let collected = assets
        .iter()
        .filter(|asset| asset.status == AssetStatus::Exported)
        .count();
    let unavailable = assets.iter().filter(|asset| asset.status != AssetStatus::Exported).map(|asset| {
        let reason = match asset.error_code.as_deref() {
            Some("DEVUP_ASSET_NODE_HIDDEN") => "hidden-node",
            Some(_) => "export-failed",
            None if artifact.capabilities.asset_capture_count == 0 => "not-requested",
            None => "capture-not-in-artifact",
        };
        let mut entry = json!({"assetId":asset.asset_id,"nodeId":asset.node_id,"reason":reason,"errorCode":asset.error_code});
        if let Some(detail) = &asset.error_detail {
            entry["errorDetail"] = json!(detail);
        }
        entry
    }).collect::<Vec<_>>();
    let excluded = assets
        .iter()
        .filter(|asset| asset.error_code.as_deref() == Some("DEVUP_ASSET_NODE_HIDDEN"))
        .count();
    let status = if assets.is_empty() {
        if incomplete { "unknown" } else { "none" }
    } else if collected == 0 {
        "not-collected"
    } else if collected < assets.len() - excluded || incomplete {
        "partial"
    } else {
        "collected"
    };
    let description = match status {
        "none" => "Complete discovery found no assets.",
        "unknown" => "Incomplete discovery cannot establish whether assets exist.",
        "not-collected" => {
            "Assets were discovered but no binary bytes were collected; unavailable explains unrequested captures, hidden exclusions, or export failures."
        }
        "partial" => {
            include_str!("../text/projection-asset_summary-2538c470.md")
        }
        "collected" => "All discovered non-hidden deliverable assets have collected binary bytes.",
        _ => unreachable!(),
    };
    json!({"status":status,"description":description,"excludedCount":excluded,"scopeRootIds":roots,
        "discovery":if incomplete { "incomplete" } else { "complete" },
        "discoveryReason":if index_only { Some("index-only") } else if incomplete { Some("snapshot-incomplete") } else { None },
        "discoveredCount":assets.len(),"collectedCount":collected,
        "manifestIncluded":manifest_requested,"unavailable":unavailable,
        "batchMerge":{"command":"devup-mcp --merge-asset-batches batch1.json batch2.json","input":"Saved complete export responses, MCP response envelopes, or poll records containing response","scope":"union-of-supplied-batches"}})
}

/// Binary collection quality, independent of whether a manifest was delivered.
fn asset_collection_quality(summary: &Value) -> super::quality::AssetsQuality {
    use super::quality::AssetsQuality;
    match summary["status"].as_str() {
        Some("collected") => AssetsQuality::Complete,
        Some("partial") => AssetsQuality::Partial,
        Some("not-collected")
            if summary["unavailable"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    matches!(
                        item["reason"].as_str(),
                        Some("export-failed" | "capture-not-in-artifact")
                    )
                })
            }) =>
        {
            AssetsQuality::Partial
        }
        _ => AssetsQuality::NotRequested,
    }
}
