use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Cursor,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ImageDecoder, ImageError, Limits, codecs::png::PngDecoder};
use rmcp::model::{CallToolResult, ContentBlock};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::large_values::{
    LargeValueResult, descriptors_in_chunk, large_value_from_result, replace_descriptor,
};
use crate::{
    AssetExportOutcome, AssetManifestEntry, AssetRequest, AssetSelection, AssetStatus, BatchBudget,
    BatchLimits, BuiltinScript, DevupError, ErrorCode, ExploreReadOptions, FigmaTarget,
    LargeValueAssembler, LargeValueReadOptions, RawNode, ReadToolCall, ResourceBatch,
    ResourceScope, ResourceStyleRef, SNAPSHOT_CURSOR_ID, SearchReadOptions, SectionIndex,
    SnapshotChunk, SnapshotCursor, SnapshotReadOptions, UnresolvedResource, UpstreamResult,
    UsedResourceRefs, asset_export_from_result, build_section_index, collect_used_resource_refs,
    decode_fast_multi_snapshot, decode_fast_snapshot, decode_fast_theme, exported_asset_from_bytes,
    merge_chunks,
    metadata::{MetadataResult, metadata_from_result_for_target},
    plan_batches, read_snapshot_cursor, resolve_asset_selections, snapshot_chunk_from_result,
    variables::{
        VariableBatchResult, VariableCatalog, batch_from_result, catalog_from_result,
        join_used_resources, library_refs, merge_used_resource_results, merge_variable_results,
    },
};

const LARGE_SUBTREE_THRESHOLD: usize = 200;

const MAX_PENDING_CALLS: usize = 4;
pub const MAX_COLLECTED_ASSET_BYTES: usize = 128 * 1024 * 1024;
// The byte budget below measures the *request* — the IDs — and the ceiling
// that matters is on the *response*: the official MCP cuts a text result at
// 20,480 UTF-8 bytes (measured; see `scripts/fast_snapshot.js`) and appends
// "// truncated to 20kb", which the batch decoder then cannot parse. A variable serialised with its valuesByMode and
// codeSyntax is five hundred to a thousand bytes on the way back, so the
// sixty-four this allowed could not fit; thirty-eight were measured filling
// the cap exactly. Twelve leaves the response near a third of it.
// Consumer relations can be huge. Compact, bounded fragments are expanded
// back to the exhaustive shape in Rust without dropping any relation.
const STYLE_CONSUMER_BATCH_SIZE: usize = 320;

const MAX_REFERENCE_PNG_BYTES: usize = 16 * 1024 * 1024;
const MAX_REFERENCE_PNG_BASE64_BYTES: usize = MAX_REFERENCE_PNG_BYTES.div_ceil(3) * 4;
pub(crate) const MAX_REFERENCE_PNG_DIMENSION: u32 = 8_192;
const MAX_REFERENCE_PNG_DECODED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectionScope {
    Node,
    Page,
    File,
}

#[derive(Debug, Clone)]
pub struct CollectionRequest {
    /// 이 수집을 실어 나를 전송이 한 번에 받을 수 있는 양.
    pub batch_budget: BatchBudget,
    pub target: FigmaTarget,
    pub scope: CollectionScope,
    pub resource_scope: ResourceScope,
    pub include_context: bool,
    pub metadata_only: bool,
    pub variables_only: bool,
    pub search: Option<SearchReadOptions>,
    pub explore: Option<ExploreReadOptions>,
    pub section: Option<SectionReadOptions>,
    pub cached_section_index: Option<SectionIndex>,
    pub asset_selections: Vec<AssetSelection>,
    pub web_ready_images: bool,
    pub reference_png: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionReadOptions {
    #[serde(default)]
    pub frame_ids: Vec<String>,
    #[serde(default)]
    pub all_screens: bool,
}

impl CollectionRequest {
    pub fn new(target: FigmaTarget, scope: CollectionScope) -> Self {
        Self {
            target,
            scope,
            // 자르는 전송을 전제로 한 값. 브리지로 수집할 때는 호출자가
            // `FigmaUpstream::batch_budget` 에서 받아 덮어쓴다.
            batch_budget: BatchBudget::default(),
            resource_scope: ResourceScope::None,
            include_context: false,
            metadata_only: false,
            variables_only: false,
            search: None,
            explore: None,
            section: None,
            cached_section_index: None,
            asset_selections: Vec::new(),
            web_ready_images: false,
            reference_png: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlannedCall {
    pub id: String,
    pub call: ReadToolCall,
    pub expected_file_key: String,
    pub expected_node_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CollectedParts {
    pub target: FigmaTarget,
    pub scope: CollectionScope,
    pub metadata: Value,
    pub snapshot_chunks: Vec<SnapshotChunk>,
    pub variables: Option<UpstreamResult>,
    pub styles: Option<UpstreamResult>,
    pub source_version: Option<String>,
    pub stats: CollectionStats,
    pub assets: Vec<AssetManifestEntry>,
    pub reference_png: Option<ReferencePng>,
    pub failures: Vec<ScreenFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenFailure {
    pub node_id: String,
    pub error_code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferencePng {
    pub mime_type: String,
    pub data_base64: String,
    pub byte_length: usize,
    pub sha256: String,
    /// The Devup Bridge plugin's word when its picture may hold layers
    /// `get_screenshot`'s leaves out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionStats {
    pub figma_tool_calls: usize,
    pub transport: String,
    pub fallback_used: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub node_count: usize,
    pub variable_count: usize,
    pub style_count: usize,
    pub raw_bytes: usize,
    pub wire_bytes: usize,
    pub envelope_chunks: usize,
    /// Set when the Devup Bridge plugin answered any of `figma_tool_calls`,
    /// naming the plugin; the rest went to Figma's metered MCP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge: Option<crate::BridgeServed>,
    /// Why reads the Devup Bridge plugin answered were read again on the
    /// metered path, one entry per read: `file-out-of-sight`,
    /// `reference-png-approximate`, `plugin-older-than-server`,
    /// `plugin-answer-too-large`, `value-from-direct-path`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub direct_fallbacks: Vec<String>,
}

impl Default for CollectionStats {
    fn default() -> Self {
        Self {
            figma_tool_calls: 0,
            // Text (optionally paginated) is the default, primary path now;
            // "legacy-cursor" only ever appears once a fast call actually
            // falls back (see `restart_legacy`).
            transport: "text".to_owned(),
            fallback_used: false,
            fallback_reason: None,
            node_count: 0,
            variable_count: 0,
            style_count: 0,
            raw_bytes: 0,
            wire_bytes: 0,
            envelope_chunks: 0,
            bridge: None,
            direct_fallbacks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum CollectorStep {
    Call(Box<PlannedCall>),
    AwaitingResults,
    Complete(Box<CollectedParts>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallKind {
    FastSnapshot,
    FastTheme,
    Metadata,
    PageCatalog,
    Snapshot,
    VariableCatalog,
    VariableBatch,
    UsedResourceBatch,
    Explore,
    ScopedSearch,
    SectionIndex,
    FastMultiRoot,
    LargeValue,
    Asset,
    Screenshot,
}

#[derive(Debug, Clone)]
struct PendingCall {
    planned: PlannedCall,
    kind: CallKind,
    order: usize,
}

#[derive(Debug, Clone)]
pub struct CollectorSession {
    request: CollectionRequest,
    queued: VecDeque<PendingCall>,
    pending: BTreeMap<String, PendingCall>,
    consumed: BTreeSet<String>,
    metadata: Option<Value>,
    metadata_nodes: BTreeMap<String, RawNode>,
    metadata_root_ids: Vec<String>,
    root_node_id: Option<String>,
    source_version: Option<String>,
    snapshot_chunks: BTreeMap<usize, SnapshotChunk>,
    variable_catalog: Option<VariableCatalog>,
    /// A file's own variables and styles, read from its catalog, while the
    /// library ones its nodes use are read.
    file_local_resources: Option<UpstreamResult>,
    used_resource_refs: Option<UsedResourceRefs>,
    variable_batches: BTreeMap<usize, VariableBatchResult>,
    variables: Option<UpstreamResult>,
    large_values: BTreeMap<(String, String), LargeValueAssembler>,
    /// An SVG or PNG export announced as too large for one answer, by the
    /// key its fragments arrive under, until the last of them has.
    chunked_assets:
        BTreeMap<(String, String), (AssetRequest, Option<crate::AssetExportDiagnostics>)>,
    /// The pages of a paginated theme so far, merged, until the last one.
    fast_theme_pages: Option<Value>,
    fast_theme_page_count: usize,
    asset_results: Vec<AssetManifestEntry>,
    retained_asset_bytes: usize,
    asset_byte_budget_exhausted: bool,
    assets_scheduled: bool,
    asset_plan_initialized: bool,
    asset_plan: VecDeque<AssetRequest>,
    reference_png: Option<ReferencePng>,
    reference_png_scheduled: bool,
    stats: CollectionStats,
    fast_attempted: bool,
    section_index: Option<SectionIndex>,
    section_selected_roots: Vec<String>,
    fast_multi_resources: Option<UpstreamResult>,
    fast_multi_has_large_values: bool,
    /// Resources merged across rounds of the paginated single-root fast
    /// snapshot (`accept_fast_snapshot`). Distinct from `fast_multi_resources`,
    /// which is scoped to Section multi-root batching; the two paths are
    /// mutually exclusive (`fast_path_eligible` requires `section.is_none()`).
    fast_snapshot_resources: Option<UpstreamResult>,
    fast_snapshot_has_large_values: bool,
    fast_snapshot_rounds: usize,
    section_fallback_roots: BTreeSet<String>,
    screen_failures: Vec<ScreenFailure>,
    next_id: usize,
    completed: bool,
}

include!("new.rs");
include!("asset_progress.rs");
include!("stats.rs");
include!("advance.rs");
include!("accept.rs");
include!("reject.rs");
include!("fast_path_eligible.rs");
include!("fast_theme_eligible.rs");
include!("accept_fast_theme.rs");
include!("accept_fast_snapshot.rs");
include!("restart_legacy.rs");
include!("finish_stats.rs");
include!("accept_page_catalog.rs");
include!("accept_projection.rs");
include!("accept_section_index.rs");
include!("prepare_cached_section_index.rs");
include!("accept_fast_multi_root.rs");
include!("fallback_multi_root_batch.rs");
include!("mark_section_legacy.rs");
include!("enqueue_section_legacy_root.rs");
include!("restore_section_visual_order.rs");
include!("record_snapshot_chunk.rs");
include!("accept_large_value.rs");
include!("record_large_value_unsupported.rs");
include!("accept_asset.rs");
include!("accept_reference_png.rs");
include!("retain_asset.rs");
include!("record_asset_failure.rs");
include!("record_asset_diagnostic.rs");
include!("accept_metadata.rs");
include!("record_metadata.rs");
include!("record_metadata_node.rs");
include!("accept_snapshot.rs");
include!("accept_variable_catalog.rs");
include!("enqueue_used_resource_batches.rs");
include!("record_unresolved_diagnostics.rs");
include!("enqueue_style_consumer_batches.rs");
include!("enqueue.rs");

/// The one image the screenshot answered with.
///
/// The official `get_screenshot` writes the PNG's URL and how to fetch it as
/// text blocks, and inlines the PNG itself as an image block beside them when
/// asked to (`enableBase64Response`). The text is not read; the image must be
/// exactly one, and a PNG. A response with no image, or two, or one hidden in
/// text or JSON, is refused as before.
fn take_single_png_data(value: Value) -> Result<String, DevupError> {
    let result = serde_json::from_value::<CallToolResult>(value)
        .map_err(|_| invalid_call("Figma screenshot response format is invalid."))?;
    if result.is_error == Some(true) {
        return Err(invalid_call(
            "Figma screenshot response must contain exactly one image/png content block.",
        ));
    }
    let mut images = result
        .content
        .into_iter()
        .filter_map(|content| match content {
            ContentBlock::Image(image) => Some(image),
            _ => None,
        });
    let (Some(image), None) = (images.next(), images.next()) else {
        return Err(invalid_call(
            "Figma screenshot response must contain exactly one image/png content block.",
        ));
    };
    if image.mime_type != "image/png" {
        return Err(invalid_call(
            "Figma screenshot response MIME type is not image/png.",
        ));
    }
    Ok(image.data)
}

fn validate_reference_png(bytes: &[u8]) -> Result<(), DevupError> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_REFERENCE_PNG_DIMENSION);
    limits.max_image_height = Some(MAX_REFERENCE_PNG_DIMENSION);
    limits.max_alloc = Some(MAX_REFERENCE_PNG_DECODED_BYTES as u64);
    let decoder =
        PngDecoder::with_limits(Cursor::new(bytes), limits).map_err(reference_png_decode_error)?;
    let (width, height) = decoder.dimensions();
    let decoded_bytes = usize::try_from(decoder.total_bytes()).map_err(|_| {
        DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "Figma reference PNG decoded size exceeds the allowed range.",
            false,
        )
    })?;
    if width == 0
        || height == 0
        || width > MAX_REFERENCE_PNG_DIMENSION
        || height > MAX_REFERENCE_PNG_DIMENSION
        || decoded_bytes > MAX_REFERENCE_PNG_DECODED_BYTES
    {
        return Err(DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "Figma reference PNG dimensions or decoded size exceed the allowed range.",
            false,
        ));
    }
    let mut pixels = vec![0_u8; decoded_bytes];
    decoder
        .read_image(&mut pixels)
        .map_err(reference_png_decode_error)
}

/// The plugin's export flattened onto the colour the plugin names under the
/// node - its ancestors' fills over the page background - which is what
/// `get_screenshot` hands back: measured on the same frames, its PNG was
/// opaque where the export kept a translucent backdrop or transparent gaps,
/// showing that colour there (the page background (30, 30, 30) on a frame
/// straight on the page). An opaque export is returned as it came.
pub(crate) fn flatten_reference_png(
    data_base64: &str,
    backdrop: [u8; 3],
) -> Result<String, DevupError> {
    let bytes = STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|_| invalid_call("Figma reference PNG base64 is invalid."))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_REFERENCE_PNG_DIMENSION);
    limits.max_image_height = Some(MAX_REFERENCE_PNG_DIMENSION);
    limits.max_alloc = Some(MAX_REFERENCE_PNG_DECODED_BYTES as u64);
    let decoder =
        PngDecoder::with_limits(Cursor::new(bytes), limits).map_err(reference_png_decode_error)?;
    let mut pixels = image::DynamicImage::from_decoder(decoder)
        .map_err(reference_png_decode_error)?
        .into_rgba8();
    if pixels.pixels().all(|pixel| pixel[3] == u8::MAX) {
        return Ok(data_base64.to_owned());
    }
    for pixel in pixels.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        for (value, under) in pixel.0.iter_mut().zip(backdrop) {
            let mixed = u32::from(*value) * alpha + u32::from(under) * (255 - alpha);
            *value = u8::try_from((mixed + 127) / 255).unwrap_or(u8::MAX);
        }
        pixel[3] = u8::MAX;
    }
    let mut encoded = Vec::new();
    pixels
        .write_with_encoder(image::codecs::png::PngEncoder::new(&mut encoded))
        .map_err(|_| invalid_call("Figma reference PNG could not be re-encoded."))?;
    Ok(STANDARD.encode(encoded))
}

fn reference_png_decode_error(error: ImageError) -> DevupError {
    if matches!(error, ImageError::Limits(_)) {
        DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "Figma reference PNG dimensions or decoded size exceed the allowed range.",
            false,
        )
    } else {
        invalid_call("Figma reference PNG data is corrupted.")
    }
}

fn take_snapshot_cursor(chunk: &mut SnapshotChunk) -> Result<Option<SnapshotCursor>, DevupError> {
    let Some(cursor) = read_snapshot_cursor(&chunk.nodes)
        .map_err(|message| invalid_call(message.korean_message()))?
    else {
        return Ok(None);
    };
    chunk.nodes.retain(|node| node.id != SNAPSHOT_CURSOR_ID);
    Ok(Some(cursor))
}

fn invalid_call(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupFigmaHandoffInvalid, message, false)
}

/// The next page of a theme onto the pages before it. Every list - the
/// resources and the scan alike - is appended in order, with an entry the
/// pages both carry kept once; the scalars are the page's.
fn merge_theme_page(merged: &mut Value, page: Value) -> Result<(), DevupError> {
    let Some(page) = page.as_object() else {
        return Err(invalid_call("Figma theme page format is invalid."));
    };
    for list in [
        "collections",
        "variables",
        "styles",
        "usedRemoteVariables",
        "usedVariableIds",
        "usedStyleIds",
        "unresolved",
    ] {
        let incoming = page
            .get(list)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let target = merged
            .get_mut(list)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| invalid_call("Figma theme page format is invalid."))?;
        for item in incoming {
            // A resource is one by its id, an id in the scan by itself.
            let same = |existing: &Value| match item.get("id") {
                Some(id) => existing.get("id") == Some(id),
                None => *existing == item,
            };
            if target.iter().any(same) {
                continue;
            }
            target.push(item);
        }
    }
    for field in ["localComplete", "usedRemoteComplete"] {
        if let Some(value) = page.get(field) {
            merged[field] = value.clone();
        }
    }
    Ok(())
}

fn fallback_category(error: &DevupError) -> String {
    error
        .details
        .get("category")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{:?}", error.code))
}

fn is_section_target_error(error: &DevupError) -> bool {
    error.message.contains("DEVUP_TARGET_IS_SECTION")
        || error
            .details
            .to_string()
            .contains("DEVUP_TARGET_IS_SECTION")
}

fn fast_call_fallback_allowed(error: &DevupError) -> bool {
    matches!(
        error.code,
        ErrorCode::DevupFigmaDirectUnavailable
            | ErrorCode::DevupFigmaCatalogRejected
            | ErrorCode::DevupFigmaResponseTooLarge
            | ErrorCode::DevupSnapshotUnsupported
            | ErrorCode::DevupFigmaHandoffInvalid
    )
}

fn resource_count(value: &Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

fn empty_used_resources() -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "collections": [],
            "variables": [],
            "styles": [],
            "usedRemoteVariables": [],
            "usedVariableIds": [],
            "usedStyleIds": [],
            "localComplete": false,
            "usedRemoteComplete": true,
            "unresolved": []
        }),
    }
}

fn merge_fast_resources(
    existing: &mut Option<UpstreamResult>,
    incoming: UpstreamResult,
) -> Result<(), DevupError> {
    let Some(current) = existing else {
        *existing = Some(incoming);
        return Ok(());
    };
    let current = current
        .raw
        .as_object_mut()
        .ok_or_else(|| invalid_call("Existing multi-root resource format is invalid."))?;
    let incoming = incoming
        .raw
        .as_object()
        .ok_or_else(|| invalid_call("multi-root resource format is invalid."))?;
    for field in ["collections", "variables", "styles", "usedRemoteVariables"] {
        let mut values = BTreeMap::<String, Value>::new();
        for value in current
            .get(field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(
                incoming
                    .get(field)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten(),
            )
        {
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_call("multi-root resource ID is missing."))?;
            if let Some(previous) = values.get(id)
                && previous != value
            {
                return Err(DevupError::new(
                    ErrorCode::DevupFigmaVersionChanged,
                    "multi-root resource contents differ between batches.",
                    true,
                ));
            }
            values.insert(id.to_owned(), value.clone());
        }
        current.insert(
            field.to_owned(),
            Value::Array(values.into_values().collect()),
        );
    }
    for field in ["usedVariableIds", "usedStyleIds"] {
        let values = current
            .get(field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(
                incoming
                    .get(field)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten(),
            )
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid_call("multi-root resource ID format is invalid."))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        current.insert(
            field.to_owned(),
            Value::Array(values.into_iter().map(Value::String).collect()),
        );
    }
    let unresolved = current
        .get("unresolved")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            incoming
                .get("unresolved")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .map(|value| {
            (
                (value["id"].to_string(), value["kind"].to_string()),
                value.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    current.insert(
        "unresolved".to_owned(),
        Value::Array(unresolved.into_values().collect()),
    );
    for field in ["localComplete", "usedRemoteComplete"] {
        let merged = current.get(field).and_then(Value::as_bool).unwrap_or(false)
            && incoming
                .get(field)
                .and_then(Value::as_bool)
                .unwrap_or(false);
        current.insert(field.to_owned(), Value::Bool(merged));
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum UsedResourceItem {
    Variable(String),
    Style(ResourceStyleRef),
}

fn used_resource_batches(
    refs: &UsedResourceRefs,
    budget: BatchBudget,
) -> Result<Vec<ResourceBatch>, DevupError> {
    let items = refs
        .variable_ids
        .iter()
        .cloned()
        .map(UsedResourceItem::Variable)
        .chain(refs.styles.iter().cloned().map(UsedResourceItem::Style));
    let mut batches = Vec::new();
    let mut current = ResourceBatch {
        variable_ids: Vec::new(),
        styles: Vec::new(),
    };

    for item in items {
        let mut candidate = current.clone();
        add_used_resource(&mut candidate, item.clone());
        if used_resource_batch_fits(&candidate, budget) {
            current = candidate;
            continue;
        }
        if current.variable_ids.is_empty() && current.styles.is_empty() {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "A single Figma resource ID exceeds the safe batch size.",
                false,
            ));
        }
        batches.push(current);
        current = ResourceBatch {
            variable_ids: Vec::new(),
            styles: Vec::new(),
        };
        add_used_resource(&mut current, item);
        if !used_resource_batch_fits(&current, budget) {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "A single Figma resource ID exceeds the safe batch size.",
                false,
            ));
        }
    }
    if !current.variable_ids.is_empty() || !current.styles.is_empty() {
        batches.push(current);
    }
    Ok(batches)
}

fn add_used_resource(batch: &mut ResourceBatch, item: UsedResourceItem) {
    match item {
        UsedResourceItem::Variable(id) => batch.variable_ids.push(id),
        UsedResourceItem::Style(style) => batch.styles.push(style),
    }
}

fn used_resource_batch_fits(batch: &ResourceBatch, budget: BatchBudget) -> bool {
    batch.variable_ids.len() + batch.styles.len() <= budget.used_resource_items
        && serde_json::to_vec(batch).is_ok_and(|bytes| bytes.len() <= budget.used_resource_bytes)
}