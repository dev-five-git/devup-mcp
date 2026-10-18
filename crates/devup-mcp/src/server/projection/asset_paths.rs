/// Reconcile only exports backed by verified bytes. Preserve an entry per node,
/// but make identical content use one file and one code reference.
fn reconcile_asset_paths(
    manifest: &mut AssetManifest,
    output_paths: &mut BTreeMap<String, String>,
    public_root: Option<&std::path::Path>,
    policy: &OutputPolicy,
) -> Result<BTreeMap<String, String>, DevupError> {
    // This verifies decoded length/hash as well as MIME before any write.
    projected_asset_outputs(manifest)?;
    let mut groups = BTreeMap::<_, Vec<usize>>::new();
    for (index, asset) in manifest.assets.iter().enumerate() {
        if asset.status != AssetStatus::Exported {
            continue;
        }
        let bytes = STANDARD
            .decode(asset.data_base64.as_deref().unwrap_or_default())
            .map_err(|_| {
                DevupError::new(
                    ErrorCode::DevupCodegenFailed,
                    "Invalid exported asset base64.",
                    false,
                )
            })?;
        let key = (
            sha256_hex(&bytes),
            asset.format.map(|format| format.extension()),
            asset.mime_type.clone(),
            asset
                .variants
                .iter()
                .map(|v| (v.sha256.clone(), v.width, v.height))
                .collect::<Vec<_>>(),
        );
        groups.entry(key).or_default().push(index);
    }
    let mut replacements = BTreeMap::new();
    for members in groups.values() {
        // Canonical code names must survive a later code-only projection.
        // Select the placeholder independently from the requested file target.
        let canonical_path = members
            .iter()
            .find_map(|&index| manifest.assets[index].path.clone());
        let output_path = members
            .iter()
            .find_map(|&index| output_paths.get(&manifest.assets[index].asset_id).cloned());
        let canonical_path = canonical_path.map(|p| {
            if manifest.assets[members[0]].format == Some(devup_mcp_figma::AssetFormat::Webp) {
                p.rsplit_once('.')
                    .map_or_else(|| format!("{p}.webp"), |(stem, _)| format!("{stem}.webp"))
            } else {
                p
            }
        });
        let path = if let (Some(root), Some(output)) = (public_root, &output_path) {
            Some(super::validation::public_asset_url(
                root,
                policy.resolve(output)?.display_path(),
            )?)
        } else {
            canonical_path
        };
        for &index in members {
            let asset = &mut manifest.assets[index];
            if let Some(output) = &output_path {
                output_paths.insert(asset.asset_id.clone(), output.clone());
            }
            let old_path = std::mem::replace(&mut asset.path, path.clone());
            if let (Some(old), Some(new)) = (&old_path, &path)
                && !asset.variants.is_empty()
            {
                replacements.insert(
                    asset_policy::variant_path(old),
                    asset_policy::variant_path(new),
                );
            }
            if let (Some(old), Some(new)) = (old_path, path.as_ref())
                && old != *new
                && let Some(previous) = replacements.insert(old, new.clone())
                && previous != *new
            {
                return Err(DevupError::new(
                    ErrorCode::DevupInvalidInput,
                    "One placeholder refers to different exported assets. Use assetNamesPerNode:true.",
                    false,
                ));
            }
        }
    }
    Ok(replacements)
}

fn rewrite_asset_references(code: &str, paths: &BTreeMap<String, String>) -> String {
    // One pass prevents a replacement from becoming another replacement's input.
    let mut result = String::with_capacity(code.len());
    let mut cursor = 0;
    while cursor < code.len() {
        let rest = &code[cursor..];
        let matched = paths.iter().find(|(old, _)| {
            rest.starts_with(old.as_str())
                && (cursor == 0
                    || matches!(
                        code.as_bytes()[cursor - 1],
                        b'"' | b'\'' | b'(' | b' ' | b','
                    ))
                && rest
                    .as_bytes()
                    .get(old.len())
                    .is_none_or(|next| matches!(next, b'"' | b'\'' | b')' | b' ' | b','))
        });
        if let Some((old, new)) = matched {
            result.push_str(new);
            cursor += old.len();
        } else {
            let character = rest.chars().next().expect("nonempty remainder");
            result.push(character);
            cursor += character.len_utf8();
        }
    }
    result
}

fn rewrite_result_asset_references(value: &mut Value, paths: &BTreeMap<String, String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(
                    key.as_str(),
                    "tsx"
                        | "componentTsx"
                        | "responsiveTsx"
                        | "generatedProperty"
                        | "generatedSource"
                        | "srcSet"
                ) {
                    match value {
                        Value::String(code) => {
                            *code = if key == "srcSet" {
                                rewrite_asset_references(code, paths)
                            } else {
                                rewrite_asset_references(&with_host_safe_asset_paths(code), paths)
                            }
                        }
                        Value::Array(items) => {
                            for item in items {
                                if let Value::String(code) = item {
                                    *code = rewrite_asset_references(
                                        &with_host_safe_asset_paths(code),
                                        paths,
                                    );
                                } else {
                                    rewrite_result_asset_references(item, paths);
                                }
                            }
                        }
                        _ => rewrite_result_asset_references(value, paths),
                    }
                } else if matches!(key.as_str(), "assetPath" | "path") {
                    if let Value::String(path) = value
                        && let Some(replacement) = paths.get(path)
                    {
                        *path = replacement.clone();
                    }
                } else if !matches!(key.as_str(), "rawSnapshot" | "rawPayload" | "originalValue") {
                    rewrite_result_asset_references(value, paths);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                rewrite_result_asset_references(value, paths);
            }
        }
        _ => {}
    }
}
