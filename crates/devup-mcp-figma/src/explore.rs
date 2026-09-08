use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::{DevupError, ErrorCode, FigmaTarget, RawNode, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExploreBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ExploreBounds {
    fn right(self) -> f64 {
        self.x + self.width
    }

    fn bottom(self) -> f64 {
        self.y + self.height
    }

    fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExploreKind {
    Screen,
    Heading,
    Annotation,
    Container,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetKind {
    File,
    Page,
    Section,
    Screen,
    Component,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExploreNode {
    pub node_id: String,
    pub name: String,
    pub node_type: String,
    pub bounds: ExploreBounds,
    pub child_count: usize,
    pub text_preview: String,
    pub parent_id: Option<String>,
    pub kind: ExploreKind,
    pub visible: bool,
    pub breadcrumb: Vec<String>,
    pub page_child_index: Option<usize>,
}

impl TryFrom<&RawNode> for ExploreNode {
    type Error = DevupError;

    fn try_from(node: &RawNode) -> Result<Self, Self::Error> {
        let view = node.typed_view();
        let bounds = view
            .value("absoluteBoundingBox")
            .and_then(|value| value.as_object())
            .and_then(|bounds| {
                Some(ExploreBounds {
                    x: bounds.get("x")?.as_f64()?,
                    y: bounds.get("y")?.as_f64()?,
                    width: bounds.get("width")?.as_f64()?,
                    height: bounds.get("height")?.as_f64()?,
                })
            })
            .or_else(|| {
                Some(ExploreBounds {
                    x: view.number("x")?,
                    y: view.number("y")?,
                    width: view.number("width")?,
                    height: view.number("height")?,
                })
            })
            .filter(|bounds| {
                bounds.x.is_finite()
                    && bounds.y.is_finite()
                    && bounds.width.is_finite()
                    && bounds.height.is_finite()
                    && bounds.width >= 0.0
                    && bounds.height >= 0.0
            })
            .ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "Figma exploration projection has no valid node bounds.",
                    false,
                )
            })?;
        let child_count = view
            .value("childCount")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or_else(|| view.child_ids().count());
        let mut result = Self {
            node_id: node.id.clone(),
            name: view.name().unwrap_or_default().to_owned(),
            node_type: node.node_type.clone(),
            bounds,
            child_count,
            text_preview: view.string("textPreview").unwrap_or_default().to_owned(),
            parent_id: view.string("parentId").map(str::to_owned),
            kind: ExploreKind::Unknown,
            visible: view.bool("visible").unwrap_or(true),
            breadcrumb: view
                .value("breadcrumb")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect(),
            page_child_index: view
                .value("pageChildIndex")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok()),
        };
        result.kind = classify_explore_node(&result);
        Ok(result)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExploreOptions {
    pub limit: usize,
}

impl Default for ExploreOptions {
    fn default() -> Self {
        Self { limit: 50 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExploreCandidate {
    pub node: ExploreNode,
    pub canonical_url: String,
    pub score: u32,
    pub selection_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExploreGroup {
    pub title: String,
    pub heading_node_id: Option<String>,
    pub bounds: ExploreBounds,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExploreResult {
    pub target_kind: TargetKind,
    pub anchor: ExploreNode,
    pub group: Option<ExploreGroup>,
    pub candidates: Vec<ExploreCandidate>,
    pub truncated: bool,
}

pub fn classify_target(snapshot: &Snapshot, target: &FigmaTarget) -> TargetKind {
    let Some(node_id) = target.node_id.as_deref() else {
        return TargetKind::File;
    };
    let Some(node) = snapshot.nodes.get(node_id) else {
        return TargetKind::Other;
    };
    match node.node_type.as_str() {
        "PAGE" => TargetKind::Page,
        "SECTION" => TargetKind::Section,
        "COMPONENT" | "COMPONENT_SET" | "INSTANCE" => TargetKind::Component,
        "FRAME" => ExploreNode::try_from(node).map_or(TargetKind::Other, |node| {
            if node.kind == ExploreKind::Screen {
                TargetKind::Screen
            } else {
                TargetKind::Other
            }
        }),
        _ => TargetKind::Other,
    }
}

pub fn classify_explore_node(node: &ExploreNode) -> ExploreKind {
    let bounds = node.bounds;
    let aspect = if bounds.height > 0.0 {
        bounds.width / bounds.height
    } else {
        f64::INFINITY
    };
    let container_type = matches!(
        node.node_type.as_str(),
        "SECTION" | "FRAME" | "COMPONENT_SET"
    );
    if container_type
        && bounds.height <= 180.0
        && bounds.width >= 600.0
        && aspect >= 4.0
        && node.child_count <= 8
    {
        return ExploreKind::Heading;
    }
    if matches!(
        node.node_type.as_str(),
        "TEXT" | "VECTOR" | "LINE" | "SHAPE_WITH_TEXT"
    ) {
        return ExploreKind::Annotation;
    }
    if container_type && node.child_count >= 2 && (bounds.width > 1800.0 || bounds.height > 2000.0)
    {
        return ExploreKind::Container;
    }
    if matches!(
        node.node_type.as_str(),
        "FRAME" | "COMPONENT" | "INSTANCE" | "COMPONENT_SET"
    ) && (240.0..=1800.0).contains(&bounds.width)
        && (300.0..=2000.0).contains(&bounds.height)
        && (0.25..=2.5).contains(&(bounds.width / bounds.height.max(1.0)))
    {
        return ExploreKind::Screen;
    }
    if bounds.width <= 320.0 && bounds.height <= 180.0 {
        return ExploreKind::Annotation;
    }
    ExploreKind::Unknown
}

pub fn explore_snapshot(
    snapshot: &Snapshot,
    target: &FigmaTarget,
    options: &ExploreOptions,
) -> Result<ExploreResult, DevupError> {
    if options.limit == 0 || options.limit > 100 {
        return Err(DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "Exploration limit must be between 1 and 100.",
            false,
        ));
    }
    let anchor_id = target.node_id.as_deref().ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Figma nearby-screen exploration requires a node-id.",
            false,
        )
    })?;
    let raw_anchor = snapshot.nodes.get(anchor_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "anchor node not found in the Figma exploration projection.",
            false,
        )
    })?;
    let mut anchor = ExploreNode::try_from(raw_anchor)?;
    enrich_node(snapshot, &mut anchor);
    let target_kind = classify_target(snapshot, target);
    let projection_truncated = snapshot
        .nodes
        .values()
        .any(|node| node.typed_view().bool("projectionTruncated") == Some(true));

    if anchor.kind == ExploreKind::Screen {
        return Ok(ExploreResult {
            target_kind,
            group: Some(ExploreGroup {
                title: anchor.name.clone(),
                heading_node_id: None,
                bounds: anchor.bounds,
                notes: String::new(),
            }),
            candidates: vec![ExploreCandidate {
                canonical_url: canonical_url(target, &anchor.node_id),
                node: anchor.clone(),
                score: 1_000,
                selection_reasons: vec!["exact-screen-anchor".to_owned()],
            }],
            anchor,
            truncated: projection_truncated,
        });
    }

    let section_scope_id = if target_kind == TargetKind::Section {
        Some(anchor.node_id.clone())
    } else {
        ancestor_ids(snapshot, &anchor.node_id, "")
            .into_iter()
            .find(|id| {
                snapshot
                    .nodes
                    .get(id)
                    .is_some_and(|node| node.node_type == "SECTION")
            })
    };

    if let Some(section_scope_id) = section_scope_id {
        let mut section_scope = ExploreNode::try_from(
            snapshot
                .nodes
                .get(&section_scope_id)
                .expect("resolved section scope exists"),
        )?;
        enrich_node(snapshot, &mut section_scope);
        let mut nodes = snapshot
            .nodes
            .values()
            .filter(|node| node.id != anchor.node_id)
            .filter(|node| node.id != section_scope_id)
            .filter(|node| node.node_type == "FRAME")
            .filter_map(|node| {
                let mut node = ExploreNode::try_from(node).ok()?;
                enrich_node(snapshot, &mut node);
                (node.visible
                    && node.kind == ExploreKind::Screen
                    && is_descendant_of(snapshot, &node.node_id, &section_scope_id))
                .then_some(node)
            })
            .collect::<Vec<_>>();
        let screen_ids = nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        nodes.retain(|node| {
            !ancestor_ids(snapshot, &node.node_id, &section_scope_id)
                .iter()
                .any(|ancestor| screen_ids.contains(ancestor))
        });
        nodes.sort_by(visual_order);
        let candidate_count = nodes.len();
        nodes.truncate(options.limit);
        let candidates = nodes
            .into_iter()
            .map(|node| ExploreCandidate {
                canonical_url: canonical_url(target, &node.node_id),
                node,
                score: 900,
                selection_reasons: vec!["screen-like".to_owned(), "inside-section".to_owned()],
            })
            .collect::<Vec<_>>();
        let group_bounds = candidates
            .iter()
            .fold(section_scope.bounds, |bounds, candidate| {
                bounds.union(candidate.node.bounds)
            });
        return Ok(ExploreResult {
            target_kind,
            group: Some(ExploreGroup {
                title: anchor.name.clone(),
                heading_node_id: (anchor.kind == ExploreKind::Heading)
                    .then(|| anchor.node_id.clone()),
                bounds: group_bounds,
                notes: collect_section_notes(snapshot, &section_scope_id)?,
            }),
            anchor,
            candidates,
            truncated: projection_truncated || candidate_count > options.limit,
        });
    }

    let mut nodes = snapshot
        .nodes
        .values()
        .filter(|node| node.id != anchor.node_id)
        .filter_map(|node| {
            let mut node = ExploreNode::try_from(node).ok()?;
            enrich_node(snapshot, &mut node);
            node.visible.then_some(node)
        })
        .collect::<Vec<_>>();
    let anchor_is_requirement = looks_like_requirement_heading(&anchor.name);
    let next_heading_y = nodes
        .iter()
        .filter(|node| node.kind == ExploreKind::Heading)
        .filter(|node| !anchor_is_requirement || looks_like_requirement_heading(&node.name))
        .filter(|node| node.bounds.y >= anchor.bounds.bottom())
        .filter(|node| node.bounds.width >= anchor.bounds.width * 0.5)
        .filter(|node| horizontal_overlap(node.bounds, anchor.bounds) > 0.0)
        .map(|node| node.bounds.y)
        .min_by(f64::total_cmp);
    let cutoff = next_heading_y.unwrap_or(f64::INFINITY);

    nodes.retain(|node| {
        if node.kind != ExploreKind::Screen {
            return false;
        }
        if anchor.kind == ExploreKind::Container {
            return node.parent_id.as_deref() == Some(anchor.node_id.as_str())
                || contains(anchor.bounds, node.bounds);
        }
        node.bounds.y >= anchor.bounds.bottom()
            && node.bounds.y < cutoff
            && horizontal_overlap(node.bounds, anchor.bounds) > 0.0
    });
    nodes.sort_by(visual_order);
    let candidate_count = nodes.len();
    nodes.truncate(options.limit);
    let candidates = nodes
        .into_iter()
        .map(|node| {
            let overlap = horizontal_overlap(node.bounds, anchor.bounds);
            let overlap_ratio = overlap / node.bounds.width.max(1.0);
            let mut reasons = vec!["screen-like".to_owned()];
            if anchor.kind == ExploreKind::Container {
                reasons.push("inside-container".to_owned());
            } else {
                reasons.push("below-anchor".to_owned());
                reasons.push("horizontal-overlap".to_owned());
                reasons.push("before-next-heading".to_owned());
            }
            ExploreCandidate {
                canonical_url: canonical_url(target, &node.node_id),
                score: 400 + (overlap_ratio.clamp(0.0, 1.0) * 100.0).round() as u32,
                node,
                selection_reasons: reasons,
            }
        })
        .collect::<Vec<_>>();
    let group_bounds = candidates.iter().fold(anchor.bounds, |bounds, candidate| {
        bounds.union(candidate.node.bounds)
    });

    Ok(ExploreResult {
        target_kind,
        group: Some(ExploreGroup {
            title: anchor.name.clone(),
            heading_node_id: (anchor.kind == ExploreKind::Heading).then(|| anchor.node_id.clone()),
            bounds: group_bounds,
            notes: String::new(),
        }),
        anchor,
        candidates,
        truncated: projection_truncated || candidate_count > options.limit,
    })
}

pub fn collect_section_notes(snapshot: &Snapshot, section_id: &str) -> Result<String, DevupError> {
    let section = snapshot.nodes.get(section_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Node to collect Section notes from not found.",
            false,
        )
    })?;
    if section.node_type != "SECTION" {
        return Err(DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "Section note collection target must be a SECTION.",
            false,
        ));
    }
    let mut notes = section
        .typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|node| node.node_type == "TEXT")
        .filter_map(|node| node.typed_view().string("characters"))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut pending = section
        .typed_view()
        .child_ids()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    pending.reverse();
    let mut visited = std::collections::BTreeSet::new();
    while let Some(node_id) = pending.pop() {
        if !visited.insert(node_id.clone()) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(&node_id) else {
            continue;
        };
        let view = node.typed_view();
        if let Some(annotations) = view
            .value("annotations")
            .and_then(serde_json::Value::as_array)
        {
            for annotation in annotations {
                let label = annotation
                    .get("label")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|label| !label.is_empty())
                    .or_else(|| {
                        annotation
                            .get("labelMarkdown")
                            .and_then(serde_json::Value::as_str)
                            .map(str::trim)
                            .filter(|label| !label.is_empty())
                    });
                if let Some(label) = label {
                    notes.push(format!("[{}] {label}", view.name().unwrap_or("Unnamed")));
                }
            }
        }
        let mut children = view.child_ids().map(str::to_owned).collect::<Vec<_>>();
        children.reverse();
        pending.extend(children);
    }
    Ok(notes.join("\n"))
}

fn enrich_node(snapshot: &Snapshot, node: &mut ExploreNode) {
    if node.breadcrumb.is_empty() {
        let mut breadcrumb = ancestor_ids(snapshot, &node.node_id, "");
        breadcrumb.reverse();
        node.breadcrumb = breadcrumb
            .into_iter()
            .filter_map(|id| snapshot.nodes.get(&id))
            .filter_map(|node| node.typed_view().name())
            .map(str::to_owned)
            .chain(std::iter::once(node.name.clone()))
            .collect();
    }
}

fn is_descendant_of(snapshot: &Snapshot, node_id: &str, ancestor_id: &str) -> bool {
    ancestor_ids(snapshot, node_id, ancestor_id)
        .iter()
        .any(|id| id == ancestor_id)
}

fn ancestor_ids(snapshot: &Snapshot, node_id: &str, stop_id: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = snapshot
        .nodes
        .get(node_id)
        .and_then(|node| node.typed_view().string("parentId"));
    let mut visited = std::collections::BTreeSet::new();
    while let Some(parent_id) = current {
        if !visited.insert(parent_id.to_owned()) {
            break;
        }
        result.push(parent_id.to_owned());
        if parent_id == stop_id {
            break;
        }
        current = snapshot
            .nodes
            .get(parent_id)
            .and_then(|node| node.typed_view().string("parentId"));
    }
    result
}

fn horizontal_overlap(left: ExploreBounds, right: ExploreBounds) -> f64 {
    left.right().min(right.right()) - left.x.max(right.x)
}

fn contains(outer: ExploreBounds, inner: ExploreBounds) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

fn visual_order(left: &ExploreNode, right: &ExploreNode) -> Ordering {
    left.bounds
        .y
        .total_cmp(&right.bounds.y)
        .then_with(|| left.bounds.x.total_cmp(&right.bounds.x))
        .then_with(|| left.node_id.cmp(&right.node_id))
}

fn looks_like_requirement_heading(name: &str) -> bool {
    let Some((identifier, _)) = name
        .trim()
        .strip_prefix('[')
        .and_then(|name| name.split_once(']'))
    else {
        return false;
    };
    let Some((prefix, number)) = identifier.split_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && prefix.bytes().all(|byte| byte.is_ascii_uppercase())
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn canonical_url(target: &FigmaTarget, node_id: &str) -> String {
    let node_id = node_id.replace(':', "-");
    if let Some(branch_key) = &target.branch_key {
        format!(
            "https://www.figma.com/branch/{}/{branch_key}/devup?node-id={node_id}",
            target.file_key
        )
    } else {
        format!(
            "https://www.figma.com/design/{}/devup?node-id={node_id}",
            target.file_key
        )
    }
}
