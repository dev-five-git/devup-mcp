use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::{DevupError, ErrorCode, FidelityImpact, Snapshot, discover_asset_manifest};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::codegen::{
    AssetKind, CodegenOutput, asset_kind, derived_padding, placed_by_a_free_layout,
};

#[path = "absolute_bounds.rs"]
mod absolute_bounds;
#[path = "attributes.rs"]
pub mod attributes;
#[path = "fingerprint.rs"]
mod fingerprint;
#[path = "resolution.rs"]
mod resolution;
#[path = "sizing.rs"]
mod sizing;
pub use fingerprint::design_fingerprints;
pub use resolution::resolution_semantics;
pub(crate) use sizing::absolute_component_verification;
pub(crate) use sizing::prop as opening_prop;
pub(crate) use sizing::{account_for_sizing, implicit_css_verification};

const START: &str = "\u{e000}DEVUP_PROVENANCE_START:";
const END: &str = "\u{e000}DEVUP_PROVENANCE_END:";
const CLOSE: char = '\u{e001}';

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceEntry {
    /// Internal renderer/validator bookkeeping; never a consumer-facing offset.
    #[serde(skip)]
    pub generated_range: Option<GeneratedRange>,
    /// How a generated value was computed from the raw one, when that is not a
    /// plain copy (e.g. a percent line-height written as px).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calculation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_property: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_pointer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    pub resolution: String,
    /// Ordinal of the JSX opening element (document order, from 0) whose tag
    /// or attribute this entry maps, in the TSX it was generated with. The
    /// same ordinal `devup_ui_validate` reports for a violation, so the two
    /// can be joined without byte offsets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_index: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMap {
    pub version: u32,
    pub entries: Vec<ProvenanceEntry>,
    /// Acquisition policy plus the properties actually emitted for each image.
    #[serde(default)]
    pub asset_css_mappings: Vec<serde_json::Value>,
    /// Trusted resource identities retained for in-process fidelity validation.
    /// A deserialized map without this context cannot establish token identity.
    #[serde(skip)]
    pub variable_tokens: BTreeMap<String, String>,
    #[serde(skip)]
    pub style_tokens: BTreeMap<String, String>,
}

impl Serialize for SourceMap {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut map = serializer.serialize_struct("SourceMap", 5)?;
        map.serialize_field("version", &self.version)?;
        map.serialize_field("entries", &self.property_entries())?;
        map.serialize_field("resolutionSemantics", &resolution_semantics())?;
        map.serialize_field("rasterizedAssets", &self.rasterized_assets())?;
        map.serialize_field("assetCssMappings", &self.asset_css_mappings)?;
        map.end()
    }
}

impl SourceMap {
    pub fn rasterized_assets(&self) -> Vec<serde_json::Value> {
        self.asset_css_mappings.iter().filter(|m|m["effectsBaked"]==true && m["emitted"]!=false).map(|m|json!({"nodeId":m["nodeId"],"assetId":m["assetId"],"effectsBakedIntoAsset":true,"effectsAppliedBy":"asset","scope":m["contextScope"],"evidence":"supplied-acquisition-representation-policy","cssEffectsSuppressed":true})).collect()
    }
    pub fn property_entries(&self) -> Vec<ProvenanceEntry> {
        self.entries
            .iter()
            .filter(|e| e.property.is_some() || e.json_pointer.is_some())
            .cloned()
            .collect()
    }

    /// Freeze field-to-generated-property facts while the renderer's private
    /// positions still refer to its own code. Delivery rewrites the property
    /// strings together with TSX; no positions cross the public boundary.
    pub(crate) fn describe_properties(&mut self, tsx: &str) {
        let elements = attributes::opening_elements(tsx);
        for entry in &mut self.entries {
            entry.element_index = entry.generated_range.as_ref().and_then(|range| {
                elements
                    .iter()
                    .filter(|element| element.start <= range.start && range.end <= element.end)
                    .min_by_key(|element| element.end - element.start)
                    .map(|element| element.ordinal)
            });
            let Some(property) = entry.property.as_deref() else {
                continue;
            };
            let source = entry
                .generated_range
                .as_ref()
                .and_then(|r| tsx.get(r.start..r.end));
            entry.generated_property = match (entry.resolution.as_str(), property, source) {
                ("accounted-for-content-sizing", _, Some(_)) => {
                    Some("implicit:text-content-sizing".into())
                }
                ("accounted-for-implicit-flex-stretch", _, Some(_)) => {
                    Some("implicit:align-self:stretch".into())
                }
                ("accounted-for-implicit-flex-grow", _, Some(source)) => {
                    source.find("flex=\"").and_then(|at| {
                        source[at + 6..]
                            .find('"')
                            .map(|end| source[at..at + 7 + end].to_owned())
                    })
                }
                ("derived-hard-break-whitespace", _, Some(source)) => Some(source.trim().into()),
                (_, "characters", Some(_)) => Some("children".into()),
                (_, _, Some(source)) if !source.trim().is_empty() => Some(source.trim().into()),
                _ => None,
            };
            if entry.generated_property.is_none() && entry.resolution == "exact" {
                entry.resolution = "unverified-property-mapping".into();
            }
        }
    }

    /// Explain unit conversions the generator performs, so a reader can
    /// reproduce `lineHeight="26px"` from the raw `{unit: PERCENT, value: 160}`.
    pub(crate) fn describe_conversions(&mut self, snapshot: &devup_mcp_figma::Snapshot) {
        for entry in &mut self.entries {
            if entry.property.as_deref() != Some("lineHeight") {
                continue;
            }
            let Some(node) = entry
                .node_id
                .as_deref()
                .and_then(|id| snapshot.nodes.get(id))
            else {
                continue;
            };
            let view = node.typed_view();
            let Some(line_height) = view.value("lineHeight") else {
                continue;
            };
            if line_height["unit"] != "PERCENT" {
                continue;
            }
            let percent =
                (line_height["value"].as_f64().unwrap_or_default() * 100.0).round() / 100.0;
            let bound = view
                .value("boundVariables")
                .and_then(|b| b.get("fontSize"))
                .is_some();
            entry.calculation = Some(
                match (
                    view.value("fontSize").and_then(serde_json::Value::as_f64),
                    bound,
                ) {
                    (Some(size), false) => format!(
                        "round(fontSize * percent / 100) = round({size} * {percent} / 100) = {}px",
                        (size * percent / 100.0).round()
                    ),
                    _ => format!(
                        "{percent}% kept as a percentage because fontSize is variable-bound or unknown"
                    ),
                },
            );
        }
    }

    pub fn empty() -> Self {
        Self {
            version: 2,
            entries: Vec::new(),
            asset_css_mappings: Vec::new(),
            variable_tokens: BTreeMap::new(),
            style_tokens: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectionDisposition {
    Emitted,
    Flattened,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionTraceEntry {
    pub node_id: String,
    pub disposition: ProjectionDisposition,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_range: Option<GeneratedRange>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionTrace {
    pub root_node_id: String,
    pub entries: Vec<ProjectionTraceEntry>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FidelityCoverage {
    pub total: usize,
    pub covered: usize,
    pub basis_points: u16,
}

impl FidelityCoverage {
    fn new(total: usize, covered: usize) -> Self {
        let basis_points = if total == 0 {
            10_000
        } else {
            ((covered.min(total) as u128 * 10_000) / total as u128) as u16
        };
        Self {
            total,
            covered: covered.min(total),
            basis_points,
        }
    }

    pub fn complete(self) -> bool {
        self.total == self.covered
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FidelityImpactCounts {
    pub none: usize,
    pub approximated: usize,
    pub lossy: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FidelityReport {
    pub syntax_valid: bool,
    pub nodes: FidelityCoverage,
    pub text: FidelityCoverage,
    pub variables: FidelityCoverage,
    pub typography: FidelityCoverage,
    pub assets: FidelityCoverage,
    pub layout: FidelityCoverage,
    pub impacts: FidelityImpactCounts,
    /// Declared children absent from both the snapshot and an asset projection.
    /// Unlike an intentionally flattened SVG operand, these are visual losses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncovered_node_ids: Vec<String>,
    /// The `nodeId#property` layout pairs the generated TSX does not account
    /// for, bounded by [`MAX_REPORTED_UNCOVERED`]. Reporting only a ratio left
    /// a shortfall untriageable: nothing said whether the layout was wrong or
    /// merely expressed another way. Purely informational — it does not feed
    /// `impacts`, `strict_compatible`, or the reported status.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncovered_layout: Vec<String>,
}

/// Enough to see the shape of a shortfall without turning a diagnostic into a
/// second payload.
const MAX_REPORTED_UNCOVERED: usize = 40;

impl FidelityReport {
    pub fn strict_compatible(&self) -> bool {
        self.syntax_valid
            && self.nodes.complete()
            && self.text.complete()
            && self.variables.complete()
            && self.typography.complete()
            && self.assets.complete()
            && self.layout.complete()
            && self.impacts.approximated == 0
            && self.impacts.lossy == 0
            && self.impacts.failed == 0
    }
}

pub(crate) fn build_projection_trace(
    snapshot: &Snapshot,
    root_id: &str,
    tsx: &str,
    source_map: &SourceMap,
) -> ProjectionTrace {
    let active = active_nodes(snapshot, root_id);
    let emitted = source_map
        .entries
        .iter()
        .filter(|entry| entry.property.is_none() && entry.resolution == "node")
        .filter_map(|entry| Some((entry.node_id.clone()?, entry.generated_range.clone()?)))
        .collect::<BTreeMap<_, _>>();
    let asset_nodes = discover_asset_manifest(snapshot)
        .assets
        .into_iter()
        .map(|asset| asset.node_id)
        .collect::<BTreeSet<_>>();
    let mut entries = Vec::new();
    let mut represented_by_parent = BTreeSet::new();
    let mut ignored_by_parent = BTreeSet::new();
    for (node_id, parent_id) in active {
        let node = snapshot.nodes.get(&node_id);
        let range = emitted.get(&node_id);
        let projection =
            if node_id == root_id && node.is_some_and(|node| node.node_type == "SECTION") {
                Some((ProjectionDisposition::Flattened, "section-root", None))
            } else if node.is_some_and(|node| node.typed_view().bool("visible") == Some(false)) {
                Some((ProjectionDisposition::Ignored, "hidden", None))
            } else if let Some(range) = range.filter(|range| range.start < range.end) {
                Some((
                    ProjectionDisposition::Emitted,
                    "direct",
                    Some((*range).clone()),
                ))
            } else if range.is_some()
                || node.is_some_and(|node| node.typed_view().bool("visible") == Some(false))
            {
                Some((ProjectionDisposition::Ignored, "hidden", None))
            } else if parent_id
                .as_ref()
                .is_some_and(|parent_id| ignored_by_parent.contains(parent_id))
            {
                Some((ProjectionDisposition::Ignored, "hidden-ancestor", None))
            } else if parent_id.as_ref().is_some_and(|parent_id| {
                asset_nodes.contains(parent_id)
                    || represented_by_parent.contains(parent_id)
                    || is_variant_component(snapshot, parent_id)
                    || snapshot.nodes.get(parent_id).is_some_and(|parent| {
                        parent.node_type == "COMPONENT_SET" && emitted.contains_key(parent_id)
                    })
                    || snapshot.nodes.get(parent_id).is_some_and(|parent| {
                        parent.node_type == "INSTANCE"
                            || parent.typed_view().bool("isAsset") == Some(true)
                    })
                    || emitted.get(parent_id).is_some_and(|range| {
                        range.end <= tsx.len()
                            && (tsx[range.start..range.end].contains("maskImage=")
                                || tsx[range.start..range.end].contains("<Image"))
                    })
            }) {
                Some((
                    ProjectionDisposition::Flattened,
                    "represented-by-parent",
                    None,
                ))
            } else {
                None
            };
        if let Some((disposition, reason, generated_range)) = projection {
            if disposition == ProjectionDisposition::Flattened && reason == "represented-by-parent"
            {
                represented_by_parent.insert(node_id.clone());
            }
            if disposition == ProjectionDisposition::Ignored {
                ignored_by_parent.insert(node_id.clone());
            }
            entries.push(ProjectionTraceEntry {
                node_id,
                disposition,
                reason: reason.to_owned(),
                generated_range,
            });
        }
    }
    ProjectionTrace {
        root_node_id: root_id.to_owned(),
        entries,
    }
}

fn is_variant_component(snapshot: &Snapshot, node_id: &str) -> bool {
    let Some(node) = snapshot.nodes.get(node_id) else {
        return false;
    };
    if node.node_type != "COMPONENT" {
        return false;
    }
    node.typed_view()
        .string("parentId")
        .and_then(|parent_id| snapshot.nodes.get(parent_id))
        .is_some_and(|parent| parent.node_type == "COMPONENT_SET")
        || snapshot.nodes.values().any(|parent| {
            parent.node_type == "COMPONENT_SET"
                && parent
                    .typed_view()
                    .child_ids()
                    .any(|child| child == node_id)
        })
}

pub fn validate_fidelity(
    snapshot: &Snapshot,
    root_id: &str,
    output: &CodegenOutput,
) -> Result<FidelityReport, DevupError> {
    let expected = active_nodes(snapshot, root_id)
        .into_iter()
        .map(|(node_id, _)| node_id)
        .collect::<BTreeSet<_>>();
    let mut observed = BTreeSet::new();
    let mut duplicates = Vec::new();
    let mut invalid_ranges = Vec::new();
    for entry in &output.projection_trace.entries {
        if !observed.insert(entry.node_id.clone()) {
            duplicates.push(entry.node_id.clone());
        }
        if let Some(range) = &entry.generated_range
            && (range.start >= range.end
                || range.end > output.tsx.len()
                || !output.tsx.is_char_boundary(range.start)
                || !output.tsx.is_char_boundary(range.end))
        {
            invalid_ranges.push(entry.node_id.clone());
        }
    }
    let missing = expected.difference(&observed).cloned().collect::<Vec<_>>();
    let unexpected = observed.difference(&expected).cloned().collect::<Vec<_>>();
    if !duplicates.is_empty()
        || !missing.is_empty()
        || !unexpected.is_empty()
        || !invalid_ranges.is_empty()
    {
        return Err(DevupError::with_details(
            ErrorCode::DevupCodegenFailed,
            "Projection trace did not account for each source node exactly once.",
            false,
            json!({
                "missingNodeIds": missing,
                "duplicateNodeIds": duplicates,
                "unexpectedNodeIds": unexpected,
                "invalidRangeNodeIds": invalid_ranges
            }),
        ));
    }

    let semantic_nodes = semantic_nodes(snapshot, root_id);
    let text_nodes = semantic_nodes
        .iter()
        .filter(|node_id| {
            snapshot
                .nodes
                .get(**node_id)
                .is_some_and(|node| node.node_type == "TEXT")
        })
        .copied()
        .collect::<BTreeSet<_>>();
    let text_segments = text_sources(snapshot, &text_nodes);
    let mut consumed_text_entries = BTreeSet::new();
    let covered_text = text_segments
        .iter()
        .filter(|(node_id, characters)| {
            output
                .source_map
                .entries
                .iter()
                .enumerate()
                .find(|(index, entry)| {
                    !consumed_text_entries.contains(index)
                        && entry.node_id.as_deref() == Some(node_id.as_str())
                        && entry.property.as_deref() == Some("characters")
                        && entry_range(entry, &output.tsx)
                            .is_some_and(|source| source_covers_text(source, characters))
                })
                .is_some_and(|(index, _)| consumed_text_entries.insert(index))
        })
        .count();
    let baked = baked_into_assets(snapshot, root_id);
    let variables = variable_sources(snapshot, &semantic_nodes)
        .into_iter()
        .filter(|(node_id, _)| !baked.contains(node_id))
        .collect::<BTreeSet<_>>();
    let covered_variables = variables
        .iter()
        .filter(|(node_id, variable_id)| {
            output.source_map.entries.iter().any(|entry| {
                entry.node_id.as_deref() == Some(node_id.as_str())
                    && entry.variable_id.as_deref() == Some(variable_id.as_str())
                    && entry.resolution == "variable-token"
                    && entry_range(entry, &output.tsx).is_some_and(|source| {
                        !source.contains(['<', '>', '\n'])
                            && output
                                .source_map
                                .variable_tokens
                                .get(variable_id)
                                .is_some_and(|token| {
                                    find_resource_id(
                                        source,
                                        &BTreeMap::from([(variable_id.clone(), token.clone())]),
                                        '$',
                                    )
                                    .as_ref()
                                        == Some(variable_id)
                                })
                            && source.ends_with('"')
                    })
            })
        })
        .count();
    let typography = typography_sources(snapshot, &text_nodes);
    let covered_typography = typography
        .iter()
        .filter(|(node_id, style_id)| {
            output.source_map.entries.iter().any(|entry| {
                entry.node_id.as_deref() == Some(*node_id)
                    && entry.style_id.as_deref() == Some(*style_id)
                    && entry.resolution == "style-token"
                    && entry_range(entry, &output.tsx).is_some_and(|source| {
                        output
                            .source_map
                            .style_tokens
                            .get(*style_id)
                            .is_some_and(|token| source == format!("typography=\"{token}\""))
                    })
            })
        })
        .count();
    let assets = discover_asset_manifest(snapshot)
        .assets
        .into_iter()
        .filter(|asset| semantic_nodes.contains(asset.node_id.as_str()))
        .map(|asset| (asset.node_id, asset.asset_id))
        .collect::<BTreeSet<_>>();
    let covered_assets = assets
        .iter()
        .filter(|(node_id, asset_id)| {
            if sizing::non_rendering_asset_accounted(snapshot, output, node_id) {
                return true;
            }
            output.source_map.entries.iter().any(|entry| {
                entry.node_id.as_deref() == Some(node_id.as_str())
                    && entry.asset_id.as_deref() == Some(asset_id.as_str())
                    && entry.resolution == "asset"
                    && entry_range(entry, &output.tsx).is_some_and(|source| {
                        asset_reference_matches(snapshot, output, entry, node_id, asset_id, source)
                    })
            })
        })
        .count();
    let asset_nodes = snapshot
        .nodes
        .values()
        .filter(|node| projects_as_asset(snapshot, node))
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    let parents = source_parents(snapshot);
    // Coverage is otherwise measured only over nodes that arrived. Count
    // declared holes too, unless an actual asset projection represents them.
    // Snapshot completeness still reports every missing operand in either case.
    let unrepresented_children = semantic_nodes
        .iter()
        .filter(|node_id| {
            !asset_nodes.contains(**node_id) && !has_asset_ancestor(node_id, &parents, &asset_nodes)
        })
        .filter_map(|node_id| snapshot.nodes.get(*node_id))
        .flat_map(|node| node.typed_view().child_ids().collect::<Vec<_>>())
        .filter(|child_id| !snapshot.nodes.contains_key(*child_id))
        .collect::<BTreeSet<_>>();
    let mut layout = semantic_nodes
        .iter()
        .filter(|node_id| !has_asset_ancestor(node_id, &parents, &asset_nodes))
        .flat_map(|node_id| {
            let is_asset = asset_nodes.contains(*node_id);
            LAYOUT_FIELDS.iter().filter_map(move |field| {
                snapshot
                    .nodes
                    .get(*node_id)
                    .filter(|node| {
                        (!is_asset || !asset_layout_field_is_internal(field))
                            && layout_field_is_semantic(snapshot, node, field)
                    })
                    .map(|_| ((*node_id).to_owned(), (*field).to_owned()))
            })
        })
        .collect::<BTreeSet<_>>();
    // Previously claimed implicit or explicit dimensions remain obligations when emitted CSS
    // is revalidated. Replacing a Box by an unknown/replaced tag must not make
    // its size disappear from coverage merely because it is no longer a Box.
    for entry in &output.source_map.entries {
        if matches!(
            entry.resolution.as_str(),
            "accounted-for-implicit-flex-stretch"
                | "accounted-for-implicit-flex-grow"
                | "accounted-for-content-sizing"
                | "verified-explicit-dimension"
        ) && let (Some(id), Some(field)) = (entry.node_id.as_deref(), entry.property.as_deref())
            && matches!(field, "width" | "height")
            && semantic_nodes.contains(id)
        {
            layout.insert((id.to_owned(), field.to_owned()));
        }
    }
    // A visible childless painted Box has no intrinsic content, even when it
    // was not classified as an asset (for example an empty HUG frame).
    for node_id in &semantic_nodes {
        let Some(node) = snapshot.nodes.get(*node_id) else {
            continue;
        };
        if devup_mcp_figma::asset_exclusion_reason(node).is_some() {
            continue;
        }
        let empty_box = empty_layout_leaf(output, node_id);
        if empty_box {
            for axis in ["width", "height"] {
                let view = node.typed_view();
                let sizing = if axis == "width" {
                    "layoutSizingHorizontal"
                } else {
                    "layoutSizingVertical"
                };
                if view
                    .value("absoluteRenderBounds")
                    .and_then(|b| b.get(axis))
                    .and_then(serde_json::Value::as_f64)
                    .is_some_and(|v| v > 0.0)
                    || (view.string(sizing) == Some("HUG")
                        && (view.number(axis).is_some_and(|v| v > 0.0)
                            || view
                                .value("absoluteBoundingBox")
                                .and_then(|b| b.get(axis))
                                .and_then(serde_json::Value::as_f64)
                                .is_some_and(|v| v > 0.0)))
                {
                    layout.insert(((*node_id).to_owned(), axis.to_owned()));
                }
            }
        }
    }
    let (covered_layout, uncovered_layout) = {
        let mut covered = 0usize;
        // Which pairs were not represented, not just how many. A count alone
        // cannot distinguish "the layout is wrong" from "the same layout is
        // expressed differently", so a shortfall was previously impossible to
        // act on or even to triage.
        let mut uncovered = Vec::new();
        for (node_id, property) in &layout {
            let policy_width = property == "width"
                && crate::codegen::width::policy_width_proof(snapshot, output, node_id).is_some();
            let represented = policy_width
                || output.source_map.entries.iter().any(|entry| {
                    entry.node_id.as_deref() == Some(node_id.as_str())
                        && entry.property.as_deref() == Some(property.as_str())
                        && entry_range(entry, &output.tsx).is_some_and(|source| {
                            if entry.resolution == "accounted-for-content-sizing" {
                                return sizing::content_sizing_accounted(
                                    snapshot, output, node_id, property,
                                );
                            }
                            if entry.resolution == "verified-explicit-dimension" {
                                return layout_source_matches(property, source)
                                    && dimension_value_matches(
                                        snapshot, output, node_id, property, source,
                                    )
                                    && snapshot.nodes.get(node_id).is_some_and(|node| {
                                        !node.field_errors.contains_key(property)
                                            && source
                                                .split_once("=\"")
                                                .and_then(|(_, v)| v.strip_suffix("px\""))
                                                .and_then(|v| v.parse::<f64>().ok())
                                                .zip(node.typed_view().number(property))
                                                .is_some_and(|(a, b)| b.is_finite() && a == b)
                                    });
                            }
                            if matches!(
                                entry.resolution.as_str(),
                                "accounted-for-implicit-flex-stretch"
                                    | "accounted-for-implicit-flex-grow"
                            ) {
                                return implicit_css_verification(
                                    snapshot, output, node_id, property,
                                )["state"]
                                    == "accounted-for";
                            }
                            if matches!(
                                property.as_str(),
                                "layoutSizingHorizontal" | "layoutSizingVertical" | "layoutGrow"
                            ) {
                                return sizing::sizing_mapping_matches(
                                    snapshot, output, node_id, property, source,
                                );
                            }
                            layout_source_matches(property, source)
                                && dimension_value_matches(
                                    snapshot, output, node_id, property, source,
                                )
                        })
                });
            if represented {
                covered += 1;
            } else if uncovered.len() < MAX_REPORTED_UNCOVERED {
                uncovered.push(format!("{node_id}#{property}"));
            }
        }
        (covered, uncovered)
    };
    let mut impacts = FidelityImpactCounts::default();
    for diagnostic in &output.diagnostics {
        let mut impact = diagnostic.fidelity_impact();
        if impact == FidelityImpact::None && diagnostic.code == "DEVUP_CODEGEN_ABSOLUTE_VERIFIED" {
            let components = absolute_component_verification(
                snapshot,
                output,
                diagnostic.node_id.as_deref().unwrap_or(root_id),
            );
            if ["height", "width", "horizontal", "vertical"]
                .iter()
                .any(|name| components[*name]["fidelityImpact"] != "none")
            {
                impact = FidelityImpact::Approximated;
            }
        }
        if impact == FidelityImpact::None && diagnostic.code == "DEVUP_CODEGEN_NON_RENDERING_ASSET"
        {
            let id = diagnostic.node_id.as_deref().unwrap_or(root_id);
            let hidden = snapshot.nodes.get(id).is_some_and(|node| {
                !node.field_errors.contains_key("visible")
                    && node.typed_view().bool("visible") == Some(false)
            });
            if !hidden && !sizing::non_rendering_asset_accounted(snapshot, output, id) {
                impact = FidelityImpact::Approximated;
            }
        }
        match impact {
            FidelityImpact::None => impacts.none += 1,
            FidelityImpact::Approximated => impacts.approximated += 1,
            FidelityImpact::Lossy => impacts.lossy += 1,
            FidelityImpact::Failed => impacts.failed += 1,
        }
    }
    impacts.lossy += unrepresented_children.len();
    Ok(FidelityReport {
        syntax_valid: true,
        nodes: FidelityCoverage::new(
            expected.len() + unrepresented_children.len(),
            observed.len(),
        ),
        uncovered_node_ids: unrepresented_children
            .into_iter()
            .take(MAX_REPORTED_UNCOVERED)
            .map(str::to_owned)
            .collect(),
        text: FidelityCoverage::new(text_segments.len(), covered_text),
        variables: FidelityCoverage::new(variables.len(), covered_variables),
        typography: FidelityCoverage::new(typography.len(), covered_typography),
        assets: FidelityCoverage::new(assets.len(), covered_assets),
        layout: FidelityCoverage::new(layout.len(), covered_layout),
        impacts,
        uncovered_layout,
    })
}

fn has_asset_ancestor(
    node_id: &str,
    parents: &BTreeMap<String, String>,
    asset_nodes: &BTreeSet<String>,
) -> bool {
    let mut parent = parents.get(node_id).map(String::as_str);
    while let Some(parent_id) = parent {
        if asset_nodes.contains(parent_id) {
            return true;
        }
        parent = parents.get(parent_id).map(String::as_str);
    }
    false
}

fn asset_layout_field_is_internal(field: &str) -> bool {
    matches!(
        field,
        "layoutMode"
            | "itemSpacing"
            | "paddingTop"
            | "paddingRight"
            | "paddingBottom"
            | "paddingLeft"
    )
}

fn layout_field_is_semantic(
    snapshot: &Snapshot,
    node: &devup_mcp_figma::RawNode,
    field: &str,
) -> bool {
    let view = node.typed_view();
    // Asset internals may be baked into the export, but the CSS element's
    // two axes are never internal. Check before HUG/canvas/padding exemptions.
    if matches!(field, "width" | "height")
        && projects_as_asset(snapshot, node)
        && devup_mcp_figma::asset_exclusion_reason(node).is_none()
    {
        return view.number(field).is_some_and(|v| v > 0.0)
            || view
                .value("absoluteBoundingBox")
                .and_then(|b| b.get(field))
                .and_then(serde_json::Value::as_f64)
                .is_some_and(|v| v > 0.0)
            || view
                .value("absoluteRenderBounds")
                .and_then(|b| b.get(field))
                .and_then(serde_json::Value::as_f64)
                .is_some_and(|v| v > 0.0);
    }
    let component_set_parent = view
        .string("parentId")
        .and_then(|parent_id| snapshot.nodes.get(parent_id))
        .is_some_and(|parent| parent.node_type == "COMPONENT_SET")
        || snapshot.nodes.values().any(|parent| {
            parent.node_type == "COMPONENT_SET"
                && parent
                    .typed_view()
                    .child_ids()
                    .any(|child| child == node.id)
        });
    // A frame sitting on a page or section is the canvas the design was drawn
    // on, and its own dimensions are deliberately left unsaid so the result is
    // not pinned to that size. Counting them would report a shortfall for
    // something the output declines to claim on purpose. Kept in step with the
    // same test in `codegen::layout`.
    let canvas_parent = component_set_parent
        || view
            .string("parentId")
            .and_then(|parent_id| snapshot.nodes.get(parent_id))
            .map(|parent| parent.node_type.as_str())
            .or_else(|| view.string("parentType"))
            .is_some_and(|kind| matches!(kind, "SECTION" | "PAGE" | "COMPONENT_SET"));
    if matches!(field, "width" | "height") && canvas_parent {
        return false;
    }
    // An out-of-flow node whose children's inset became padding takes its size
    // from that padding plus its content, so the size is not restated and
    // counting it would report a shortfall for something said another way.
    // Kept in step with the same test in `codegen::layout`.
    if matches!(field, "width" | "height")
        && view.string("layoutPositioning") == Some("ABSOLUTE")
        && derived_padding(snapshot, node).is_some()
    {
        return false;
    }
    // An out-of-flow node that holds something takes its height from what it
    // holds, and `codegen::layout` drops it for exactly that reason. Counting
    // it here reported a shortfall against a value the converter is right not
    // to state: a header pinned across the top of a screen came back as
    // unaccounted-for height, and the reference implementation does not state
    // it either.
    let placed_out_of_flow = view.string("layoutPositioning") == Some("ABSOLUTE")
        || placed_by_a_free_layout(
            snapshot,
            node,
            view.string("parentId")
                .and_then(|parent_id| snapshot.nodes.get(parent_id)),
            canvas_parent,
        );
    if field == "height"
        && placed_out_of_flow
        && view.child_ids().next().is_some()
        && !projects_as_asset(snapshot, node)
    {
        return false;
    }
    match field {
        "layoutSizingHorizontal" => view.string(field) == Some("FILL"),
        "layoutSizingVertical" => {
            view.string(field) == Some("FILL")
                || crate::codegen::vertical_fill_container(snapshot, node)
        }
        // Growth is shared out by an auto-layout parent, and the code has one
        // only where that parent is in it. A root's parent is outside the
        // export, and a frame without auto-layout - or a pinned child - takes
        // no part in it: the value is inert in Figma and has nothing to say in
        // the code. An instance placed on the canvas keeps the 1 its component
        // had in a row, and that alone graded a standalone StatusCard lossy.
        // Kept in step with `codegen::layout`, which writes `flex` only there.
        "layoutGrow" => {
            view.number(field).is_some_and(|v| v > 0.0)
                && view.string("layoutPositioning") != Some("ABSOLUTE")
                && view
                    .string("parentId")
                    .and_then(|parent_id| snapshot.nodes.get(parent_id))
                    .is_some_and(|parent| {
                        matches!(
                            parent.typed_view().string("layoutMode"),
                            Some("HORIZONTAL" | "VERTICAL")
                        )
                    })
        }
        "layoutMode" => matches!(view.string(field), Some("HORIZONTAL" | "VERTICAL" | "GRID")),
        "layoutPositioning" => view.string(field) == Some("ABSOLUTE"),
        "width" => {
            view.value(field).is_some()
                && view
                    .string("layoutSizingHorizontal")
                    .is_none_or(|value| value == "FIXED")
        }
        "height" => {
            view.value(field).is_some()
                && view
                    .string("layoutSizingVertical")
                    .is_none_or(|value| value == "FIXED")
        }
        "itemSpacing" => {
            // Spacing describes the distance between rendered siblings, so a
            // hidden child leaves nothing to space apart and the generated code
            // rightly omits the gap. Counting it here would report a shortfall
            // for a fact that was deliberately not expressed. Kept in step with
            // the same test in `codegen::layout`.
            let visible_children = view
                .child_ids()
                .filter_map(|id| snapshot.nodes.get(id))
                .filter(|child| child.typed_view().bool("visible") != Some(false))
                .count();
            visible_children > 1
                && view.string("primaryAxisAlignItems") != Some("SPACE_BETWEEN")
                && !projects_as_asset(snapshot, node)
                && view.number(field).is_some_and(|value| value != 0.0)
        }
        "paddingTop" | "paddingRight" | "paddingBottom" | "paddingLeft" => {
            view.number(field).is_some_and(|value| value != 0.0)
        }
        _ => false,
    }
}

fn projects_as_asset(snapshot: &Snapshot, node: &devup_mcp_figma::RawNode) -> bool {
    asset_kind(snapshot, node).is_some()
}

fn semantic_nodes<'a>(snapshot: &'a Snapshot, root_id: &str) -> BTreeSet<&'a str> {
    let mut visible = BTreeSet::new();
    let mut hidden = BTreeSet::new();
    for (node_id, parent_id) in active_nodes(snapshot, root_id) {
        let is_hidden = parent_id
            .as_ref()
            .is_some_and(|parent_id| hidden.contains(parent_id.as_str()))
            || snapshot
                .nodes
                .get(&node_id)
                .is_some_and(|node| node.typed_view().bool("visible") == Some(false));
        if is_hidden {
            hidden.insert(node_id);
        } else if let Some((node_id, _)) = snapshot.nodes.get_key_value(&node_id) {
            visible.insert(node_id.as_str());
        }
    }
    visible
}