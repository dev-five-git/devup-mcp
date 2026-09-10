use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::result_contract::{
    attach_review_and_deliverable, failure_issue, output_result, scope_output,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_devup_ui::{
    codegen::{
        CodegenOptions, generate_component, normalize_component_name, responsive::merge_breakpoints,
    },
    theme::{generate_devup_json, variable_snapshot_from_result},
};
use devup_mcp_figma::{
    AssetManifest, AssetStatus, CollectedPayload, CollectionStats, DevupError, Diagnostic,
    DiagnosticSeverity, ErrorCode, ExploreOptions, SearchOptions, TargetKind, classify_target,
    explore_snapshot, search_snapshot,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{
    artifacts::{ArtifactLookup, ArtifactStore, OutputReservation},
    delivery::{DeliveryMode, ProjectedOutput, choose_delivery_for_result},
    format_epoch_rfc3339,
    operation::PendingOperation,
    output::{OutputPolicy, OutputTransaction},
    parse_scope,
    quality::{
        AcquisitionQuality, OutputQuality, ProjectionQuality, acquisition_quality, assets_quality,
        projection_quality, theme_quality,
    },
    section_candidate_as_explore, section_index_from_payload,
};

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
                && cursor > 0
                && matches!(code.as_bytes()[cursor - 1], b'"' | b'\'' | b'(')
                && rest
                    .as_bytes()
                    .get(old.len())
                    .is_some_and(|next| matches!(next, b'"' | b'\'' | b')'))
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
                ) {
                    match value {
                        Value::String(code) => {
                            *code =
                                rewrite_asset_references(&with_host_safe_asset_paths(code), paths)
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

pub(super) fn projected_outputs_from_result(
    result: &Map<String, Value>,
) -> Result<Vec<ProjectedOutput>, DevupError> {
    let mut outputs = Vec::new();
    for field in ["tsx", "componentTsx", "responsiveTsx"] {
        if let Some(tsx) = result.get(field).and_then(Value::as_str) {
            outputs.push(ProjectedOutput::text(
                field,
                "text/typescript",
                tsx.as_bytes().to_vec(),
            ));
        }
    }
    if let Some(devup_json) = result.get("devupJson").and_then(Value::as_str) {
        outputs.push(ProjectedOutput::text(
            "devupJson",
            "application/json",
            devup_json.as_bytes().to_vec(),
        ));
    }
    if let Some(reference) = result.get("referencePng") {
        let data = reference
            .get("dataBase64")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "The referencePng resource has no base64 data.",
                    false,
                )
            })?;
        let bytes = STANDARD.decode(data.as_bytes()).map_err(|_| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The referencePng resource base64 is invalid.",
                false,
            )
        })?;
        outputs.push(ProjectedOutput::binary("reference.png", "image/png", bytes));
    }
    for (field, name) in [
        ("rawSnapshot", "raw-snapshot.json"),
        ("rawPayload", "raw-payload.json"),
        ("sourceMap", "source-map.json"),
        ("assetManifest", "asset-manifest.json"),
    ] {
        if let Some(value) = result.get(field) {
            outputs.push(ProjectedOutput::text(
                name,
                "application/json",
                encode_projected_json(value)?,
            ));
        }
    }
    if let Some(frames) = result.get("frames").and_then(Value::as_array) {
        for (index, frame) in frames.iter().enumerate() {
            for field in ["tsx", "componentTsx"] {
                if let Some(tsx) = frame.get(field).and_then(Value::as_str) {
                    let name = if field == "tsx" {
                        format!("frame-{}.tsx", index + 1)
                    } else {
                        format!("frame-{}.component.tsx", index + 1)
                    };
                    outputs.push(ProjectedOutput::text(
                        name,
                        "text/typescript",
                        tsx.as_bytes().to_vec(),
                    ));
                }
            }
            if let Some(source_map) = frame.get("sourceMap") {
                outputs.push(ProjectedOutput::text(
                    format!("frame-{}.source-map.json", index + 1),
                    "application/json",
                    encode_projected_json(source_map)?,
                ));
            }
        }
    }
    Ok(outputs)
}

fn encode_projected_json(value: &Value) -> Result<Vec<u8>, DevupError> {
    serde_json::to_vec(value).map_err(|error| {
        DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            format!("Cannot serialize the resource output to JSON: {error}"),
            false,
        )
    })
}

pub(super) struct DeliveryAttachment {
    reservation: OutputReservation,
}

pub(super) async fn apply_delivery(
    result: &mut Value,
    mode: DeliveryMode,
    artifact_store: &ArtifactStore,
    artifact: &ArtifactLookup,
    mut outputs: Vec<ProjectedOutput>,
) -> Result<Option<DeliveryAttachment>, DevupError> {
    if outputs.is_empty() || choose_delivery_for_result(mode, result, &outputs)?.inline {
        return Ok(None);
    }
    let debug_outputs: Vec<_> = [
        "tsx",
        "componentTsx",
        "devupJson",
        "rawSnapshot",
        "rawPayload",
        "sourceMap",
    ]
    .into_iter()
    .filter(|field| {
        result.get(*field).is_some()
            || result["frames"]
                .as_array()
                .is_some_and(|frames| frames.iter().any(|f| f.get(*field).is_some()))
    })
    .collect();
    if mode == DeliveryMode::Auto
        && debug_outputs
            .iter()
            .any(|f| matches!(*f, "rawSnapshot" | "rawPayload" | "sourceMap"))
    {
        let frame_ids: Vec<_> = result["frames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f.get("nodeId").cloned())
            .collect();
        result["nextAction"] = json!({"how":"Debug outputs exceeded auto inline delivery. Read the linked resources, or reproject this artifact with delivery:inline. Inline has a 1 MiB total limit; request fewer frames/outputs if necessary.",
            "example":{"tool":"devup_figma_export","arguments":{"artifactId":artifact.artifact_id,"frameIds":frame_ids,"outputs":debug_outputs,"debug":true,"delivery":"inline"}}});
    }
    let projection_key = projection_key(&outputs);
    materialize_asset_resource_references(result, &mut outputs, &artifact.artifact_id)?;
    let reservation = artifact_store
        .reserve_outputs(&artifact.artifact_id, &projection_key, outputs)
        .await?;
    let manifests = reservation.manifests().to_vec();
    let result = result.as_object_mut().ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaHandoffInvalid,
            "The resource delivery result is not a JSON object.",
            false,
        )
    })?;
    for field in [
        "tsx",
        "componentTsx",
        "responsiveTsx",
        "devupJson",
        "rawSnapshot",
        "rawPayload",
        "sourceMap",
        "assetManifest",
        "referencePng",
    ] {
        result.remove(field);
    }
    if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames {
            if let Some(frame) = frame.as_object_mut() {
                frame.remove("tsx");
                frame.remove("componentTsx");
                frame.remove("sourceMap");
            }
        }
    }
    if let Some(deliverable) = result.get_mut("deliverable") {
        deliverable["note"] = json!(if deliverable["isFinal"] == true {
            "The final TSX deliverables are in resources. Read the linked resources to implement them."
        } else {
            "The available TSX outputs are in resources. Review projectionIssues and outputResults before using them; these outputs are not final."
        });
    }
    result.insert(
        "resources".to_owned(),
        Value::Array(
            manifests
                .into_iter()
                .map(|manifest| {
                    json!({
                        "type": "resource_link",
                        "uri": manifest.manifest_uri,
                        "name": manifest.name,
                        "mimeType": manifest.mime_type,
                        "size": manifest.raw_bytes,
                        "contentHash": manifest.sha256,
                        "expiresAt": format_epoch_rfc3339(manifest.expires_at_epoch_seconds)
                    })
                })
                .collect(),
        ),
    );
    Ok(Some(DeliveryAttachment { reservation }))
}

pub(super) async fn rollback_delivery(
    _artifact_store: &ArtifactStore,
    _artifact: &ArtifactLookup,
    attachment: Option<DeliveryAttachment>,
) {
    if let Some(attachment) = attachment {
        attachment.reservation.rollback();
    }
}

pub(super) fn commit_delivery(attachment: Option<DeliveryAttachment>) {
    if let Some(attachment) = attachment {
        attachment.reservation.commit();
    }
}

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
                    "byteLength":output.bytes.len(),"sha256":hash
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
    for (index, asset) in manifest.assets.iter().enumerate() {
        if asset.status != AssetStatus::Exported {
            continue;
        }
        let data = asset.data_base64.as_deref().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The exported asset binary is not in the artifact.",
                false,
            )
        })?;
        let bytes = STANDARD.decode(data.as_bytes()).map_err(|_| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The exported asset binary base64 is invalid.",
                false,
            )
        })?;
        let mime_type = asset.mime_type.as_deref().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The exported asset has no MIME type.",
                false,
            )
        })?;
        let expected_hash = asset.sha256.as_deref().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The exported asset has no hash.",
                false,
            )
        })?;
        if asset.byte_length != Some(bytes.len()) || expected_hash != sha256_hex(&bytes) {
            return Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The exported asset length or hash does not match.",
                false,
            ));
        }
        let extension = asset
            .format
            .map_or("bin", devup_mcp_figma::AssetFormat::extension);
        if !seen.insert((sha256_hex(&bytes), extension, mime_type)) {
            continue;
        }
        outputs.push(ProjectedOutput::asset(
            format!("asset-{}.{extension}", index + 1),
            mime_type,
            bytes,
            &asset.asset_id,
        ));
    }
    Ok(outputs)
}

/// Bound generated filenames, retaining a hash when transliteration would lose
/// identity. The generated folders are short; user-chosen output roots remain
/// subject to OutputPolicy and the host filesystem's own path limits.
fn host_safe_file_name(name: &str) -> String {
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let mut slug = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug = slug.trim_matches('-').to_owned();
    let upper = slug.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'));
    let needs_hash =
        !name.is_ascii() || slug != stem || slug.len() > 120 || slug.is_empty() || reserved;
    if slug.is_empty() || reserved {
        slug.insert_str(0, "asset-");
    }
    if needs_hash {
        slug.truncate(100);
    }
    if needs_hash {
        slug.push('-');
        slug.push_str(&sha256_hex(name.as_bytes())[..12]);
    }
    let extension = extension
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(10)
        .collect::<String>();
    if !extension.is_empty() {
        slug.push('.');
        slug.push_str(&extension);
    }
    slug
}

/// `/icons/ic:round-arrow-left.svg` as `/icons/ic-round-arrow-left-b041d24e668d.svg`. Only
/// the file name is touched; the folder the code looks in is left alone.
fn host_safe_asset_path(path: &str) -> String {
    for prefix in ["/icons/", "/images/"] {
        if let Some(name) = path.strip_prefix(prefix) {
            return format!("{prefix}{}", host_safe_file_name(name));
        }
    }
    match path.rsplit_once('/') {
        Some((directory, name)) => format!("{directory}/{}", host_safe_file_name(name)),
        None => host_safe_file_name(path),
    }
}

/// Every asset the generated code points at, renamed the way its bytes will
/// be written. The code holds each path inside a string or a `url(...)`, so a
/// name runs to the first quote or closing parenthesis - spaces and dots
/// belong to it, as they do in `Frame 269.png`.
fn with_host_safe_asset_paths(code: &str) -> String {
    let mut safe = String::with_capacity(code.len());
    let mut rest = code;
    loop {
        let Some((at, prefix)) = ["/icons/", "/images/"]
            .into_iter()
            .filter_map(|prefix| rest.find(prefix).map(|at| (at, prefix)))
            .min_by_key(|(at, _)| *at)
        else {
            safe.push_str(rest);
            return safe;
        };
        let after = at + prefix.len();
        safe.push_str(&rest[..after]);
        let end = rest[after..]
            .find(['"', '\'', ')'])
            .map_or(rest.len(), |offset| after + offset);
        safe.push_str(&host_safe_file_name(&rest[after..end]));
        rest = &rest[end..];
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Compare the generated text, removing only known asset references in codegen's
/// src and CSS URL forms. Literal text, layout, imports and component names stay
/// significant. Separate segments avoid collisions with a placeholder string.
fn generated_structure(code: &str, paths: &[String]) -> Vec<String> {
    let mut ranges = Vec::new();
    for attribute in ["src", "bg", "backgroundImage", "maskImage"] {
        let opening = format!("{attribute}=\"");
        for (at, _) in code.match_indices(&opening) {
            // An attribute name must start at a word boundary, not inside an
            // unrelated prop. Codegen quotes literal text containing quotes.
            if !code[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
            {
                continue;
            }
            let start = at + opening.len();
            let Some(end) = code[start..].find('"') else {
                continue;
            };
            let value = &code[start..start + end];
            for path in paths {
                if attribute == "src" {
                    if value == path {
                        ranges.push((start, start + path.len()));
                    }
                    continue;
                }
                // Preserve all paint layers and positions while recognizing
                // every image, including URLs after gradients or other images.
                for (prefix, suffix) in [("url(", ")"), ("url('", "')")] {
                    let reference = format!("{prefix}{path}{suffix}");
                    for (offset, _) in value.match_indices(&reference) {
                        let asset_start = start + offset + prefix.len();
                        ranges.push((asset_start, asset_start + path.len()));
                    }
                }
            }
        }
    }
    ranges.sort_unstable();
    ranges.dedup();
    let mut segments = Vec::new();
    let mut previous = 0;
    for (start, end) in ranges {
        if start < previous {
            continue;
        }
        segments.push(code[previous..start].to_owned());
        previous = end;
    }
    segments.push(code[previous..].to_owned());
    segments
}

fn annotate_identical_frames(frames: &mut [Value], payload: &CollectedPayload, per_node: bool) {
    let paths = devup_mcp_figma::discover_asset_manifest(&payload.snapshot)
        .assets
        .iter()
        .filter_map(|asset| {
            match asset
                .field
                .strip_prefix("fills/")
                .and_then(|n| n.parse::<usize>().ok())
            {
                Some(index) => devup_mcp_devup_ui::codegen::image_fill_path(
                    &payload.snapshot,
                    &asset.node_id,
                    index,
                    per_node,
                ),
                None => devup_mcp_devup_ui::codegen::asset_path(
                    &payload.snapshot,
                    &asset.node_id,
                    per_node,
                ),
            }
        })
        .map(|path| host_safe_asset_path(&path))
        .collect::<Vec<_>>();
    let mut seen = BTreeMap::new();
    for frame in frames {
        // Fidelity warnings may make a frame partial without changing the
        // generated text. Compare exactly the outputs that exist; the key and
        // comparedOutputs prevent missing outputs from implying equivalence.
        let generated = ["tsx", "componentTsx"]
            .into_iter()
            .filter_map(|field| frame[field].as_str().map(|code| (field, code.to_owned())))
            .collect::<Vec<_>>();
        if generated.is_empty() {
            continue;
        }
        let key = generated
            .iter()
            .map(|(field, code)| (*field, generated_structure(code, &paths)))
            .collect::<Vec<_>>();
        if let Some((node_id, original)) = seen.get(&key) {
            frame["structurallyIdenticalTo"] = json!(node_id);
            frame["differsOnly"] = if original == &generated {
                json!([])
            } else {
                json!(["assetReferences"])
            };
            frame["comparedOutputs"] = json!(
                generated
                    .iter()
                    .map(|(field, _)| *field)
                    .collect::<Vec<_>>()
            );
        } else {
            seen.insert(key, (frame["nodeId"].clone(), generated));
        }
    }
}

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
    let mut assets = devup_mcp_figma::discover_asset_manifest(&snapshot).assets;
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
        json!({"assetId":asset.asset_id,"nodeId":asset.node_id,"reason":reason,"errorCode":asset.error_code})
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
            "Only some deliverable asset bytes were collected, or discovery is incomplete. capture-not-in-artifact means not collected in this batch, not an export failure. Merge saved batch responses with devup-mcp --merge-asset-batches batch1.json batch2.json."
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

fn projection_key(outputs: &[ProjectedOutput]) -> String {
    let mut hasher = Sha256::new();
    for output in outputs {
        hasher.update(output.name.as_bytes());
        hasher.update([0]);
        hasher.update(output.mime_type.as_bytes());
        hasher.update([u8::from(output.is_binary)]);
        hasher.update((output.bytes.len() as u64).to_le_bytes());
        hasher.update(&output.bytes);
    }
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

pub(super) fn artifact_metadata(artifact: &ArtifactLookup) -> Value {
    let origin_collection = &artifact.payload.stats;
    json!({
        "artifactId": artifact.artifact_id,
        "contentHash": artifact.content_hash,
        "cacheHit": artifact.cache_hit,
        "reuseKind": artifact.reuse_kind,
        "ageSeconds": artifact.age_seconds,
        "remainingTtlSeconds": artifact.remaining_ttl_seconds,
        "avoidedFigmaToolCalls": if artifact.cache_hit { origin_collection.figma_tool_calls } else { 0 },
        "originCollection": origin_collection,
        "capabilities": artifact.capabilities,
        "sizeBytes": artifact.size_bytes,
        "acquiredAt": format_epoch_rfc3339(artifact.created_at_epoch_seconds),
        "expiresAt": format_epoch_rfc3339(artifact.expires_at_epoch_seconds)
    })
}

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

fn generate_collected_component(
    payload: &CollectedPayload,
    node_id: &str,
    options: &CodegenOptions,
) -> Result<devup_mcp_devup_ui::codegen::CodegenOutput, DevupError> {
    if !options.inline_instances {
        let mut pending = vec![node_id];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = payload.snapshot.nodes.get(id) else {
                continue;
            };
            let view = node.typed_view();
            if view.bool("visible") == Some(false) {
                continue;
            }
            if node.node_type == "INSTANCE" {
                let component_id = view
                    .string("mainComponentId")
                    .or_else(|| view.string("componentId"))
                    .or_else(|| view.string("mainComponent"))
                    .or_else(|| {
                        view.value("mainComponent")
                            .and_then(|value| value.get("id"))
                            .and_then(Value::as_str)
                    });
                if let Some(component_id) = component_id
                    && !payload.snapshot.nodes.contains_key(component_id)
                {
                    return Err(DevupError::with_details(
                        ErrorCode::DevupFigmaNodeNotFound,
                        "componentTsx needs a component master outside this snapshot. Re-collect the component master, or request tsx to inline the instance.",
                        false,
                        json!({"componentId":component_id,"instanceNodeId":id}),
                    ));
                }
            }
            pending.extend(view.child_ids());
        }
    }
    generate_component(&payload.snapshot, node_id, options)
}

pub(super) async fn complete_operation(
    mut operation: PendingOperation,
    payload: &CollectedPayload,
    source_kind: &str,
    artifact: &ArtifactLookup,
    output_policy: &OutputPolicy,
    artifact_store: &ArtifactStore,
) -> Result<Value, DevupError> {
    let collection = if artifact.cache_hit {
        CollectionStats::default()
    } else {
        payload.stats.clone()
    };
    let completeness_report = payload.completeness_report();
    if source_kind == "artifact"
        && let PendingOperation::Export {
            frame_ids,
            all_screens,
            ..
        } = &mut operation
        && frame_ids.is_empty()
        && !*all_screens
        && let Some(saved) = &artifact.capabilities.section_selection
    {
        frame_ids.clone_from(&saved.frame_ids);
        *all_screens = saved.all_screens;
    }
    match operation {
        PendingOperation::Search {
            query,
            node_types,
            match_kind,
            limit,
        } => {
            let matches = search_snapshot(
                &payload.snapshot,
                &payload.target,
                &SearchOptions {
                    query: query.clone(),
                    node_types,
                    match_kind,
                    limit,
                },
            )?;
            let quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, true),
                projection: projection_quality(false, &[]),
                theme: theme_quality(false, 0, 0),
                assets: assets_quality(false, &[], &[]),
            };
            let mut response = json!({
                "status": quality.status(),
                "quality": quality,
                "query": query,
                "count": matches.len(),
                "matches": matches,
                "completeness": payload.completeness,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "version": payload.snapshot.version
                }
            });
            attach_completeness_report(&mut response, quality, &completeness_report, false);
            Ok(response)
        }
        PendingOperation::Explore { limit, target } => {
            let result = explore_snapshot(&payload.snapshot, &target, &ExploreOptions { limit })?;
            let count = result.candidates.len();
            let quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, true),
                projection: projection_quality(false, &[]),
                theme: theme_quality(false, 0, 0),
                assets: assets_quality(false, &[], &[]),
            };
            let mut response = json!({
                "status": quality.status(),
                "quality": quality,
                "targetKind": result.target_kind,
                "anchor": result.anchor,
                "group": result.group,
                "count": count,
                "candidates": result.candidates,
                "truncated": result.truncated,
                "diagnostics": payload.snapshot.diagnostics,
                "completeness": payload.completeness,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": target.file_key,
                    "nodeId": target.node_id,
                    "version": payload.snapshot.version
                }
            });
            attach_completeness_report(&mut response, quality, &completeness_report, false);
            Ok(response)
        }
        PendingOperation::Export {
            outputs,
            component_name,
            include_diagnostics,
            root_layout,
            asset_names_per_node,
            scope,
            strict,
            output_paths,
            frame_ids,
            all_screens,
            asset_captures,
            mut asset_output_paths,
            asset_public_root,
            delivery,
        } => {
            if outputs.iter().any(|output| output == "sourceMap")
                && !outputs
                    .iter()
                    .any(|output| matches!(output.as_str(), "tsx" | "componentTsx" | "devupJson"))
            {
                return Err(unavailable_output(
                    payload,
                    "sourceMap",
                    r#"A source map needs a generated output. Even when reusing artifactId, include tsx, componentTsx or devupJson in outputs in the same call. Example: {"artifactId":"<artifactId>","outputs":["tsx","rawSnapshot","sourceMap"],"debug":true}"#,
                ));
            }
            let mut result = Map::new();
            // Not restated by quality: this grades how far token resolution
            // reached - whether external library variables were covered - where
            // quality.theme only says whether what was resolved conflicts.
            result.insert("completeness".to_owned(), json!(payload.completeness));
            result.insert("collection".to_owned(), json!(collection));
            result.insert("cache".to_owned(), artifact_metadata(artifact));
            let mut failures = payload
                .failures
                .iter()
                .map(|failure| json!(failure))
                .collect::<Vec<_>>();
            let mut warnings = Vec::new();
            for asset in payload
                .assets
                .iter()
                .filter(|asset| asset.status == AssetStatus::Failed)
            {
                let failure = json!({"assetId":asset.asset_id,"nodeId":asset.node_id,
                    "errorCode":asset.error_code,"message":"Asset export failed. Re-collect this asset from its URL.",
                    "retryable":true});
                if asset.error_code.as_deref() == Some("DEVUP_ASSET_NODE_HIDDEN") {
                    warnings.push(failure);
                } else {
                    failures.push(failure);
                }
            }
            result.insert("failures".to_owned(), json!(failures));
            if !warnings.is_empty() {
                result.insert("warnings".to_owned(), json!(warnings));
            }
            result.insert(
                "source".to_owned(),
                json!({
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "nodeId": payload.target.node_id,
                    "version": payload.snapshot.version
                }),
            );
            let payload_section_index = section_index_from_payload(payload);
            let target_kind = if payload_section_index.is_some() {
                TargetKind::Section
            } else {
                classify_target(&payload.snapshot, &payload.target)
            };
            result.insert("targetKind".to_owned(), json!(target_kind));
            let selection_index = if target_kind == TargetKind::Section {
                let mut index = match &payload_section_index {
                    Some(index) => index.clone(),
                    None => {
                        devup_mcp_figma::build_section_index(&payload.snapshot, &payload.target)?
                    }
                };
                for (id, owner) in devup_mcp_figma::screen_owners(
                    &payload.snapshot,
                    index.candidates.iter().map(|c| c.node_id.as_str()),
                ) {
                    if owner.is_some() || !index.node_screen_ids.contains_key(&id) {
                        index.node_screen_ids.insert(id, owner);
                    }
                }
                Some(index)
            } else {
                None
            };
            let manifest_requested = outputs.iter().any(|output| output == "assetManifest");
            result.insert(
                "assetSummary".to_owned(),
                asset_summary(
                    payload,
                    artifact,
                    &payload.snapshot.roots,
                    manifest_requested,
                ),
            );

            if !frame_ids.is_empty() && all_screens {
                return Err(DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "frameIds and allScreens cannot be used together.",
                    false,
                ));
            }
            if target_kind != TargetKind::Section && (!frame_ids.is_empty() || all_screens) {
                return Err(DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "frameIds and allScreens can only be used on a Section artifact.",
                    false,
                ));
            }

            let mut candidates_truncated = false;
            let section_candidates = if target_kind == TargetKind::Section
                && outputs.iter().any(|output| {
                    matches!(output.as_str(), "tsx" | "componentTsx" | "responsiveTsx")
                }) {
                Some(if let Some(index) = &payload_section_index {
                    candidates_truncated = index.truncated;
                    index
                        .candidates
                        .iter()
                        .map(|candidate| {
                            let mut explored = section_candidate_as_explore(candidate);
                            explored.node.kind =
                                devup_mcp_figma::classify_explore_node(&explored.node);
                            explored.selection_reasons = if explored.node.is_screen_candidate() {
                                vec!["screen-like".into(), "inside-section".into()]
                            } else {
                                vec!["explicit-selection-only".into(), "inside-section".into()]
                            };
                            explored
                        })
                        .collect()
                } else {
                    let explored = explore_snapshot(
                        &payload.snapshot,
                        &payload.target,
                        &ExploreOptions { limit: 100 },
                    )?;
                    candidates_truncated = explored.candidates_truncated;
                    explored.candidates
                })
            } else {
                None
            };
            if let Some(candidates) = &section_candidates
                && frame_ids.is_empty()
                && !all_screens
            {
                // Both candidate producers report actual list truncation.
                // Reaching the limit alone does not mean candidates were omitted.
                let truncated = candidates_truncated;
                let (screens, explicit_candidates): (Vec<_>, Vec<_>) = candidates
                    .iter()
                    .partition(|candidate| candidate.node.is_screen_candidate());
                let quality = OutputQuality {
                    acquisition: acquisition_quality(&completeness_report, false),
                    projection: projection_quality(false, &[]),
                    theme: theme_quality(false, 0, 0),
                    assets: assets_quality(false, &[], &[]),
                };
                result.insert("status".to_owned(), json!("selection_required"));
                if let Some(summary) = result.get_mut("assetSummary") {
                    summary["manifestIncluded"] = json!(false);
                }
                result.insert("quality".to_owned(), json!(quality));
                result.insert(
                    "selection".to_owned(),
                    json!({
                        "kind": "screen-frame",
                        "status": if truncated { "partial" } else { "complete" },
                        "count": screens.len(),
                        "candidates": screens,
                        "explicitCandidates": explicit_candidates,
                        "truncated": truncated
                    }),
                );
                result.insert(
                    "nextAction".to_owned(),
                    json!({
                        "why": "This is a Section candidate list. Screen artifacts have not been exported yet.",
                        "how": "Review selection.candidates using name, nodeType and textPreview. Use allScreens:true for these automatic screen candidates in a complete list. Nonstandard cases and notes are in selection.explicitCandidates; select them with frameIds or their canonicalUrl.",
                        "doNot": "Do not try to collect the whole Section at once."
                    }),
                );
                // An example built from this call's own artifact and a real
                // candidate, so the next step is a call to run rather than a
                // shape to assemble.
                if let Some(candidate) = screens.first().or_else(|| explicit_candidates.first()) {
                    result
                        .get_mut("nextAction")
                        .expect("nextAction was inserted")["example"] = json!({
                        "tool": "devup_figma_export",
                        "arguments": {
                            "artifactId": artifact.artifact_id,
                            "frameIds": [candidate.node.node_id],
                            "outputs": outputs,
                            "delivery": "resource",
                            "componentName": component_name,
                            "includeDiagnostics": include_diagnostics,
                            "rootLayout": if root_layout == devup_mcp_devup_ui::codegen::RootLayout::Embedded { "embedded" } else { "standalone" },
                            "assetNamesPerNode": asset_names_per_node,
                            "scope": scope,
                            "strict": strict,
                            "debug": outputs.iter().any(|output| super::validation::DIAGNOSIS_OUTPUTS.contains(&output.as_str())),
                            "outputPaths": output_paths,
                            "assetPublicRoot": asset_public_root,
                            "assetRequests": asset_captures.iter().map(|capture| json!({
                                "assetId":capture.asset_id,"format":capture.format,"scale":capture.scale,
                                "outputPath":asset_output_paths.get(&capture.asset_id)
                            })).collect::<Vec<_>>()
                        }
                    });
                    let arguments =
                        result.get_mut("nextAction").expect("nextAction")["example"]["arguments"]
                            .as_object_mut()
                            .expect("example arguments");
                    arguments.insert("scope".into(), json!("node"));
                    if outputs.iter().any(|output| output == "referencePng") {
                        arguments.remove("artifactId");
                        arguments.remove("frameIds");
                        arguments.insert("url".into(), json!(candidate.canonical_url));
                    }
                } else {
                    result.get_mut("nextAction").expect("nextAction")["example"] = json!({
                        "tool":"devup_figma_explore", "arguments":{
                            "url":format!("https://www.figma.com/design/{}?node-id={}", payload.target.file_key,
                                payload.target.node_id.as_deref().unwrap_or_default().replace(':', "-")),
                            "limit":100
                        }
                    });
                }
                return Ok(Value::Object(result));
            }

            let mut written_paths = Map::new();
            let mut section_tsx_projected = false;
            let mut selected_projection_roots = None;
            let mut tsx_source_map = None;
            let mut generated_output_target = "devupJson";
            let mut devup_json_source_map = None;
            let mut projection_diagnostics = Vec::new();
            let mut fidelity_reports = Vec::new();
            let mut theme_conflict_count = 0;
            let mut theme_unresolved_count = 0;
            let mut pending_text_outputs = std::collections::BTreeMap::new();
            let mut pending_binary_outputs = std::collections::BTreeMap::new();
            let mut pending_asset_manifest = None;
            let mut asset_resource_outputs = Vec::new();
            if let Some(candidates) = section_candidates {
                let by_id = candidates
                    .iter()
                    .map(|candidate| (candidate.node.node_id.as_str(), candidate))
                    .collect::<std::collections::BTreeMap<_, _>>();
                let failed_ids = payload
                    .failures
                    .iter()
                    .map(|failure| failure.node_id.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                let selected = if all_screens {
                    candidates
                        .iter()
                        .filter(|candidate| candidate.node.is_screen_candidate())
                        .filter(|candidate| !failed_ids.contains(candidate.node.node_id.as_str()))
                        .collect::<Vec<_>>()
                } else {
                    let requested = frame_ids
                        .iter()
                        .map(String::as_str)
                        .collect::<std::collections::BTreeSet<_>>();
                    if requested.len() != frame_ids.len() {
                        return Err(DevupError::new(
                            ErrorCode::DevupSnapshotUnsupported,
                            "frameIds contains a duplicate node.",
                            false,
                        ));
                    }
                    if requested
                        .iter()
                        .any(|node_id| !by_id.contains_key(*node_id))
                    {
                        selection_index
                            .as_ref()
                            .expect("Section index")
                            .select(&frame_ids, false)?;
                    }
                    candidates
                        .iter()
                        .filter(|candidate| {
                            requested.contains(candidate.node.node_id.as_str())
                                && !failed_ids.contains(candidate.node.node_id.as_str())
                        })
                        .collect::<Vec<_>>()
                };
                if selected.is_empty() && failed_ids.is_empty() {
                    return Err(unavailable_output(
                        payload,
                        "Section outputs",
                        "No screen frames match this selection. Use explicit frameIds for a nonstandard frame, or explore another Section.",
                    ));
                }
                selected_projection_roots = Some(
                    selected
                        .iter()
                        .map(|candidate| candidate.node.node_id.clone())
                        .collect::<Vec<_>>(),
                );
                const MAX_SECTION_FRAMES: usize = 6;
                if selected.len() > MAX_SECTION_FRAMES {
                    return Err(DevupError::with_details(
                        ErrorCode::DevupInvalidInput,
                        "This Section projection exceeds the batch budget. Request at most 6 frameIds per call; reuse this artifactId for subsequent batches.",
                        false,
                        json!({"artifactId":artifact.artifact_id,"requestedFrameCount":selected.len(),
                            "recommendedBatchSize":3,"maxFrameCount":MAX_SECTION_FRAMES,"maxFrameOutputUnits":12,
                            "recommendedFrameIds":selected.iter().take(3).map(|c| &c.node.node_id).collect::<Vec<_>>(),
                            "remainingFrameIds":selected.iter().skip(3).map(|c| &c.node.node_id).collect::<Vec<_>>() }),
                    ));
                }
                super::validation::validate_export_budget(
                    &selected
                        .iter()
                        .map(|candidate| candidate.node.node_id.clone())
                        .collect::<Vec<_>>(),
                    &outputs,
                )?;
                let mut frames = Vec::with_capacity(selected.len());
                for (index, candidate) in selected.iter().enumerate() {
                    let frame_component_name = component_name.as_ref().map(|name| {
                        if selected.len() == 1 {
                            name.clone()
                        } else {
                            format!("{name}{}", index + 1)
                        }
                    });
                    let mut frame = json!({"nodeId":candidate.node.node_id,"name":candidate.node.name,"outputResults":{},
                        "canonicalUrl":candidate.canonical_url});
                    let before_failures = failures.len();
                    let mut frame_diagnostics = Vec::new();
                    for field in ["tsx", "componentTsx"] {
                        if !outputs.iter().any(|output| output == field) {
                            continue;
                        }
                        let output = generate_collected_component(
                            payload,
                            &candidate.node.node_id,
                            &CodegenOptions {
                                component_name: frame_component_name.clone(),
                                include_diagnostics,
                                inline_instances: field == "tsx",
                                root_layout,
                                asset_names_per_node,
                                ..CodegenOptions::default()
                            }
                            .with_payload_tokens(payload),
                        );
                        let mut output = match output {
                            Ok(output) => output,
                            Err(error) => {
                                failures.push(projection_failure(
                                    payload,
                                    &candidate.node.node_id,
                                    field,
                                    &error,
                                ));
                                continue;
                            }
                        };
                        scope_output(&mut output, field);
                        frame["outputResults"][field] = output_result(&output);
                        frame_diagnostics.extend(output.diagnostics.iter().cloned());
                        fidelity_reports.push(output.fidelity_report.clone());
                        frame[field] = json!(output.tsx);
                        attach_fidelity(&mut frame, &output.fidelity_report, include_diagnostics);
                        if outputs.iter().any(|output| output == "sourceMap") {
                            frame["sourceMap"] = json!({"version":output.source_map.version,
                                "entries":output.source_map.property_entries(),"source":{"fileKey":payload.target.file_key,
                                "rootNodeId":candidate.node.node_id,"sourceVersion":payload.source_version,
                                "generatedOutput":field,"mappingKind":"node-field-property"}});
                        }
                    }
                    frame["placementContracts"] = json!(
                        frame_diagnostics
                            .iter()
                            .filter(|d| d.code == "DEVUP_CODEGEN_PLACEMENT_CONTRACT")
                            .collect::<Vec<_>>()
                    );
                    frame["projectionIssues"] = json!(
                            frame_diagnostics
                                .iter()
                                .filter(|d| d.fidelity_impact()
                                    != devup_mcp_figma::FidelityImpact::None)
                                .collect::<Vec<_>>()
                        );
                    projection_diagnostics.extend(frame_diagnostics.iter().cloned());
                    let mut frame_quality = OutputQuality {
                        acquisition: acquisition_quality(&completeness_report, false),
                        projection: projection_quality(true, &frame_diagnostics),
                        theme: theme_quality(false, 0, 0),
                        assets: assets_quality(false, &[], &[]),
                    };
                    if failures.len() > before_failures {
                        frame_quality.projection =
                            if frame.get("tsx").is_some() || frame.get("componentTsx").is_some() {
                                ProjectionQuality::Lossy
                            } else {
                                ProjectionQuality::Failed
                            };
                    }
                    frame["quality"] = json!(frame_quality);
                    frame["status"] = json!(if failures.len() > before_failures {
                        "partial"
                    } else {
                        frame_quality.status()
                    });
                    frame["assetSummary"] = asset_summary(
                        payload,
                        artifact,
                        std::slice::from_ref(&candidate.node.node_id),
                        manifest_requested,
                    );
                    frame["projectionEvidence"] = json!(
                        frame_diagnostics
                            .iter()
                            .filter(|d| {
                                d.fidelity_impact() == devup_mcp_figma::FidelityImpact::None
                                    && matches!(
                                        d.code.as_str(),
                                        "DEVUP_CODEGEN_NON_RENDERING_ASSET"
                                            | "DEVUP_CODEGEN_ABSOLUTE_VERIFIED"
                                            | "DEVUP_CODEGEN_LAYOUT_ACCOUNTED_FOR"
                                    )
                            })
                            .collect::<Vec<_>>()
                    );
                    frame_quality.assets = asset_collection_quality(&frame["assetSummary"]);
                    frame["quality"] = json!(frame_quality);
                    frame["status"] = json!(if failures.len() > before_failures {
                        "partial"
                    } else {
                        frame_quality.status()
                    });
                    attach_completeness_report(
                        &mut frame,
                        frame_quality,
                        &completeness_report,
                        include_diagnostics,
                    );
                    if include_diagnostics {
                        frame["diagnostics"] = json!(frame_diagnostics);
                    }
                    frames.push(frame);
                }
                annotate_identical_frames(&mut frames, payload, asset_names_per_node);
                if let Some(roots) = &selected_projection_roots {
                    result.insert(
                        "assetSummary".to_owned(),
                        asset_summary(payload, artifact, roots, manifest_requested),
                    );
                }
                if outputs
                    .iter()
                    .any(|output| matches!(output.as_str(), "tsx" | "componentTsx"))
                {
                    result.insert("frames".to_owned(), Value::Array(frames));
                }
                section_tsx_projected = true;
            }

            let mut responsive_snapshot = std::borrow::Cow::Borrowed(&payload.snapshot);
            if let Some(roots) = selected_projection_roots {
                responsive_snapshot.to_mut().roots = roots;
            }
            let component_name_for_components = component_name.clone();
            // A screen captured with its other widths is convertible as one
            // tree. It is offered whenever those widths are present rather than
            // only on request, because a caller asking for a screen that has
            // them almost always wants the responsive form and cannot know from
            // the node id alone whether it exists.
            if outputs
                .iter()
                .any(|output| output == "tsx" || output == "responsiveTsx")
                && let Some(merged) = merge_breakpoints(
                    &responsive_snapshot,
                    &CodegenOptions {
                        component_name: component_name_for_components.clone(),
                        include_diagnostics,
                        inline_instances: false,
                        root_layout,
                        asset_names_per_node,
                        ..CodegenOptions::default()
                    }
                    .with_payload_tokens(payload),
                )?
                && (outputs.iter().any(|output| output == "responsiveTsx")
                    || !merged.slots.windows(2).any(|pair| pair[0] == pair[1]))
            {
                // Named as the plugin names it: after the Section the widths
                // sit in, with `Page` on the end — `AboutPage` for a Section
                // called `about`. The Section is outside the collected
                // subtree, so each width carries its name as `parentName`.
                // A caller's own name wins; without one and without a
                // Section name the page is `ResponsivePage`.
                let section_name = payload
                    .snapshot
                    .roots
                    .iter()
                    .filter_map(|root| payload.snapshot.nodes.get(root))
                    .find_map(|root| root.typed_view().string("parentName").map(str::to_owned));
                let name = component_name_for_components.clone().map_or_else(
                    || {
                        section_name.map_or_else(
                            || "ResponsivePage".to_owned(),
                            |section| format!("{}Page", normalize_component_name(&section)),
                        )
                    },
                    |name| normalize_component_name(&name),
                );
                if merged
                    .slots
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != merged.slots.len()
                {
                    return Err(unavailable_output(
                        payload,
                        "responsiveTsx",
                        "Two frames occupy the same breakpoint slot. Request tsx separately, or select one frame per breakpoint slot.",
                    ));
                }
                let module = merged.module(&name);
                // The responsive tree builder does not run the ordinary
                // finalizer. Preserve each breakpoint's source limitations.
                let mut breakpoint_fidelity = Vec::new();
                for root_id in &responsive_snapshot.roots {
                    let mut source_output = generate_component(
                        &responsive_snapshot,
                        root_id,
                        &CodegenOptions {
                            inline_instances: true,
                            root_layout,
                            asset_names_per_node,
                            ..CodegenOptions::default()
                        }
                        .with_payload_tokens(payload),
                    )?;
                    scope_output(&mut source_output, "responsiveTsx");
                    breakpoint_fidelity
                        .push(json!({"rootId":root_id,"fidelity":source_output.fidelity_report}));
                    for mut issue in source_output
                        .diagnostics
                        .into_iter()
                        .filter(|d| d.fidelity_impact() != devup_mcp_figma::FidelityImpact::None)
                    {
                        if let Some(details) = issue.details.as_mut().and_then(Value::as_object_mut)
                        {
                            details.insert(
                                "evidenceScope".into(),
                                json!("breakpoint-source-projection"),
                            );
                            details.insert("breakpointRootId".into(), json!(root_id));
                            details.insert("output".into(), json!("responsiveTsx"));
                        }
                        if !projection_diagnostics.contains(&issue) {
                            projection_diagnostics.push(issue);
                        }
                    }
                }
                if output_paths.contains_key("responsiveTsx") {
                    pending_text_outputs.insert("responsiveTsx".to_owned(), module.clone());
                }
                result.insert("responsiveTsx".to_owned(), json!(module));
                result.insert("responsiveSlots".to_owned(), json!(merged.slots));
                let responsive_issues: Vec<_> = projection_diagnostics
                    .iter()
                    .filter(|d| {
                        d.details
                            .as_ref()
                            .is_some_and(|v| v["output"] == "responsiveTsx")
                    })
                    .cloned()
                    .collect();
                let responsive_quality = if merged.unrepresented.is_empty() {
                    projection_quality(true, &responsive_issues)
                } else {
                    ProjectionQuality::Lossy
                };
                result.entry("outputResults").or_insert_with(|| json!({}))["responsiveTsx"] = json!({
                    "state":"produced","projection":responsive_quality,
                    "fidelity":{"scope":"breakpoint-source-projections","sourceOutput":"tsx","breakpoints":breakpoint_fidelity,
                        "syntaxValid":devup_mcp_devup_ui::validation::validate_tsx(&module).is_ok(),
                        "mergedMappingVerified":false,"unrepresentedCount":merged.unrepresented.len(),
                        "note":"Breakpoint reports describe their source projections; they do not certify the merged responsive module. Review responsiveUnrepresented and projectionIssues."}});

                if !merged.unrepresented.is_empty() {
                    for note in &merged.unrepresented {
                        projection_diagnostics.push(Diagnostic {
                            code:"DEVUP_CODEGEN_RESPONSIVE_LOSS".into(),
                            message:note.detail.clone(),
                            node_id:Some(note.node_id.clone()),
                            property:Some("childrenIds".into()),
                            details:Some(json!({"originalValue":payload.snapshot.nodes.get(&note.node_id).map(|n| &n.fields),
                                "appliedValue":null,"appliedValueReason":"Content is unrepresented in the merged output.","stage":"projection","output":"responsiveTsx"})),
                            fidelity_impact:Some(devup_mcp_figma::FidelityImpact::Lossy),
                            severity:Some(DiagnosticSeverity::Warning),
                            ..Diagnostic::default()
                        });
                    }
                    result.insert(
                        "responsiveUnrepresented".to_owned(),
                        json!(
                            merged
                                .unrepresented
                                .iter()
                                .map(|note| json!({
                                    "nodeId": note.node_id,
                                    "detail": note.detail
                                }))
                                .collect::<Vec<_>>()
                        ),
                    );
                }
            }

            if outputs.iter().any(|output| output == "responsiveTsx")
                && !result.contains_key("responsiveTsx")
            {
                return Err(unavailable_output(
                    payload,
                    "responsiveTsx",
                    "The snapshot has no mergeable breakpoint frames. Request tsx for individual screens, or re-collect a screen with its breakpoint siblings.",
                ));
            }

            if outputs.iter().any(|output| output == "tsx") && !section_tsx_projected {
                let node_id = payload.target.node_id.as_deref().ok_or_else(|| {
                    DevupError::new(
                        ErrorCode::DevupFigmaNodeNotFound,
                        "A TSX export payload requires a node ID.",
                        false,
                    )
                })?;
                let mut output = generate_component(
                    &payload.snapshot,
                    node_id,
                    &CodegenOptions {
                        component_name,
                        include_diagnostics,
                        inline_instances: true,
                        root_layout,
                        asset_names_per_node,
                        ..CodegenOptions::default()
                    }
                    .with_payload_tokens(payload),
                )?;
                scope_output(&mut output, "tsx");
                result.entry("outputResults").or_insert_with(|| json!({}))["tsx"] =
                    output_result(&output);
                projection_diagnostics.extend(output.diagnostics.iter().cloned());
                fidelity_reports.push(output.fidelity_report.clone());
                generated_output_target = "tsx";
                tsx_source_map = Some(output.source_map.clone());
                if output_paths.contains_key("tsx") {
                    pending_text_outputs.insert("tsx".to_owned(), output.tsx.clone());
                }
                result.insert("tsx".to_owned(), json!(output.tsx));
                if include_diagnostics {
                    result.insert("diagnostics".to_owned(), json!(&output.diagnostics));
                }
            }

            if outputs.iter().any(|output| output == "componentTsx") && !section_tsx_projected {
                let node_id = payload.target.node_id.as_deref().ok_or_else(|| {
                    DevupError::new(
                        ErrorCode::DevupFigmaNodeNotFound,
                        "A component TSX export payload requires a node ID.",
                        false,
                    )
                })?;
                let output = generate_collected_component(
                    payload,
                    node_id,
                    &CodegenOptions {
                        component_name: component_name_for_components.clone(),
                        include_diagnostics: false,
                        inline_instances: false,
                        root_layout,
                        asset_names_per_node,
                        ..CodegenOptions::default()
                    }
                    .with_payload_tokens(payload),
                );
                match output {
                    Ok(mut output) => {
                        scope_output(&mut output, "componentTsx");
                        result.entry("outputResults").or_insert_with(|| json!({}))["componentTsx"] =
                            output_result(&output);
                        projection_diagnostics.extend(output.diagnostics.iter().cloned());
                        fidelity_reports.push(output.fidelity_report.clone());
                        if tsx_source_map.is_none() {
                            generated_output_target = "componentTsx";
                            tsx_source_map = Some(output.source_map.clone());
                        }
                        if output_paths.contains_key("componentTsx") {
                            pending_text_outputs
                                .insert("componentTsx".to_owned(), output.tsx.clone());
                        }
                        result.insert("componentTsx".to_owned(), json!(output.tsx));
                    }
                    Err(error) => {
                        failures.push(projection_failure(payload, node_id, "componentTsx", &error))
                    }
                }
            }

            if outputs.iter().any(|output| output == "devupJson") {
                let variables = payload.variables.as_ref().ok_or_else(|| unavailable_output(payload, "devupJson",
                    "No variable/style resources were collected. Re-collect from the URL with devupJson in outputs and the desired scope."))?;
                let variables = variable_snapshot_from_result(variables)?;
                let output = generate_devup_json(&variables, parse_scope(&scope)?)?;
                theme_conflict_count = output.conflicts.len();
                theme_unresolved_count = output.unresolved_variables.len();
                devup_json_source_map = Some(output.source_map.clone());
                if output_paths.contains_key("devupJson") {
                    pending_text_outputs.insert("devupJson".to_owned(), output.json.clone());
                }
                result.insert("devupJson".to_owned(), json!(output.json));
                result.insert("themeCounts".to_owned(), json!(output.counts));
                result.insert("themeCompleteness".to_owned(), json!(output.completeness));
                result.insert("conflicts".to_owned(), json!(output.conflicts));
                result.insert(
                    "unresolvedVariables".to_owned(),
                    json!(output.unresolved_variables),
                );
                if include_diagnostics && !result.contains_key("diagnostics") {
                    result.insert("diagnostics".to_owned(), json!(output.diagnostics));
                }
            }

            if outputs.iter().any(|output| output == "rawSnapshot") {
                let raw = serde_json::to_value(&payload.snapshot).map_err(|error| {
                    DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        format!("Cannot serialize the raw snapshot: {error}"),
                        false,
                    )
                })?;
                if output_paths.contains_key("rawSnapshot") {
                    pending_text_outputs.insert(
                        "rawSnapshot".to_owned(),
                        serde_json::to_string_pretty(&raw).unwrap_or_default(),
                    );
                }
                result.insert("rawSnapshot".to_owned(), raw);
            }

            // The whole collection, not only its node tree. A snapshot kept on
            // its own can be converted, but not the way the server converts
            // it: the token names come from the variables and styles collected
            // beside it, and without them a `$gray200` fill comes out as the
            // tail of its variable ID and a `typography="h4"` as five font
            // props. Captures kept as fixtures need the resources too, so
            // this writes them. The reference PNG is left out — it is large,
            // binary, and has its own output.
            if outputs.iter().any(|output| output == "rawPayload") {
                let mut without_png = payload.clone();
                without_png.reference_png = None;
                let raw = serde_json::to_value(&without_png).map_err(|error| {
                    DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        format!("Cannot serialize the raw payload: {error}"),
                        false,
                    )
                })?;
                if output_paths.contains_key("rawPayload") {
                    pending_text_outputs.insert(
                        "rawPayload".to_owned(),
                        serde_json::to_string_pretty(&raw).unwrap_or_default(),
                    );
                }
                result.insert("rawPayload".to_owned(), raw);
            }

            if outputs.iter().any(|output| output == "sourceMap") && !section_tsx_projected {
                let source_map = json!({
                    "version": 2,
                    "tsx": tsx_source_map.map(|source_map| source_map.property_entries()).unwrap_or_default(),
                    "devupJson": devup_json_source_map
                        .map(|source_map| source_map.entries)
                        .unwrap_or_default(),
                    "source": {
                        "fileKey": payload.target.file_key,
                        "rootNodeId": payload.target.node_id,
                        "sourceVersion": payload.source_version,
                        "generatedOutput":generated_output_target,"mappingKind":"node-field-property"
                    }
                });
                if output_paths.contains_key("sourceMap") {
                    pending_text_outputs.insert(
                        "sourceMap".to_owned(),
                        serde_json::to_string_pretty(&source_map).unwrap_or_default(),
                    );
                }
                result.insert("sourceMap".to_owned(), source_map);
            }

            if outputs.iter().any(|output| output == "referencePng") {
                let reference = payload.reference_png.as_ref().ok_or_else(|| unavailable_output(payload, "referencePng",
                    "No reference PNG was collected. Re-collect a single screen URL with referencePng in outputs."))?;
                let bytes = STANDARD
                    .decode(reference.data_base64.as_bytes())
                    .map_err(|_| {
                        DevupError::new(
                            ErrorCode::DevupSnapshotUnsupported,
                            "The artifact reference PNG base64 is invalid.",
                            false,
                        )
                    })?;
                if bytes.len() != reference.byte_length
                    || Sha256::digest(&bytes)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                        != reference.sha256
                {
                    return Err(DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        "The artifact reference PNG length or hash does not match.",
                        false,
                    ));
                }
                if output_paths.contains_key("referencePng") {
                    pending_binary_outputs.insert("referencePng".to_owned(), bytes);
                }
                result.insert("referencePng".to_owned(), json!(reference));
            }

            let reconcile_captured_assets = payload
                .assets
                .iter()
                .any(|asset| asset.status == AssetStatus::Exported)
                && outputs.iter().any(|output| {
                    matches!(output.as_str(), "tsx" | "componentTsx" | "responsiveTsx")
                });
            if manifest_requested || reconcile_captured_assets {
                let mut manifest = devup_mcp_figma::discover_asset_manifest(&payload.snapshot);
                for exported in &payload.assets {
                    if let Some(existing) = manifest
                        .assets
                        .iter_mut()
                        .find(|asset| asset.asset_id == exported.asset_id)
                    {
                        *existing = exported.clone();
                    } else {
                        manifest.assets.push(exported.clone());
                    }
                }
                manifest.assets.retain(|asset| {
                    if asset.error_code.as_deref() != Some("DEVUP_ASSET_NODE_HIDDEN") {
                        return true;
                    }
                    if !warnings
                        .iter()
                        .any(|warning| warning["assetId"] == asset.asset_id)
                    {
                        warnings.push(json!({"assetId":asset.asset_id,"nodeId":asset.node_id,
                            "errorCode":asset.error_code,"severity":"warning",
                            "message":"Hidden asset excluded from deliverables."}));
                    }
                    false
                });
                manifest
                    .assets
                    .sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
                // Where the generated code refers to each asset, so a
                // consumer can write the bytes there without re-deriving the
                // name - which, for two icons of one layer name, it would get
                // wrong.
                for asset in &mut manifest.assets {
                    if asset.path.is_none() {
                        // An image fill is named from the node it is painted
                        // on and which fill it is, which holds for a layout
                        // box carrying a photograph as much as for a node
                        // drawn entirely from a file. Anything else is named
                        // after the node itself.
                        asset.path = match asset
                            .field
                            .strip_prefix("fills/")
                            .and_then(|index| index.parse::<usize>().ok())
                        {
                            Some(fill_index) => devup_mcp_devup_ui::codegen::image_fill_path(
                                &payload.snapshot,
                                &asset.node_id,
                                fill_index,
                                asset_names_per_node,
                            ),
                            None => devup_mcp_devup_ui::codegen::asset_path(
                                &payload.snapshot,
                                &asset.node_id,
                                asset_names_per_node,
                            ),
                        };
                    }
                }
                // Where the bytes will be written, if a layer name is one a
                // file system refuses. The code that points at them is
                // renamed the same way once every output is assembled.
                for asset in &mut manifest.assets {
                    if let Some(path) = asset.path.as_deref() {
                        let safe = host_safe_asset_path(path);
                        if safe != path {
                            asset.path = Some(safe);
                        }
                    }
                }
                for capture in &asset_captures {
                    if !payload.assets.iter().any(|asset| {
                        asset.asset_id == capture.asset_id
                            && asset.format == Some(capture.format)
                            && asset.scale == Some(capture.scale)
                            && matches!(asset.status, AssetStatus::Exported | AssetStatus::Failed)
                    }) {
                        return Err(DevupError::new(
                            ErrorCode::DevupFigmaHandoffInvalid,
                            format!(
                                "The exact requested asset export is not in the artifact. Re-collect it from the URL: {}",
                                capture.asset_id
                            ),
                            false,
                        ));
                    }
                }
                manifest.diagnostics = payload
                    .snapshot
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.resource_kind.as_deref() == Some("asset"))
                    .cloned()
                    .collect();
                pending_asset_manifest = Some(manifest);
            }
            // Reconcile every generated output, including reusable and responsive
            // code, against exclusions before inline/resource/file delivery.
            let mut assets = devup_mcp_figma::discover_asset_manifest(&payload.snapshot).assets;
            for captured in &payload.assets {
                if let Some(existing) = assets
                    .iter_mut()
                    .find(|asset| asset.asset_id == captured.asset_id)
                {
                    *existing = captured.clone();
                } else {
                    assets.push(captured.clone());
                }
            }
            let reference_paths = |asset: &devup_mcp_figma::AssetManifestEntry| {
                let generated = asset
                    .field
                    .strip_prefix("fills/")
                    .and_then(|i| i.parse::<usize>().ok())
                    .and_then(|i| {
                        devup_mcp_devup_ui::codegen::image_fill_path(
                            &payload.snapshot,
                            &asset.node_id,
                            i,
                            asset_names_per_node,
                        )
                    })
                    .or_else(|| {
                        devup_mcp_devup_ui::codegen::asset_path(
                            &payload.snapshot,
                            &asset.node_id,
                            asset_names_per_node,
                        )
                    });
                generated
                    .into_iter()
                    .chain(asset.path.clone())
                    .flat_map(|path| [host_safe_asset_path(&path), path])
                    .collect::<Vec<_>>()
            };
            let available_paths = assets
                .iter()
                .filter(|asset| asset.error_code.as_deref() != Some("DEVUP_ASSET_NODE_HIDDEN"))
                .flat_map(&reference_paths)
                .collect::<std::collections::BTreeSet<_>>();
            for asset in assets
                .iter()
                .filter(|asset| asset.error_code.as_deref() == Some("DEVUP_ASSET_NODE_HIDDEN"))
            {
                let paths = reference_paths(asset)
                    .into_iter()
                    .filter(|path| !available_paths.contains(path))
                    .collect::<Vec<_>>();
                let references = |code: &Value| {
                    code.as_str()
                        .is_some_and(|code| paths.iter().any(|path| code.contains(path)))
                };
                let source_node = payload.snapshot.nodes.get(&asset.node_id);
                let exclusion_details = json!({"referencePaths":paths,
                    "assetNodeId":asset.node_id,
                    "exclusionReason":source_node.and_then(devup_mcp_figma::asset_exclusion_reason),
                    "visible":source_node.and_then(|n| n.typed_view().value("visible")),
                    "opacity":source_node.and_then(|n| n.typed_view().value("opacity")),
                    "absoluteRenderBounds":source_node.and_then(|n| n.typed_view().value("absoluteRenderBounds")),
                    "policyConflict":source_node.is_some_and(|n| devup_mcp_figma::asset_exclusion_reason(n).is_none())});
                for field in ["tsx", "componentTsx", "responsiveTsx"] {
                    if result.get(field).is_some_and(&references) {
                        result.remove(field);
                        pending_text_outputs.remove(field);
                        failures.push(json!({"nodeId":payload.target.node_id,"assetId":asset.asset_id,"output":field,
                            "errorCode":"DEVUP_EXCLUDED_ASSET_REFERENCED","stage":"projection","details":exclusion_details,
                            "message":"Generated code references an excluded asset; this output was withheld."}));
                    }
                    if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
                        for frame in frames {
                            if frame.get(field).is_some_and(&references) {
                                frame.as_object_mut().unwrap().remove(field);
                                frame["status"] = json!("partial");
                                frame["quality"]["projection"] = json!("lossy");
                                frame["quality"]["assets"] = json!("partial");
                                failures.push(json!({"nodeId":frame["nodeId"],"assetId":asset.asset_id,"output":field,
                                    "errorCode":"DEVUP_EXCLUDED_ASSET_REFERENCED","stage":"projection","details":exclusion_details,
                                    "message":"Generated code references an excluded asset; this output was withheld."}));
                            }
                        }
                    }
                }
            }
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
                        .filter(|d| d.fidelity_impact() != devup_mcp_figma::FidelityImpact::None)
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
                                )
                        })
                        .collect::<Vec<_>>()
                ),
            );
            let mut quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, false),
                projection: projection_quality(
                    outputs.iter().any(|output| {
                        matches!(output.as_str(), "tsx" | "componentTsx" | "responsiveTsx")
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
            if let Some(summary) = result.get("assetSummary") {
                quality.assets = asset_collection_quality(summary);
            }
            if failures
                .iter()
                .any(|failure| failure.get("assetId").is_some())
            {
                quality.assets = super::quality::AssetsQuality::Partial;
            }
            if failures
                .iter()
                .any(|failure| failure.get("output").is_some())
            {
                quality.projection = super::quality::ProjectionQuality::Lossy;
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
                    let mut arguments = json!({"url":format!("https://www.figma.com/design/{}/?node-id={}",
                        payload.target.file_key, payload.target.node_id.as_deref().unwrap_or_default().replace(':', "-")),
                        "frameIds":failed_frames,"outputs":outputs,"scope":scope,"delivery":delivery,
                        "rootLayout":match root_layout { devup_mcp_devup_ui::codegen::RootLayout::Standalone => "standalone", devup_mcp_devup_ui::codegen::RootLayout::Embedded => "embedded" },
                        "assetNamesPerNode":asset_names_per_node,"strict":strict,"includeDiagnostics":include_diagnostics});
                    if let Some(branch) = &payload.target.branch_key {
                        arguments["url"] = json!(format!(
                            "https://www.figma.com/branch/{}/{}/?node-id={}",
                            payload.target.file_key,
                            branch,
                            payload
                                .target
                                .node_id
                                .as_deref()
                                .unwrap_or_default()
                                .replace(':', "-")
                        ));
                    }
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
            if strict && (quality.strict_violation() || fidelity_violation || !failures.is_empty())
            {
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
                        "completenessReport": completeness_report
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
                        &outputs,
                        &frame_failures,
                        Some(&id),
                    );
                }
            }
            attach_review_and_deliverable(
                &mut result,
                &outputs,
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
                &completeness_report,
                include_diagnostics,
            );
            if let Some(report) = carrier.get("completenessReport") {
                result.insert("completenessReport".to_owned(), report.clone());
            }
            let replacements = if let Some(manifest) = &mut pending_asset_manifest {
                reconcile_asset_paths(
                    manifest,
                    &mut asset_output_paths,
                    asset_public_root.as_deref(),
                    output_policy,
                )?
            } else {
                BTreeMap::new()
            };
            let owners = if let Some(index) = &selection_index {
                index.node_screen_ids.clone()
            } else {
                devup_mcp_figma::screen_owners(
                    &payload.snapshot,
                    payload.snapshot.roots.iter().map(String::as_str),
                )
            };
            let mut value = Value::Object(std::mem::take(&mut result));
            attach_diagnostic_screens(&mut value, &owners);
            rewrite_result_asset_references(&mut value, &replacements);
            result = value.as_object().expect("result object").clone();
            for (name, contents) in &mut pending_text_outputs {
                if matches!(name.as_str(), "tsx" | "componentTsx" | "responsiveTsx") {
                    if let Some(code) = result.get(name).and_then(Value::as_str) {
                        *contents = code.to_owned();
                    }
                } else if name == "sourceMap" {
                    *contents =
                        serde_json::to_string_pretty(&result["sourceMap"]).unwrap_or_default();
                } else if matches!(name.as_str(), "rawSnapshot" | "rawPayload") {
                    *contents = serde_json::to_string_pretty(&result[name]).unwrap_or_default();
                }
            }
            let mut planned_outputs = Vec::new();
            for (output, contents) in pending_text_outputs {
                if let Some(path) = output_paths.get(&output) {
                    planned_outputs.push((
                        output,
                        output_policy.resolve(path)?,
                        contents.into_bytes(),
                    ));
                }
            }
            for (output, bytes) in pending_binary_outputs {
                if let Some(path) = output_paths.get(&output) {
                    planned_outputs.push((output, output_policy.resolve(path)?, bytes));
                }
            }
            if let Some(manifest) = pending_asset_manifest.as_ref() {
                for asset in &manifest.assets {
                    if asset.status != AssetStatus::Exported {
                        continue;
                    }
                    let Some(path) = asset_output_paths.get(&asset.asset_id) else {
                        continue;
                    };
                    let data = asset.data_base64.as_deref().ok_or_else(|| {
                        DevupError::new(
                            ErrorCode::DevupSnapshotUnsupported,
                            "The exported asset binary is not in the artifact.",
                            false,
                        )
                    })?;
                    let bytes = STANDARD.decode(data.as_bytes()).map_err(|_| {
                        DevupError::new(
                            ErrorCode::DevupSnapshotUnsupported,
                            "The exported asset binary base64 is invalid.",
                            false,
                        )
                    })?;
                    planned_outputs.push((
                        format!("asset:{}", asset.asset_id),
                        output_policy.resolve(path)?,
                        bytes,
                    ));
                }
            }
            let mut transaction = OutputTransaction::new();
            // Two nodes can be one picture - a logo drawn at three sizes
            // shares a file, as the plugin has it - so several assets resolve
            // to one path. That is one write, not a collision. Only differing
            // bytes under one name are a mistake, and that is worth refusing.
            // Two different drawings under one layer name land here too: the
            // plugin names an asset after its layer, and a snapshot cannot
            // tell two drawings of one name apart - only the exported bytes
            // can. The first is written and the rest are reported, so the
            // code still points at a file that exists and the caller learns
            // which names hide more than one picture.
            let mut staged_content: BTreeMap<String, String> = BTreeMap::new();
            let mut shared_names: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (name, target, bytes) in planned_outputs {
                let path = target.display_path().to_string_lossy().into_owned();
                written_paths.insert(name.clone(), json!(path.clone()));
                let fingerprint = sha256_hex(&bytes);
                match staged_content.get(&path) {
                    Some(staged) if *staged == fingerprint => continue,
                    Some(_) => {
                        let asset = name.strip_prefix("asset:").unwrap_or(&name).to_owned();
                        shared_names.entry(path).or_default().push(asset);
                        continue;
                    }
                    None => {
                        staged_content.insert(path, fingerprint);
                    }
                }
                transaction.stage(name, target, &bytes)?;
            }
            if manifest_requested && let Some(mut manifest) = pending_asset_manifest {
                asset_resource_outputs = projected_asset_outputs(&manifest)?;
                for asset in &mut manifest.assets {
                    if asset.status != AssetStatus::Exported {
                        continue;
                    }
                    let output_name = format!("asset:{}", asset.asset_id);
                    let Some(path) = written_paths.get(&output_name).and_then(Value::as_str) else {
                        continue;
                    };
                    asset.output_path = Some(path.to_owned());
                    asset.data_base64 = None;
                }
                for (path, others) in &shared_names {
                    manifest.diagnostics.push(Diagnostic {
                        code: "DEVUP_ASSET_NAME_SHARED".to_owned(),
                        message: format!(
                            "{} further drawing(s) claim the file {path}; the first is written. \
                             An asset is named after its layer, as the plugin names it, so two \
                             drawings a designer named alike cannot both be written.",
                            others.len()
                        ),
                        severity: Some(DiagnosticSeverity::Warning),
                        resource_kind: Some("asset".to_owned()),
                        details: Some(json!({ "outputPath": path, "notWritten": others })),
                        ..Diagnostic::default()
                    });
                }
                let mut manifest = json!(manifest);
                if let Some(assets) = manifest["assets"].as_array_mut() {
                    for asset in assets {
                        let name = asset["nodeId"]
                            .as_str()
                            .and_then(|id| payload.snapshot.nodes.get(id))
                            .and_then(|node| node.typed_view().name());
                        if let Some(name) = name {
                            asset["layerName"] = json!(name);
                        }
                    }
                }
                result.insert("assetManifest".to_owned(), manifest);
            }
            result.insert("outputPaths".to_owned(), Value::Object(written_paths));
            for value in result.values_mut() {
                attach_diagnostic_screens(value, &owners);
            }
            let projected_outputs = projected_outputs_from_result(&result)?;
            let mut projected_outputs = projected_outputs;
            projected_outputs.extend(asset_resource_outputs);
            let mut result = Value::Object(result);
            let attachment = apply_delivery(
                &mut result,
                delivery,
                artifact_store,
                artifact,
                projected_outputs,
            )
            .await?;
            if let Err(error) = transaction.commit() {
                rollback_delivery(artifact_store, artifact, attachment).await;
                return Err(error);
            }
            commit_delivery(attachment);
            Ok(result)
        }
        PendingOperation::Collect | PendingOperation::Artifact { .. } => Err(DevupError::new(
            ErrorCode::DevupFigmaHandoffInvalid,
            "An internal collect operation cannot be completed from an MCP artifact.",
            false,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{host_safe_asset_path, with_host_safe_asset_paths};

    #[test]
    fn a_layer_name_no_file_system_takes_is_renamed_wherever_the_code_points_at_it() {
        let code = concat!(
            "<Box maskImage=\"url(/icons/ic:round-arrow-left.svg)\" />\n",
            "<Image src=\"/images/Frame 269.png\" />\n",
            "<Box maskImage=\"url('/icons/Frame 287.svg')\" />\n",
            "<Box maskImage=\"url(/icons/grommet-icons:language.svg)\" />\n",
        );
        assert_eq!(
            with_host_safe_asset_paths(code),
            concat!(
                "<Box maskImage=\"url(/icons/ic-round-arrow-left-b041d24e668d.svg)\" />\n",
                "<Image src=\"/images/Frame-269-3e2af8571e3f.png\" />\n",
                "<Box maskImage=\"url('/icons/Frame-287-cf31d759843d.svg')\" />\n",
                "<Box maskImage=\"url(/icons/grommet-icons-language-d201f62b16f5.svg)\" />\n",
            )
        );
    }

    #[test]
    fn the_written_path_is_renamed_the_same_way_the_code_is() {
        assert_eq!(
            host_safe_asset_path("/icons/grommet-icons:language.svg"),
            "/icons/grommet-icons-language-d201f62b16f5.svg"
        );
        assert_eq!(
            host_safe_asset_path("/images/Frame 269.png"),
            "/images/Frame-269-3e2af8571e3f.png"
        );
    }
}

/// Apply at the delivery boundary so diagnostics, evidence, and issues share
/// the same ID contract, including resource-delivered diagnostic containers.
fn attach_diagnostic_screens(value: &mut Value, owners: &BTreeMap<String, Option<String>>) {
    match value {
        Value::Array(items) => {
            for item in items {
                attach_diagnostic_screens(item, owners);
            }
        }
        Value::Object(object) => {
            if object.get("code").and_then(Value::as_str).is_some()
                && object.get("message").and_then(Value::as_str).is_some()
            {
                let owner = object
                    .get("nodeId")
                    .and_then(Value::as_str)
                    .and_then(|id| owners.get(id))
                    .and_then(|id| id.as_deref());
                object.insert("screenId".into(), json!(owner));
                if owner.is_none() {
                    object.insert("screenIdReason".into(), json!("This diagnostic has no node in a known selectable screen; no screen ID was inferred."));
                }
            }
            for child in object.values_mut() {
                attach_diagnostic_screens(child, owners);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod w1_regressions {
    use super::super::artifacts::ArtifactRequestKey;
    use super::*;
    use devup_mcp_figma::{CollectionRequest, CollectionScope};

    #[tokio::test]
    async fn r10_content_sizing_uncertainty_survives_diagnostics_opt_out() {
        let data: CollectedPayload = serde_json::from_str(include_str!(
            "../../../../fixtures/r2/wquw-118-payload.json"
        ))
        .unwrap();
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export {
            frame_ids,
            include_diagnostics,
            ..
        } = &mut op
        {
            *frame_ids = vec!["3997:46129".into()];
            *include_diagnostics = false;
        }
        let result = project(data, op).await.unwrap();
        for owner in [&result, &result["frames"][0]] {
            let evidence: Vec<_> = owner["projectionEvidence"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|d| d["details"]["resolution"] == "accounted-for-content-sizing")
                .collect();
            assert_eq!(evidence.len(), 2);
            for d in evidence {
                assert_eq!(d["screenId"], "3997:46129");
                assert_eq!(
                    d["details"]["verification"]["reasonCode"],
                    "font-metrics-not-measured"
                );
                assert_eq!(d["fidelityImpact"], "none");
            }
        }
    }

    #[tokio::test]
    async fn r10_diagnostics_identify_exportable_screen() {
        let data: CollectedPayload = serde_json::from_str(include_str!(
            "../../../../fixtures/r2/wquw-118-payload.json"
        ))
        .unwrap();
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export {
            frame_ids,
            include_diagnostics,
            ..
        } = &mut op
        {
            *frame_ids = vec!["3997:46129".into()];
            *include_diagnostics = true;
        }
        let result = project(data, op).await.unwrap();
        for key in ["diagnostics", "projectionIssues", "placementContracts"] {
            for d in result["frames"][0][key].as_array().unwrap() {
                assert_eq!(d["screenId"], "3997:46129", "{d}");
            }
        }
        for d in result["projectionIssues"].as_array().unwrap() {
            assert_eq!(d["screenId"], "3997:46129", "{d}");
        }
    }

    #[tokio::test]
    async fn r10_resource_note_does_not_claim_nonfinal_tsx_is_final() {
        let data: CollectedPayload = serde_json::from_str(include_str!(
            "../../../../fixtures/r2/wquw-120-payload.json"
        ))
        .unwrap();
        let mut op = operation(&["tsx", "componentTsx"]);
        if let PendingOperation::Export {
            frame_ids,
            delivery,
            ..
        } = &mut op
        {
            *frame_ids = vec!["3997:46582".into()];
            *delivery = DeliveryMode::Resource;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["deliverable"]["isFinal"], false);
        assert!(
            !result["deliverable"]["note"]
                .as_str()
                .unwrap()
                .contains("final TSX")
        );
    }

    #[tokio::test]
    async fn r10_descendant_selection_has_corrected_arguments() {
        let mut data = section(2);
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["child"]));
        data.snapshot.nodes.insert("child".into(), serde_json::from_value(json!({"id":"child","type":"TEXT","fields":{"parentId":"1:1","characters":"hello"}})).unwrap());
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["child".into()];
        }
        let error = project(data.clone(), op).await.unwrap_err();
        assert_eq!(
            error.details["nextAction"]["arguments"]["frameIds"],
            json!(["1:1"]),
            "{error:?}"
        );
        assert_eq!(
            error.details["selectionIssues"][0]["reason"],
            "descendant-of-screen"
        );
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["absent".into()];
        }
        let error = project(data, op).await.unwrap_err();
        assert_eq!(
            error.details["selectionIssues"][0]["reason"],
            "not-found-in-section"
        );
        assert!(error.details["nextAction"].is_null());
    }

    #[tokio::test]
    async fn r3_responsive_has_its_own_fidelity_result() {
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(responsive_payload(), op).await.unwrap();
        assert_eq!(
            result["outputResults"]["responsiveTsx"]["state"],
            "produced"
        );
        let fidelity = &result["outputResults"]["responsiveTsx"]["fidelity"];
        assert_eq!(fidelity["scope"], "breakpoint-source-projections");
        assert_eq!(fidelity["breakpoints"].as_array().unwrap().len(), 2);
        assert_eq!(fidelity["mergedMappingVerified"], false);
    }

    #[tokio::test]
    async fn r2_responsive_hidden_instance_and_visible_desktop() {
        for instance in [true, false] {
            let mut data = responsive_payload();
            for i in 1..=2 {
                let root = format!("1:{i}");
                let child = format!("2:{i}");
                data.snapshot
                    .nodes
                    .get_mut(&root)
                    .unwrap()
                    .fields
                    .insert("childrenIds".into(), json!([child]));
                data.snapshot.nodes.insert(
                    child.clone(),
                    serde_json::from_value(json!({
                        "id":child,"type":if instance {"INSTANCE"} else {"FRAME"},
                        "fields":{"parentId":root,"name":"Logo","width":24,"height":24,
                            "opacity":if instance || i == 1 {0} else {1},
                            "fills":[{"type":"IMAGE","imageHash":"logo","scaleMode":"TILE"}]}
                    }))
                    .unwrap(),
                );
            }
            let mut op = operation(&["responsiveTsx"]);
            if let PendingOperation::Export { all_screens, .. } = &mut op {
                *all_screens = true;
            }
            let result = project(data, op).await.unwrap();
            let code = result["responsiveTsx"]
                .as_str()
                .expect("responsive TSX exists");
            devup_mcp_devup_ui::validation::validate_tsx(code).unwrap();
            assert!(
                code.contains("visibility="),
                "hidden layout must survive: {code}"
            );
            if instance {
                assert!(!code.contains("/images/"), "{code}");
            } else {
                // Different paint prop sets can produce separate responsive
                // branches. The desktop branch must then be independently
                // visible instead of inheriting the hidden mobile box.
                let desktop = &code[code.find("bg=").expect("desktop paint")..];
                assert!(
                    code.contains("\"initial\"")
                        || (!desktop.contains("visibility=\"hidden\"")
                            && desktop.contains("\"block\"")),
                    "desktop must restore paint: {code}"
                );
                assert!(code.contains("/images/"), "desktop paint retained: {code}");
            }
        }
    }

    #[tokio::test]
    async fn r2_nonrendering_background_and_responsive_keep_code() {
        for scale in ["FILL", "TILE", "BACKGROUND"] {
            for field in ["tsx", "componentTsx", "responsiveTsx"] {
                let mut data = responsive_payload();
                for i in 1..=2 {
                    let root = format!("1:{i}");
                    let child = format!("2:{i}");
                    data.snapshot
                        .nodes
                        .get_mut(&root)
                        .unwrap()
                        .fields
                        .insert("childrenIds".into(), json!([child]));
                    data.snapshot.nodes.insert(child.clone(), serde_json::from_value(json!({"id":child,"type":if field == "componentTsx" { "INSTANCE" } else { "FRAME" },
                        "fields":{"parentId":root,"name":"Transparent paint","width":24,"height":24,"visible":true,"opacity":0,
                            "fills":[{"type":"IMAGE","imageHash":"hidden","scaleMode":if scale == "BACKGROUND" { "FILL" } else { scale }}]}})).unwrap());
                    if scale == "BACKGROUND" {
                        let text_id = format!("3:{i}");
                        data.snapshot
                            .nodes
                            .get_mut(&child)
                            .unwrap()
                            .fields
                            .insert("childrenIds".into(), json!([text_id]));
                        data.snapshot.nodes.insert(text_id.clone(), serde_json::from_value(json!({"id":text_id,"type":"TEXT",
                            "fields":{"parentId":child,"characters":"Retained text","width":20,"height":12}})).unwrap());
                    }
                }
                let mut op = operation(&[field, "assetManifest"]);
                if let PendingOperation::Export { all_screens, .. } = &mut op {
                    *all_screens = true;
                }
                let result = project(data, op).await.unwrap();
                assert!(
                    result["failures"].as_array().unwrap().is_empty(),
                    "{scale}/{field}: {result}"
                );
                if field != "responsiveTsx" {
                    for frame in result["frames"].as_array().unwrap() {
                        let code = frame[field]
                            .as_str()
                            .expect("background exclusion must retain code");
                        assert!(!code.contains("/images/"));
                        assert!(code.contains("visibility=\"hidden\""), "{field}: {code}");
                        devup_mcp_devup_ui::validation::validate_tsx(code).unwrap();
                    }
                } else {
                    let code = result[field]
                        .as_str()
                        .expect("responsive exclusion must retain code");
                    assert!(!code.contains("/images/"));
                    devup_mcp_devup_ui::validation::validate_tsx(code).unwrap();
                }
            }
        }
    }

    #[tokio::test]
    async fn r2_production_six_frames_return_valid_story_tsx() {
        let data: CollectedPayload = serde_json::from_str(include_str!(
            "../../../../fixtures/r2/wquw-119-payload.json"
        ))
        .unwrap();
        let generated = generate_collected_component(
            &data,
            "3997:46690",
            &CodegenOptions {
                inline_instances: true,
                asset_names_per_node: true,
                ..CodegenOptions::default()
            }
            .with_payload_tokens(&data),
        )
        .unwrap();
        let path =
            devup_mcp_devup_ui::codegen::image_fill_path(&data.snapshot, "3997:46703", 0, true)
                .unwrap();
        eprintln!(
            "story pre-guard excluded path {path}; referenced={}",
            generated.tsx.contains(&path)
        );
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export {
            frame_ids,
            include_diagnostics,
            ..
        } = &mut op
        {
            *frame_ids = [
                "3997:46315",
                "3997:46715",
                "3997:46333",
                "3997:46361",
                "3997:46461",
                "3997:46690",
            ]
            .map(str::to_owned)
            .to_vec();
            *include_diagnostics = false;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"].as_array().unwrap().len(), 6);
        for frame in result["frames"].as_array().unwrap() {
            let tsx = frame["tsx"]
                .as_str()
                .expect("every selected frame must retain TSX");
            devup_mcp_devup_ui::validation::validate_tsx(tsx).unwrap();
            assert!(!tsx.contains(&host_safe_asset_path(&path)));
            if frame["nodeId"] == "3997:46690" {
                assert!(tsx.contains("작은 시장, 큰 사랑"));
                assert!(tsx.contains("visibility=\"hidden\""));
                assert!(tsx.contains("boxSize=\"24px\""));
            }
        }
        assert!(result["failures"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn r2_withheld_output_has_issue_and_explicit_deliverable() {
        let mut data = payload();
        data.snapshot.nodes.get_mut("1:1").unwrap().node_type = "VECTOR".into();
        data.assets.push(serde_json::from_value(json!({"assetId":"1:1:node", "nodeId":"1:1",
            "field":"node", "sourceKind":"node", "status":"failed", "errorCode":"DEVUP_ASSET_NODE_HIDDEN"})).unwrap());
        let result = project(data, operation(&["tsx", "assetManifest"]))
            .await
            .unwrap();
        assert_eq!(result["deliverable"]["isFinal"], false);
        assert_eq!(result["deliverable"]["withheldOutputs"][0]["output"], "tsx");
        let issue = result["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["code"] == "DEVUP_EXCLUDED_ASSET_REFERENCED")
            .unwrap();
        assert_eq!(issue["code"], "DEVUP_EXCLUDED_ASSET_REFERENCED");
        assert_eq!(issue["property"], "assetReference");
        assert!(issue["details"]["originalValue"].is_object());
        assert_eq!(issue["details"]["appliedValue"]["state"], "withheld");
        assert!(issue["details"]["nextAction"].is_string());
    }

    #[tokio::test]
    async fn r2_component_layout_has_generated_evidence_and_output_scope() {
        let mut data = section(1);
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["2:1"]));
        data.snapshot.nodes.insert(
            "2:1".into(),
            serde_json::from_value(json!({
                "id":"2:1","type":"INSTANCE","fields":{"name":"Header", "parentId":"1:1",
                "visible":true,"width":360,"height":66,"childrenIds":[]}
            }))
            .unwrap(),
        );
        let mut op = operation(&["tsx", "componentTsx"]);
        if let PendingOperation::Export {
            all_screens,
            include_diagnostics,
            ..
        } = &mut op
        {
            *all_screens = true;
            *include_diagnostics = false;
        }
        let result = project(data, op).await.unwrap();
        let frame = &result["frames"][0];
        let issues = frame["projectionIssues"].as_array().unwrap();
        for issue in issues
            .iter()
            .filter(|i| i["code"] == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
        {
            assert!(issue["details"]["appliedValue"].is_object(), "{issue}");
            assert!(issue["details"]["output"].is_string(), "{issue}");
            assert!(issue["details"]["nextAction"].is_string(), "{issue}");
        }
        assert!(frame["outputResults"]["tsx"]["fidelity"].is_object());
        assert!(frame["outputResults"]["componentTsx"]["fidelity"].is_object());
        assert!(
            !frame["projectionReview"]["groups"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn r5_wquw119_missing_mask_size_is_output_scoped_and_not_final() {
        let mut data = payload();
        data.snapshot =
            serde_json::from_str(include_str!("../../../../fixtures/wquw-119-snapshot.json"))
                .unwrap();
        data.target.node_id = Some("3997:46315".into());
        data.target.file_key = data.snapshot.file_key.clone();
        let node = data.snapshot.nodes.get_mut("3997:46317").unwrap();
        node.fields.remove("height");
        node.fields.remove("absoluteBoundingBox");
        let mut op = operation(&["tsx", "sourceMap"]);
        if let PendingOperation::Export {
            include_diagnostics,
            ..
        } = &mut op
        {
            *include_diagnostics = false;
        }
        let result = project(data, op).await.unwrap();
        assert_ne!(result["quality"]["projection"], "exact");
        assert_eq!(result["deliverable"]["isFinal"], false);
        assert!(
            result["projectionIssues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| issue["nodeId"] == "3997:46317"
                    && issue["property"] == "height"
                    && issue["details"]["output"] == "tsx"),
            "{result}"
        );
    }

    #[tokio::test]
    async fn r4_direct_export_discloses_capture_boundary_without_diagnostics() {
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export {
            include_diagnostics,
            ..
        } = &mut op
        {
            *include_diagnostics = false;
        }
        let result = project(payload(), op).await.unwrap();
        let contract = &result["placementContracts"][0]["details"];
        assert_eq!(contract["output"], "tsx");
        assert_eq!(contract["parentCollected"], false);
        assert!(contract["parentMissingReason"].is_string());
        assert_eq!(contract["containingBlock"], "normal-flow-host");
        assert!(contract["coordinateBasis"].is_string());
    }

    #[tokio::test]
    async fn r4_export_exposes_placement_contract_without_diagnostics() {
        let data: CollectedPayload = serde_json::from_str(include_str!(
            "../../../../fixtures/r2/wquw-120-payload.json"
        ))
        .unwrap();
        let mut op = operation(&["tsx", "componentTsx"]);
        if let PendingOperation::Export {
            frame_ids,
            include_diagnostics,
            ..
        } = &mut op
        {
            *frame_ids = vec!["3997:46582".into()];
            *include_diagnostics = false;
        }
        let result = project(data, op).await.unwrap();
        let frame = &result["frames"][0];
        for field in ["tsx", "componentTsx"] {
            let source = frame[field].as_str().unwrap();
            let tag = source
                .split("return (")
                .nth(1)
                .unwrap()
                .split('>')
                .next()
                .unwrap();
            for prop in ["pos=\"relative\"", "w=\"360px\"", "h=\"740px\""] {
                assert!(tag.contains(prop), "{tag}");
            }
            let contract = frame["placementContracts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["details"]["output"] == field)
                .unwrap();
            assert_eq!(contract["details"]["containingBlock"], "generated-root");
        }
        assert_eq!(result["placementContracts"].as_array().unwrap().len(), 2);
        assert_eq!(
            frame["outputResults"]["tsx"]["fidelity"]["layout"]["covered"],
            117 // R9: prior 114 + verified root width/height and explicit modal height.
        );
        assert_eq!(
            frame["outputResults"]["componentTsx"]["fidelity"]["impacts"]["lossy"],
            40
        );
    }

    #[tokio::test]
    async fn r9_confirmed_evidence_survives_diagnostics_opt_out() {
        let mut data: CollectedPayload = serde_json::from_str(include_str!(
            "../../../../fixtures/r2/wquw-120-payload.json"
        ))
        .unwrap();
        data.snapshot
            .nodes
            .get_mut("3997:46621")
            .unwrap()
            .fields
            .insert("visible".into(), json!(false));
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export {
            frame_ids,
            include_diagnostics,
            ..
        } = &mut op
        {
            *frame_ids = vec!["3997:46582".into()];
            *include_diagnostics = false;
        }
        let result = project(data, op).await.unwrap();
        for owner in [&result, &result["frames"][0]] {
            let evidence = owner["projectionEvidence"]
                .as_array()
                .expect("confirmed evidence remains available without diagnostics");
            let d = evidence
                .iter()
                .find(|d| {
                    d["nodeId"] == "3997:46621" && d["code"] == "DEVUP_CODEGEN_NON_RENDERING_ASSET"
                })
                .unwrap();
            assert_eq!(d["fidelityImpact"], "none");
            assert_eq!(d["details"]["verification"]["field"], "visible");
            assert_eq!(d["details"]["verification"]["state"], "accounted-for");
            assert_eq!(d["details"]["output"], "tsx");
            assert!(
                !owner["projectionIssues"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|d| d["nodeId"] == "3997:46621")
            );
        }
    }

    #[tokio::test]
    async fn r2_production_layout_evidence_is_actionable_per_output() {
        for (raw, ids, fields, count) in [
            (
                include_str!("../../../../fixtures/r2/wquw-118-payload.json"),
                vec!["3997:46242", "3997:46277", "3997:46129"],
                vec!["tsx", "assetManifest"],
                0, // R10: both auto-size dimensions are accounted for, not lost.
            ),
            (
                include_str!("../../../../fixtures/r2/wquw-120-payload.json"),
                vec!["3997:46582"],
                vec!["tsx", "componentTsx", "assetManifest"],
                40,
            ),
        ] {
            let data: CollectedPayload = serde_json::from_str(raw).unwrap();
            let mut op = operation(&fields);
            if let PendingOperation::Export {
                frame_ids,
                include_diagnostics,
                ..
            } = &mut op
            {
                *frame_ids = ids.into_iter().map(str::to_owned).collect();
                *include_diagnostics = false;
            }
            let result = project(data, op).await.unwrap();
            let issues = result["projectionIssues"].as_array().unwrap();
            for issue in issues
                .iter()
                .filter(|i| i["code"] == "DEVUP_CODEGEN_ABSOLUTE_FALLBACK")
            {
                let original = &issue["details"]["originalValue"];
                assert_eq!(original["x"], 0);
                assert_eq!(original["y"], 0);
                assert_eq!(original["width"], 360);
                assert_eq!(original["height"], 740);
                assert_eq!(original["parent"]["width"], 360);
                assert_eq!(original["constraints"]["horizontal"], "CENTER");
                assert_eq!(original["children"][0]["x"], 20);
                let padding = &issue["details"]["appliedValue"]["derivedPadding"];
                assert_eq!(padding["left"], 20.0);
                assert_eq!(padding["top"], if count == 40 { 232.5 } else { 185.5 });
            }
            let layout: Vec<_> = issues
                .iter()
                .filter(|i| i["code"] == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
                .collect();
            let original_class = if count == 40 {
                "component-reference"
            } else {
                "text-auto-size"
            };
            if count == 0 {
                assert_eq!(
                    result["projectionEvidence"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|d| d["details"]["resolution"] == "accounted-for-content-sizing")
                        .count(),
                    2
                );
            }
            assert_eq!(
                layout
                    .iter()
                    .filter(|i| i["details"]["classification"] == original_class)
                    .count(),
                count
            );
            for issue in layout {
                assert_eq!(
                    issue["details"]["appliedValue"]["state"], "emitted",
                    "{issue}"
                );
                assert!(
                    issue["details"]["appliedValue"]["generatedSource"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                );
                assert!(issue["details"]["nextAction"].is_string());
                if issue["details"]["classification"] == "component-reference" {
                    assert_eq!(issue["details"]["output"], "componentTsx");
                    assert_eq!(issue["details"]["classification"], "component-reference");
                } else if issue["property"] == "layoutSizingVertical" {
                    // R6 exposes a real percentage-height dependency that R2 did not count.
                    assert_eq!(issue["nodeId"], "3997:46313");
                    assert_eq!(issue["details"]["output"], "tsx");
                    assert_eq!(issue["fidelityImpact"], "lossy");
                    assert_eq!(issue["details"]["originalValue"], "FILL");
                    assert_eq!(
                        issue["details"]["implicitCssVerification"]["state"],
                        "not-accounted-for"
                    );
                } else if matches!(
                    issue["details"]["classification"].as_str(),
                    Some("asset-projection" | "property-unmapped")
                ) {
                    // R5 additionally discloses unproven asset percentage axes.
                    assert_eq!(issue["details"]["output"], "tsx");
                    assert!(
                        matches!(issue["property"].as_str(), Some("width" | "height")),
                        "{issue}"
                    );
                } else {
                    assert_eq!(issue["details"]["output"], "tsx");
                    assert_eq!(issue["details"]["classification"], "text-auto-size");
                    assert!(
                        issue["details"]["appliedValue"]["generatedSource"]
                            .as_str()
                            .unwrap()
                            .contains("<Text")
                    );
                }
            }
            if count == 40 {
                let frame = &result["frames"][0];
                assert_eq!(frame["outputResults"]["tsx"]["projection"], "approximated");
                assert_eq!(
                    frame["outputResults"]["componentTsx"]["projection"],
                    "lossy"
                );
                let groups = frame["projectionReview"]["groups"].as_array().unwrap();
                assert_eq!(
                    groups
                        .iter()
                        .filter(|g| g["classification"] == "component-reference")
                        .count(),
                    5
                );
            }
        }
    }

    #[tokio::test]
    async fn r1_hidden_asset_paths_absent_from_all_six_frames() {
        let mut data = section(6);
        data.snapshot
            .nodes
            .get_mut("1:6")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["3997:46703"]));
        data.snapshot.nodes.insert("3997:46703".into(), serde_json::from_value(json!({
            "id":"3997:46703","type":"FRAME","fields":{"name":"Hidden image", "parentId":"1:6",
            "visible":false,"width":24,"height":24,"fills":[{"type":"IMAGE","imageHash":"hidden","scaleMode":"FILL"}]}
        })).unwrap());
        let path = host_safe_asset_path(
            &devup_mcp_devup_ui::codegen::image_fill_path(&data.snapshot, "3997:46703", 0, true)
                .unwrap(),
        );
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export {
            all_screens,
            include_diagnostics,
            ..
        } = &mut op
        {
            *all_screens = true;
            *include_diagnostics = false;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"].as_array().unwrap().len(), 6);
        assert!(
            result["assetManifest"]["assets"]
                .as_array()
                .unwrap()
                .iter()
                .all(|a| a["assetId"] != "3997:46703:fills:0")
        );
        for frame in result["frames"].as_array().unwrap() {
            assert!(!frame["tsx"].as_str().unwrap().contains(&path), "{frame}");
        }
        assert_eq!(result["quality"]["assets"], "not-requested");
        assert!(result["assetSummary"]["description"].is_string());
    }

    #[tokio::test]
    async fn r1_approximation_is_explained_without_diagnostics() {
        let mut data = section(1);
        let node = data.snapshot.nodes.get_mut("1:1").unwrap();
        node.fields
            .insert("layoutPositioning".into(), json!("ABSOLUTE"));
        node.fields
            .insert("relativeTransform".into(), json!([[1, 0, 0], [0.5, 1, 0]]));
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export {
            all_screens,
            include_diagnostics,
            ..
        } = &mut op
        {
            *all_screens = true;
            *include_diagnostics = false;
        }
        let result = project(data, op).await.unwrap();
        let frame = &result["frames"][0];
        assert_eq!(frame["quality"]["projection"], "approximated");
        let issue = &frame["projectionIssues"][0];
        assert_eq!(issue["nodeId"], "1:1");
        assert_eq!(issue["property"], "layoutPositioning");
        assert_eq!(
            issue["details"]["originalValue"]["layoutPositioning"],
            "ABSOLUTE"
        );
        assert_eq!(
            issue["details"]["originalValue"]["relativeTransform"],
            json!([[1, 0, 0], [0.5, 1, 0]])
        );
        assert!(issue["details"]["originalValue"]["parentMissingReason"].is_string());
        assert!(issue["details"]["appliedValue"].is_object());
        assert!(issue["message"].is_string());
    }

    #[tokio::test]
    async fn r1_uncovered_layout_cannot_be_exact() {
        let mut data = section(1);
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["2:1"]));
        data.snapshot.nodes.insert(
            "2:1".into(),
            serde_json::from_value(json!({
                "id":"2:1","type":"INSTANCE","fields":{"name":"Widget", "parentId":"1:1",
                "visible":true,"width":24,"height":24,"childrenIds":[]}
            }))
            .unwrap(),
        );
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export {
            all_screens,
            include_diagnostics,
            ..
        } = &mut op
        {
            *all_screens = true;
            *include_diagnostics = false;
        }
        let result = project(data, op).await.unwrap();
        let frame = &result["frames"][0];
        if frame["fidelity"]["layout"]["covered"] != frame["fidelity"]["layout"]["total"] {
            assert_ne!(frame["quality"]["projection"], "exact");
            assert!(
                frame["projectionIssues"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|i| i["code"] == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
            );
            assert_ne!(result["deliverable"]["isFinal"], true);
        } else {
            panic!("fixture must reproduce a layout shortfall: {frame}");
        }
    }

    #[tokio::test]
    async fn r1_required_excluded_asset_withholds_code_and_finality() {
        let mut data = payload();
        data.snapshot.nodes.get_mut("1:1").unwrap().node_type = "VECTOR".into();
        data.assets.push(serde_json::from_value(json!({"assetId":"1:1:node", "nodeId":"1:1",
            "field":"node", "sourceKind":"node", "status":"failed", "errorCode":"DEVUP_ASSET_NODE_HIDDEN"})).unwrap());
        let result = project(data, operation(&["tsx", "assetManifest"]))
            .await
            .unwrap();
        assert!(result.get("tsx").is_none());
        assert_eq!(result["status"], "partial");
        assert_ne!(result["deliverable"]["isFinal"], true);
        assert!(
            result["failures"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["errorCode"] == "DEVUP_EXCLUDED_ASSET_REFERENCED")
        );
    }

    #[tokio::test]
    async fn r1_responsive_only_reports_source_approximation() {
        let mut data = responsive_payload();
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("layoutPositioning".into(), json!("ABSOLUTE"));
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export {
            all_screens,
            include_diagnostics,
            ..
        } = &mut op
        {
            *all_screens = true;
            *include_diagnostics = false;
        }
        let result = project(data, op).await.unwrap();
        assert_ne!(result["quality"]["projection"], "exact");
        assert!(
            result["projectionIssues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["nodeId"] == "1:1" && i["property"] == "layoutPositioning")
        );
    }

    #[tokio::test]
    async fn r1_captured_asset_overrides_hidden_discovery_prediction() {
        let mut data = payload();
        let node = data.snapshot.nodes.get_mut("1:1").unwrap();
        node.node_type = "VECTOR".into();
        node.fields.insert(
            "absoluteBoundingBox".into(),
            json!({"x":0,"y":0,"width":24,"height":24}),
        );
        data.assets.push(
            devup_mcp_figma::exported_asset_from_bytes(
                &devup_mcp_figma::AssetRequest {
                    asset_id: "1:1:node".into(),
                    node_id: "1:1".into(),
                    field: "node".into(),
                    image_hash: None,
                    format: devup_mcp_figma::AssetFormat::Svg,
                    scale: 1,
                },
                b"<svg/>",
            )
            .unwrap(),
        );
        let result = project(data, operation(&["tsx", "assetManifest"]))
            .await
            .unwrap();
        assert!(result["tsx"].is_string());
        assert_eq!(result["failures"], json!([]));
        assert_eq!(result["assetSummary"]["status"], "collected");
    }

    #[tokio::test]
    async fn r1_shared_visible_asset_path_is_not_excluded() {
        let mut data = payload();
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["2:1", "2:2", "2:3"]));
        for (id, visible) in [("2:1", false), ("2:2", true)] {
            data.snapshot.nodes.insert(id.into(), serde_json::from_value(json!({"id":id,"type":"VECTOR",
                "fields":{"name":"Shared", "parentId":"1:1", "visible":visible,"width":24,"height":24}})).unwrap());
        }
        data.snapshot.nodes.insert("2:3".into(), serde_json::from_value(json!({"id":"2:3", "type":"TEXT", "fields":{"parentId":"1:1", "characters":"Label"}})).unwrap());
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export {
            asset_names_per_node,
            ..
        } = &mut op
        {
            *asset_names_per_node = false;
        }
        let result = project(data, op).await.unwrap();
        assert!(result["tsx"].is_string());
        assert_eq!(result["failures"], json!([]));
        let path = result["assetManifest"]["assets"][0]["path"]
            .as_str()
            .unwrap();
        assert!(result["tsx"].as_str().unwrap().contains(path));
    }

    fn payload() -> CollectedPayload {
        serde_json::from_value(json!({
            "target": {"fileKey":"W1Fixture", "nodeId":"1:1", "branchKey":null},
            "scope":"node", "metadata":{}, "variables":null, "styles":null,
            "completeness":"resolved-values-only", "sourceVersion":null,
            "snapshot": {"fileKey":"W1Fixture", "version":null, "roots":["1:1"],
                "nodes":{"1:1":{"id":"1:1","type":"FRAME","fields":{
                    "name":"Screen", "x":0,"y":0,"width":390,"height":800,
                    "visible":true,"childrenIds":[]}}},"diagnostics":[]}
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn p1_frames_report_generated_asset_only_equivalence() {
        let mut data = section(3);
        for i in 1..=3 {
            let id = format!("1:{i}");
            let node = data.snapshot.nodes.get_mut(&id).unwrap();
            node.fields.insert(
                "fills".into(),
                json!([{"type":"IMAGE", "imageHash":format!("image{i}"), "scaleMode":"FILL"}]),
            );
        }
        data.snapshot
            .nodes
            .get_mut("1:3")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["3:1"]));
        data.snapshot.nodes.insert(
            "3:1".into(),
            serde_json::from_value(json!({
                "id":"3:1", "type":"TEXT", "fields":{"name":"Copy", "parentId":"1:3",
                    "characters":"Different copy", "fontSize":16, "width":100, "height":24}
            }))
            .unwrap(),
        );
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"][1]["structurallyIdenticalTo"], "1:1");
        assert_eq!(
            result["frames"][1]["differsOnly"],
            json!(["assetReferences"])
        );
        assert!(result["frames"][2].get("structurallyIdenticalTo").is_none());
    }

    #[tokio::test]
    async fn p1_asset_summary_distinguishes_none_uncollected_and_hidden() {
        let result = project(payload(), operation(&["tsx"])).await.unwrap();
        assert_eq!(result["assetSummary"]["status"], "none");
        let mut data = payload();
        data.snapshot.nodes.get_mut("1:1").unwrap().node_type = "VECTOR".into();
        let result = project(data.clone(), operation(&["assetManifest"]))
            .await
            .unwrap();
        assert_eq!(result["assetSummary"]["status"], "not-collected");
        assert_eq!(
            result["assetSummary"]["unavailable"][0]["reason"],
            "not-requested"
        );
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("visible".into(), json!(false));
        let result = project(data, operation(&["assetManifest"])).await.unwrap();
        assert_eq!(result["assetSummary"]["status"], "not-collected");
        assert_eq!(
            result["assetSummary"]["unavailable"][0]["reason"],
            "hidden-node"
        );
    }

    #[tokio::test]
    async fn p1_incomplete_asset_discovery_never_claims_no_assets() {
        let mut data = payload();
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["missing"]));
        let result = project(data, operation(&["assetManifest"])).await.unwrap();
        assert_eq!(result["assetSummary"]["status"], "unknown");
        assert_eq!(result["assetSummary"]["discovery"], "incomplete");
    }

    #[tokio::test]
    async fn p1_selection_separates_explicit_cases_from_automatic_screens() {
        let mut data = section(2);
        data.metadata["sectionIndex"]["candidates"][1]["bounds"]["width"] = json!(150);
        data.metadata["sectionIndex"]["candidates"][1]["bounds"]["height"] = json!(150);
        let result = project(data.clone(), operation(&["tsx"])).await.unwrap();
        assert_eq!(result["selection"]["count"], 1);
        assert_eq!(
            result["selection"]["explicitCandidates"][0]["node"]["nodeId"],
            "1:2"
        );
        assert_eq!(
            result["selection"]["explicitCandidates"][0]["node"]["kind"],
            "annotation"
        );
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["1:2".into()];
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"][0]["nodeId"], "1:2");
    }

    #[test]
    fn p1_asset_comparison_preserves_literal_text_and_unknown_paths() {
        let paths = vec!["/icons/a.svg".into(), "/icons/b.svg".into()];
        assert_eq!(
            generated_structure("<Box maskImage=\"url('/icons/a.svg')\" />", &paths),
            generated_structure("<Box maskImage=\"url('/icons/b.svg')\" />", &paths)
        );
        assert_ne!(
            generated_structure("<Text>{\"url(/icons/a.svg)\"}</Text>", &paths),
            generated_structure("<Text>{\"url(/icons/b.svg)\"}</Text>", &paths)
        );
        assert_ne!(
            generated_structure("<Image src=\"/icons/a.svg\" />", &paths),
            generated_structure("<Image src=\"/icons/unknown.svg\" />", &paths)
        );
    }

    #[test]
    fn p1_layered_backgrounds_compare_every_asset_reference() {
        let paths = vec!["/images/a.png".into(), "/images/b.png".into()];
        let first = "<Box bg=\"linear-gradient(red, blue), url('/images/a.png') center/cover, url(/images/b.png)\" />";
        let second = "<Box bg=\"linear-gradient(red, blue), url('/images/b.png') center/cover, url(/images/a.png)\" />";
        assert_eq!(
            generated_structure(first, &paths),
            generated_structure(second, &paths)
        );
        assert_ne!(
            generated_structure(first, &paths),
            generated_structure(&second.replace("red", "green"), &paths)
        );
    }

    #[tokio::test]
    async fn p1_selection_does_not_claim_to_include_an_unproduced_manifest() {
        let result = project(section(2), operation(&["tsx", "assetManifest"]))
            .await
            .unwrap();
        assert_eq!(result["status"], "selection_required");
        assert!(result.get("assetManifest").is_none());
        assert_eq!(result["assetSummary"]["manifestIncluded"], false);
    }

    #[test]
    fn p1_comparison_scopes_partial_frames_to_their_generated_outputs() {
        let mut frames = vec![
            json!({"nodeId":"1", "status":"complete", "tsx":"code", "componentTsx":"component"}),
            json!({"nodeId":"2", "status":"partial", "tsx":"code"}),
            json!({"nodeId":"3", "status":"partial", "tsx":"code", "componentTsx":"component"}),
            json!({"nodeId":"4", "status":"partial"}),
        ];
        annotate_identical_frames(&mut frames, &payload(), true);
        assert!(frames[1].get("structurallyIdenticalTo").is_none());
        assert_eq!(frames[2]["structurallyIdenticalTo"], "1");
        assert_eq!(frames[2]["comparedOutputs"], json!(["tsx", "componentTsx"]));
        assert!(frames[3].get("structurallyIdenticalTo").is_none());
    }

    #[tokio::test]
    async fn p1_identical_hints_and_asset_summary_survive_resource_delivery() {
        let mut op = operation(&["tsx", "componentTsx"]);
        if let PendingOperation::Export {
            all_screens,
            delivery,
            ..
        } = &mut op
        {
            *all_screens = true;
            *delivery = DeliveryMode::Resource;
        }
        let result = project(section(2), op).await.unwrap();
        assert!(result["frames"][0].get("tsx").is_none());
        assert_eq!(result["frames"][1]["structurallyIdenticalTo"], "1:1");
        assert_eq!(result["frames"][1]["differsOnly"], json!([]));
        assert_eq!(
            result["frames"][1]["comparedOutputs"],
            json!(["tsx", "componentTsx"])
        );
        assert!(result["assetSummary"].is_object());
        assert!(result["frames"][0]["assetSummary"].is_object());
    }

    #[tokio::test]
    async fn p1_collected_assets_are_distinct_from_delivered_manifest() {
        let mut data = payload();
        data.snapshot.nodes.get_mut("1:1").unwrap().node_type = "VECTOR".into();
        data.assets.push(
            devup_mcp_figma::exported_asset_from_bytes(
                &devup_mcp_figma::AssetRequest {
                    asset_id: "1:1:node".into(),
                    node_id: "1:1".into(),
                    field: "node".into(),
                    image_hash: None,
                    format: devup_mcp_figma::AssetFormat::Svg,
                    scale: 1,
                },
                b"<svg/>",
            )
            .unwrap(),
        );
        let result = project(data, operation(&["tsx"])).await.unwrap();
        assert_eq!(result["assetSummary"]["status"], "collected");
        assert_eq!(result["assetSummary"]["collectedCount"], 1);
        assert_eq!(result["assetSummary"]["manifestIncluded"], false);
        assert!(result.get("assetManifest").is_none());
    }

    #[tokio::test]
    async fn p3_identical_bytes_share_manifest_paths() {
        let mut data = payload();
        for (id, path) in [("1:2", "/icons/a.svg"), ("1:3", "/icons/b.svg")] {
            data.assets.push(
                serde_json::from_value(json!({
                    "assetId":id,"nodeId":id,"field":"svg","sourceKind":"svg",
                    "status":"exported","format":"svg","scale":1,"mimeType":"image/svg+xml",
                    "dataBase64":STANDARD.encode(b"<svg/>"), "byteLength":6,
                    "sha256":sha256_hex(b"<svg/>"),"path":path
                }))
                .unwrap(),
            );
        }
        let result = project(data, operation(&["assetManifest"])).await.unwrap();
        let assets = result["assetManifest"]["assets"].as_array().unwrap();
        assert_eq!(assets[0]["path"], assets[1]["path"]);
    }

    #[tokio::test]
    async fn p3_selection_call_preserves_export_options() {
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export {
            component_name,
            strict,
            ..
        } = &mut op
        {
            *component_name = Some("TicketScreen".into());
            *strict = true;
        }
        let result = project(section(2), op).await.unwrap();
        let args = &result["nextAction"]["example"]["arguments"];
        assert_eq!(args["componentName"], "TicketScreen");
        assert_eq!(args["strict"], true);
        assert_eq!(args["frameIds"], json!(["1:1"]));
    }

    fn p3_asset_payload(second_bytes: &[u8]) -> CollectedPayload {
        let mut data = payload();
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["1:2", "1:3"]));
        for (id, bytes) in [("1:2", b"first image".as_slice()), ("1:3", second_bytes)] {
            data.snapshot.nodes.insert(
                id.into(),
                serde_json::from_value(json!({
                    "id":id,"type":"RECTANGLE","fields":{"name":"Logo","parentId":"1:1",
                    "visible":true,"width":40,"height":40,"x":0,"y":0,"isAsset":true,
                    "fills":[{"type":"IMAGE","imageHash":id,"scaleMode":"FILL"}]}
                }))
                .unwrap(),
            );
            data.assets.push(serde_json::from_value(json!({
                "assetId":format!("{id}:fills:0"),"nodeId":id,"field":"fills/0","sourceKind":"image",
                "status":"exported","format":"png","scale":1,"mimeType":"image/png",
                "dataBase64":STANDARD.encode(bytes),"byteLength":bytes.len(),"sha256":sha256_hex(bytes)
            })).unwrap());
        }
        data
    }

    #[tokio::test]
    async fn p3_public_mapping_and_dedup_rewrite_inline_and_written_tsx() {
        let directory = std::env::temp_dir().join(format!(
            "devup-p3-assets-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let root = dunce::canonicalize(&directory).unwrap();
        let first_path = root.join("icons/BI icon.png");
        let second_path = root.join("icons/other.png");
        let tsx_path = root.join("Screen.tsx");
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export {
            asset_public_root,
            asset_output_paths,
            output_paths,
            ..
        } = &mut op
        {
            *asset_public_root = Some(root.clone());
            asset_output_paths.insert("1:2:fills:0".into(), first_path.to_string_lossy().into());
            asset_output_paths.insert("1:3:fills:0".into(), second_path.to_string_lossy().into());
            output_paths.insert("tsx".into(), tsx_path.to_string_lossy().into());
        }
        let result = project(p3_asset_payload(b"first image"), op).await.unwrap();
        let code = result["tsx"].as_str().unwrap();
        assert!(code.contains("/icons/BI%20icon.png"), "{code}");
        assert_eq!(std::fs::read_to_string(&tsx_path).unwrap(), code);
        assert_eq!(std::fs::read(&first_path).unwrap(), b"first image");
        assert!(!second_path.exists());
        for asset in result["assetManifest"]["assets"].as_array().unwrap() {
            assert_eq!(asset["path"], "/icons/BI%20icon.png");
            assert_eq!(
                std::path::Path::new(asset["outputPath"].as_str().unwrap()),
                first_path
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn p3_same_name_different_bytes_keep_distinct_paths_and_default_names() {
        let result = project(
            p3_asset_payload(b"different image"),
            operation(&["tsx", "assetManifest"]),
        )
        .await
        .unwrap();
        let assets = result["assetManifest"]["assets"].as_array().unwrap();
        assert_ne!(assets[0]["path"], assets[1]["path"]);
        for asset in assets {
            assert!(
                result["tsx"]
                    .as_str()
                    .unwrap()
                    .contains(asset["path"].as_str().unwrap())
            );
        }
    }

    #[tokio::test]
    async fn p3_identical_assets_share_one_binary_resource() {
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export { delivery, .. } = &mut op {
            *delivery = DeliveryMode::Resource;
        }
        let result = project(p3_asset_payload(b"first image"), op).await.unwrap();
        let resources = result["resources"].as_array().unwrap();
        assert_eq!(
            resources
                .iter()
                .filter(|resource| resource["mimeType"] == "image/png")
                .count(),
            1
        );
    }

    #[test]
    fn p3_rewrites_all_code_variants_without_cascading_or_prefix_changes() {
        let paths = BTreeMap::from([
            ("/icons/a.svg".into(), "/icons/b.svg".into()),
            ("/icons/b.svg".into(), "/icons/c.svg".into()),
        ]);
        let mut result = json!({"tsx":"<Image src=\"/icons/a.svg\" />",
            "componentTsx":"<Box maskImage=\"url(/icons/a.svg)\" />",
            "responsiveTsx":"<Image src=\"/icons/b.svg\" />",
            "frames":[{"tsx":"<Image src=\"/icons/a.svg\" />"}],
            "other":"/icons/a.svg"});
        rewrite_result_asset_references(&mut result, &paths);
        assert_eq!(result["tsx"], "<Image src=\"/icons/b.svg\" />");
        assert_eq!(result["frames"][0]["tsx"], result["tsx"]);
        assert_eq!(
            result["componentTsx"],
            "<Box maskImage=\"url(/icons/b.svg)\" />"
        );
        assert_eq!(result["responsiveTsx"], "<Image src=\"/icons/c.svg\" />");
        assert_eq!(result["other"], "/icons/a.svg");
        assert_eq!(
            rewrite_asset_references("url(/icons/a.svg.extra)", &paths),
            "url(/icons/a.svg.extra)"
        );
    }

    #[tokio::test]
    async fn p3_selection_reference_png_followup_targets_single_url() {
        let result = project(section(2), operation(&["tsx", "referencePng"]))
            .await
            .unwrap();
        let args = &result["nextAction"]["example"]["arguments"];
        assert!(args["url"].is_string());
        assert!(args.get("frameIds").is_none());
        assert!(args.get("artifactId").is_none());
        assert_eq!(args["scope"], "node");
    }

    #[tokio::test]
    async fn p3_default_paths_are_not_rewritten_to_requested_files() {
        let directory = std::env::temp_dir().join(format!(
            "devup-p3-default-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("chosen.png");
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export {
            asset_output_paths, ..
        } = &mut op
        {
            asset_output_paths.insert("1:2:fills:0".into(), path.to_string_lossy().into());
        }
        let result = project(p3_asset_payload(b"different"), op).await.unwrap();
        assert!(path.exists());
        let asset = &result["assetManifest"]["assets"][0];
        assert_ne!(asset["path"], "/chosen.png");
        assert!(
            result["tsx"]
                .as_str()
                .unwrap()
                .contains(asset["path"].as_str().unwrap())
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn p3_unverified_equal_hashes_do_not_merge_different_bytes() {
        let mut data = p3_asset_payload(b"different");
        data.assets[1].sha256 = data.assets[0].sha256.clone();
        let error = project(data, operation(&["tsx", "assetManifest"]))
            .await
            .unwrap_err();
        assert!(error.message.contains("hash does not match"), "{error}");
        assert!(!super::super::validation::is_caller_mistake(&error));
    }

    #[tokio::test]
    async fn p3_tsx_only_reprojection_preserves_deduplicated_references() {
        let data = p3_asset_payload(b"first image");
        let with_manifest = project(data.clone(), operation(&["tsx", "assetManifest"]))
            .await
            .unwrap();
        let without_manifest = project(data, operation(&["tsx"])).await.unwrap();
        assert_eq!(with_manifest["tsx"], without_manifest["tsx"]);
        assert!(without_manifest.get("assetManifest").is_none());
    }

    #[tokio::test]
    async fn p3_canonical_reference_is_independent_of_requested_duplicate_file() {
        let directory = std::env::temp_dir().join(format!(
            "devup-p3-canonical-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let data = p3_asset_payload(b"first image");
        let canonical = project(data.clone(), operation(&["tsx", "assetManifest"]))
            .await
            .unwrap();
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export {
            asset_output_paths, ..
        } = &mut op
        {
            asset_output_paths.insert(
                "1:3:fills:0".into(),
                directory.join("chosen.png").to_string_lossy().into(),
            );
        }
        let written = project(data, op).await.unwrap();
        assert_eq!(canonical["tsx"], written["tsx"]);
        assert_eq!(
            canonical["assetManifest"]["assets"][0]["path"],
            written["assetManifest"]["assets"][0]["path"]
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn section(count: usize) -> CollectedPayload {
        let mut payload = payload();
        let template = payload.snapshot.nodes["1:1"].clone();
        payload.target.node_id = Some("0:1".into());
        payload.snapshot.nodes.clear();
        payload.snapshot.roots.clear();
        let mut candidates = Vec::new();
        for i in 1..=count {
            let id = format!("1:{i}");
            let mut node = template.clone();
            node.id = id.clone();
            payload.snapshot.nodes.insert(id.clone(), node);
            payload.snapshot.roots.push(id.clone());
            candidates.push(json!({"nodeId":id,"name":"Screen","nodeType":"FRAME",
                "textPreview":"","visible":true,"bounds":{"x":0,"y":0,"width":390,"height":800},
                "parentId":"0:1","breadcrumb":[],"directChildCount":0,
                "subtreeNodeCount":1,"estimatedSerializedBytes":500,"selectionReasons":["inside-section"],
                "canonicalUrl":format!("https://www.figma.com/design/W1Fixture/Test?node-id=1-{i}")}));
        }
        payload.metadata = json!({"sectionIndex":{"fileKey":"W1Fixture","sourceVersion":null,
            "section":{"nodeId":"0:1","name":"Section","bounds":{"x":0,"y":0,"width":2000,"height":2000}},
            "candidates":candidates,"truncated":false}});
        payload
    }

    fn operation(outputs: &[&str]) -> PendingOperation {
        PendingOperation::Export {
            outputs: outputs.iter().map(|s| (*s).into()).collect(),
            component_name: None,
            include_diagnostics: true,
            root_layout: Default::default(),
            asset_names_per_node: true,
            scope: "node".into(),
            strict: false,
            output_paths: BTreeMap::new(),
            frame_ids: vec![],
            all_screens: false,
            asset_captures: vec![],
            asset_output_paths: BTreeMap::new(),
            asset_public_root: None,
            delivery: DeliveryMode::Inline,
        }
    }

    fn section_without_index(count: usize, projection_truncated: bool) -> CollectedPayload {
        let mut data = section(count);
        data.metadata = json!({});
        let mut root = data.snapshot.nodes["1:1"].clone();
        root.id = "0:1".into();
        root.node_type = "SECTION".into();
        root.fields = serde_json::from_value(json!({
            "name":"Section", "visible":true, "x":0, "y":0,
            "width":2000, "height":2000,
            "childrenIds":data.snapshot.roots,
            "projectionTruncated":projection_truncated
        }))
        .unwrap();
        for node in data.snapshot.nodes.values_mut() {
            node.fields.insert("parentId".into(), json!("0:1"));
        }
        data.snapshot.nodes.insert(root.id.clone(), root);
        data.snapshot.roots = vec!["0:1".into()];
        data
    }

    #[tokio::test]
    async fn w1_selection_exactly_100_candidates_is_complete() {
        let result = project(section_without_index(100, false), operation(&["tsx"]))
            .await
            .unwrap();
        assert_eq!(result["status"], "selection_required");
        assert_eq!(result["selection"]["count"], 100);
        assert_eq!(result["selection"]["truncated"], false);
        assert_eq!(result["selection"]["status"], "complete");
    }

    #[tokio::test]
    async fn w1_selection_over_100_candidates_is_partial() {
        let result = project(section_without_index(101, false), operation(&["tsx"]))
            .await
            .unwrap();
        assert_eq!(result["selection"]["count"], 100);
        assert_eq!(result["selection"]["truncated"], true);
        assert_eq!(result["selection"]["status"], "partial");
    }

    #[tokio::test]
    async fn w1_selection_separates_projection_and_candidate_truncation() {
        let data = section_without_index(100, true);
        let explored =
            explore_snapshot(&data.snapshot, &data.target, &ExploreOptions { limit: 100 }).unwrap();
        assert!(explored.truncated);
        assert!(!explored.candidates_truncated);
        let result = project(data, operation(&["tsx"])).await.unwrap();
        assert_eq!(result["selection"]["truncated"], false);
        assert_eq!(result["selection"]["status"], "complete");
    }

    #[tokio::test]
    async fn r7_real_wquw118_layout_contracts_are_preserved() {
        let data: CollectedPayload = serde_json::from_str(include_str!(
            "../../../../fixtures/r2/wquw-118-payload.json"
        ))
        .unwrap();
        let mut op = operation(&["tsx", "sourceMap"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["3997:46277".into()];
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"].as_array().unwrap().len(), 1);
        let frame = &result["frames"][0];
        assert_eq!(frame["nodeId"], "3997:46277");
        let entries = frame["sourceMap"]["entries"].as_array().unwrap();
        for (id, field, resolution) in [
            ("3997:46293", "width", "accounted-for-implicit-flex-grow"),
            ("3997:46295", "width", "accounted-for-implicit-flex-grow"),
            (
                "3997:46313",
                "layoutSizingVertical",
                "accounted-for-implicit-flex-stretch",
            ),
        ] {
            assert!(
                entries.iter().any(|e| e["nodeId"] == id
                    && e["property"] == field
                    && e["resolution"] == resolution),
                "{id}"
            );
        }
        assert!(
            frame["tsx"]
                .as_str()
                .unwrap()
                .contains("alignSelf=\"stretch\"")
        );
        for id in ["3997:46293", "3997:46295", "3997:46313"] {
            assert!(
                !result["projectionIssues"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|i| i["nodeId"] == id && i["fidelityImpact"] == "lossy"),
                "{result}"
            );
        }
    }

    #[tokio::test]
    async fn r7_artifact_reuses_stored_selection() {
        let data = section(2);
        let store = ArtifactStore::default();
        let mut request = CollectionRequest::new(data.target.clone(), CollectionScope::Node);
        request.section = Some(devup_mcp_figma::SectionReadOptions {
            frame_ids: vec!["1:2".into()],
            all_screens: false,
        });
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request), data)
            .await
            .unwrap();
        let result = complete_operation(
            operation(&["tsx"]),
            &artifact.payload,
            "artifact",
            &artifact,
            &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
            &store,
        )
        .await
        .unwrap();
        assert_ne!(result["status"], "selection_required");
        assert_eq!(result["frames"][0]["nodeId"], "1:2");
        assert_eq!(result["frames"].as_array().unwrap().len(), 1);
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["1:1".into()];
        }
        let explicit = complete_operation(
            op,
            &artifact.payload,
            "artifact",
            &artifact,
            &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
            &store,
        )
        .await
        .unwrap();
        assert_eq!(explicit["frames"][0]["nodeId"], "1:1");
    }

    #[tokio::test]
    async fn r7_auto_debug_resource_has_inline_reprojection_guidance() {
        let mut data = payload();
        data.metadata = json!({"large":"x".repeat(300_000)});
        let mut op = operation(&["tsx", "rawPayload", "sourceMap"]);
        if let PendingOperation::Export { delivery, .. } = &mut op {
            *delivery = DeliveryMode::Auto;
        }
        let result = project(data, op).await.unwrap();
        assert!(result["resources"].is_array());
        assert_eq!(
            result["nextAction"]["example"]["arguments"]["delivery"],
            "inline"
        );
        assert_eq!(result["nextAction"]["example"]["arguments"]["debug"], true);
        assert!(
            result["nextAction"]["how"]
                .as_str()
                .unwrap()
                .contains("resource")
        );
    }

    #[test]
    fn r8_nonsection_semantic_asset_mapping_uses_delivered_path() {
        let mut result = json!({"tsx":"<Image src=\"/icons/grommet-icons:language.svg\" />", "sourceMap":{"tsx":[{"nodeId":"한글:1","property":"fills","generatedProperty":"src=\"/icons/grommet-icons:language.svg\"","resolution":"asset"}]}});
        rewrite_result_asset_references(&mut result, &BTreeMap::new());
        assert_eq!(
            result["sourceMap"]["tsx"][0]["generatedProperty"],
            "src=\"/icons/grommet-icons-language-d201f62b16f5.svg\""
        );
    }

    #[tokio::test]
    async fn r8_three_frame_nine_output_units_measure_local_projection() {
        let data: CollectedPayload = serde_json::from_str(include_str!(
            "../../../../fixtures/r2/wquw-119-payload.json"
        ))
        .unwrap();
        let mut op = operation(&["tsx", "rawSnapshot", "sourceMap"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec![
                "3997:46315".into(),
                "3997:46715".into(),
                "3997:46333".into(),
            ];
        }
        let began = std::time::Instant::now();
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"].as_array().unwrap().len(), 3);
        for frame in result["frames"].as_array().unwrap() {
            assert!(frame["tsx"].is_string());
            assert!(frame["sourceMap"].is_object());
        }
        eprintln!(
            "R8 offline 3-frame/9-unit projection: {:?}; no upstream timing inferred",
            began.elapsed()
        );
    }

    #[tokio::test]
    async fn r8_final_korean_semantic_map_follows_asset_path_rewrites() {
        let mut data: CollectedPayload = serde_json::from_str(include_str!(
            "../../../../fixtures/r2/wquw-120-payload.json"
        ))
        .unwrap();
        data.snapshot =
            serde_json::from_str(include_str!("../../../../fixtures/r8/modal-snapshot.json"))
                .unwrap();
        let mut op = operation(&["tsx", "sourceMap"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["3997:46582".into()];
        }
        let started = std::time::Instant::now();
        let result = project(data, op).await.unwrap();
        eprintln!(
            "R8 modal projection: {:?}; executable: {:?}",
            started.elapsed(),
            std::env::current_exe().unwrap()
        );
        let frame = &result["frames"][0];
        let tsx = frame["tsx"].as_str().unwrap();
        assert!(tsx.contains("추가 체험"));
        let entries = frame["sourceMap"]["entries"].as_array().unwrap();
        for entry in entries {
            assert!(
                entry.get("generatedRange").is_none(),
                "public offsets must not return: {entry}"
            );
            assert!(entry["nodeId"].is_string() && entry["property"].is_string());
            let generated = entry["generatedProperty"]
                .as_str()
                .expect("mapped property required");
            assert!(!generated.is_empty());
            if !generated.starts_with("implicit:") && generated != "children" {
                assert!(
                    tsx.contains(generated),
                    "mapping must name emitted property: {entry}"
                );
            }
        }
        let modal_type = entries
            .iter()
            .find(|e| e["nodeId"] == "3997:46621" && e["property"] == "type")
            .unwrap();
        assert_eq!(modal_type["generatedProperty"], "Box");
        assert_eq!(modal_type["resolution"], "exact");
        assert_eq!(frame["sourceMap"]["source"]["generatedOutput"], "tsx");
        eprintln!(
            "R8 sourceMap={} bytes, TSX={} bytes",
            serde_json::to_vec(&frame["sourceMap"]).unwrap().len(),
            tsx.len()
        );
    }

    async fn project(
        payload: CollectedPayload,
        operation: PendingOperation,
    ) -> Result<Value, DevupError> {
        let store = ArtifactStore::default();
        let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
            payload.target.clone(),
            CollectionScope::Node,
        ));
        let artifact = store.insert(key, payload).await.unwrap();
        complete_operation(
            operation,
            &artifact.payload,
            "fixture",
            &artifact,
            &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
            &store,
        )
        .await
    }

    #[tokio::test]
    async fn w1_component_tsx_projects_selected_frames_without_section_root() {
        let mut op = operation(&["componentTsx"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["1:1".into()];
        }
        let result = project(section(2), op).await.unwrap();
        assert_eq!(result["frames"].as_array().unwrap().len(), 1);
        assert!(
            result["frames"][0]["componentTsx"]
                .as_str()
                .unwrap()
                .contains("export function")
        );
        assert_ne!(result["quality"]["projection"], "not-requested");
    }

    #[tokio::test]
    async fn w1_unavailable_responsive_output_is_actionable_even_with_tsx_and_strict() {
        for outputs in [vec!["responsiveTsx"], vec!["tsx", "responsiveTsx"]] {
            for strict_value in [false, true] {
                let mut op = operation(&outputs);
                if let PendingOperation::Export {
                    all_screens,
                    strict,
                    ..
                } = &mut op
                {
                    *all_screens = true;
                    *strict = strict_value;
                }
                let error = project(section(2), op).await.unwrap_err();
                assert_eq!(error.code, ErrorCode::DevupInvalidInput);
                assert!(error.message.contains("responsiveTsx"));
                assert!(error.message.contains("tsx"));
                assert!(error.details.is_object());
            }
        }
    }

    #[tokio::test]
    async fn w1_asset_failure_is_partial_and_strict_refuses_it() {
        let mut data = payload();
        data.assets.push(serde_json::from_value(json!({"assetId":"1:1:fills:0","nodeId":"1:1",
            "field":"fills/0","sourceKind":"image","status":"failed","errorCode":"DEVUP_ASSET_EXPORT_FAILED"})).unwrap());
        let result = project(data.clone(), operation(&["assetManifest"]))
            .await
            .unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["failures"][0]["assetId"], "1:1:fills:0");
        assert_ne!(result["quality"]["assets"], "complete");
        let mut strict_op = operation(&["assetManifest"]);
        if let PendingOperation::Export { strict, .. } = &mut strict_op {
            *strict = true;
        }
        assert!(project(data, strict_op).await.is_err());
    }

    #[tokio::test]
    async fn w1_frame_failure_preserves_other_frames_and_context() {
        let mut data = section(2);
        data.snapshot.nodes.remove("1:2");
        data.snapshot.roots.retain(|id| id != "1:2");
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["status"], "partial");
        assert!(result["frames"][0]["tsx"].is_string());
        assert_eq!(result["failures"][0]["nodeId"], "1:2");
        assert_eq!(result["frames"][1]["quality"]["projection"], "failed");
        assert_eq!(
            result["failures"][0]["details"]["snapshotRootIds"],
            json!(["1:1"])
        );
    }

    #[tokio::test]
    async fn w1_all_screens_uses_explore_classification() {
        let mut data = section(3);
        data.metadata["sectionIndex"]["candidates"][1]["bounds"] =
            json!({"x":0,"y":0,"width":900,"height":100});
        data.metadata["sectionIndex"]["candidates"][2]["bounds"] =
            json!({"x":0,"y":0,"width":100,"height":100});
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"].as_array().unwrap().len(), 1);
        assert_eq!(result["frames"][0]["nodeId"], "1:1");
    }

    #[tokio::test]
    async fn w1_large_section_has_actionable_batch_refusal() {
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let error = project(section(14), op).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.details["recommendedFrameIds"].is_array());
    }

    #[tokio::test]
    async fn w1_resource_delivery_moves_all_code_and_updates_note() {
        let store = ArtifactStore::default();
        let data = payload();
        let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
            data.target.clone(),
            CollectionScope::Node,
        ));
        let artifact = store.insert(key, data).await.unwrap();
        let mut result = json!({"tsx":"screen", "responsiveTsx":"responsive", "componentTsx":"component",
            "frames":[{"componentTsx":"frame-component"}],
            "deliverable":{"note":"Implement from this value."}});
        let outputs = projected_outputs_from_result(result.as_object().unwrap()).unwrap();
        let attachment = apply_delivery(
            &mut result,
            DeliveryMode::Resource,
            &store,
            &artifact,
            outputs,
        )
        .await
        .unwrap();
        commit_delivery(attachment);
        assert!(result.get("componentTsx").is_none());
        assert!(result.get("responsiveTsx").is_none());
        assert!(result["frames"][0].get("componentTsx").is_none());
        assert_eq!(result["resources"].as_array().unwrap().len(), 4);
        assert!(
            result["deliverable"]["note"]
                .as_str()
                .unwrap()
                .contains("resources")
        );
    }

    #[test]
    fn w1_asset_paths_are_ascii_bounded_and_windows_safe() {
        assert_eq!(
            host_safe_asset_path("/images/Frame 269.png"),
            "/images/Frame-269-3e2af8571e3f.png"
        );
        for name in [
            "한글.svg".to_owned(),
            "CON.svg".to_owned(),
            format!("{}.png", "a".repeat(300)),
        ] {
            let path = host_safe_asset_path(&format!("/icons/{name}"));
            assert!(path.is_ascii());
            assert!(path.len() < 200);
            assert_ne!(path.to_uppercase(), "/ICONS/CON.SVG");
            assert_eq!(host_safe_asset_path(&path), path);
        }
        assert_ne!(
            host_safe_asset_path("/icons/한글.svg"),
            host_safe_asset_path("/icons/한국.svg")
        );
    }

    #[test]
    fn w1_fixture_asset_paths_are_normalized_without_changing_other_code() {
        let code = include_str!("../../../../fixtures/plugin-answers/notice/pure.tsx");
        let safe = with_host_safe_asset_paths(code);
        assert!(!safe.contains("/icons/Property 1="));
        assert_eq!(with_host_safe_asset_paths(&safe), safe);
    }
    #[tokio::test]
    async fn w1_index_accepts_component_projection_selection() {
        let store = ArtifactStore::default();
        let data = section(2);
        let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
            data.target.clone(),
            CollectionScope::Node,
        ));
        let artifact = store.insert(key, data).await.unwrap();
        for output in ["componentTsx", "responsiveTsx"] {
            super::super::validation::validate_artifact_projection(
                &artifact,
                &[output.into()],
                "node",
                &[],
            )
            .unwrap();
        }
    }

    #[tokio::test]
    async fn w1_hidden_asset_is_warning_and_requested_failure_is_partial() {
        let mut data = payload();
        data.assets.push(
            serde_json::from_value(json!({"assetId":"1:1:fills:0","nodeId":"1:1",
            "field":"fills/0","sourceKind":"image","status":"failed","format":"png","scale":1,
            "errorCode":"DEVUP_ASSET_NODE_HIDDEN"}))
            .unwrap(),
        );
        let mut op = operation(&["assetManifest"]);
        if let PendingOperation::Export { asset_captures, .. } = &mut op {
            asset_captures.push(devup_mcp_figma::AssetSelection {
                asset_id: "1:1:fills:0".into(),
                format: devup_mcp_figma::AssetFormat::Png,
                scale: 1,
            });
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["failures"], json!([]));
        assert_eq!(result["warnings"][0]["assetId"], "1:1:fills:0");
        assert_eq!(result["status"], "complete");
    }

    #[tokio::test]
    async fn w1_manifest_preserves_layer_name_and_section_paths_match() {
        let mut data = section(1);
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("name".into(), json!("BI - 아이콘"));
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["2:1"]));
        data.snapshot.nodes.insert(
            "2:1".into(),
            serde_json::from_value(json!({"id":"2:1","type":"VECTOR",
            "fields":{"name":"BI - 아이콘", "parentId":"1:1","width":24,"height":24,"visible":true,
                "fills":[{"type":"SOLID","color":{"r":0,"g":0,"b":0}}]}}))
            .unwrap(),
        );
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        let asset = &result["assetManifest"]["assets"][0];
        assert_eq!(asset["layerName"], "BI - 아이콘");
        let path = asset["path"].as_str().unwrap();
        assert!(path.is_ascii());
        assert!(!path.contains(' '));
        assert!(result["frames"][0]["tsx"].as_str().unwrap().contains(path));
    }

    #[tokio::test]
    async fn w1_missing_master_is_partial_with_component_id() {
        let mut data = section(1);
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["2:1"]));
        data.snapshot.nodes.insert(
            "2:1".into(),
            serde_json::from_value(json!({"id":"2:1","type":"INSTANCE",
            "fields":{"name":"Button","parentId":"1:1","componentId":"9:9","visible":true}}))
            .unwrap(),
        );
        let mut op = operation(&["tsx", "componentTsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["status"], "partial");
        assert!(result["frames"][0]["tsx"].is_string());
        assert_eq!(result["failures"][0]["details"]["componentId"], "9:9");
        assert_eq!(result["failures"][0]["details"]["instanceNodeId"], "2:1");
    }

    #[tokio::test]
    async fn w1_source_map_without_generator_is_actionable() {
        let error = project(payload(), operation(&["sourceMap"]))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.message.contains("tsx"));
    }

    #[tokio::test]
    async fn r6_source_map_error_explains_artifact_outputs() {
        let error = project(payload(), operation(&["rawSnapshot", "sourceMap"]))
            .await
            .unwrap_err();
        assert!(
            error.message.contains("artifactId") && error.message.contains("same call"),
            "{}",
            error.message
        );
        assert!(error.message.contains(r#"{"artifactId":"<artifactId>","outputs":["tsx","rawSnapshot","sourceMap"],"debug":true}"#));
    }

    #[tokio::test]
    async fn r6_completion_vertical_loss_is_scoped_to_tsx() {
        let mut data = payload();
        data.snapshot = serde_json::from_str(include_str!(
            "../../../devup-mcp-devup-ui/tests/fixtures/r6-completion.json"
        ))
        .unwrap();
        data.target.node_id = Some("3997:46333".into());
        let mut op = operation(&["tsx", "sourceMap"]);
        if let PendingOperation::Export { root_layout, .. } = &mut op {
            *root_layout = devup_mcp_devup_ui::codegen::RootLayout::Embedded;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["deliverable"]["isFinal"], false);
        assert!(
            result["projectionIssues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["nodeId"] == "3997:46333"
                    && d["property"] == "layoutSizingVertical"
                    && d["fidelityImpact"] == "lossy"
                    && d["details"]["output"] == "tsx")
        );
    }

    fn responsive_payload() -> CollectedPayload {
        let mut data = section(2);
        for (id, name, width) in [("1:1", "mobile", 390), ("1:2", "desktop", 1440)] {
            let node = data.snapshot.nodes.get_mut(id).unwrap();
            node.fields.insert("name".into(), json!(name));
            node.fields.insert("width".into(), json!(width));
        }
        data
    }

    #[tokio::test]
    async fn w1_responsive_does_not_merge_unselected_frames() {
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["1:1".into()];
        }
        let result = project(responsive_payload(), op).await;
        assert!(
            result.is_err(),
            "one selected width cannot be a responsive merge"
        );
    }

    #[tokio::test]
    async fn w1_responsive_success_has_code_resource_and_no_empty_frames() {
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export {
            all_screens,
            delivery,
            ..
        } = &mut op
        {
            *all_screens = true;
            *delivery = DeliveryMode::Resource;
        }
        let result = project(responsive_payload(), op).await.unwrap();
        assert_ne!(result["quality"]["projection"], "not-requested");
        assert!(
            result.get("frames").is_none(),
            "no per-frame outputs were requested"
        );
        assert!(
            result["resources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["name"] == "responsiveTsx")
        );
    }

    #[tokio::test]
    async fn w1_empty_all_screens_is_actionable() {
        let mut data = section(1);
        data.metadata["sectionIndex"]["candidates"][0]["bounds"] =
            json!({"x":0,"y":0,"width":100,"height":100});
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let error = project(data, op).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.message.contains("frameIds"));
    }

    #[tokio::test]
    async fn w1_partial_frames_provide_retry_arguments() {
        let mut data = section(2);
        data.snapshot.nodes.remove("1:2");
        data.snapshot.roots.retain(|id| id != "1:2");
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["resume"]["arguments"]["frameIds"], json!(["1:2"]));
        assert_eq!(result["resume"]["arguments"]["outputs"], json!(["tsx"]));
        assert!(result["resume"]["arguments"]["url"].is_string());
    }

    #[tokio::test]
    async fn w1_discovered_hidden_assets_are_warnings_not_failed_deliverables() {
        let mut data = payload();
        let node = data.snapshot.nodes.get_mut("1:1").unwrap();
        node.node_type = "VECTOR".into();
        node.fields.insert("visible".into(), json!(false));
        let result = project(data, operation(&["assetManifest"])).await.unwrap();
        assert_eq!(result["status"], "complete");
        assert_eq!(result["assetManifest"]["assets"], json!([]));
        assert_eq!(
            result["warnings"][0]["errorCode"],
            "DEVUP_ASSET_NODE_HIDDEN"
        );
    }

    #[tokio::test]
    async fn w1_index_unavailable_output_explains_collection_step() {
        let store = ArtifactStore::default();
        let data = section(2);
        let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
            data.target.clone(),
            CollectionScope::Node,
        ));
        let artifact = store.insert(key, data).await.unwrap();
        let error = super::super::validation::validate_artifact_projection(
            &artifact,
            &["assetManifest".into()],
            "node",
            &[],
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.message.contains("frameIds"));
    }
    #[test]
    fn w1_slash_in_layer_name_matches_manifest_and_code() {
        let original = "/icons/한글 Foo/Bar-1-2.svg";
        let path = host_safe_asset_path(original);
        let code = with_host_safe_asset_paths(&format!("<Box maskImage=\"url('{original}')\" />"));
        assert!(path.is_ascii());
        assert!(!path.contains(' '));
        assert_eq!(path.matches('/').count(), 2);
        assert!(code.contains(&path));
    }

    #[tokio::test]
    async fn w1_captured_main_component_id_is_reported_when_missing() {
        let mut data = section(1);
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["2:1"]));
        data.snapshot.nodes.insert(
            "2:1".into(),
            serde_json::from_value(json!({"id":"2:1","type":"INSTANCE",
            "fields":{"name":"Button","parentId":"1:1","mainComponentId":"9:9","visible":true}}))
            .unwrap(),
        );
        let mut op = operation(&["componentTsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["failures"][0]["details"]["componentId"], "9:9");
    }

    #[tokio::test]
    async fn w1_responsive_text_loss_is_partial_and_strict_rejects() {
        let mut data = responsive_payload();
        for (root, id, text) in [
            ("1:1", "2:1", "Mobile only"),
            ("1:2", "2:2", "Desktop only"),
        ] {
            data.snapshot
                .nodes
                .get_mut(root)
                .unwrap()
                .fields
                .insert("childrenIds".into(), json!([id]));
            data.snapshot.nodes.insert(id.into(), serde_json::from_value(json!({"id":id,"type":"TEXT",
                "fields":{"name":"Label","parentId":root,"characters":text,"visible":true,"fontSize":16}})).unwrap());
        }
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data.clone(), op.clone()).await.unwrap();
        assert!(
            result["responsiveUnrepresented"]
                .as_array()
                .is_some_and(|notes| !notes.is_empty())
        );
        assert_eq!(result["status"], "partial");
        assert_eq!(result["quality"]["projection"], "lossy");
        if let PendingOperation::Export { strict, .. } = &mut op {
            *strict = true;
        }
        assert!(project(data, op).await.is_err());
    }

    #[tokio::test]
    async fn w1_responsive_slot_collision_is_actionable() {
        let mut data = responsive_payload();
        data.snapshot
            .nodes
            .get_mut("1:2")
            .unwrap()
            .fields
            .insert("width".into(), json!(400));
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let error = project(data, op).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.message.contains("breakpoint"));
    }

    #[tokio::test]
    async fn w1_missing_collected_outputs_explain_how_to_recollect() {
        for output in ["devupJson", "referencePng"] {
            let error = project(payload(), operation(&[output])).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::DevupInvalidInput);
            assert!(error.message.contains(output));
            assert!(error.details["snapshotRootIds"].is_array());
        }
    }
    #[tokio::test]
    async fn w1_optional_responsive_collision_keeps_requested_tsx() {
        let mut data = responsive_payload();
        data.snapshot
            .nodes
            .get_mut("1:2")
            .unwrap()
            .fields
            .insert("width".into(), json!(400));
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"].as_array().unwrap().len(), 2);
        assert!(result["frames"][0]["tsx"].is_string());
        assert!(result.get("responsiveTsx").is_none());
    }

    #[test]
    fn w1_ascii_slugs_do_not_merge_distinct_variant_assets() {
        assert_ne!(
            host_safe_asset_path("/icons/Icons=a b.svg"),
            host_safe_asset_path("/icons/Icons=a-b.svg")
        );
        assert_ne!(
            host_safe_asset_path("/icons/Foo Bar.svg"),
            host_safe_asset_path("/icons/Foo-Bar.svg")
        );
    }
}
