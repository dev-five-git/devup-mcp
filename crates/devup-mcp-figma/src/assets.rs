use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    DevupError, Diagnostic, ErrorCode, LargeValueCursor, LargeValueDescriptor, RawNode, Snapshot,
    UpstreamResult,
};

pub const MAX_ASSET_BYTES: usize = 80 * 1024 * 1024;

/// Full source acquisition is the default; rendered modes are explicit.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetRenderMode {
    /// Legacy source-byte contract: matching codec and scale 1.
    Fill,
    Auto,
    #[default]
    #[serde(rename = "original-source")]
    OriginalSource,
    Node,
    Visible,
}

impl AssetRenderMode {
    pub fn is_original(self) -> bool {
        matches!(self, Self::Fill | Self::OriginalSource)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetExportDiagnostics {
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_info: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_error: Option<String>,
    pub stages: Vec<AssetExportStage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetExportStage {
    pub stage: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetFormat {
    Png,
    Webp,
    Jpg,
    Svg,
    Pdf,
}

impl AssetFormat {
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Webp => "image/webp",
            Self::Jpg => "image/jpeg",
            Self::Svg => "image/svg+xml",
            Self::Pdf => "application/pdf",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Jpg => "jpg",
            Self::Svg => "svg",
            Self::Pdf => "pdf",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetStatus {
    Available,
    Exported,
    Failed,
    /// Announced by the export script for an SVG too large for one answer:
    /// the bytes follow in fragments, and the collector reports the asset as
    /// `Exported` or `Failed` once they have. Never in a manifest.
    Chunked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRequest {
    /// Optional caller-declared use-site ratio; never measured browser evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_aspect_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<u32>,
    #[serde(default)]
    pub srcset: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_scope_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossless: Option<bool>,
    #[serde(default)]
    pub render_mode: AssetRenderMode,
    pub asset_id: String,
    pub node_id: String,
    pub field: String,
    pub image_hash: Option<String>,
    pub format: AssetFormat,
    pub scale: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetSelection {
    /// Optional caller-declared use-site ratio; never measured browser evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_aspect_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<u32>,
    #[serde(default)]
    pub srcset: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_scope_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossless: Option<bool>,
    #[serde(default)]
    pub render_mode: AssetRenderMode,
    pub asset_id: String,
    pub format: AssetFormat,
    pub scale: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetManifestEntry {
    /// Optional caller-declared use-site ratio; never measured browser evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_aspect_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<u32>,
    #[serde(default)]
    pub srcset: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_scope_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<AssetVariant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossless: Option<bool>,
    #[serde(default = "legacy_descriptor_mode")]
    pub render_mode: AssetRenderMode,
    /// Observations and explicit unknowns; dimensions are measured from bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_info: Option<Value>,
    /// A safe hierarchy-based suggestion, never an automatic write destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_filename: Option<String>,
    pub asset_id: String,
    pub node_id: String,
    pub field: String,
    pub source_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<AssetFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    pub status: AssetStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// What failed, in the exporter's words: the step, the time it took and
    /// the message Figma or the transport gave. Bounded to 500 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_diagnostics: Option<AssetExportDiagnostics>,
    /// Where the generated code refers to this asset, `/icons/x.svg` or
    /// `/images/x.png`, filled in by the server from the code generator's
    /// naming so the bytes can be written where the code will look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetManifest {
    pub version: u32,
    pub assets: Vec<AssetManifestEntry>,
    pub diagnostics: Vec<Diagnostic>,
}

/// A bounded derivative of the acquired source; no hidden upscaling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetVariant {
    pub density: f64,
    pub width: u32,
    pub height: u32,
    pub byte_length: usize,
    pub sha256: String,
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AssetNode {
    Svg,
    Png {
        fill_index: usize,
        image_hash: Option<String>,
    },
    /// A container that draws nothing of its own and holds one picture. It
    /// is that picture, but it does not carry the fill, so its bytes come
    /// from rendering the node rather than from a fill index it lacks.
    PngNode {
        image_hash: Option<String>,
    },
}

pub fn discover_asset_manifest(snapshot: &Snapshot) -> AssetManifest {
    let mut assets = Vec::new();
    let mut pending = snapshot.roots.iter().rev().cloned().collect::<Vec<_>>();
    let mut visited = std::collections::BTreeSet::new();

    while let Some(node_id) = pending.pop() {
        let Some(node) = snapshot.nodes.get(&node_id) else {
            continue;
        };
        if !visited.insert(node_id) {
            continue;
        }

        if let Some(asset) = compute_asset_node(snapshot, node, false) {
            assets.push(manifest_entry(node, asset));
            continue;
        }

        // A container carries pictures of its own: a section drawn over a
        // photograph is painted `bg: url(...)` by the code generator, yet the
        // node is a layout box rather than an asset, so the walk goes on into
        // its children. Listed here, the picture the code points at can be
        // exported; unlisted, nothing can deliver it and the screen renders
        // with a hole where the photograph belongs.
        assets.extend(image_fill_entries(node));

        let child_ids = node.typed_view().child_ids().collect::<Vec<_>>();
        pending.extend(child_ids.into_iter().rev().map(str::to_owned));
    }

    assets.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
    crate::asset_render_info::annotate_assets(snapshot, &mut assets);
    AssetManifest {
        version: 1,
        assets,
        diagnostics: Vec::new(),
    }
}

/// Full in-scope IMAGE census, including paints below flattened asset boundaries.
pub fn discover_image_fill_assets(snapshot: &Snapshot) -> Vec<AssetManifestEntry> {
    let mut pending = snapshot
        .roots
        .iter()
        .rev()
        .map(|id| (id.clone(), false))
        .collect::<Vec<_>>();
    let mut seen = std::collections::BTreeSet::new();
    let mut assets = Vec::new();
    while let Some((id, ancestor_hidden)) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(&id) else {
            continue;
        };
        let hidden = ancestor_hidden || asset_exclusion_reason(node).is_some();
        for (index, paint) in fills(node)
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, p)| fill_type(p) == Some("IMAGE"))
        {
            let mut asset = manifest_entry(
                node,
                AssetNode::Png {
                    fill_index: index,
                    image_hash: paint
                        .get("imageHash")
                        .or_else(|| paint.get("imageRef"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
            );
            if hidden
                || !is_visible_fill(paint)
                || paint["opacity"].as_f64().is_some_and(|n| n <= 0.0)
            {
                asset.status = AssetStatus::Failed;
                asset.error_code = Some("DEVUP_ASSET_NODE_HIDDEN".into());
                asset.error_detail = Some("The node, paint or an acquired ancestor is hidden, transparent or outside rendered bounds; no image was rendered.".into());
            }
            assets.push(asset);
        }
        pending.extend(
            node.typed_view()
                .child_ids()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|id| (id.to_owned(), hidden)),
        );
    }
    assets.sort_by(|a, b| a.asset_id.cmp(&b.asset_id));
    crate::annotate_assets(snapshot, &mut assets);
    assets
}

/// Every acquired IMAGE paint, including hidden/failed dispositions.
pub fn discover_web_ready_assets(snapshot: &Snapshot) -> Vec<AssetManifestEntry> {
    discover_image_fill_assets(snapshot)
}

fn direct_node_candidate(snapshot: &Snapshot, asset_id: &str) -> Option<AssetManifestEntry> {
    let id = asset_id.strip_suffix(":node")?;
    let node = snapshot.nodes.get(id)?;
    // Snapshot membership alone is insufficient: context nodes may be acquired
    // outside the selected roots. Walk only roots and their actual child edges.
    if !in_scope(snapshot, id) {
        return None;
    }
    if matches!(node.typed_view().node_type(), "PAGE" | "DOCUMENT") {
        return None;
    }
    let mut entry = manifest_entry(node, AssetNode::Svg);
    entry.source_kind = "node".into();
    Some(entry)
}

pub(crate) fn in_scope(snapshot: &Snapshot, wanted: &str) -> bool {
    let mut pending = snapshot.roots.clone();
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if id == wanted {
            return true;
        }
        if let Some(node) = snapshot.nodes.get(&id) {
            pending.extend(node.typed_view().child_ids().map(str::to_owned));
        }
    }
    false
}

fn visible_scope(
    snapshot: &Snapshot,
    node_id: &str,
    explicit: Option<&str>,
) -> Result<String, DevupError> {
    let mut current = node_id.to_owned();
    let mut seen = std::collections::BTreeSet::new();
    loop {
        if !seen.insert(current.clone()) {
            break;
        }
        if explicit == Some(current.as_str()) && in_scope(snapshot, &current) {
            return Ok(current);
        }
        let parent = snapshot
            .nodes
            .values()
            .find(|n| n.typed_view().child_ids().any(|id| id == current));
        let Some(parent) = parent else {
            break;
        };
        current = parent.id.clone();
        if explicit.is_none() && in_scope(snapshot, &current) {
            return Ok(current);
        }
    }
    Err(invalid(include_str!(
        "text/assets-visible_scope-00ec4145.md"
    )))
}

pub fn validate_asset_options(
    format: AssetFormat,
    mode: AssetRenderMode,
    scale: f64,
    max_width: Option<u32>,
    srcset: bool,
) -> Result<(), DevupError> {
    if !scale.is_finite() || scale <= 0.0 || scale > 4.0 {
        return Err(invalid(
            "Asset scale must be finite, greater than zero and at most 4; rendered bitmaps default to 2 and recommend 1.5 to 3.",
        ));
    }
    if max_width.is_some_and(|w| w == 0 || w > crate::MAX_WEBP_DIMENSION)
        || ((max_width.is_some() || srcset)
            && (format != AssetFormat::Webp
                || !mode.is_original()
                || mode == AssetRenderMode::Fill))
    {
        return Err(invalid(
            "maxWidth (1..8192) and srcset require original-source WebP; legacy fill preserves its no-resize contract.",
        ));
    }
    Ok(())
}

fn compute_asset_node(snapshot: &Snapshot, node: &RawNode, nested: bool) -> Option<AssetNode> {
    let view = node.typed_view();
    if matches!(view.node_type(), "TEXT" | "COMPONENT_SET")
        || view
            .value("inferredAutoLayout")
            .and_then(|layout| layout.get("layoutMode"))
            .and_then(Value::as_str)
            == Some("GRID")
    {
        return None;
    }

    if has_smart_animate_reaction(node)
        || view
            .string("parentId")
            .and_then(|parent_id| snapshot.nodes.get(parent_id))
            .is_some_and(has_smart_animate_reaction)
    {
        return None;
    }

    // A boolean's rendered shape is authoritative even when its operands
    // were not included in the snapshot. Export the node, never its bounds.
    if matches!(
        view.node_type(),
        "VECTOR" | "STAR" | "POLYGON" | "BOOLEAN_OPERATION"
    ) {
        return Some(AssetNode::Svg);
    }

    if view.node_type() == "ELLIPSE"
        && view
            .value("arcData")
            .and_then(|arc_data| arc_data.get("innerRadius"))
            .and_then(Value::as_f64)
            .is_some_and(|inner_radius| inner_radius != 0.0)
    {
        return Some(AssetNode::Svg);
    }

    let child_ids = view.child_ids().collect::<Vec<_>>();
    if child_ids.is_empty() {
        return compute_leaf_asset(node, nested);
    }

    if child_ids.len() == 1 {
        if ["paddingLeft", "paddingRight", "paddingTop", "paddingBottom"]
            .into_iter()
            .any(|field| view.number(field).is_some_and(|padding| padding > 0.0))
            || fills(node).is_some_and(|fills| fills.iter().any(is_visible_fill))
        {
            return None;
        }

        // The container stands in for the child, and the code names the
        // file after the container. Asking Figma for `fills/0` of a node
        // whose fills are empty is a request it can only refuse, and it did:
        // the devup-ui landing page's footer logo sits in such a frame and
        // was the one asset of 182 that never arrived. Rendering the node
        // gives the same picture, and is a request the node can answer.
        return match snapshot
            .nodes
            .get(child_ids[0])
            .and_then(|child| compute_asset_node(snapshot, child, true))
        {
            Some(AssetNode::Svg) => Some(AssetNode::Svg),
            Some(AssetNode::Png { image_hash, .. } | AssetNode::PngNode { image_hash }) => {
                Some(AssetNode::PngNode { image_hash })
            }
            None => None,
        };
    }

    let mut visible_children = Vec::new();
    for child_id in child_ids {
        let child = snapshot.nodes.get(child_id)?;
        if child.typed_view().bool("visible") != Some(false) {
            visible_children.push(child);
        }
    }

    visible_children
        .into_iter()
        .all(|child| compute_asset_node(snapshot, child, true) == Some(AssetNode::Svg))
        .then_some(AssetNode::Svg)
}

/// Every picture a node paints from an image fill. The code generator writes
/// a `url(...)` for each visible image fill whatever its scale mode, so each
/// one is an asset a caller has to be able to fetch. A node that is an asset
/// in its own right never reaches this: its bytes come from the node itself.
fn image_fill_entries(node: &RawNode) -> Vec<AssetManifestEntry> {
    fills(node)
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, fill)| is_visible_fill(fill) && fill_type(fill) == Some("IMAGE"))
        .map(|(fill_index, fill)| {
            manifest_entry(
                node,
                AssetNode::Png {
                    fill_index,
                    image_hash: fill
                        .get("imageHash")
                        .or_else(|| fill.get("imageRef"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
            )
        })
        .collect()
}

fn compute_leaf_asset(node: &RawNode, nested: bool) -> Option<AssetNode> {
    let node_fills = fills(node);
    if node_fills.is_some_and(|fills| {
        fills.iter().any(|fill| {
            is_visible_fill(fill)
                && (fill_type(fill) == Some("PATTERN")
                    || (fill_type(fill) == Some("IMAGE")
                        && fill.get("scaleMode").and_then(Value::as_str) == Some("TILE")))
        })
    }) {
        return None;
    }

    if node.typed_view().bool("isAsset") == Some(true) {
        if let Some((fill_index, fill)) = node_fills.and_then(|fills| {
            fills.iter().enumerate().find(|(_, fill)| {
                is_visible_fill(fill)
                    && fill_type(fill) == Some("IMAGE")
                    && fill.get("scaleMode").and_then(Value::as_str) != Some("TILE")
            })
        }) {
            if node_fills.is_some_and(|fills| fills.len() == 1) {
                return Some(AssetNode::Png {
                    fill_index,
                    image_hash: fill
                        .get("imageHash")
                        .or_else(|| fill.get("imageRef"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                });
            }
            return None;
        }

        if node_fills.is_none_or(|fills| {
            fills
                .iter()
                .all(|fill| is_visible_fill(fill) && fill_type(fill) == Some("SOLID"))
        }) {
            return nested.then_some(AssetNode::Svg);
        }

        return Some(AssetNode::Svg);
    }

    (nested
        && node_fills.is_some_and(|fills| {
            fills.iter().all(|fill| {
                !is_visible_fill(fill)
                    || !matches!(fill_type(fill), Some("IMAGE" | "VIDEO" | "PATTERN"))
            })
        }))
    .then_some(AssetNode::Svg)
}

fn fills(node: &RawNode) -> Option<&Vec<Value>> {
    node.typed_view().value("fills").and_then(Value::as_array)
}

fn fill_type(fill: &Value) -> Option<&str> {
    fill.get("type").and_then(Value::as_str)
}

fn is_visible_fill(fill: &Value) -> bool {
    fill.get("visible").and_then(Value::as_bool) != Some(false)
}

fn has_smart_animate_reaction(node: &RawNode) -> bool {
    node.typed_view()
        .value("reactions")
        .and_then(Value::as_array)
        .is_some_and(|reactions| {
            reactions.iter().any(|reaction| {
                reaction
                    .get("actions")
                    .and_then(Value::as_array)
                    .is_some_and(|actions| {
                        actions.iter().any(|action| {
                            action.get("type").and_then(Value::as_str) == Some("NODE")
                                && action
                                    .get("transition")
                                    .and_then(|transition| transition.get("type"))
                                    .and_then(Value::as_str)
                                    == Some("SMART_ANIMATE")
                        })
                    })
            })
        })
}

/// Shared discovery/codegen policy for assets that cannot produce visible bytes.
pub fn asset_exclusion_reason(node: &RawNode) -> Option<&'static str> {
    let view = node.typed_view();
    if view.bool("visible") == Some(false) {
        Some("visible-false")
    } else if view.number("opacity").is_some_and(|opacity| opacity <= 0.0) {
        Some("zero-opacity")
    } else if view.value("absoluteBoundingBox").is_some()
        && view
            .value("absoluteRenderBounds")
            .is_none_or(Value::is_null)
    {
        Some("no-render-bounds")
    } else {
        None
    }
}

fn manifest_entry(node: &RawNode, asset: AssetNode) -> AssetManifestEntry {
    let (asset_id, field, source_kind, image_hash) = match asset {
        AssetNode::Svg => (
            format!("{}:node", node.id),
            "node".to_owned(),
            "vector-node".to_owned(),
            None,
        ),
        AssetNode::Png {
            fill_index,
            image_hash,
        } => (
            format!("{}:fills:{fill_index}", node.id),
            format!("fills/{fill_index}"),
            "image-fill".to_owned(),
            image_hash,
        ),
        AssetNode::PngNode { image_hash } => (
            format!("{}:node", node.id),
            "node".to_owned(),
            "image-node".to_owned(),
            image_hash,
        ),
    };

    // Figma refuses to export a node that has no visible layers, so a node
    // that draws nothing can never produce bytes. Advertising it as available
    // promised something the export would always refuse, and the caller only
    // found out once the failure surfaced from inside Figma, far from its
    // cause.
    //
    // Drawing nothing is not only being hidden. A node left fully transparent
    // is `visible`, and renders no pixel all the same: the about page carries
    // two such icons, and they were the only two exports Figma turned down.
    //
    // Nor is it only about the node itself. One that sits entirely outside an
    // ancestor that clips is visible, opaque, and still draws nothing, and
    // Figma says so by leaving `absoluteRenderBounds` off it - that field is
    // the bounds of what the node actually renders, and there are none. The
    // devup-ui landing page's mobile and tablet each carry one such icon,
    // pushed past the edge of a clipped panel, and they were the only two
    // exports of 215 that Figma turned down.
    let hidden = asset_exclusion_reason(node).is_some();
    let (status, error_code) = if hidden {
        (
            AssetStatus::Failed,
            Some("DEVUP_ASSET_NODE_HIDDEN".to_owned()),
        )
    } else {
        (AssetStatus::Available, None)
    };

    AssetManifestEntry {
        usage_aspect_ratio: None,
        max_width: None,
        srcset: false,
        visible_scope_node_id: None,
        variants: Vec::new(),
        render_mode: AssetRenderMode::Node,
        quality: None,
        lossless: None,
        render_info: None,
        suggested_filename: None,
        asset_id,
        node_id: node.id.clone(),
        field,
        source_kind,
        image_hash,
        format: None,
        scale: None,
        status,
        byte_length: None,
        sha256: None,
        mime_type: None,
        data_base64: None,
        output_path: None,
        error_code,
        error_detail: None,
        export_diagnostics: None,
        path: None,
    }
}

pub fn validate_asset_requests(
    snapshot: &Snapshot,
    requests: &[AssetRequest],
) -> Result<(), DevupError> {
    if requests.len() > 16 {
        return Err(invalid("At most 16 assets can be exported at once."));
    }
    let mut available = discover_asset_manifest(snapshot);
    available
        .assets
        .extend(discover_image_fill_assets(snapshot));
    let mut seen = std::collections::BTreeSet::new();
    for request in requests {
        crate::validate_webp_options(request.format, request.quality, request.lossless)?;
        if request
            .usage_aspect_ratio
            .is_some_and(|r| !r.is_finite() || r <= 0.0 || r > 10000.0)
        {
            return Err(invalid(
                "usageAspectRatio must be finite and positive, at most10000.",
            ));
        }
        validate_asset_options(
            request.format,
            request.render_mode,
            request.scale,
            request.max_width,
            request.srcset,
        )?;
        if request.scale == 0.0 || request.scale > 4.0 || !seen.insert(request.asset_id.as_str()) {
            return Err(invalid(
                "asset request has an invalid scale or a duplicate ID.",
            ));
        }
        let direct = direct_node_candidate(snapshot, &request.asset_id);
        let Some(candidate) = available
            .assets
            .iter()
            .find(|asset| asset.asset_id == request.asset_id)
            .or(direct.as_ref())
        else {
            return Err(invalid("The requested asset is not in the snapshot."));
        };
        if let Some(node) = snapshot.nodes.get(&request.node_id) {
            if !request.render_mode.is_original()
                && ["width", "height"].into_iter().any(|key| {
                    node.typed_view()
                        .number(key)
                        .is_some_and(|n| !n.is_finite() || n <= 0.0)
                })
            {
                return Err(invalid(
                    "Native capture dimensions must be finite and positive.",
                ));
            }
            if request.srcset
                && node
                    .typed_view()
                    .number("width")
                    .is_none_or(|w| !w.is_finite() || w <= 0.0)
            {
                return Err(invalid(
                    "srcset requires a known positive node display width.",
                ));
            }
        }
        if !in_scope(snapshot, &request.node_id) {
            return Err(invalid(
                "The requested node is outside the acquired snapshot scope.",
            ));
        }
        if candidate.node_id != request.node_id
            || candidate.field != request.field
            || candidate.image_hash != request.image_hash
        {
            return Err(invalid("asset request does not match the snapshot source."));
        }
        if request.render_mode == AssetRenderMode::Fill
            && (!request.field.starts_with("fills/")
                || !matches!(
                    request.format,
                    AssetFormat::Png | AssetFormat::Jpg | AssetFormat::Webp
                )
                || request.scale != 1.0)
        {
            return Err(invalid(include_str!(
                "text/assets-validate_asset_requests-09bfca92.md"
            )));
        }
        // Reject what the manifest already knows cannot be exported, so the
        // reason travels with the rejection instead of arriving later as an
        // opaque failure from inside Figma.
        if request.render_mode == AssetRenderMode::OriginalSource
            && (!request.field.starts_with("fills/")
                || request.image_hash.is_none()
                || !matches!(
                    request.format,
                    AssetFormat::Png | AssetFormat::Jpg | AssetFormat::Webp
                ))
        {
            return Err(invalid(
                "original-source requires a resolvable IMAGE fill; request renderMode node explicitly for native raster boundaries, or retain SVG for vectors.",
            ));
        }
        if request.render_mode == AssetRenderMode::Visible {
            visible_scope(
                snapshot,
                &request.node_id,
                request.visible_scope_node_id.as_deref(),
            )?;
            if !matches!(
                request.format,
                AssetFormat::Png | AssetFormat::Jpg | AssetFormat::Webp
            ) {
                return Err(invalid("Visible scoped captures require PNG, JPG or WebP."));
            }
        }
        if request.render_mode != AssetRenderMode::Visible
            && request.visible_scope_node_id.is_some()
        {
            return Err(invalid(
                "visibleScopeNodeId applies only to renderMode visible.",
            ));
        }
        if candidate.status == AssetStatus::Failed
            && request.render_mode != AssetRenderMode::OriginalSource
        {
            return Err(invalid(
                "The requested asset cannot be exported: the node is hidden in Figma.",
            ));
        }
    }
    Ok(())
}

pub fn resolve_asset_selections(
    snapshot: &Snapshot,
    selections: &[AssetSelection],
) -> Result<Vec<AssetRequest>, DevupError> {
    if selections.len() > 16 {
        return Err(invalid("At most 16 assets can be exported at once."));
    }
    let mut manifest = discover_asset_manifest(snapshot);
    manifest.assets.extend(discover_image_fill_assets(snapshot));
    let mut seen = std::collections::BTreeSet::new();
    let requests = selections
        .iter()
        .map(|selection| {
            if selection.scale == 0.0
                || selection.scale > 4.0
                || !seen.insert(selection.asset_id.as_str())
            {
                return Err(invalid(
                    "asset selection has an invalid scale or a duplicate ID.",
                ));
            }
            let direct = direct_node_candidate(snapshot, &selection.asset_id);
            let asset = manifest
                .assets
                .iter()
                .find(|asset| asset.asset_id == selection.asset_id)
                .or(direct.as_ref())
                .ok_or_else(|| {
                    invalid("The selected asset or nodeId is not in the acquired snapshot scope.")
                })?;
            let capture_scope = if selection.render_mode == AssetRenderMode::Visible {
                Some(visible_scope(
                    snapshot,
                    &asset.node_id,
                    selection.visible_scope_node_id.as_deref(),
                )?)
            } else {
                selection.visible_scope_node_id.clone()
            };
            Ok(AssetRequest {
                usage_aspect_ratio: selection.usage_aspect_ratio,
                max_width: selection.max_width,
                srcset: selection.srcset,
                visible_scope_node_id: capture_scope,
                asset_id: asset.asset_id.clone(),
                node_id: asset.node_id.clone(),
                field: asset.field.clone(),
                image_hash: asset.image_hash.clone(),
                format: selection.format,
                scale: selection.scale,
                render_mode: selection.render_mode,
                quality: selection.quality,
                lossless: selection.lossless,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_asset_requests(snapshot, &requests)?;
    Ok(requests)
}

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
                include_str!("text/assets-asset_export_from_result-a4ad1a7b.md"),
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
