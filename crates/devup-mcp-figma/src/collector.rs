use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    BuiltinScript, DevupError, ErrorCode, FigmaTarget, ReadToolCall, ResourceBatch,
    ResourceStyleRef, SnapshotChunk, UpstreamResult,
    metadata::metadata_from_result_for_target,
    snapshot_chunk_from_result,
    variables::{
        VariableBatchResult, VariableCatalog, batch_from_result, catalog_from_result,
        merge_variable_results,
    },
};

const LARGE_SUBTREE_THRESHOLD: usize = 200;
const MAX_PENDING_CALLS: usize = 4;
const VARIABLE_BATCH_SIZE: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectionScope {
    Node,
    Page,
    File,
}

#[derive(Debug, Clone)]
pub struct CollectionRequest {
    pub target: FigmaTarget,
    pub scope: CollectionScope,
    pub include_variables: bool,
    pub include_context: bool,
}

impl CollectionRequest {
    pub fn new(target: FigmaTarget, scope: CollectionScope) -> Self {
        Self {
            target,
            scope,
            include_variables: false,
            include_context: false,
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
}

#[derive(Debug, Clone)]
pub enum CollectorStep {
    Call(PlannedCall),
    AwaitingResults,
    Complete(Box<CollectedParts>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallKind {
    Metadata,
    Snapshot,
    VariableCatalog,
    VariableBatch,
}

#[derive(Debug, Clone)]
struct PendingCall {
    planned: PlannedCall,
    kind: CallKind,
    order: usize,
}

#[derive(Debug)]
pub struct CollectorSession {
    request: CollectionRequest,
    queued: VecDeque<PendingCall>,
    pending: BTreeMap<String, PendingCall>,
    consumed: BTreeSet<String>,
    metadata: Option<Value>,
    root_node_id: Option<String>,
    source_version: Option<String>,
    snapshot_chunks: BTreeMap<usize, SnapshotChunk>,
    variable_catalog: Option<VariableCatalog>,
    variable_batches: BTreeMap<usize, VariableBatchResult>,
    variables: Option<UpstreamResult>,
    next_id: usize,
    completed: bool,
}

impl CollectorSession {
    pub fn new(request: CollectionRequest) -> Self {
        Self {
            request,
            queued: VecDeque::new(),
            pending: BTreeMap::new(),
            consumed: BTreeSet::new(),
            metadata: None,
            root_node_id: None,
            source_version: None,
            snapshot_chunks: BTreeMap::new(),
            variable_catalog: None,
            variable_batches: BTreeMap::new(),
            variables: None,
            next_id: 0,
            completed: false,
        }
    }

    pub fn advance(&mut self) -> Result<CollectorStep, DevupError> {
        if self.completed {
            return Err(invalid_call("완료된 Figma 수집 session입니다."));
        }
        if self.metadata.is_none() && self.pending.is_empty() && self.queued.is_empty() {
            let node_id = self.request.target.node_id.as_deref();
            self.enqueue(
                ReadToolCall::metadata(&self.request.target.file_key, node_id),
                node_id.map(str::to_owned),
                CallKind::Metadata,
            );
        }
        if self.pending.len() < MAX_PENDING_CALLS
            && let Some(call) = self.queued.pop_front()
        {
            self.pending.insert(call.planned.id.clone(), call.clone());
            return Ok(CollectorStep::Call(call.planned));
        }
        if !self.pending.is_empty() {
            return Ok(CollectorStep::AwaitingResults);
        }
        if self.metadata.is_some()
            && self.request.include_variables
            && self.variable_catalog.is_none()
            && self.variables.is_none()
            && self.queued.is_empty()
        {
            let node_id = self
                .root_node_id
                .clone()
                .ok_or_else(|| invalid_call("Figma 변수 수집에 사용할 root node ID가 없습니다."))?;
            self.enqueue(
                ReadToolCall::snapshot(
                    &self.request.target.file_key,
                    &node_id,
                    BuiltinScript::VariableCatalog,
                ),
                Some(node_id),
                CallKind::VariableCatalog,
            );
            return self.advance();
        }
        if self.metadata.is_some()
            && self.request.include_variables
            && self.variable_catalog.is_some()
            && self.variables.is_none()
            && self.queued.is_empty()
        {
            let catalog = self
                .variable_catalog
                .take()
                .ok_or_else(|| invalid_call("Figma 변수 catalog가 없습니다."))?;
            self.variables = Some(merge_variable_results(
                catalog,
                std::mem::take(&mut self.variable_batches).into_values(),
            ));
        }
        if self.metadata.is_some() && self.queued.is_empty() {
            self.completed = true;
            return Ok(CollectorStep::Complete(Box::new(CollectedParts {
                target: self.request.target.clone(),
                scope: self.request.scope,
                metadata: self.metadata.take().unwrap_or(Value::Null),
                snapshot_chunks: std::mem::take(&mut self.snapshot_chunks)
                    .into_values()
                    .collect(),
                variables: self.variables.clone(),
                styles: self.variables.clone(),
                source_version: self.source_version.clone(),
            })));
        }
        Ok(CollectorStep::AwaitingResults)
    }

    pub fn accept(&mut self, call_id: &str, result: UpstreamResult) -> Result<(), DevupError> {
        let pending = self
            .pending
            .remove(call_id)
            .ok_or_else(|| invalid_call("알 수 없거나 이미 처리한 Figma call ID입니다."))?;
        self.consumed.insert(call_id.to_owned());
        match pending.kind {
            CallKind::Metadata => self.accept_metadata(result),
            CallKind::Snapshot => self.accept_snapshot(&pending.planned, pending.order, result),
            CallKind::VariableCatalog => self.accept_variable_catalog(result),
            CallKind::VariableBatch => {
                self.variable_batches
                    .insert(pending.order, batch_from_result(&result)?);
                Ok(())
            }
        }
    }

    fn accept_metadata(&mut self, result: UpstreamResult) -> Result<(), DevupError> {
        let document = metadata_from_result_for_target(
            &result,
            &self.request.target.file_key,
            self.request.target.node_id.as_deref(),
        )?;
        if document.file_key != self.request.target.file_key {
            return Err(invalid_call("Figma metadata의 file key가 요청과 다릅니다."));
        }
        self.source_version = document.version.clone();
        self.root_node_id = Some(document.root_id.clone());
        self.metadata = Some(result.raw);
        let root = document.root().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "Figma metadata에서 대상 node를 찾지 못했습니다.",
                false,
            )
        })?;
        let split = !root.children_ids.is_empty()
            && (root.descendant_count > LARGE_SUBTREE_THRESHOLD
                || matches!(
                    self.request.scope,
                    CollectionScope::Page | CollectionScope::File
                ));
        let targets = if split {
            root.children_ids.clone()
        } else {
            vec![root.id.clone()]
        };
        for node_id in targets {
            self.enqueue(
                ReadToolCall::snapshot(
                    &self.request.target.file_key,
                    &node_id,
                    BuiltinScript::NodeSnapshot,
                ),
                Some(node_id),
                CallKind::Snapshot,
            );
        }
        Ok(())
    }

    fn accept_snapshot(
        &mut self,
        planned: &PlannedCall,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let chunk = snapshot_chunk_from_result(&result)?;
        if chunk.file_key != planned.expected_file_key {
            return Err(invalid_call("Figma snapshot의 file key가 요청과 다릅니다."));
        }
        if chunk.version != self.source_version {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaVersionChanged,
                "수집 중 Figma 파일 버전이 변경되었습니다.",
                true,
            ));
        }
        self.snapshot_chunks.insert(order, chunk);
        Ok(())
    }

    fn accept_variable_catalog(&mut self, result: UpstreamResult) -> Result<(), DevupError> {
        let catalog = catalog_from_result(&result)?;
        let node_id = self
            .root_node_id
            .clone()
            .ok_or_else(|| invalid_call("Figma 변수 batch에 사용할 root node ID가 없습니다."))?;
        let resources = catalog
            .variable_ids
            .iter()
            .cloned()
            .map(ResourceItem::Variable)
            .chain(catalog.styles.iter().cloned().map(ResourceItem::Style))
            .collect::<Vec<_>>();
        for chunk in resources.chunks(VARIABLE_BATCH_SIZE) {
            let mut batch = ResourceBatch {
                variable_ids: Vec::new(),
                styles: Vec::new(),
            };
            for resource in chunk {
                match resource {
                    ResourceItem::Variable(id) => batch.variable_ids.push(id.clone()),
                    ResourceItem::Style(style) => batch.styles.push(style.clone()),
                }
            }
            self.enqueue(
                ReadToolCall::resource_batch(&self.request.target.file_key, &node_id, batch),
                Some(node_id.clone()),
                CallKind::VariableBatch,
            );
        }
        self.variable_catalog = Some(catalog);
        Ok(())
    }

    fn enqueue(&mut self, call: ReadToolCall, expected_node_id: Option<String>, kind: CallKind) {
        let order = self.next_id;
        let id = format!("call-{order}");
        self.next_id += 1;
        self.queued.push_back(PendingCall {
            planned: PlannedCall {
                id,
                call,
                expected_file_key: self.request.target.file_key.clone(),
                expected_node_id,
            },
            kind,
            order,
        });
    }
}

#[derive(Debug, Clone)]
enum ResourceItem {
    Variable(String),
    Style(ResourceStyleRef),
}

fn invalid_call(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupFigmaHandoffInvalid, message, false)
}
