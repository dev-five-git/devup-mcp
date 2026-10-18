fn materialize_asset_resource_references(
    result: &mut Value,
    outputs: &mut [ProjectedOutput],
    artifact_id: &str,
) -> Result<(), DevupError> {
    let Some(assets) = result
        .get_mut("assetManifest")
        .and_then(|manifest| manifest.get_mut("assets"))
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    for output in outputs.iter().filter(|output| output.asset_id.is_some()) {
        let asset_id = output.asset_id.as_deref().unwrap_or_default();
        if !assets
            .iter()
            .any(|asset| asset["assetId"].as_str() == Some(asset_id))
        {
            return Err(DevupError::new(
                ErrorCode::DevupCodegenFailed,
                "No manifest entry matches this asset resource.",
                false,
            ));
        }
        let format = assets
            .iter()
            .find(|asset| asset["assetId"].as_str() == Some(asset_id))
            .expect("asset checked above")["format"]
            .clone();
        let hash = sha256_hex(&output.bytes);
        for asset in assets.iter_mut() {
            for variant in asset
                .get_mut("variants")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                if variant["sha256"].as_str() == Some(hash.as_str())
                    && variant["mimeType"] == output.mime_type
                    && variant["byteLength"].as_u64() == Some(output.bytes.len() as u64)
                {
                    let variant = variant.as_object_mut().expect("variant object");
                    variant.remove("dataBase64");
                    variant.insert("resource".into(),json!({"uri":output.manifest_uri(artifact_id),"mimeType":output.mime_type,"byteLength":output.bytes.len(),"sha256":hash}));
                }
            }
        }
        for asset in assets.iter_mut().filter(|asset| {
            asset["status"] == "exported"
                && asset["sha256"].as_str() == Some(hash.as_str())
                && asset["mimeType"] == output.mime_type
                && asset["format"] == format
        }) {
            let asset = asset.as_object_mut().expect("manifest asset object");
            asset.remove("dataBase64");
            asset.insert(
                "resource".to_owned(),
                json!({
                    "uri":output.manifest_uri(artifact_id),"mimeType":output.mime_type,
                    "byteLength":output.bytes.len(),"sha256":hash,"assetMetadata":output.asset_metadata
                }),
            );
        }
    }
    if let Some(manifest_output) = outputs
        .iter_mut()
        .find(|output| output.name == "asset-manifest.json")
    {
        manifest_output.bytes =
            encode_projected_json(result.get("assetManifest").ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "The asset manifest resource is missing.",
                    false,
                )
            })?)?;
    }
    Ok(())
}

fn projected_asset_outputs(manifest: &AssetManifest) -> Result<Vec<ProjectedOutput>, DevupError> {
    let mut outputs = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let invalid =
        |message: &str| DevupError::new(ErrorCode::DevupSnapshotUnsupported, message, false);
    for (index, asset) in manifest
        .assets
        .iter()
        .enumerate()
        .filter(|(_, a)| a.status == AssetStatus::Exported)
    {
        let mut renditions = vec![(
            asset.data_base64.as_deref(),
            asset.byte_length,
            asset.sha256.as_deref(),
            asset.mime_type.as_deref(),
            None,
        )];
        renditions.extend(asset.variants.iter().map(|v| {
            (
                v.data_base64.as_deref(),
                Some(v.byte_length),
                Some(v.sha256.as_str()),
                Some(v.mime_type.as_str()),
                Some(v),
            )
        }));
        for (data, length, hash, mime, variant) in renditions {
            let data =
                data.ok_or_else(|| invalid("The exported asset binary is not in the artifact."))?;
            if data.len() > devup_mcp_figma::MAX_ASSET_BYTES.div_ceil(3) * 4 {
                return Err(invalid("Asset base64 exceeds its encoded budget."));
            }
            let bytes = STANDARD
                .decode(data)
                .map_err(|_| invalid("The exported asset binary base64 is invalid."))?;
            let mime = mime.ok_or_else(|| invalid("The exported asset has no MIME type."))?;
            if length != Some(bytes.len()) || hash != Some(sha256_hex(&bytes).as_str()) {
                return Err(invalid("The exported asset length or hash does not match."));
            }
            if let Some(variant) = variant {
                if mime != "image/webp" || !variant.density.is_finite() || variant.density <= 1.0 {
                    return Err(invalid("Invalid srcset derivative identity."));
                }
                let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
                    .with_guessed_format()
                    .map_err(|_| invalid("Invalid variant codec."))?;
                if reader.into_dimensions().ok() != Some((variant.width, variant.height)) {
                    return Err(invalid("Variant encoded dimensions do not match."));
                }
            }
            let extension = if variant.is_some() {
                "webp"
            } else {
                asset
                    .format
                    .map_or("bin", devup_mcp_figma::AssetFormat::extension)
            };
            if !seen.insert((sha256_hex(&bytes), extension.to_owned(), mime.to_owned())) {
                continue;
            }
            let mut output = ProjectedOutput::asset(
                format!(
                    "asset-{}{}.{extension}",
                    index + 1,
                    if variant.is_some() { "-2x" } else { "" }
                ),
                mime,
                bytes,
                &asset.asset_id,
            );
            output.asset_metadata = Some(
                json!({"assets":manifest.assets.iter().filter(|other|other.status==AssetStatus::Exported&&(other.sha256.as_deref()==hash || other.variants.iter().any(|v|Some(v.sha256.as_str())==hash))).map(|other|json!({"assetId":other.asset_id,"nodeId":other.node_id,"renderMode":other.render_mode,"format":other.format,"scale":other.scale,"quality":other.quality,"lossless":other.lossless,"maxWidth":other.max_width,"srcset":other.srcset,"visibleScopeNodeId":other.visible_scope_node_id,"renderInfo":other.render_info,"suggestedFilename":other.suggested_filename})).collect::<Vec<_>>(),"density":variant.map(|v|v.density)}),
            );
            outputs.push(output);
        }
    }
    Ok(outputs)
}
