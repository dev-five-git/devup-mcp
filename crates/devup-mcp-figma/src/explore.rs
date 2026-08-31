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
                    "Figma 탐색 projection에 유효한 node bounds가 없습니다.",
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExploreResult {
    pub anchor: ExploreNode,
    pub group: Option<ExploreGroup>,
    pub candidates: Vec<ExploreCandidate>,
    pub truncated: bool,
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
            "탐색 limit은 1 이상 100 이하여야 합니다.",
            false,
        ));
    }
    let anchor_id = target.node_id.as_deref().ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Figma 주변 화면 탐색에는 node-id가 필요합니다.",
            false,
        )
    })?;
    let raw_anchor = snapshot.nodes.get(anchor_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Figma 탐색 projection에서 anchor node를 찾지 못했습니다.",
            false,
        )
    })?;
    let anchor = ExploreNode::try_from(raw_anchor)?;
    let projection_truncated = snapshot
        .nodes
        .values()
        .any(|node| node.typed_view().bool("projectionTruncated") == Some(true));

    if anchor.kind == ExploreKind::Screen {
        return Ok(ExploreResult {
            group: Some(ExploreGroup {
                title: anchor.name.clone(),
                heading_node_id: None,
                bounds: anchor.bounds,
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

    let mut nodes = snapshot
        .nodes
        .values()
        .filter(|node| node.id != anchor.node_id)
        .filter_map(|node| ExploreNode::try_from(node).ok())
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
        group: Some(ExploreGroup {
            title: anchor.name.clone(),
            heading_node_id: (anchor.kind == ExploreKind::Heading).then(|| anchor.node_id.clone()),
            bounds: group_bounds,
        }),
        anchor,
        candidates,
        truncated: projection_truncated || candidate_count > options.limit,
    })
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
