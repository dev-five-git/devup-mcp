use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    DevupError, ErrorCode, ExploreBounds, ExploreKind, ExploreNode, FigmaTarget, Snapshot,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionSummary {
    pub node_id: String,
    pub name: String,
    pub bounds: ExploreBounds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionCandidate {
    pub node_id: String,
    pub name: String,
    pub node_type: String,
    pub visible: bool,
    pub bounds: ExploreBounds,
    pub parent_id: Option<String>,
    pub breadcrumb: Vec<String>,
    pub direct_child_count: usize,
    pub subtree_node_count: usize,
    pub estimated_serialized_bytes: usize,
    pub selection_reasons: Vec<String>,
    pub canonical_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionIndex {
    pub file_key: String,
    pub source_version: Option<String>,
    pub section: SectionSummary,
    pub candidates: Vec<SectionCandidate>,
    pub truncated: bool,
}

impl SectionIndex {
    pub fn select(
        &self,
        frame_ids: &[String],
        all_screens: bool,
    ) -> Result<Vec<String>, DevupError> {
        if all_screens && !frame_ids.is_empty() {
            return Err(invalid_selection(
                "frameIds and allScreens cannot be used together.",
            ));
        }
        if !all_screens && frame_ids.is_empty() {
            return Err(invalid_selection(
                "Section root collection requires frameIds or allScreens.",
            ));
        }
        if self.truncated && all_screens {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "allScreens cannot be used with a truncated Section index.",
                false,
            ));
        }
        let requested = frame_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if requested.len() != frame_ids.len() {
            return Err(invalid_selection("frameIds contains duplicate nodes."));
        }
        let candidates = self
            .candidates
            .iter()
            .map(|candidate| candidate.node_id.as_str())
            .collect::<BTreeSet<_>>();
        if let Some(foreign) = requested.difference(&candidates).next() {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                format!("Not a screen frame inside the Section, or it does not exist: {foreign}"),
                false,
            ));
        }
        Ok(self
            .candidates
            .iter()
            .filter(|candidate| all_screens || requested.contains(candidate.node_id.as_str()))
            .map(|candidate| candidate.node_id.clone())
            .collect())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchLimits {
    pub max_estimated_bytes: usize,
    pub max_nodes: usize,
}

impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            max_estimated_bytes: 6 * 1024 * 1024,
            max_nodes: 4_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionBatch {
    pub root_ids: Vec<String>,
    pub estimated_bytes: usize,
    pub node_count: usize,
    pub oversized: bool,
}

pub fn build_section_index(
    snapshot: &Snapshot,
    target: &FigmaTarget,
) -> Result<SectionIndex, DevupError> {
    if snapshot.file_key != target.file_key {
        return Err(invalid_selection(
            "Section index file key does not match the request.",
        ));
    }
    let section_id = target.node_id.as_deref().ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Section index requires a node-id.",
            false,
        )
    })?;
    let section = snapshot.nodes.get(section_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Target node not found for the Section index.",
            false,
        )
    })?;
    if section.node_type != "SECTION" {
        return Err(invalid_selection("Section index target must be a SECTION."));
    }
    let section_node = ExploreNode::try_from(section)?;
    let mut screen_nodes = Vec::new();
    for node in snapshot.nodes.values() {
        if node.id == section_id || node.node_type != "FRAME" {
            continue;
        }
        let explore = match ExploreNode::try_from(node) {
            Ok(explore) => explore,
            Err(_) => continue,
        };
        if explore.visible
            && explore.kind == ExploreKind::Screen
            && is_descendant(snapshot, &node.id, section_id)
        {
            screen_nodes.push(explore);
        }
    }
    let screen_ids = screen_nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    screen_nodes.retain(|node| {
        !ancestor_ids(snapshot, &node.node_id, section_id)
            .iter()
            .any(|ancestor| ancestor != section_id && screen_ids.contains(ancestor.as_str()))
    });
    screen_nodes.sort_by(|left, right| {
        left.bounds
            .y
            .total_cmp(&right.bounds.y)
            .then_with(|| left.bounds.x.total_cmp(&right.bounds.x))
            .then_with(|| left.node_id.cmp(&right.node_id))
    });

    let candidates = screen_nodes
        .into_iter()
        .map(|node| {
            let (computed_count, computed_bytes) = subtree_estimate(snapshot, &node.node_id);
            let raw = snapshot
                .nodes
                .get(&node.node_id)
                .expect("candidate originated from snapshot");
            let view = raw.typed_view();
            let direct_child_count = view
                .value("directChildCount")
                .or_else(|| view.value("childCount"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|count| usize::try_from(count).ok())
                .unwrap_or_else(|| view.child_ids().count());
            let subtree_node_count = view
                .value("subtreeNodeCount")
                .and_then(serde_json::Value::as_u64)
                .and_then(|count| usize::try_from(count).ok())
                .unwrap_or(computed_count);
            let estimated_serialized_bytes = view
                .value("estimatedSerializedBytes")
                .and_then(serde_json::Value::as_u64)
                .and_then(|count| usize::try_from(count).ok())
                .unwrap_or(computed_bytes);
            let breadcrumb = if node.breadcrumb.is_empty() {
                breadcrumb(snapshot, &node.node_id)
            } else {
                node.breadcrumb.clone()
            };
            SectionCandidate {
                canonical_url: canonical_url(target, &node.node_id),
                node_id: node.node_id,
                name: node.name,
                node_type: node.node_type,
                visible: node.visible,
                bounds: node.bounds,
                parent_id: node.parent_id,
                breadcrumb,
                direct_child_count,
                subtree_node_count,
                estimated_serialized_bytes,
                selection_reasons: vec!["screen-like".to_owned(), "inside-section".to_owned()],
            }
        })
        .collect();

    Ok(SectionIndex {
        file_key: snapshot.file_key.clone(),
        source_version: snapshot.version.clone(),
        section: SectionSummary {
            node_id: section_id.to_owned(),
            name: section_node.name,
            bounds: section_node.bounds,
        },
        candidates,
        truncated: snapshot
            .nodes
            .values()
            .any(|node| node.typed_view().bool("projectionTruncated") == Some(true)),
    })
}

pub fn plan_batches(
    index: &SectionIndex,
    selected_root_ids: &[String],
    limits: BatchLimits,
) -> Result<Vec<SectionBatch>, DevupError> {
    if limits.max_estimated_bytes == 0 || limits.max_nodes == 0 {
        return Err(invalid_selection(
            "Section batch limits must be greater than 0.",
        ));
    }
    let selected = index.select(selected_root_ids, false)?;
    let by_id = index
        .candidates
        .iter()
        .map(|candidate| (candidate.node_id.as_str(), candidate))
        .collect::<BTreeMap<_, _>>();
    let visual_rank = selected
        .iter()
        .enumerate()
        .map(|(rank, root_id)| (root_id.clone(), rank))
        .collect::<BTreeMap<_, _>>();
    let mut candidates = selected
        .into_iter()
        .map(|root_id| {
            let candidate = by_id
                .get(root_id.as_str())
                .copied()
                .ok_or_else(|| invalid_selection("Section batch candidate is missing."))?;
            let rank = visual_rank[&root_id];
            Ok((rank, root_id, candidate))
        })
        .collect::<Result<Vec<_>, DevupError>>()?;
    // Best-fit decreasing avoids the avoidable extra calls produced by visual-order first-fit.
    // The normalized pressure comparison uses integers, so equal requests always pack alike.
    candidates.sort_by(|left, right| {
        packing_pressure(right.2, limits)
            .cmp(&packing_pressure(left.2, limits))
            .then_with(|| {
                right
                    .2
                    .estimated_serialized_bytes
                    .cmp(&left.2.estimated_serialized_bytes)
            })
            .then_with(|| right.2.subtree_node_count.cmp(&left.2.subtree_node_count))
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut batches: Vec<SectionBatch> = Vec::new();
    for (_rank, root_id, candidate) in candidates {
        let oversized = candidate.estimated_serialized_bytes > limits.max_estimated_bytes
            || candidate.subtree_node_count > limits.max_nodes;
        if oversized {
            batches.push(SectionBatch {
                root_ids: vec![root_id],
                estimated_bytes: candidate.estimated_serialized_bytes,
                node_count: candidate.subtree_node_count,
                oversized: true,
            });
            continue;
        }
        let best_batch = batches
            .iter()
            .enumerate()
            .filter_map(|(index, batch)| {
                if batch.oversized {
                    return None;
                }
                let bytes = batch
                    .estimated_bytes
                    .checked_add(candidate.estimated_serialized_bytes)?;
                let nodes = batch.node_count.checked_add(candidate.subtree_node_count)?;
                if bytes > limits.max_estimated_bytes || nodes > limits.max_nodes {
                    return None;
                }
                Some((packing_slack(bytes, nodes, limits), index))
            })
            .min();
        if let Some((_slack, index)) = best_batch {
            let batch = &mut batches[index];
            batch.root_ids.push(root_id);
            batch.estimated_bytes += candidate.estimated_serialized_bytes;
            batch.node_count += candidate.subtree_node_count;
        } else {
            batches.push(SectionBatch {
                root_ids: vec![root_id],
                estimated_bytes: candidate.estimated_serialized_bytes,
                node_count: candidate.subtree_node_count,
                oversized: false,
            });
        }
    }
    for batch in &mut batches {
        batch.root_ids.sort_by_key(|root_id| visual_rank[root_id]);
    }
    batches.sort_by_key(|batch| {
        batch
            .root_ids
            .iter()
            .map(|root_id| visual_rank[root_id])
            .min()
            .unwrap_or(usize::MAX)
    });
    Ok(batches)
}

fn packing_pressure(candidate: &SectionCandidate, limits: BatchLimits) -> u128 {
    ((candidate.estimated_serialized_bytes as u128) * (limits.max_nodes as u128))
        .max((candidate.subtree_node_count as u128) * (limits.max_estimated_bytes as u128))
}

fn packing_slack(bytes: usize, nodes: usize, limits: BatchLimits) -> u128 {
    ((limits.max_estimated_bytes - bytes) as u128) * (limits.max_nodes as u128)
        + ((limits.max_nodes - nodes) as u128) * (limits.max_estimated_bytes as u128)
}

fn subtree_estimate(snapshot: &Snapshot, root_id: &str) -> (usize, usize) {
    let mut count = 0_usize;
    let mut bytes = 0_usize;
    let mut pending = vec![root_id.to_owned()];
    let mut visited = BTreeSet::new();
    while let Some(node_id) = pending.pop() {
        if !visited.insert(node_id.clone()) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(&node_id) else {
            continue;
        };
        count = count.saturating_add(1);
        bytes = bytes.saturating_add(serde_json::to_vec(node).map_or(0, |value| value.len()));
        pending.extend(node.typed_view().child_ids().map(str::to_owned));
    }
    (count, bytes)
}

fn ancestor_ids(snapshot: &Snapshot, node_id: &str, stop_id: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = snapshot
        .nodes
        .get(node_id)
        .and_then(|node| node.typed_view().string("parentId"));
    let mut visited = BTreeSet::new();
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

fn is_descendant(snapshot: &Snapshot, node_id: &str, section_id: &str) -> bool {
    ancestor_ids(snapshot, node_id, section_id)
        .iter()
        .any(|ancestor| ancestor == section_id)
}

fn breadcrumb(snapshot: &Snapshot, node_id: &str) -> Vec<String> {
    let mut ids = ancestor_ids(snapshot, node_id, "");
    ids.reverse();
    ids.push(node_id.to_owned());
    ids.into_iter()
        .filter_map(|id| snapshot.nodes.get(&id))
        .filter_map(|node| node.typed_view().name())
        .map(str::to_owned)
        .collect()
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

fn invalid_selection(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupFigmaHandoffInvalid, message, false)
}
