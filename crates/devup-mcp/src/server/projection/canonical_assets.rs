//! Project-wide asset identities and semantic public paths, committed with their assets.
use super::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    path: String,
    identity: String,
    sha256: String,
    mime_type: String,
    #[serde(default)]
    mask: bool,
    #[serde(default)]
    geometry: Option<String>,
    #[serde(default)]
    sources: BTreeSet<String>,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Registry {
    version: u32,
    assets: Vec<Record>,
}

pub(super) struct Resolution {
    pub replacements: BTreeMap<String, String>,
    pub registry_write: Option<(String, Vec<u8>, Option<String>)>,
    pub registry: Value,
    pub filters: BTreeMap<String, String>,
}

struct Identity {
    key: String,
    geometry: Option<String>,
    filter: Option<String>,
}

fn error(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}

fn meaningful_name(path: &str) -> bool {
    let decoded = super::super::mask_guard::percent_decode(path);
    let path = decoded.as_str();
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .rsplit_once('.')
        .map_or(path, |(name, _)| name);
    name.split(['-', '_']).any(|part| {
        let part = part.trim_end_matches(|c: char| c.is_ascii_digit());
        !part.is_empty()
            && !part.chars().all(|c| c.is_ascii_digit())
            && !(part.len() >= 16 && part.chars().all(|c| c.is_ascii_hexdigit()))
            && ![
                "image",
                "asset",
                "frame",
                "screen",
                "rectangle",
                "vector",
                "group",
                "layer",
                "shape",
                "node",
                "fill",
                "section",
                "instance",
                "component",
                "default",
                "variant",
                "small",
                "large",
                "mobile",
                "desktop",
                "alternative",
            ]
            .contains(&part.to_lowercase().as_str())
    })
}

fn identity(asset: &devup_mcp_figma::AssetManifestEntry, bytes: &[u8], mask: bool) -> Identity {
    let mime = asset
        .mime_type
        .as_deref()
        .unwrap_or("application/octet-stream");
    let svg = (mime == "image/svg+xml")
        .then(|| super::svg_identity::identity(bytes, mask))
        .flatten();
    let variants = asset
        .variants
        .iter()
        .map(|v| format!("{}:{}:{}:{}", v.density, v.width, v.height, v.sha256))
        .collect::<Vec<_>>()
        .join("|");
    Identity {
        key: format!(
            "{mime}:{}:{variants}",
            svg.as_ref()
                .map_or_else(|| sha256_hex(bytes), |svg| svg.key.clone())
        ),
        geometry: svg.as_ref().map(|svg| svg.geometry.clone()),
        filter: svg.and_then(|svg| svg.grayscale_filter),
    }
}

fn codes(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.as_str(), "tsx" | "componentTsx" | "responsiveTsx") {
                    if let Some(source) = value.as_str() {
                        out.push(with_host_safe_asset_paths(source));
                    }
                } else if !matches!(key.as_str(), "rawSnapshot" | "rawPayload" | "originalValue") {
                    codes(value, out);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                codes(value, out);
            }
        }
        _ => {}
    }
}

fn usage(sources: &[String], path: &str) -> (bool, bool) {
    let mut mask = false;
    let mut image = false;
    let mut other = false;
    for source in sources {
        let attributes = devup_mcp_devup_ui::provenance::attributes::generated_attributes(source);
        for attr in attributes.iter().filter(|a| references(&a.source, path)) {
            if attr.name == "maskImage" {
                mask = true;
            } else if attr.name == "src"
                && matches!(attr.tag.as_str(), "Image" | "img")
                && !attributes
                    .iter()
                    .any(|a| a.element == attr.element && a.name == "filter")
            {
                image = true;
            } else {
                other = true;
            }
        }
    }
    (mask && !image && !other, image && !mask && !other)
}

fn references(source: &str, path: &str) -> bool {
    source.match_indices(path).any(|(at, _)| {
        (at == 0 || matches!(source.as_bytes()[at - 1], b'"' | b'\'' | b'(' | b' ' | b','))
            && source
                .as_bytes()
                .get(at + path.len())
                .is_none_or(|next| matches!(next, b'"' | b'\'' | b')' | b' ' | b','))
    })
}

fn read(path: &Path, policy: &OutputPolicy, limit: usize) -> Result<Option<Vec<u8>>, DevupError> {
    let target = policy.resolve(&path.to_string_lossy())?;
    match std::fs::symlink_metadata(target.display_path()) {
        Ok(_) => target.read_bounded(limit).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(error(
            "Cannot inspect the canonical asset registry or file.",
        )),
    }
}

fn public_target(root: &Path, url: &str, policy: &OutputPolicy) -> Result<PathBuf, DevupError> {
    let decoded = super::super::mask_guard::percent_decode(url);
    let url = decoded.as_str();
    if !url.starts_with('/')
        || url.starts_with("//")
        || url.split('/').skip(1).any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.contains(['\\', '?', '#', '\n', '\r'])
        })
    {
        return Err(error("Asset registry contains an unsafe public path."));
    }
    let target = policy.resolve(&root.join(url.trim_start_matches('/')).to_string_lossy())?;
    Ok(target.display_path().to_path_buf())
}

fn registry_path(root: &Path, policy: &OutputPolicy) -> Result<PathBuf, DevupError> {
    let sibling = root
        .parent()
        .unwrap_or(root)
        .join(".devup/public-assets.json");
    // A caller may scope writes to the public root itself. Never widen that authority
    // merely to put metadata in the usual application sibling directory.
    let target = policy
        .resolve(&sibling.to_string_lossy())
        .or_else(|_| policy.resolve(&root.join(".devup/public-assets.json").to_string_lossy()))?;
    Ok(target.display_path().to_path_buf())
}

fn imported_records(
    root: &Path,
    project: Option<&str>,
    policy: &OutputPolicy,
) -> Result<Vec<Record>, DevupError> {
    let mut records = Vec::new();
    let registry = registry_path(root, policy)?;
    if let Some(bytes) = read(&registry, policy, 4 * 1024 * 1024)? {
        let stored: Registry = serde_json::from_slice(&bytes).map_err(|_| {
            error("Canonical asset registry JSON is invalid; repair it before export.")
        })?;
        if stored.version != 1 {
            return Err(error("Unsupported canonical asset registry version."));
        }
        records = stored.assets;
    }
    if let Some(project) = project {
        let path = Path::new(project).join("docs/public-assets.json");
        if let Some(bytes) = read(&path, policy, 4 * 1024 * 1024)? {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| error("Project public asset registry JSON is invalid."))?;
            for entry in value["assets"].as_array().into_iter().flatten() {
                let (Some(url), Some(hash)) = (entry["path"].as_str(), entry["sha256"].as_str())
                else {
                    continue;
                };
                if records.iter().any(|record| record.path == url) {
                    continue;
                }
                let path = public_target(root, url, policy)?;
                let Some(bytes) = read(&path, policy, devup_mcp_figma::MAX_ASSET_BYTES)? else {
                    continue;
                };
                if sha256_hex(&bytes) != hash {
                    continue;
                }
                let mime = if url.ends_with(".svg") {
                    "image/svg+xml"
                } else if url.ends_with(".webp") {
                    "image/webp"
                } else if url.ends_with(".png") {
                    "image/png"
                } else if url.ends_with(".jpg") || url.ends_with(".jpeg") {
                    "image/jpeg"
                } else if url.ends_with(".pdf") {
                    "application/pdf"
                } else {
                    continue;
                };
                let mask = entry["usage"] == "mask";
                let svg = (mime == "image/svg+xml")
                    .then(|| super::svg_identity::identity(&bytes, mask))
                    .flatten();
                records.push(Record {
                    path: url.into(),
                    identity: format!(
                        "{mime}:{}:",
                        svg.as_ref().map_or_else(|| hash.into(), |s| s.key.clone())
                    ),
                    sha256: hash.into(),
                    mime_type: mime.into(),
                    mask,
                    geometry: svg.map(|s| s.geometry),
                    sources: BTreeSet::new(),
                });
            }
        }
    }
    Ok(records)
}

pub(super) fn reconcile(
    manifest: &mut AssetManifest,
    outputs: &mut BTreeMap<String, String>,
    root: Option<&Path>,
    project: Option<&str>,
    policy: &OutputPolicy,
    result: &Map<String, Value>,
) -> Result<Resolution, DevupError> {
    let normalized_root = root
        .map(|root| {
            policy
                .resolve(&root.join(".devup-root").to_string_lossy())
                .map(|target| {
                    target
                        .display_path()
                        .parent()
                        .expect("asset parent")
                        .to_path_buf()
                })
        })
        .transpose()?;
    let root = normalized_root.as_deref();
    let registry_base = if let Some(root) = root {
        read(&registry_path(root, policy)?, policy, 4 * 1024 * 1024)?
    } else {
        None
    };
    projected_asset_outputs(manifest)?;
    let mut sources = Vec::new();
    for value in result.values() {
        codes(value, &mut sources);
    }
    // Include top-level TSX fields as well as frame outputs.
    for key in ["tsx", "componentTsx", "responsiveTsx"] {
        if let Some(code) = result.get(key).and_then(Value::as_str) {
            sources.push(with_host_safe_asset_paths(code));
        }
    }
    let imported = if let Some(root) = root {
        imported_records(root, project, policy)?
    } else {
        Vec::new()
    };
    let mut valid = Vec::new();
    for record in imported {
        if let Some(root) = root
            && let Some(bytes) = read(
                &public_target(root, &record.path, policy)?,
                policy,
                devup_mcp_figma::MAX_ASSET_BYTES,
            )?
            && sha256_hex(&bytes) == record.sha256
        {
            valid.push(record);
        }
    }
    let mut registry = Registry {
        version: 1,
        assets: valid,
    };
    let mut captures = Vec::new();
    for (index, asset) in manifest
        .assets
        .iter()
        .enumerate()
        .filter(|(_, a)| a.status == AssetStatus::Exported)
    {
        let bytes = STANDARD
            .decode(asset.data_base64.as_deref().unwrap_or_default())
            .map_err(|_| error("Invalid asset base64."))?;
        let path = asset.path.as_deref().unwrap_or("/images/image.png");
        let (mask, image) = usage(&sources, path);
        let identity = identity(asset, &bytes, mask);
        captures.push((index, bytes, identity, mask, image));
    }
    captures.sort_by_key(|(_, _, identity, _, _)| identity.filter.is_some());
    let mut replacements = BTreeMap::new();
    let mut naming_review = Vec::new();
    let mut filters = BTreeMap::new();
    let mut bytes_by_path = BTreeMap::<String, Vec<u8>>::new();
    for (index, acquired, identity, mask, image) in captures {
        let asset = &mut manifest.assets[index];
        let old = asset.path.clone().unwrap_or_else(|| {
            format!(
                "/images/{}",
                host_safe_file_name(asset.suggested_filename.as_deref().unwrap_or("image.png"))
            )
        });
        let mime = asset.mime_type.clone().unwrap_or_default();
        let mut existing = registry
            .assets
            .iter()
            .position(|r| r.identity == identity.key);
        let mut filter = None;
        if existing.is_none() && image && identity.filter.is_some() {
            existing = registry.assets.iter().position(|r| {
                r.geometry.is_some()
                    && r.geometry == identity.geometry
                    && r.mime_type == mime
                    && !r.mask
            });
            if existing.is_some() {
                filter = identity.filter.clone();
            }
        }
        let mut canonical = None;
        if let Some(position) = existing {
            let record = &registry.assets[position];
            let bytes = if let Some(bytes) = bytes_by_path.get(&record.path) {
                Some(bytes.clone())
            } else if let Some(root) = root {
                read(
                    &public_target(root, &record.path, policy)?,
                    policy,
                    devup_mcp_figma::MAX_ASSET_BYTES,
                )?
            } else {
                None
            };
            if let Some(bytes) = bytes
                && sha256_hex(&bytes) == record.sha256
            {
                registry.assets[position]
                    .sources
                    .insert(asset.asset_id.clone());
                canonical = Some((registry.assets[position].path.clone(), bytes));
            } else {
                existing = None;
                filter = None;
            }
        }
        let (path, bytes) = if let Some(canonical) = canonical {
            canonical
        } else {
            let explicit = root.is_some() && outputs.contains_key(&asset.asset_id);
            let proposed = if let (Some(root), Some(output)) = (root, outputs.get(&asset.asset_id))
            {
                super::super::validation::public_asset_url(
                    root,
                    policy.resolve(output)?.display_path(),
                )?
            } else {
                old.clone()
            };
            let extension = asset
                .format
                .map_or("bin", devup_mcp_figma::AssetFormat::extension);
            let stem = proposed
                .rsplit_once('/')
                .map_or(proposed.as_str(), |(_, s)| s)
                .rsplit_once('.')
                .map_or("image", |(s, _)| s);
            let folder = proposed
                .rsplit_once('/')
                .map(|(folder, _)| folder.trim_start_matches('/'))
                .filter(|folder| !folder.is_empty())
                .unwrap_or(if extension == "svg" {
                    "icons"
                } else {
                    "images"
                });
            let semantic = host_safe_file_name(&format!("{stem}.{extension}"));
            let mut path = if explicit {
                proposed.clone()
            } else {
                format!("/{folder}/{semantic}")
            };
            let mut attempt = 0;
            loop {
                let occupied = registry
                    .assets
                    .iter()
                    .any(|r| r.path.eq_ignore_ascii_case(&path) && r.identity != identity.key);
                let disk = if let Some(root) = root {
                    read(
                        &public_target(root, &path, policy)?,
                        policy,
                        devup_mcp_figma::MAX_ASSET_BYTES,
                    )?
                } else {
                    None
                };
                if !occupied && disk.as_ref().is_none_or(|bytes| *bytes == acquired) {
                    break;
                }
                attempt += 1;
                if attempt > 16 {
                    return Err(error(
                        "Asset semantic name is ambiguous; supply a distinct content or crop name.",
                    ));
                }
                let suffix = if attempt == 1 {
                    "-alternative".into()
                } else {
                    format!("-alternative-{attempt}")
                };
                path = format!("/{folder}/{stem}{suffix}.{extension}");
            }
            if attempt > 0 {
                naming_review.push(json!({"assetId":asset.asset_id,"nodeId":asset.node_id,"path":path,"reason":"different-content-name-collision","verifiedSemanticName":false,"nextAction":"Choose a content/crop-specific assetRequests.outputPath; numeric collision suffix is a provisional path, not completed naming."}));
            }
            registry.assets.push(Record {
                path: path.clone(),
                identity: identity.key.clone(),
                sha256: sha256_hex(&acquired),
                mime_type: mime.clone(),
                mask,
                geometry: identity.geometry.clone(),
                sources: BTreeSet::from([asset.asset_id.clone()]),
            });
            (path, acquired)
        };
        let previous = asset.sha256.clone();
        if let Some(prior) = replacements.insert(old.clone(), path.clone())
            && prior != path
        {
            return Err(error(
                "One code placeholder identifies different pictures. Re-export with assetNamesPerNode:true; no files were written.",
            ));
        }
        if !asset.variants.is_empty() {
            replacements.insert(
                asset_policy::variant_path(&old),
                asset_policy::variant_path(&path),
            );
        }
        if !asset.render_info.as_ref().is_some_and(Value::is_object) {
            asset.render_info = Some(json!({}));
        }
        if let Some(filter) = filter {
            filters.insert(old.clone(), filter.clone());
            asset.render_info.get_or_insert_with(|| json!({}))["canonicalPresentation"] = json!({"filter":filter,"status":"derived-flat-grayscale","preserveElementSize":true});
        }
        asset.render_info.get_or_insert_with(|| json!({}))["canonicalAsset"] = json!({"path":path,"requestedOutputPath":outputs.get(&asset.asset_id),"acquiredSha256":previous,"deliveredSha256":sha256_hex(&bytes),"identityPolicy":"exact-bytes-or-parsed-svg-identity; unsupported SVG remains distinct","intrinsicSize":"preserve use-site dimensions; do not rely on canonical intrinsic width"});
        asset.path = Some(path.clone());
        if !meaningful_name(&path) {
            naming_review.push(json!({"assetId":asset.asset_id,"nodeId":asset.node_id,"path":path,"reason":"anonymous-source-label","verifiedSemanticName":false,"nextAction":"Inspect the acquired image and source hierarchy, then reproject with a descriptive assetRequests.outputPath."}));
        }
        asset.sha256 = Some(sha256_hex(&bytes));
        asset.byte_length = Some(bytes.len());
        asset.data_base64 = Some(STANDARD.encode(&bytes));
        if let Some(root) = root
            && outputs.contains_key(&asset.asset_id) {
            outputs.insert(
                asset.asset_id.clone(),
                public_target(root, &path, policy)?.to_string_lossy().into(),
            );
        }
        bytes_by_path.insert(path, bytes);
        let _ = existing;
    }
    registry.assets.sort_by(|a, b| a.path.cmp(&b.path));
    let registry_write = if let Some(root) = root.filter(|_| !outputs.is_empty()) {
        let path = registry_path(root, policy)?;
        Some((
            path.to_string_lossy().into(),
            serde_json::to_vec_pretty(&registry)
                .map_err(|_| error("Cannot encode asset registry."))?,
            registry_base.as_ref().map(|bytes| sha256_hex(bytes)),
        ))
    } else {
        None
    };
    Ok(Resolution {
        replacements,
        registry_write,
        registry: json!({"version":1,"assetCount":registry.assets.len(),"namingComplete":naming_review.is_empty(),"namingReviewCount":naming_review.len(),"namingReview":naming_review,"assets":registry.assets,"policy":"semantic-public-paths; immutable-publication; no automatic unused deletion","unverified":["SVG paint/filter constructs not proven equivalent stay separate","Raster crop/resolution differences require source-identity or independent visual evidence before consolidation","Renaming persisted API/DB URLs requires a project migration; existing files are not deleted"]}),
        filters,
    })
}

pub(super) fn apply_filters(value: &mut Value, filters: &BTreeMap<String, String>) {
    match value {
        Value::Object(object) => {
            for field in ["tsx", "componentTsx", "responsiveTsx"] {
                let mut derived = Vec::new();
                if let Some(Value::String(code)) = object.get_mut(field) {
                    let attributes =
                        devup_mcp_devup_ui::provenance::attributes::generated_attributes(code);
                    let mut inserts = Vec::new();
                    let mut touched = BTreeSet::new();
                    for attr in attributes.iter().filter(|a| a.name == "src") {
                        if let Some((_, filter)) = filters
                            .iter()
                            .find(|(old, _)| references(&attr.source, old))
                        {
                            inserts.push((attr.start, format!("filter=\"{filter}\" ")));
                            touched.insert(attr.element);
                        }
                    }
                    inserts.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
                    for (at, insert) in inserts {
                        code.insert_str(at, &insert);
                    }
                    for attr in
                        devup_mcp_devup_ui::provenance::attributes::generated_attributes(code)
                            .into_iter()
                            .filter(|attr| attr.name == "filter" && touched.contains(&attr.element))
                    {
                        derived.push(json!({"elementIndex":attr.element,"tag":attr.tag,"generatedProperty":attr.source,"verified":true,"derivation":{"classification":"derived-canonical-grayscale","source":"parsed SVG geometry and uniform gray paint","scope":"exact paint transform; not a captured Figma filter","preserveElementSize":true}}));
                    }
                }
                if !derived.is_empty() {
                    let result = object
                        .entry("outputResults")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                        .expect("output results");
                    let output = result.entry(field).or_insert_with(|| json!({}));
                    if !output["_propertyContract"].is_array() {
                        output["_propertyContract"] = json!([]);
                    }
                    output["_propertyContract"]
                        .as_array_mut()
                        .unwrap()
                        .extend(derived.clone());
                    if !object
                        .get("projectionEvidence")
                        .is_some_and(Value::is_array)
                    {
                        object.insert("projectionEvidence".into(), json!([]));
                    }
                    object
                        .get_mut("projectionEvidence")
                        .unwrap()
                        .as_array_mut()
                        .unwrap()
                        .extend(derived);
                }
            }
            for (key, value) in object {
                if !matches!(
                    key.as_str(),
                    "tsx"
                        | "componentTsx"
                        | "responsiveTsx"
                        | "rawSnapshot"
                        | "rawPayload"
                        | "originalValue"
                ) {
                    apply_filters(value, filters);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                apply_filters(value, filters);
            }
        }
        _ => {}
    }
}

pub(super) fn enforce_naming_review(value: &mut Value) {
    if value["assetRegistry"]["namingComplete"] == false {
        if let Some(deliverable) = value.get_mut("deliverable").and_then(Value::as_object_mut) {
            deliverable.insert("isFinal".into(), json!(false));
            deliverable.insert("assetNamingVerified".into(), json!(false));
            deliverable.insert("assetNamingState".into(), json!("review-required"));
        }
        if value["status"] == "complete" {
            value["status"] = json!("partial");
        }
    }
}

#[cfg(test)]
#[path = "canonical_assets_tests.rs"]
mod tests;
