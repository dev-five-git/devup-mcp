use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{DevupError, ErrorCode, UpstreamResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FidelityImpact {
    #[default]
    None,
    Approximated,
    Lossy,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<DiagnosticSeverity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fidelity_impact: Option<FidelityImpact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recoverable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl Diagnostic {
    pub fn fidelity_impact(&self) -> FidelityImpact {
        if let Some(impact) = self.fidelity_impact {
            return impact;
        }
        match self.code.as_str() {
            "DEVUP_CODEGEN_PROJECTION_FAILED" => FidelityImpact::Failed,
            "DEVUP_CODEGEN_MASK_FALLBACK" | "DEVUP_CODEGEN_EFFECT_FALLBACK" => {
                FidelityImpact::Lossy
            }
            "DEVUP_CODEGEN_ABSOLUTE_FALLBACK" => FidelityImpact::Approximated,
            _ if self.code.starts_with("DEVUP_CODEGEN_") => match self.severity {
                Some(DiagnosticSeverity::Error) => FidelityImpact::Failed,
                Some(DiagnosticSeverity::Info) => FidelityImpact::None,
                Some(DiagnosticSeverity::Warning) | None => FidelityImpact::Approximated,
            },
            _ => FidelityImpact::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub fields: Map<String, Value>,
    #[serde(default)]
    pub extra: Map<String, Value>,
    #[serde(default)]
    pub field_errors: BTreeMap<String, String>,
}

impl RawNode {
    pub fn typed_view(&self) -> TypedNode<'_> {
        TypedNode { node: self }
    }
}

/// Sentinel node ID every paginating snapshot script appends to report where
/// the next page starts.
pub const SNAPSHOT_CURSOR_ID: &str = "__DEVUP_SNAPSHOT_CURSOR__";

/// Page state carried by the `__DEVUP_SNAPSHOT_CURSOR__` marker node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotCursor {
    pub offset: usize,
    pub next_offset: usize,
    pub complete: bool,
    pub total_nodes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotCursorError {
    Duplicated,
    Shape,
}

impl SnapshotCursorError {
    pub fn korean_message(self) -> &'static str {
        match self {
            Self::Duplicated => "Figma snapshot 응답에 cursor가 중복되었습니다.",
            Self::Shape => "Figma snapshot cursor 형식이 올바르지 않습니다.",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::Duplicated => "cursorMultiplicity",
            Self::Shape => "cursorShape",
        }
    }
}

/// Reads the page cursor out of a node list without mutating it.
///
/// Both the legacy cursor collector and the fast envelope decoder go through
/// here so the marker is parsed against exactly one field list - the two used
/// to keep separate lists, and drifted apart.
pub fn read_snapshot_cursor(
    nodes: &[RawNode],
) -> Result<Option<SnapshotCursor>, SnapshotCursorError> {
    let markers = nodes
        .iter()
        .filter(|node| node.id == SNAPSHOT_CURSOR_ID)
        .collect::<Vec<_>>();
    let marker = match markers.as_slice() {
        [] => return Ok(None),
        [marker] => *marker,
        _ => return Err(SnapshotCursorError::Duplicated),
    };
    if marker.node_type != "DEVUP_INTERNAL" {
        return Err(SnapshotCursorError::Shape);
    }
    let view = marker.typed_view();
    let index = |field: &str| {
        view.value(field)
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(SnapshotCursorError::Shape)
    };
    Ok(Some(SnapshotCursor {
        offset: index("offset")?,
        next_offset: index("nextOffset")?,
        complete: view.bool("complete").ok_or(SnapshotCursorError::Shape)?,
        total_nodes: index("totalNodes")?,
    }))
}

#[derive(Debug, Clone, Copy)]
pub struct TypedNode<'a> {
    node: &'a RawNode,
}

impl<'a> TypedNode<'a> {
    pub fn id(&self) -> &'a str {
        &self.node.id
    }

    pub fn node_type(&self) -> &'a str {
        &self.node.node_type
    }

    pub fn value(&self, field: &str) -> Option<&'a Value> {
        self.node
            .fields
            .get(field)
            .or_else(|| self.node.extra.get(field))
    }

    pub fn string(&self, field: &str) -> Option<&'a str> {
        self.value(field).and_then(Value::as_str)
    }

    pub fn number(&self, field: &str) -> Option<f64> {
        self.value(field).and_then(Value::as_f64)
    }

    pub fn bool(&self, field: &str) -> Option<bool> {
        self.value(field).and_then(Value::as_bool)
    }

    pub fn name(&self) -> Option<&'a str> {
        self.string("name")
    }

    pub fn child_ids(&self) -> impl Iterator<Item = &'a str> {
        self.value("childrenIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotChunk {
    pub file_key: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub root_ids: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<RawNode>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub file_key: String,
    pub version: Option<String>,
    pub roots: Vec<String>,
    pub nodes: BTreeMap<String, RawNode>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompletenessState {
    Complete,
    Partial,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingChild {
    pub parent_id: String,
    pub child_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentMismatch {
    pub parent_id: String,
    pub child_id: String,
    pub observed_parent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldLocation {
    pub node_id: String,
    pub field: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildCountMismatch {
    pub node_id: String,
    pub declared_count: usize,
    pub exported_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotAudit {
    pub state: CompletenessState,
    pub root_count: usize,
    pub preserved_node_count: usize,
    pub reachable_node_count: usize,
    pub missing_root_ids: Vec<String>,
    pub orphan_node_ids: Vec<String>,
    pub declared_child_count: usize,
    pub exported_child_count: usize,
    pub missing_children: Vec<MissingChild>,
    pub parent_mismatches: Vec<ParentMismatch>,
    pub child_count_mismatches: Vec<ChildCountMismatch>,
    pub truncated_fields: Vec<FieldLocation>,
    pub field_error_count: usize,
}

impl Snapshot {
    pub fn audit(&self) -> SnapshotAudit {
        let mut reachable = BTreeSet::new();
        let mut pending = self.roots.iter().rev().cloned().collect::<Vec<_>>();
        let mut declared_child_count = 0usize;
        let mut exported_child_count = 0usize;
        let mut missing_children = Vec::new();
        let mut parent_mismatches = Vec::new();
        let mut child_count_mismatches = Vec::new();
        let missing_root_ids = self
            .roots
            .iter()
            .filter(|root_id| !self.nodes.contains_key(*root_id))
            .cloned()
            .collect::<Vec<_>>();

        while let Some(node_id) = pending.pop() {
            let Some(node) = self.nodes.get(&node_id) else {
                continue;
            };
            if !reachable.insert(node_id) {
                continue;
            }
            let child_ids = node.typed_view().child_ids().collect::<Vec<_>>();
            if let Some(observed_count) = node
                .typed_view()
                .value("childCount")
                .and_then(Value::as_u64)
                .and_then(|count| usize::try_from(count).ok())
                && observed_count != child_ids.len()
            {
                child_count_mismatches.push(ChildCountMismatch {
                    node_id: node.id.clone(),
                    declared_count: observed_count,
                    exported_count: child_ids.len(),
                });
            }
            declared_child_count = declared_child_count.saturating_add(child_ids.len());
            let mut existing_child_ids = Vec::new();
            for child_id in child_ids {
                let Some(child) = self.nodes.get(child_id) else {
                    missing_children.push(MissingChild {
                        parent_id: node.id.clone(),
                        child_id: child_id.to_owned(),
                    });
                    continue;
                };
                exported_child_count = exported_child_count.saturating_add(1);
                if let Some(observed_parent_id) = child.typed_view().string("parentId")
                    && observed_parent_id != node.id
                {
                    parent_mismatches.push(ParentMismatch {
                        parent_id: node.id.clone(),
                        child_id: child.id.clone(),
                        observed_parent_id: Some(observed_parent_id.to_owned()),
                    });
                }
                existing_child_ids.push(child_id.to_owned());
            }
            for child_id in existing_child_ids.into_iter().rev() {
                pending.push(child_id);
            }
        }

        let orphan_node_ids = self
            .nodes
            .keys()
            .filter(|node_id| !reachable.contains(*node_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut truncated_fields = Vec::new();
        let mut field_error_count = 0usize;
        for node in self.nodes.values() {
            field_error_count = field_error_count.saturating_add(node.field_errors.len());
            for (field, value) in node.fields.iter().chain(&node.extra) {
                if contains_truncation(value) {
                    truncated_fields.push(FieldLocation {
                        node_id: node.id.clone(),
                        field: field.clone(),
                    });
                }
            }
        }
        let state = if !missing_root_ids.is_empty() {
            CompletenessState::Failed
        } else if missing_children.is_empty()
            && parent_mismatches.is_empty()
            && child_count_mismatches.is_empty()
            && orphan_node_ids.is_empty()
            && truncated_fields.is_empty()
            && field_error_count == 0
        {
            CompletenessState::Complete
        } else {
            CompletenessState::Partial
        };

        SnapshotAudit {
            state,
            root_count: self.roots.len(),
            preserved_node_count: self.nodes.len(),
            reachable_node_count: reachable.len(),
            missing_root_ids,
            orphan_node_ids,
            declared_child_count,
            exported_child_count,
            missing_children,
            parent_mismatches,
            child_count_mismatches,
            truncated_fields,
            field_error_count,
        }
    }
}

fn contains_truncation(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key("$truncated")
                || object.contains_key("$largeValue")
                || object.values().any(contains_truncation)
        }
        Value::Array(values) => values.iter().any(contains_truncation),
        _ => false,
    }
}

pub fn merge_chunks(chunks: Vec<SnapshotChunk>) -> Result<Snapshot, DevupError> {
    let first = chunks.first().ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "병합할 Figma snapshot이 없습니다.",
            false,
        )
    })?;
    let file_key = first.file_key.clone();
    let version = first.version.clone();
    let mut roots = Vec::new();
    let mut root_set = BTreeSet::new();
    let mut nodes = BTreeMap::new();
    let mut diagnostics = Vec::new();

    for chunk in chunks {
        if chunk.file_key != file_key || chunk.version != version {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaVersionChanged,
                "수집 중 Figma 파일 버전이 변경되었습니다. 다시 시도하세요.",
                true,
            ));
        }
        for root in chunk.root_ids {
            if root_set.insert(root.clone()) {
                roots.push(root);
            }
        }
        for node in chunk.nodes {
            if let Some(existing) = nodes.get(&node.id) {
                if existing != &node {
                    return Err(DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        "동일한 Figma node에 서로 다른 snapshot 데이터가 반환되었습니다.",
                        true,
                    ));
                }
            } else {
                nodes.insert(node.id.clone(), node);
            }
        }
        diagnostics.extend(chunk.diagnostics);
    }

    Ok(Snapshot {
        file_key,
        version,
        roots,
        nodes,
        diagnostics,
    })
}

pub fn snapshot_chunk_from_result(result: &UpstreamResult) -> Result<SnapshotChunk, DevupError> {
    find_snapshot(&result.raw).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "Figma MCP 응답에서 snapshot 데이터를 찾지 못했습니다.",
            false,
        )
    })
}

fn find_snapshot(value: &Value) -> Option<SnapshotChunk> {
    if let Ok(snapshot) = serde_json::from_value::<SnapshotChunk>(value.clone())
        && !snapshot.file_key.is_empty()
        && !snapshot.nodes.is_empty()
    {
        return Some(snapshot);
    }

    match value {
        Value::Object(object) => {
            if let Some(Value::String(text)) = object.get("text")
                && let Some(snapshot) = parse_snapshot_text(text)
            {
                return Some(snapshot);
            }
            object.values().find_map(find_snapshot)
        }
        Value::Array(values) => values.iter().find_map(find_snapshot),
        Value::String(text) => parse_snapshot_text(text),
        _ => None,
    }
}

fn parse_snapshot_text(text: &str) -> Option<SnapshotChunk> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| find_snapshot(&value))
}
