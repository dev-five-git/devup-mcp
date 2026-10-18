

pub fn asset_export_from_result(
    result: &UpstreamResult,
    file_key: &str,
    version: Option<&str>,
    request: &AssetRequest,
) -> Result<AssetExportOutcome, DevupError> {
    crate::validate_webp_options(request.format, request.quality, request.lossless)?;
    let descriptor = find_descriptor(&result.raw)
        .ok_or_else(|| invalid("asset descriptor not found in the Figma MCP response."))?;
    if descriptor.file_key != file_key
        || descriptor.version.as_deref() != version
        || descriptor.asset_id != request.asset_id
        || descriptor.node_id != request.node_id
        || descriptor.field != request.field
        || descriptor.image_hash != request.image_hash
        || descriptor.format != request.format
        || descriptor.scale != request.scale
        || descriptor.render_mode != request.render_mode
        || descriptor.max_width != request.max_width
        || descriptor.usage_aspect_ratio != request.usage_aspect_ratio
        || descriptor.srcset != request.srcset
        || descriptor.visible_scope_node_id != request.visible_scope_node_id
    {
        return Err(invalid(
            "asset descriptor target or version does not match the request.",
        ));
    }
    if descriptor.status == AssetStatus::Exported {
        let info = descriptor
            .export_diagnostics
            .as_ref()
            .and_then(|d| d.render_info.as_ref());
        if request.render_mode == AssetRenderMode::OriginalSource
            && (info.is_none_or(|i| i["representation"] != "original-fill")
                || descriptor
                    .export_diagnostics
                    .as_ref()
                    .is_none_or(|d| d.method != "original-fill"))
        {
            return Err(invalid(
                "Original-source export lacks full-source acquisition provenance; native renditions cannot substitute.",
            ));
        }
        if request.render_mode == AssetRenderMode::Visible
            && info.is_none_or(|i| {
                i["representation"] != "native-visible"
                    || i["captureNodeId"].as_str() != request.visible_scope_node_id.as_deref()
                    || i["contextScope"] != "selected-ancestor-subtree"
            })
        {
            return Err(invalid(
                "Visible capture lacks bound selected-ancestor scope provenance.",
            ));
        }
    }
    if matches!(
        request.render_mode,
        AssetRenderMode::Node | AssetRenderMode::Visible
    ) && descriptor
        .export_diagnostics
        .as_ref()
        .and_then(|d| d.render_info.as_ref())
        .is_some_and(|i| {
            i["representation"]
                .as_str()
                .is_some_and(|r| r.starts_with("original-fill"))
        })
    {
        return Err(invalid(
            "Native capture cannot substitute original fill bytes; explicit renderMode fill or original-source is required.",
        ));
    }
    if request.format == AssetFormat::Webp
        && !request.render_mode.is_original()
        && descriptor
            .export_diagnostics
            .as_ref()
            .and_then(|d| d.render_info.as_ref())
            .is_some_and(|info| {
                info["representation"]
                    .as_str()
                    .is_some_and(|r| r.starts_with("original-fill"))
            })
    {
        return Err(invalid(
            "Node/auto WebP requires a native PNG rendition. Original-fill WebP conversion requires explicit renderMode fill at scale 1; no native provenance was assigned.",
        ));
    }
    if (request.render_mode.is_original()
        || descriptor
            .export_diagnostics
            .as_ref()
            .and_then(|d| d.render_info.as_ref())
            .is_some_and(|i| i["representation"] == "original-fill"))
        && descriptor.status == AssetStatus::Chunked
    {
        return Err(invalid(
            "original fill bytes cannot use node re-export fragments.",
        ));
    }
    let source_kind = source_kind_of(request);
    if descriptor.status == AssetStatus::Chunked {
        let (Some(byte_length), Some(sha256), Some(cursor)) = (
            descriptor.byte_length,
            descriptor.sha256.clone(),
            descriptor.cursor.clone(),
        ) else {
            return Err(invalid(
                "chunked asset export descriptor is missing its length, hash or cursor.",
            ));
        };
        // The field names what the fragments are cut from. A PNG carries its
        // scale on the field, so the re-export behind each fragment is the
        // same bytes that were announced.
        let field = match request.format {
            AssetFormat::Svg => SVG_EXPORT_FIELD.to_owned(),
            AssetFormat::Png | AssetFormat::Webp => {
                let mut field = format!("{PNG_EXPORT_FIELD}@{}", request.scale);
                if request.render_mode == AssetRenderMode::Visible {
                    field.push_str(&format!(
                        "@visible@{}",
                        request
                            .visible_scope_node_id
                            .as_deref()
                            .ok_or_else(|| invalid("Visible capture scope is missing."))?
                    ));
                }
                field
            }
            _ => {
                return Err(invalid(
                    "only an SVG or PNG export is carried in fragments.",
                ));
            }
        };
        let descriptor = LargeValueDescriptor {
            node_id: request.node_id.clone(),
            field,
            byte_length,
            sha256,
            cursor,
        };
        return Ok(AssetExportOutcome::Chunked(descriptor));
    }
    if descriptor.status == AssetStatus::Failed {
        return Ok(AssetExportOutcome::Entry(consistent_entry(
            AssetManifestEntry {
                usage_aspect_ratio: request.usage_aspect_ratio,
                max_width: request.max_width,
                srcset: request.srcset,
                visible_scope_node_id: request.visible_scope_node_id.clone(),
                variants: Vec::new(),
                render_mode: request.render_mode,
                quality: request.quality,
                lossless: request.lossless,
                render_info: Some(crate::asset_render_info::measured_info(
                    request.render_mode,
                    request.format,
                    request.scale,
                    descriptor
                        .export_diagnostics
                        .as_ref()
                        .and_then(|d| d.render_info.clone()),
                    None,
                )),
                suggested_filename: None,
                asset_id: request.asset_id.clone(),
                node_id: request.node_id.clone(),
                field: request.field.clone(),
                source_kind: source_kind.to_owned(),
                image_hash: request.image_hash.clone(),
                format: Some(request.format),
                scale: Some(request.scale),
                status: AssetStatus::Failed,
                byte_length: None,
                sha256: None,
                mime_type: None,
                data_base64: None,
                output_path: None,
                error_code: descriptor.error_code,
                error_detail: bounded(descriptor.error_detail),
                export_diagnostics: descriptor.export_diagnostics,
                path: None,
            },
        )));
    }
    let payload = if request.format == AssetFormat::Webp {
        find_payload(&result.raw, "image/png").or_else(|| {
            (request.render_mode.is_original())
                .then(|| {
                    ["image/jpeg", "image/gif", "image/webp"]
                        .into_iter()
                        .find_map(|mime| find_payload(&result.raw, mime))
                })
                .flatten()
        })
    } else if request.render_mode == AssetRenderMode::OriginalSource {
        ["image/png", "image/jpeg", "image/webp", "image/gif"]
            .into_iter()
            .find_map(|mime| find_payload(&result.raw, mime))
    } else {
        find_payload(&result.raw, request.format.mime_type())
    }
    .ok_or_else(|| {
        // Which shapes the response *did* carry. Without this the failure is
        // indistinguishable between "no attachment came back", "it came back
        // under a different mime type" and "it came back in a field this
        // search does not read" — three very different bugs.
        DevupError::with_details(
            ErrorCode::DevupSnapshotUnsupported,
            format!(
                include_str!("../text/assets-asset_export_from_result-a4ad1a7b.md"),
                request.format.extension()
            ),
            false,
            json!({
                "expectedMimeType": request.format.mime_type(),
                "observed": observed_payload_shapes(&result.raw),
            }),
        )
    })?;
    let (bytes, data) = match payload {
        AssetPayload::Base64(data) => {
            if data.len() > MAX_ASSET_BYTES.div_ceil(3) * 4 {
                return Err(invalid("asset export base64 exceeds the byte budget."));
            }
            let bytes = STANDARD
                .decode(data.as_bytes())
                .map_err(|_| invalid("asset export binary base64 is invalid."))?;
            (bytes, data)
        }
        // Re-encoded so every consumer downstream still receives base64,
        // regardless of how the upstream happened to carry the payload.
        AssetPayload::Text(text) => {
            let bytes = text.into_bytes();
            let data = STANDARD.encode(&bytes);
            (bytes, data)
        }
    };
    if bytes.is_empty()
        || bytes.len() > MAX_ASSET_BYTES
        || descriptor.byte_length != Some(bytes.len())
        || descriptor.sha256.as_deref() != Some(sha256_hex(&bytes).as_str())
    {
        return Err(invalid(
            "asset export binary length or hash does not match.",
        ));
    }
    if request.format != AssetFormat::Webp
        && (request.render_mode == AssetRenderMode::Fill
            || (request.render_mode == AssetRenderMode::Auto
                && descriptor
                    .export_diagnostics
                    .as_ref()
                    .and_then(|d| d.render_info.as_ref())
                    .is_some_and(|i| i["representation"] == "original-fill")))
        && image::guess_format(&bytes).ok()
            != Some(match request.format {
                AssetFormat::Png => image::ImageFormat::Png,
                AssetFormat::Jpg => image::ImageFormat::Jpeg,
                _ => return Err(invalid("original fill format is unsupported.")),
            })
    {
        return Err(invalid(
            "original fill bytes do not match the requested codec.",
        ));
    }
    if request.render_mode == AssetRenderMode::Auto
        && let Some(info) = descriptor
            .export_diagnostics
            .as_ref()
            .and_then(|d| d.render_info.as_ref())
        && info["representation"] == "original-fill"
    {
        let measured = crate::asset_render_info::measured_info(
            request.render_mode,
            request.format,
            request.scale,
            Some(info.clone()),
            Some(&bytes),
        );
        if request.scale != 1.0
            || info["autoDecision"]["eligible"] != true
            || info["autoDecision"]["selected"] != "fill"
            || !info["autoDecision"]["reasons"]
                .as_array()
                .is_some_and(Vec::is_empty)
            || measured["outputSize"].is_null()
            || measured["outputSize"]["width"].as_f64() != measured["nodeSize"]["width"].as_f64()
            || measured["outputSize"]["height"].as_f64() != measured["nodeSize"]["height"].as_f64()
        {
            return Err(invalid(
                "auto raw fill lacks validated safety or intrinsic dimension evidence.",
            ));
        }
    }
    Ok(AssetExportOutcome::Entry(consistent_entry(
        crate::webp::finish(
            request,
            AssetManifestEntry {
                usage_aspect_ratio: request.usage_aspect_ratio,
                max_width: request.max_width,
                srcset: request.srcset,
                visible_scope_node_id: request.visible_scope_node_id.clone(),
                variants: Vec::new(),
                render_mode: request.render_mode,
                quality: None,
                lossless: None,
                render_info: Some(crate::asset_render_info::measured_info(
                    request.render_mode,
                    request.format,
                    request.scale,
                    descriptor
                        .export_diagnostics
                        .as_ref()
                        .and_then(|d| d.render_info.clone()),
                    Some(&bytes),
                )),
                suggested_filename: None,
                asset_id: request.asset_id.clone(),
                node_id: request.node_id.clone(),
                field: request.field.clone(),
                source_kind: source_kind.to_owned(),
                image_hash: request.image_hash.clone(),
                format: Some(request.format),
                scale: Some(request.scale),
                status: AssetStatus::Exported,
                byte_length: Some(bytes.len()),
                sha256: descriptor.sha256,
                mime_type: Some(request.format.mime_type().to_owned()),
                data_base64: Some(data),
                output_path: None,
                error_code: None,
                error_detail: None,
                export_diagnostics: descriptor.export_diagnostics,
                path: None,
            },
        )?,
    )))
}

/// The virtual field a fragmented SVG export is read under: the large-value
/// script re-exports the node as SVG text and slices that, where for any
/// other field it slices the field's JSON.
/// What an export is cut from, from what the request already says: an
/// image fill, a node that is a picture, or a node that is line work. The
/// entry an export produces replaces the one discovery listed, so the two
/// have to agree; deriving both from the same two fields is how they do.
pub fn source_kind_of(request: &AssetRequest) -> &'static str {
    if request.field.starts_with("fills/") {
        "image-fill"
    } else if request.image_hash.is_some() {
        "image-node"
    } else {
        "vector-node"
    }
}

fn consistent_entry(mut entry: AssetManifestEntry) -> Box<AssetManifestEntry> {
    if entry.status == AssetStatus::Failed
        && let Some(info) = &mut entry.render_info
    {
        info["representation"] = json!("not-exported");
        info["renderedAs"] = json!("not-exported");
        info["fidelity"] = json!("unverified");
        info["effectsBakedIntoAsset"] = json!(false);
    }
    if let Some(info) = &mut entry.render_info {
        info["sourceKind"] = json!(entry.source_kind);
        info["usageAspectRatio"] = json!(entry.usage_aspect_ratio);
    }
    if let Some(diagnostics) = &mut entry.export_diagnostics {
        diagnostics.render_info = entry.render_info.clone();
    }
    Box::new(entry)
}

pub const SVG_EXPORT_FIELD: &str = "$export:svg";

/// The virtual field a PNG too large for one attachment is read back
/// through, with its scale appended: `$export:png@2`. Figma's remote MCP
/// returns a written PNG as an attachment only up to about a megabyte once
/// base64-encoded; a larger one is written, reported exported, and never
/// arrives, so it is carried in fragments instead.
pub const PNG_EXPORT_FIELD: &str = "$export:png";

/// What an asset export call answered with: the asset, exported or failed,
/// or the announcement of an SVG too large for one answer, to be read back
/// in fragments.
#[derive(Debug, Clone, PartialEq)]
pub enum AssetExportOutcome {
    Entry(Box<AssetManifestEntry>),
    Chunked(LargeValueDescriptor),
}

/// The entry for an export that arrived in fragments, from the bytes the
/// fragments assembled to. The length and hash were checked against the
/// announcement by the assembler.
pub fn exported_asset_from_bytes(
    request: &AssetRequest,
    bytes: &[u8],
) -> Result<AssetManifestEntry, DevupError> {
    if request.render_mode == AssetRenderMode::OriginalSource {
        return Err(invalid(
            "Original-source requires a full-source acquisition descriptor; node fragments cannot substitute.",
        ));
    }
    if bytes.is_empty() || bytes.len() > MAX_ASSET_BYTES {
        return Err(invalid("asset export binary length is out of range."));
    }
    crate::webp::finish(
        request,
        AssetManifestEntry {
            usage_aspect_ratio: request.usage_aspect_ratio,
            max_width: request.max_width,
            srcset: request.srcset,
            visible_scope_node_id: request.visible_scope_node_id.clone(),
            variants: Vec::new(),
            render_mode: request.render_mode,
            quality: None,
            lossless: None,
            render_info: Some(crate::asset_render_info::measured_info(
                request.render_mode,
                request.format,
                request.scale,
                None,
                Some(bytes),
            )),
            suggested_filename: None,
            asset_id: request.asset_id.clone(),
            node_id: request.node_id.clone(),
            field: request.field.clone(),
            source_kind: source_kind_of(request).to_owned(),
            image_hash: request.image_hash.clone(),
            format: Some(request.format),
            scale: Some(request.scale),
            status: AssetStatus::Exported,
            byte_length: Some(bytes.len()),
            sha256: Some(sha256_hex(bytes)),
            mime_type: Some(request.format.mime_type().to_owned()),
            data_base64: Some(STANDARD.encode(bytes)),
            output_path: None,
            error_code: None,
            error_detail: None,
            export_diagnostics: None,
            path: None,
        },
    )
}

fn legacy_descriptor_mode() -> AssetRenderMode {
    AssetRenderMode::Node
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetExportDescriptor {
    #[serde(default = "legacy_descriptor_mode")]
    render_mode: AssetRenderMode,
    #[serde(default)]
    max_width: Option<u32>,
    #[serde(default)]
    usage_aspect_ratio: Option<f64>,
    #[serde(default)]
    srcset: bool,
    #[serde(default)]
    visible_scope_node_id: Option<String>,
    kind: String,
    file_key: String,
    version: Option<String>,
    asset_id: String,
    node_id: String,
    field: String,
    image_hash: Option<String>,
    format: AssetFormat,
    scale: f64,
    status: AssetStatus,
    byte_length: Option<usize>,
    sha256: Option<String>,
    error_code: Option<String>,
    #[serde(default)]
    error_detail: Option<String>,
    #[serde(default)]
    export_diagnostics: Option<AssetExportDiagnostics>,
    #[serde(default)]
    cursor: Option<LargeValueCursor>,
}

/// The exporter's own words, kept short: it runs in the plugin, so its length
/// is not ours to trust.
fn bounded(text: Option<String>) -> Option<String> {
    text.map(|text| text.chars().take(500).collect())
}

fn find_descriptor(value: &Value) -> Option<AssetExportDescriptor> {
    if let Ok(mut descriptor) = serde_json::from_value::<AssetExportDescriptor>(value.clone())
        && descriptor.kind == "devupAssetExport"
    {
        if let Some(diagnostics) = &mut descriptor.export_diagnostics {
            if diagnostics
                .render_info
                .as_ref()
                .is_some_and(|v| v.to_string().len() > 16 * 1024)
            {
                diagnostics.render_info = Some(
                    json!({"representation":"unknown", "warnings":["render-observations-exceeded-limit"]}),
                );
            }
            diagnostics.native_error = bounded(diagnostics.native_error.take());
            diagnostics.method = diagnostics.method.chars().take(80).collect();
            diagnostics.stages.truncate(16);
            for stage in &mut diagnostics.stages {
                stage.stage = stage.stage.chars().take(80).collect();
            }
        }
        return Some(descriptor);
    }
    match value {
        Value::Object(object) => object.values().find_map(find_descriptor),
        Value::Array(values) => values.iter().find_map(find_descriptor),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| find_descriptor(&value)),
        _ => None,
    }
}

pub(crate) fn asset_export_diagnostics(result: &UpstreamResult) -> Option<AssetExportDiagnostics> {
    find_descriptor(&result.raw)?.export_diagnostics
}

/// How an upstream carried the exported asset.
enum AssetPayload {
    /// An image content block or a blob resource, which are base64.
    Base64(String),
    /// A text resource. MCP models a text-based document — SVG being the one
    /// devup-mcp exports — as `text` holding the document itself rather than
    /// base64 of it, so an SVG export used to be invisible to a search that
    /// only looked for `data`/`blob` and every request failed with "asset
    /// export response does not contain the requested binary".
    Text(String),
}

fn find_payload(value: &Value, mime_type: &str) -> Option<AssetPayload> {
    match value {
        Value::Object(object) => {
            if object.get("mimeType").and_then(Value::as_str) == Some(mime_type) {
                // Base64 first: when a payload offers both, the binary form is
                // the exact bytes, while `text` may be a lossy preview.
                if let Some(data) = object
                    .get("data")
                    .or_else(|| object.get("blob"))
                    .and_then(Value::as_str)
                {
                    return Some(AssetPayload::Base64(data.to_owned()));
                }
                if let Some(text) = object.get("text").and_then(Value::as_str) {
                    return Some(AssetPayload::Text(text.to_owned()));
                }
            }
            object
                .values()
                .find_map(|value| find_payload(value, mime_type))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_payload(value, mime_type)),
        // The descriptor — and, for SVG, the payload inlined beside it —
        // arrives as JSON inside a text content block, so the search has to
        // step through that encoding exactly as `find_descriptor` does.
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| find_payload(&value, mime_type)),
        _ => None,
    }
}

/// Describes every payload-carrying object in a response by its `type` and
/// `mimeType` and which of `data`/`blob`/`text` it holds, without ever
/// including the payload itself. Bounded so a large response cannot turn a
/// diagnostic into another problem.
fn observed_payload_shapes(value: &Value) -> Vec<String> {
    fn walk(value: &Value, found: &mut Vec<String>) {
        if found.len() >= 12 {
            return;
        }
        match value {
            Value::Object(object) => {
                let carriers: Vec<&str> = ["data", "blob", "text", "uri"]
                    .into_iter()
                    .filter(|key| object.contains_key(*key))
                    .collect();
                if !carriers.is_empty() {
                    let kind = object
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("<no type>");
                    let mime = object
                        .get("mimeType")
                        .and_then(Value::as_str)
                        .unwrap_or("<no mimeType>");
                    found.push(format!(
                        "type={kind} mimeType={mime} carries=[{}]",
                        carriers.join(",")
                    ));
                }
                for child in object.values() {
                    walk(child, found);
                }
            }
            Value::Array(values) => {
                for child in values {
                    walk(child, found);
                }
            }
            _ => {}
        }
    }

    let mut found = Vec::new();
    walk(value, &mut found);
    found
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn invalid(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupSnapshotUnsupported, message, false)
}
