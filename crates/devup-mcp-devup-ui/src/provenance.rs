use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::{DevupError, ErrorCode, FidelityImpact, Snapshot, discover_asset_manifest};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::codegen::{CodegenOutput, asset_kind};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_range: Option<GeneratedRange>,
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMap {
    pub version: u32,
    pub entries: Vec<ProvenanceEntry>,
}

impl SourceMap {
    pub fn empty() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
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
    let variables = variable_sources(snapshot, &semantic_nodes);
    let covered_variables = variables
        .iter()
        .filter(|(node_id, variable_id)| {
            output.source_map.entries.iter().any(|entry| {
                entry.node_id.as_deref() == Some(node_id.as_str())
                    && entry.variable_id.as_deref() == Some(variable_id.as_str())
                    && entry.resolution == "variable-token"
                    && entry_range(entry, &output.tsx).is_some_and(|source| {
                        !source.contains(['<', '>', '\n'])
                            && source.contains('$')
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
                    && entry_range(entry, &output.tsx)
                        .is_some_and(|source| source.starts_with("typography=\""))
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
            output.source_map.entries.iter().any(|entry| {
                entry.node_id.as_deref() == Some(node_id.as_str())
                    && entry.asset_id.as_deref() == Some(asset_id.as_str())
                    && entry.resolution == "asset"
                    && entry_range(entry, &output.tsx).is_some_and(|source| {
                        source.starts_with("src=\"")
                            || source.starts_with("maskImage=\"")
                            || (source.starts_with("bg=\"") && source.contains("url("))
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
    let layout = semantic_nodes
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
    let (covered_layout, uncovered_layout) = {
        let mut covered = 0usize;
        // Which pairs were not represented, not just how many. A count alone
        // cannot distinguish "the layout is wrong" from "the same layout is
        // expressed differently", so a shortfall was previously impossible to
        // act on or even to triage.
        let mut uncovered = Vec::new();
        for (node_id, property) in &layout {
            let represented = output.source_map.entries.iter().any(|entry| {
                entry.node_id.as_deref() == Some(node_id.as_str())
                    && entry.property.as_deref() == Some(property.as_str())
                    && entry_range(entry, &output.tsx)
                        .is_some_and(|source| layout_source_matches(property, source))
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
        match diagnostic.fidelity_impact() {
            FidelityImpact::None => impacts.none += 1,
            FidelityImpact::Approximated => impacts.approximated += 1,
            FidelityImpact::Lossy => impacts.lossy += 1,
            FidelityImpact::Failed => impacts.failed += 1,
        }
    }
    Ok(FidelityReport {
        syntax_valid: true,
        nodes: FidelityCoverage::new(expected.len(), observed.len()),
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
    match field {
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

fn variable_sources(
    snapshot: &Snapshot,
    semantic_nodes: &BTreeSet<&str>,
) -> BTreeSet<(String, String)> {
    fn scan(node_id: &str, value: &serde_json::Value, output: &mut BTreeSet<(String, String)>) {
        match value {
            serde_json::Value::Object(object) => {
                if object.get("type").and_then(serde_json::Value::as_str) == Some("VARIABLE_ALIAS")
                    && let Some(id) = object
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty() && *id != "figma.mixed" && *id != "MIXED")
                {
                    output.insert((node_id.to_owned(), id.to_owned()));
                }
                for child in object.values() {
                    scan(node_id, child, output);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    scan(node_id, child, output);
                }
            }
            _ => {}
        }
    }

    let mut output = BTreeSet::new();
    for node_id in semantic_nodes {
        if let Some(node) = snapshot.nodes.get(*node_id) {
            for value in node.fields.values().chain(node.extra.values()) {
                scan(node_id, value, &mut output);
            }
        }
    }
    output
}

fn text_sources(snapshot: &Snapshot, text_nodes: &BTreeSet<&str>) -> Vec<(String, String)> {
    let mut output = Vec::new();
    for node_id in text_nodes {
        let Some(node) = snapshot.nodes.get(*node_id) else {
            continue;
        };
        let segments = node
            .typed_view()
            .value("styledTextSegments")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|segment| {
                segment
                    .get("characters")
                    .and_then(serde_json::Value::as_str)
            })
            .filter(|characters| !characters.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if segments.is_empty() {
            if let Some(characters) = node
                .typed_view()
                .string("characters")
                .filter(|characters| !characters.is_empty())
            {
                output.push(((*node_id).to_owned(), characters.to_owned()));
            }
        } else {
            output.extend(
                segments
                    .into_iter()
                    .map(|characters| ((*node_id).to_owned(), characters)),
            );
        }
    }
    output
}

fn entry_range<'a>(entry: &ProvenanceEntry, tsx: &'a str) -> Option<&'a str> {
    let range = entry.generated_range.as_ref()?;
    (range.start < range.end
        && range.end <= tsx.len()
        && tsx.is_char_boundary(range.start)
        && tsx.is_char_boundary(range.end))
    .then(|| &tsx[range.start..range.end])
}

fn source_covers_text(source: &str, characters: &str) -> bool {
    if source == encode_jsx_text(characters) {
        return true;
    }
    let fragments = characters
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|fragment| !fragment.is_empty());
    let mut cursor = 0;
    for fragment in fragments {
        let encoded = encode_jsx_text(fragment);
        let Some(found) = source[cursor..].find(&encoded) else {
            return false;
        };
        cursor += found + encoded.len();
    }
    cursor > 0 && cursor == source.len()
}

fn encode_jsx_text(input: &str) -> String {
    let leading = input
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    let trailing = input
        .chars()
        .rev()
        .take_while(|character| *character == ' ')
        .count();
    let middle_end = input.len().saturating_sub(trailing);
    let middle = &input[leading..middle_end];
    let mut result = String::new();
    if leading > 0 {
        result.push_str(&format!("{{\"{}\"}}", " ".repeat(leading)));
    }
    let mut characters = middle.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '{' => result.push_str("{\"{\"}"),
            '}' => result.push_str("{\"}\"}"),
            '&' => result.push_str("{\"&\"}"),
            '<' => result.push_str("{\"<\"}"),
            '>' => result.push_str("{\">\"}"),
            '\'' => result.push_str("{\"'\"}"),
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                if characters.peek().is_none() {
                    result.push_str("{\" \"}");
                } else {
                    result.push_str("<br />");
                }
            }
            '\n' => {
                if characters.peek().is_none() {
                    result.push_str("{\" \"}");
                } else {
                    result.push_str("<br />");
                }
            }
            value => result.push(value),
        }
    }
    if trailing > 0 {
        result.push_str(&format!("{{\"{}\"}}", " ".repeat(trailing)));
    }
    result
}

fn layout_source_matches(property: &str, source: &str) -> bool {
    let is_prop = |name: &str| {
        source.starts_with(&format!("{name}=\""))
            || source.starts_with(&format!("{name}={{"))
            || source.starts_with(&format!("\"{name}\":"))
    };
    match property {
        "layoutMode" => {
            is_prop("flexDir") || matches!(source, "VStack" | "Flex" | "Grid" | "Center")
        }
        "layoutPositioning" => is_prop("pos"),
        "width" => is_prop("w") || is_prop("boxSize") || is_prop("aspectRatio"),
        "height" => is_prop("h") || is_prop("boxSize"),
        "itemSpacing" => is_prop("gap") || is_prop("m"),
        "paddingTop" => is_prop("p") || is_prop("py") || is_prop("pt"),
        "paddingRight" => is_prop("p") || is_prop("px") || is_prop("pr"),
        "paddingBottom" => is_prop("p") || is_prop("py") || is_prop("pb"),
        "paddingLeft" => is_prop("p") || is_prop("px") || is_prop("pl"),
        _ => false,
    }
}

fn active_nodes(snapshot: &Snapshot, root_id: &str) -> Vec<(String, Option<String>)> {
    let mut output = Vec::new();
    let Some(root) = snapshot.nodes.get(root_id) else {
        return output;
    };
    output.push((root_id.to_owned(), None));
    let start = if root.node_type == "SECTION" {
        root.typed_view().child_ids().next()
    } else {
        Some(root_id)
    };
    let Some(start) = start else {
        return output;
    };
    let mut stack = if start == root_id {
        let mut children = root
            .typed_view()
            .child_ids()
            .map(|child| (child.to_owned(), Some(root_id.to_owned())))
            .collect::<Vec<_>>();
        children.reverse();
        children
    } else {
        vec![(start.to_owned(), Some(root_id.to_owned()))]
    };
    let mut seen = BTreeSet::from([root_id.to_owned()]);
    while let Some((node_id, parent_id)) = stack.pop() {
        if !seen.insert(node_id.clone()) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(&node_id) else {
            continue;
        };
        output.push((node_id.clone(), parent_id));
        let mut children = node.typed_view().child_ids().collect::<Vec<_>>();
        children.reverse();
        for child_id in children {
            stack.push((child_id.to_owned(), Some(node_id.clone())));
        }
    }
    output
}

fn typography_sources<'a>(
    snapshot: &'a Snapshot,
    text_nodes: &BTreeSet<&'a str>,
) -> BTreeSet<(&'a str, &'a str)> {
    let mut styles = BTreeSet::new();
    for node_id in text_nodes {
        let Some(node) = snapshot.nodes.get(*node_id) else {
            continue;
        };
        if let Some(style_id) = node
            .typed_view()
            .string("textStyleId")
            .filter(|id| !id.is_empty())
        {
            styles.insert((*node_id, style_id));
        }
        if let Some(segments) = node
            .typed_view()
            .value("styledTextSegments")
            .and_then(serde_json::Value::as_array)
        {
            for style_id in segments.iter().filter_map(|segment| {
                segment
                    .get("textStyleId")
                    .and_then(serde_json::Value::as_str)
                    .filter(|id| !id.is_empty())
            }) {
                styles.insert((*node_id, style_id));
            }
        }
    }
    styles
}

const LAYOUT_FIELDS: &[&str] = &[
    "layoutMode",
    "layoutPositioning",
    "width",
    "height",
    "itemSpacing",
    "paddingTop",
    "paddingRight",
    "paddingBottom",
    "paddingLeft",
];

pub(crate) fn mark_node(node_id: &str, rendered: String) -> String {
    format!("{START}{node_id}{CLOSE}{rendered}{END}{node_id}{CLOSE}")
}

pub(crate) fn finalize_tsx(
    marked: &str,
    snapshot: &Snapshot,
    variable_tokens: &BTreeMap<String, String>,
    style_tokens: &BTreeMap<String, String>,
) -> (String, SourceMap) {
    let (tsx, node_ranges) = strip_markers(marked);
    let emitted_ranges = node_ranges
        .iter()
        .cloned()
        .collect::<BTreeMap<String, GeneratedRange>>();
    let mut assets_by_node = BTreeMap::<String, Vec<_>>::new();
    for asset in discover_asset_manifest(snapshot).assets {
        assets_by_node
            .entry(asset.node_id.clone())
            .or_default()
            .push(asset);
    }
    let mut entries = Vec::new();
    for (node_id, range) in node_ranges {
        let Some(node) = snapshot.nodes.get(&node_id) else {
            continue;
        };
        entries.push(ProvenanceEntry {
            generated_range: Some(range.clone()),
            json_pointer: None,
            node_id: Some(node_id.clone()),
            property: None,
            variable_id: None,
            style_id: None,
            asset_id: None,
            resolution: "node".to_owned(),
        });
        if node.node_type == "COMPONENT_SET" {
            continue;
        }
        let node_source = &tsx[range.start..range.end];
        let Some(open_relative) = node_source.find('<') else {
            continue;
        };
        let Some(close_relative) = node_source[open_relative..].find('>') else {
            continue;
        };
        let opening_end = open_relative + close_relative + 1;
        let opening = &node_source[open_relative..opening_end];
        let component_start = open_relative + 1;
        let component_end = opening[1..]
            .find(|character: char| character.is_whitespace() || matches!(character, '>' | '/'))
            .map_or(open_relative + opening.len(), |offset| {
                open_relative + offset + 1
            });
        if component_end > component_start {
            entries.push(generated_entry(
                range.start + component_start,
                range.start + component_end,
                &node_id,
                "type",
                None,
                None,
                "exact",
            ));
            if matches!(
                node.typed_view().string("layoutMode"),
                Some("HORIZONTAL" | "VERTICAL" | "GRID")
            ) {
                entries.push(generated_entry(
                    range.start + component_start,
                    range.start + component_end,
                    &node_id,
                    "layoutMode",
                    None,
                    None,
                    "raw-fallback",
                ));
            }
        }

        let mut selector_properties = BTreeSet::new();
        if let Some(selector) = non_default_variant_selector(snapshot, node) {
            for (prop, property) in PROP_SOURCES {
                let Some((start, end)) = selector_prop_range(opening, &selector, prop) else {
                    continue;
                };
                selector_properties.insert(*property);
                entries.push(generated_entry(
                    range.start + open_relative + start,
                    range.start + open_relative + end,
                    &node_id,
                    property,
                    None,
                    None,
                    "variant-selector",
                ));
            }
        }

        for (prop, property) in PROP_SOURCES {
            if selector_properties.contains(property) {
                continue;
            }
            let needle = format!("{prop}=\"");
            let Some(start) = find_prop(opening, &needle) else {
                let expression = format!("{prop}={{");
                if let Some(start) = find_prop(opening, &expression) {
                    entries.push(generated_entry(
                        range.start + open_relative + start,
                        range.start + open_relative + start + expression.len(),
                        &node_id,
                        property,
                        None,
                        None,
                        "raw-fallback",
                    ));
                }
                continue;
            };
            let value_start = start + needle.len();
            let Some(value_end) = opening[value_start..].find('"') else {
                continue;
            };
            let end = value_start + value_end + 1;
            let value = &opening[value_start..value_start + value_end];
            let variable_id = find_resource_id(value, variable_tokens, '$');
            let style_id = (*prop == "typography")
                .then(|| {
                    style_tokens
                        .iter()
                        .find_map(|(id, token)| (token == value).then(|| id.clone()))
                })
                .flatten();
            let resolution = if variable_id.is_some() {
                "variable-token"
            } else if style_id.is_some() {
                "style-token"
            } else {
                "raw-fallback"
            };
            entries.push(generated_entry(
                range.start + open_relative + start,
                range.start + open_relative + end,
                &node_id,
                property,
                variable_id,
                style_id,
                resolution,
            ));
        }

        if let Some(asset) = assets_by_node
            .get(&node_id)
            .and_then(|assets| assets.first())
            && let Some((start, end)) = asset_prop_range(opening)
        {
            entries.push(ProvenanceEntry {
                generated_range: Some(GeneratedRange {
                    start: range.start + open_relative + start,
                    end: range.start + open_relative + end,
                }),
                json_pointer: None,
                node_id: Some(node_id.clone()),
                property: Some(asset.field.clone()),
                variable_id: None,
                style_id: None,
                asset_id: Some(asset.asset_id.clone()),
                resolution: "asset".to_owned(),
            });
        }

        if node.node_type == "TEXT" {
            add_text_entries(
                &tsx,
                &range,
                &node_id,
                node,
                variable_tokens,
                style_tokens,
                &mut entries,
            );
        }
    }
    add_flattened_resource_entries(
        &tsx,
        snapshot,
        &emitted_ranges,
        variable_tokens,
        &assets_by_node,
        &mut entries,
    );
    entries.sort_by(|left, right| {
        let left_range = left.generated_range.as_ref();
        let right_range = right.generated_range.as_ref();
        left_range
            .map(|range| (range.start, range.end))
            .cmp(&right_range.map(|range| (range.start, range.end)))
            .then_with(|| left.node_id.cmp(&right.node_id))
            .then_with(|| left.property.cmp(&right.property))
    });
    (
        tsx,
        SourceMap {
            version: 1,
            entries,
        },
    )
}

fn add_flattened_resource_entries(
    tsx: &str,
    snapshot: &Snapshot,
    emitted_ranges: &BTreeMap<String, GeneratedRange>,
    variable_tokens: &BTreeMap<String, String>,
    assets_by_node: &BTreeMap<String, Vec<devup_mcp_figma::AssetManifestEntry>>,
    entries: &mut Vec<ProvenanceEntry>,
) {
    let parents = source_parents(snapshot);
    let all_nodes = snapshot
        .nodes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for (node_id, variable_id) in variable_sources(snapshot, &all_nodes) {
        if entries.iter().any(|entry| {
            entry.node_id.as_deref() == Some(node_id.as_str())
                && entry.variable_id.as_deref() == Some(variable_id.as_str())
                && entry.resolution == "variable-token"
        }) {
            continue;
        }
        let Some(token) = variable_tokens.get(&variable_id) else {
            continue;
        };
        let needle = format!("${token}");
        let mut represented_by = Some(node_id.as_str());
        while let Some(candidate_id) = represented_by {
            if let Some(range) = emitted_ranges.get(candidate_id) {
                let source = &tsx[range.start..range.end];
                if let Some((start, end)) = token_prop_range(source, &needle) {
                    entries.push(generated_entry(
                        range.start + start,
                        range.start + end,
                        &node_id,
                        variable_property(snapshot, &node_id, &variable_id),
                        Some(variable_id.clone()),
                        None,
                        "variable-token",
                    ));
                    break;
                }
            }
            represented_by = parents.get(candidate_id).map(String::as_str);
        }
    }

    for (node_id, assets) in assets_by_node {
        for asset in assets {
            if entries.iter().any(|entry| {
                entry.node_id.as_deref() == Some(node_id.as_str())
                    && entry.asset_id.as_deref() == Some(asset.asset_id.as_str())
                    && entry.resolution == "asset"
            }) {
                continue;
            }
            let mut represented_by = parents.get(node_id.as_str()).map(String::as_str);
            while let Some(candidate_id) = represented_by {
                if let Some(range) = emitted_ranges.get(candidate_id) {
                    let source = &tsx[range.start..range.end];
                    if let Some((start, end)) = asset_range_in_node_source(source) {
                        entries.push(ProvenanceEntry {
                            generated_range: Some(GeneratedRange {
                                start: range.start + start,
                                end: range.start + end,
                            }),
                            json_pointer: None,
                            node_id: Some(node_id.clone()),
                            property: Some(asset.field.clone()),
                            variable_id: None,
                            style_id: None,
                            asset_id: Some(asset.asset_id.clone()),
                            resolution: "asset".to_owned(),
                        });
                        break;
                    }
                }
                represented_by = parents.get(candidate_id).map(String::as_str);
            }
        }
    }
}

fn source_parents(snapshot: &Snapshot) -> BTreeMap<String, String> {
    let mut parents = BTreeMap::new();
    for node in snapshot.nodes.values() {
        for child_id in node.typed_view().child_ids() {
            parents
                .entry(child_id.to_owned())
                .or_insert_with(|| node.id.clone());
        }
    }
    for node in snapshot.nodes.values() {
        if let Some(parent_id) = node.typed_view().string("parentId") {
            parents
                .entry(node.id.clone())
                .or_insert_with(|| parent_id.to_owned());
        }
    }
    parents
}

fn variable_property<'a>(snapshot: &'a Snapshot, node_id: &str, variable_id: &str) -> &'a str {
    fn contains_alias(value: &serde_json::Value, variable_id: &str) -> bool {
        match value {
            serde_json::Value::Object(object) => {
                (object.get("type").and_then(serde_json::Value::as_str) == Some("VARIABLE_ALIAS")
                    && object.get("id").and_then(serde_json::Value::as_str) == Some(variable_id))
                    || object
                        .values()
                        .any(|value| contains_alias(value, variable_id))
            }
            serde_json::Value::Array(values) => values
                .iter()
                .any(|value| contains_alias(value, variable_id)),
            _ => false,
        }
    }

    let Some(node) = snapshot.nodes.get(node_id) else {
        return "boundVariables";
    };
    for field in ["fills", "strokes", "effects", "boundVariables"] {
        if node
            .typed_view()
            .value(field)
            .is_some_and(|value| contains_alias(value, variable_id))
        {
            return field;
        }
    }
    node.fields
        .iter()
        .chain(&node.extra)
        .find_map(|(field, value)| contains_alias(value, variable_id).then_some(field.as_str()))
        .unwrap_or("boundVariables")
}

fn non_default_variant_selector(
    snapshot: &Snapshot,
    node: &devup_mcp_figma::RawNode,
) -> Option<String> {
    let parent = snapshot.nodes.get(node.typed_view().string("parentId")?)?;
    if parent.node_type != "COMPONENT_SET" {
        return None;
    }
    let definitions = parent
        .typed_view()
        .value("componentPropertyDefinitions")?
        .as_object()?;
    let (effect_name, definition) = definitions.iter().find(|(name, definition)| {
        name.eq_ignore_ascii_case("effect")
            && definition.get("type").and_then(serde_json::Value::as_str) == Some("VARIANT")
    })?;
    let default = definition
        .get("defaultValue")
        .and_then(serde_json::Value::as_str)?;
    let value = node
        .typed_view()
        .value("variantProperties")?
        .get(effect_name)?
        .as_str()?;
    (value != default).then(|| format!("_{value}"))
}

fn selector_prop_range(opening: &str, selector: &str, prop: &str) -> Option<(usize, usize)> {
    let selector_needle = format!("{selector}={{{{");
    let selector_start = find_prop(opening, &selector_needle)?;
    let block = &opening[selector_start..];
    let mut depth = 0_u32;
    let mut quoted = false;
    let mut escaped = false;
    let mut block_end = None;
    for (offset, character) in block.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => quoted = true,
            '{' => depth += 1,
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    block_end = Some(offset + character.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }
    let block = &block[..block_end?];
    let needle = format!("\"{prop}\":");
    let relative = block.find(&needle)?;
    Some((
        selector_start + relative,
        selector_start + relative + needle.len(),
    ))
}

const PROP_SOURCES: &[(&str, &str)] = &[
    ("alignContent", "textAlignVertical"),
    ("alignItems", "counterAxisAlignItems"),
    ("aspectRatio", "targetAspectRatio"),
    ("bg", "fills"),
    ("border", "strokes"),
    ("borderBottom", "strokes"),
    ("borderLeft", "strokes"),
    ("borderRight", "strokes"),
    ("borderTop", "strokes"),
    ("borderRadius", "cornerRadius"),
    ("bottom", "y"),
    ("boxSize", "width"),
    ("boxSize", "height"),
    ("color", "fills"),
    ("flex", "layoutGrow"),
    ("flexDir", "layoutMode"),
    ("fontFamily", "fontName"),
    ("fontSize", "fontSize"),
    ("fontStyle", "fontName"),
    ("fontWeight", "fontWeight"),
    ("gap", "itemSpacing"),
    ("h", "height"),
    ("justifyContent", "primaryAxisAlignItems"),
    ("left", "x"),
    ("letterSpacing", "letterSpacing"),
    ("lineHeight", "lineHeight"),
    ("m", "itemSpacing"),
    ("maxH", "maxHeight"),
    ("maxW", "maxWidth"),
    ("minH", "minHeight"),
    ("minW", "minWidth"),
    ("opacity", "opacity"),
    ("overflow", "clipsContent"),
    ("p", "paddingTop"),
    ("p", "paddingRight"),
    ("p", "paddingBottom"),
    ("p", "paddingLeft"),
    ("pb", "paddingBottom"),
    ("pl", "paddingLeft"),
    ("pos", "layoutPositioning"),
    ("pr", "paddingRight"),
    ("pt", "paddingTop"),
    ("px", "paddingLeft"),
    ("px", "paddingRight"),
    ("py", "paddingTop"),
    ("py", "paddingBottom"),
    ("right", "x"),
    ("textAlign", "textAlignHorizontal"),
    ("top", "y"),
    ("typography", "textStyleId"),
    ("w", "width"),
    ("whiteSpace", "textAutoResize"),
    ("wordBreak", "characters"),
];

fn add_text_entries(
    tsx: &str,
    range: &GeneratedRange,
    node_id: &str,
    node: &devup_mcp_figma::RawNode,
    variable_tokens: &BTreeMap<String, String>,
    style_tokens: &BTreeMap<String, String>,
    entries: &mut Vec<ProvenanceEntry>,
) {
    let source = &tsx[range.start..range.end];
    let view = node.typed_view();
    let mut text_segments = view
        .value("styledTextSegments")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|segment| {
            segment
                .get("characters")
                .and_then(serde_json::Value::as_str)
        })
        .filter(|characters| !characters.is_empty())
        .collect::<Vec<_>>();
    if text_segments.is_empty()
        && let Some(characters) = view
            .string("characters")
            .filter(|characters| !characters.is_empty())
    {
        text_segments.push(characters);
    }
    let mut cursor = 0;
    for characters in text_segments {
        if let Some((start, end)) = find_text_span(source, characters, cursor) {
            entries.push(generated_entry(
                range.start + start,
                range.start + end,
                node_id,
                "characters",
                None,
                None,
                "exact",
            ));
            cursor = end;
        }
    }
    for (variable_id, token) in variable_tokens {
        let needle = format!("${token}");
        for start in match_indices(source, &needle) {
            let prop_start = source[..start]
                .rfind(|character: char| character.is_whitespace() || character == '<')
                .map_or(start, |offset| offset + 1);
            let prop_end = source[start..]
                .find('"')
                .map_or(start + needle.len(), |offset| start + offset + 1);
            entries.push(generated_entry(
                range.start + prop_start,
                range.start + prop_end,
                node_id,
                "styledTextSegments",
                Some(variable_id.clone()),
                None,
                "variable-token",
            ));
        }
    }
    for (style_id, token) in style_tokens {
        let needle = format!("typography=\"{token}\"");
        for start in match_indices(source, &needle) {
            entries.push(generated_entry(
                range.start + start,
                range.start + start + needle.len(),
                node_id,
                "styledTextSegments",
                None,
                Some(style_id.clone()),
                "style-token",
            ));
        }
    }
}

fn find_text_span(source: &str, characters: &str, search_start: usize) -> Option<(usize, usize)> {
    let rendered = encode_jsx_text(characters);
    if let Some(start) = source[search_start..].find(&rendered) {
        let start = search_start + start;
        return Some((start, start + rendered.len()));
    }
    let fragments = characters
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|fragment| !fragment.is_empty())
        .map(encode_jsx_text)
        .collect::<Vec<_>>();
    let mut cursor = search_start;
    let mut start = None;
    let mut end = None;
    for fragment in fragments {
        let found = source[cursor..].find(&fragment)? + cursor;
        start.get_or_insert(found);
        end = Some(found + fragment.len());
        cursor = found + fragment.len();
    }
    Some((start?, end?))
}

fn asset_prop_range(opening: &str) -> Option<(usize, usize)> {
    for prop in ["src", "maskImage", "bg"] {
        let needle = format!("{prop}=\"");
        let Some(start) = find_prop(opening, &needle) else {
            continue;
        };
        let value_start = start + needle.len();
        let Some(value_end) = opening[value_start..].find('"') else {
            continue;
        };
        let end = value_start + value_end + 1;
        if prop != "bg" || opening[value_start..value_start + value_end].contains("url(") {
            return Some((start, end));
        }
    }
    None
}

fn asset_range_in_node_source(source: &str) -> Option<(usize, usize)> {
    let opening_start = source.find('<')?;
    let opening_end = opening_start + source[opening_start..].find('>')? + 1;
    let (start, end) = asset_prop_range(&source[opening_start..opening_end])?;
    Some((opening_start + start, opening_start + end))
}

fn token_prop_range(source: &str, needle: &str) -> Option<(usize, usize)> {
    let token_start = source.find(needle)?;
    let prop_start = source[..token_start]
        .rfind(|character: char| character.is_whitespace() || character == '<')
        .map_or(token_start, |offset| offset + 1);
    let prop_end = source[token_start..]
        .find('"')
        .map_or(token_start + needle.len(), |offset| {
            token_start + offset + 1
        });
    Some((prop_start, prop_end))
}

fn find_resource_id(
    value: &str,
    tokens: &BTreeMap<String, String>,
    prefix: char,
) -> Option<String> {
    tokens.iter().find_map(|(id, token)| {
        let needle = format!("{prefix}{token}");
        match_indices(value, &needle)
            .into_iter()
            .any(|start| {
                value[start + needle.len()..]
                    .chars()
                    .next()
                    .is_none_or(|character| !is_token_character(character))
            })
            .then(|| id.clone())
    })
}

fn find_prop(opening: &str, needle: &str) -> Option<usize> {
    match_indices(opening, needle).into_iter().find(|start| {
        *start > 0
            && opening[..*start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
    })
}

fn is_token_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-')
}

fn match_indices(source: &str, needle: &str) -> Vec<usize> {
    let mut result = Vec::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(needle) {
        let absolute = offset + found;
        result.push(absolute);
        offset = absolute + needle.len();
    }
    result
}

fn generated_entry(
    start: usize,
    end: usize,
    node_id: &str,
    property: &str,
    variable_id: Option<String>,
    style_id: Option<String>,
    resolution: &str,
) -> ProvenanceEntry {
    ProvenanceEntry {
        generated_range: Some(GeneratedRange { start, end }),
        json_pointer: None,
        node_id: Some(node_id.to_owned()),
        property: Some(property.to_owned()),
        variable_id,
        style_id,
        asset_id: None,
        resolution: resolution.to_owned(),
    }
}

fn strip_markers(marked: &str) -> (String, Vec<(String, GeneratedRange)>) {
    let mut output = String::with_capacity(marked.len());
    let mut stack = Vec::<(String, usize)>::new();
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < marked.len() {
        let rest = &marked[cursor..];
        let next_start = rest.find(START);
        let next_end = rest.find(END);
        let next = match (next_start, next_end) {
            (Some(start), Some(end)) => start.min(end),
            (Some(start), None) => start,
            (None, Some(end)) => end,
            (None, None) => {
                output.push_str(rest);
                break;
            }
        };
        output.push_str(&rest[..next]);
        cursor += next;
        let is_start = marked[cursor..].starts_with(START);
        let prefix = if is_start { START } else { END };
        cursor += prefix.len();
        let Some(close) = marked[cursor..].find(CLOSE) else {
            output.push_str(&marked[cursor - prefix.len()..]);
            break;
        };
        let node_id = marked[cursor..cursor + close].to_owned();
        cursor += close + CLOSE.len_utf8();
        if is_start {
            stack.push((node_id, output.len()));
        } else if let Some(index) = stack.iter().rposition(|(id, _)| id == &node_id) {
            let (_, start) = stack.remove(index);
            ranges.push((
                node_id,
                GeneratedRange {
                    start,
                    end: output.len(),
                },
            ));
        }
    }
    ranges.sort_by(|left, right| {
        left.1
            .start
            .cmp(&right.1.start)
            .then_with(|| right.1.end.cmp(&left.1.end))
            .then_with(|| left.0.cmp(&right.0))
    });
    (output, ranges)
}

pub(crate) fn json_pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
