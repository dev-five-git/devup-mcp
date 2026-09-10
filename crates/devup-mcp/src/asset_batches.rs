//! Offline union of saved export responses. Never upgrades projection quality
//! or treats historical file-state claims as a fresh filesystem verification.
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub fn merge_files(paths: &[PathBuf]) -> anyhow::Result<Value> {
    anyhow::ensure!(
        !paths.is_empty(),
        "--merge-asset-batches requires JSON response paths"
    );
    let values = paths
        .iter()
        .map(|path| -> anyhow::Result<Value> {
            let bytes = std::fs::read(path)?;
            let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
            Ok(serde_json::from_slice(bytes)?)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    merge(&values)
}

fn unwrap_response(value: &Value) -> anyhow::Result<Value> {
    if let Some(response) = value.get("response") {
        return unwrap_response(response);
    }
    if let Some(response) = value.get("structuredContent") {
        return unwrap_response(response);
    }
    if let Some(content) = value["content"].as_array() {
        for item in content {
            if item["type"] == "text"
                && let Some(text) = item["text"].as_str()
                && let Ok(parsed) = serde_json::from_str::<Value>(text)
            {
                return unwrap_response(&parsed);
            }
        }
        anyhow::bail!("Response content contains no export JSON");
    }
    anyhow::ensure!(
        value["source"]["fileKey"].as_str().is_some(),
        "Each batch must contain its export source.fileKey; pending jobs and bare manifests cannot establish source identity"
    );
    anyhow::ensure!(
        value["assetSummary"].is_object(),
        "Each batch must contain assetSummary discovery evidence"
    );
    Ok(value.clone())
}

#[derive(Default)]
struct Asset {
    captures: BTreeMap<(String, u64), Value>,
    reasons: BTreeSet<String>,
    paths: BTreeSet<String>,
    written: bool,
    requested: bool,
}

pub fn merge(values: &[Value]) -> anyhow::Result<Value> {
    anyhow::ensure!(!values.is_empty(), "No batch responses supplied");
    let mut assets: BTreeMap<String, Asset> = BTreeMap::new();
    let mut file_key = None;
    let mut versions = BTreeSet::new();
    let mut unknown_version = false;
    let mut roots = BTreeSet::new();
    let mut incomplete = false;
    let mut incomplete_inventory_batches = 0;
    for value in values {
        let value = unwrap_response(value)?;
        let file = value["source"]["fileKey"].as_str().unwrap().to_owned();
        anyhow::ensure!(
            file_key.as_ref().is_none_or(|key| key == &file),
            "Cannot merge different Figma files"
        );
        file_key = Some(file);
        if let Some(version) = value["source"]["version"].as_str() {
            versions.insert(version.to_owned());
        } else {
            unknown_version = true;
        }
        anyhow::ensure!(versions.len() <= 1, "Cannot merge different Figma versions");
        incomplete |= value["assetSummary"]["discovery"] != "complete";
        for root in value["assetSummary"]["scopeRootIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            roots.insert(root.to_owned());
        }
        let mut batch_ids = BTreeSet::new();
        let mut captured_ids = BTreeSet::new();
        for item in value["assetSummary"]["unavailable"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(id) = item["assetId"].as_str() {
                batch_ids.insert(id.to_owned());
                assets
                    .entry(id.to_owned())
                    .or_default()
                    .reasons
                    .insert(item["reason"].as_str().unwrap_or("unknown").to_owned());
            }
        }
        for (items, requested) in [
            (&value["assetManifest"]["assets"], false),
            (&value["assetJob"]["assets"], true),
        ] {
            for item in items.as_array().into_iter().flatten() {
                let Some(id) = item["assetId"].as_str() else {
                    anyhow::bail!("Asset entry is missing assetId");
                };
                batch_ids.insert(id.to_owned());
                let asset = assets.entry(id.to_owned()).or_default();
                asset.requested |= requested;
                if item["status"] == "exported" {
                    let hash = item["sha256"]
                        .as_str()
                        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()));
                    let size = item["byteLength"].as_u64().filter(|v| *v > 0);
                    if let (Some(hash), Some(size)) = (hash, size) {
                        captured_ids.insert(id.to_owned());
                        asset
                            .captures
                            .insert((hash.to_ascii_lowercase(), size), item.clone());
                        asset.written |= item["fileState"] == "written";
                        if let Some(path) = item["outputPath"].as_str() {
                            asset.paths.insert(path.to_owned());
                        }
                    } else {
                        asset.reasons.insert("missing-byte-evidence".into());
                    }
                } else if item["errorCode"].as_str().is_some_and(|s| !s.is_empty()) {
                    asset.reasons.insert(
                        if item["errorCode"] == "DEVUP_ASSET_NODE_HIDDEN" {
                            "hidden-node"
                        } else {
                            "export-failed"
                        }
                        .into(),
                    );
                }
            }
        }
        if value["assetSummary"]["discoveredCount"].as_u64() != Some(batch_ids.len() as u64)
            || value["assetSummary"]["collectedCount"].as_u64() != Some(captured_ids.len() as u64)
        {
            incomplete_inventory_batches += 1;
        }
    }
    let mut collected = 0;
    let mut failed = 0;
    let mut unrequested = 0;
    let mut excluded = 0;
    let mut pending = 0;
    let mut conflicts = 0;
    let mut written = 0;
    let manifest = assets.into_iter().map(|(id, asset)| {
        let state = if asset.captures.len() > 1 { conflicts += 1; "conflict" }
            else if asset.captures.len() == 1 { collected += 1; written += usize::from(asset.written); "collected" }
            else if asset.reasons.contains("export-failed") || asset.reasons.contains("missing-byte-evidence") { failed += 1; "failed" }
            else if asset.reasons.contains("hidden-node") { excluded += 1; "excluded" }
            else if asset.requested { pending += 1; "pending" }
            else { unrequested += 1; "unrequested" };
        json!({"assetId":id,"state":state,"reportedWritten":asset.written && state == "collected",
            "outputPaths":asset.paths,"observedReasons":asset.reasons,
            "captures":asset.captures.into_iter().map(|((sha256,byte_length), _)| json!({"sha256":sha256,"byteLength":byte_length})).collect::<Vec<_>>()})
    }).collect::<Vec<_>>();
    let complete = !incomplete
        && incomplete_inventory_batches == 0
        && failed + unrequested + pending + conflicts == 0;
    Ok(json!({"status":if complete {"complete"} else {"partial"},
        "description":"Cumulative binary collection across the supplied batches only. Per-batch partial can mean assets were not requested in that batch; it is not an export failure. Projection quality is not merged. File writes are historical reports, not reverified files.",
        "scope":"union-of-supplied-batches", "inventoryComplete":incomplete_inventory_batches == 0,
        "incompleteInventoryBatchCount":incomplete_inventory_batches, "source":{"fileKey":file_key,"versions":versions,"versionVerified":!unknown_version},
        "scopeRootIds":roots,"batchCount":values.len(),"discovery":if incomplete {"incomplete"} else {"complete"},
        "discoveredCount":manifest.len(),"collectedCount":collected,"failedCount":failed,
        "unrequestedCount":unrequested,"pendingCount":pending,"excludedCount":excluded,"conflictCount":conflicts,
        "reportedWrittenCount":written,"assets":manifest}))
}
