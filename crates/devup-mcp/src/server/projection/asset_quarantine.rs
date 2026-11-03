//! Unnamed assets are retained privately; no application source/public writes escape this gate.
use super::*;
use std::path::{Path, PathBuf};

pub(super) struct Request<'a> {
    pub manifest: &'a AssetManifest,
    pub registry: &'a Value,
    pub artifact_id: &'a str,
    pub policy: &'a OutputPolicy,
    pub public_root: Option<&'a Path>,
    pub project_root: Option<&'a str>,
    pub recovery_arguments: &'a Value,
    pub write: bool,
}

fn private_base(request: &Request<'_>) -> Option<PathBuf> {
    let base = request
        .project_root
        .map(PathBuf::from)
        .or_else(|| {
            request
                .public_root
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|| request.policy.primary_root().to_path_buf());
    let target = request
        .policy
        .resolve(&base.join(".devup/asset-staging/.probe").to_string_lossy())
        .ok()?;
    let path = target.display_path().parent()?.to_path_buf();
    if let Some(root) = request.public_root {
        let public = request
            .policy
            .resolve(&root.join(".probe").to_string_lossy())
            .ok()?;
        if path.starts_with(public.display_path().parent()?) {
            return None;
        }
    }
    Some(path)
}

pub(super) fn retain(request: Request<'_>) -> Result<Value, DevupError> {
    let artifact = request
        .artifact_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(96)
        .collect::<String>();
    let revision=sha256_hex(&serde_json::to_vec(&json!({"manifest":request.manifest,"review":request.registry["namingReview"],"arguments":request.recovery_arguments})).unwrap());
    let folder = private_base(&request).map(|path| path.join(&artifact).join(&revision[..16]));
    let mut transaction = OutputTransaction::new();
    let mut staged = BTreeMap::new();
    let mut records = Vec::new();
    let required = request.registry["namingReview"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for asset in &request.manifest.assets {
        if asset.status != AssetStatus::Exported {
            continue;
        }
        let bytes = STANDARD
            .decode(asset.data_base64.as_deref().unwrap_or_default())
            .map_err(|_| {
                DevupError::new(
                    ErrorCode::DevupInvalidInput,
                    "Invalid quarantine asset bytes",
                    false,
                )
            })?;
        let hash = sha256_hex(&bytes);
        let extension = asset
            .format
            .map_or("bin", devup_mcp_figma::AssetFormat::extension);
        let stage = folder
            .as_ref()
            .map(|folder| folder.join(format!("{hash}.{extension}")));
        if request.write
            && let Some(path) = &stage
            && staged.insert(path.clone(), hash.clone()).is_none()
        {
            transaction.stage_asset(
                format!("staged:{hash}"),
                request.policy.resolve(&path.to_string_lossy())?,
                &bytes,
            )?;
        }
        records.push(json!({"assetId":asset.asset_id,"nodeId":asset.node_id,"sha256":hash,"byteLength":bytes.len(),"format":asset.format,"proposedPublicPath":asset.path,"stagingPath":stage,"namingRequired":required.iter().any(|r|r["assetId"]==asset.asset_id),"published":false}));
    }
    let mut arguments = request.recovery_arguments.clone();
    if !arguments.is_object() {
        arguments = json!({});
    }
    arguments["artifactId"] = json!(request.artifact_id);
    arguments["assetNamesPerNode"] = json!(true);
    let receipt = json!({"version":1,"artifactId":request.artifact_id,"records":records,"namingReview":required,"reprojectionArguments":arguments,"finalization":"Choose meaningful assetRequests.outputPath values for every namingRequired record and reproject this same artifact. The export transaction publishes assets, registry and TSX together only after naming passes. If the artifact expires, reacquire the original Figma nodes; staged bytes are retained as evidence."});
    if request.write
        && let Some(folder) = &folder
    {
        transaction.stage_asset(
            "quarantineReceipt",
            request
                .policy
                .resolve(&folder.join("receipt.json").to_string_lossy())?,
            &serde_json::to_vec_pretty(&receipt).unwrap(),
        )?;
    }
    let output_paths = if request.write {
        transaction.commit()?
    } else {
        BTreeMap::new()
    };
    Ok(
        json!({"status":"review-required","artifactId":request.artifact_id,"deliverable":{"isFinal":false,"codeDelivered":false,"assetNamingVerified":false,"state":"asset-naming-review-required"},"assetRegistry":request.registry,"assetQuarantine":{"state":"review-required","published":false,"codeDelivered":false,"retainedInArtifact":true,"persisted":request.write&&folder.is_some(),"records":records,"outputPaths":output_paths,"privateStagingAvailable":folder.is_some(),"stagingRequirement":if folder.is_none(){"Authorize an application/workspace directory outside public/ to persist private staging. No public .devup directory is used for quarantine."}else{"Private staging is separate from public publication."}},"outputPaths":{},"nextAction":{"tool":"devup_figma_export","arguments":arguments,"requiresNamingEdits":true,"requiredEdits":required,"how":"Set descriptive assetRequests.outputPath values; no TSX, public asset or public registry was written by this attempt."}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unnamed_export_retains_bytes_privately_and_never_returns_code() {
        let root = std::env::temp_dir().join(format!("devup-quarantine-{}", rand::random::<u64>()));
        std::fs::create_dir_all(root.join("public")).unwrap();
        let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
        let manifest:AssetManifest=serde_json::from_value(json!({"version":1,"assets":[{"assetId":"opaque","nodeId":"1:1","field":"node","sourceKind":"node","status":"exported","format":"png","path":"/images/image.png","mimeType":"image/png","sha256":sha256_hex(b"picture"),"byteLength":7,"dataBase64":STANDARD.encode(b"picture")}],"diagnostics":[]})).unwrap();
        let registry = json!({"namingComplete":false,"namingReview":[{"assetId":"opaque","reason":"anonymous-source-label"}]});
        let result = retain(Request {
            manifest: &manifest,
            registry: &registry,
            artifact_id: "artifact",
            policy: &policy,
            public_root: Some(&root.join("public")),
            project_root: Some(root.to_str().unwrap()),
            recovery_arguments: &json!({"outputs":["tsx","assetManifest"]}),
            write: true,
        })
        .unwrap();
        assert_eq!(result["assetQuarantine"]["published"], false);
        assert_eq!(result["deliverable"]["codeDelivered"], false);
        assert!(result.get("tsx").is_none());
        let staged = result["assetQuarantine"]["records"][0]["stagingPath"]
            .as_str()
            .unwrap();
        assert_eq!(std::fs::read(staged).unwrap(), b"picture");
        assert!(!root.join("public/images/image.png").exists());
        assert!(!root.join(".devup/public-assets.json").exists());
        drop(policy);
        std::fs::remove_dir_all(root).unwrap();
    }
}
