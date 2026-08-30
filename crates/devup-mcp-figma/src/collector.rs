use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    BuiltinScript, DevupError, ErrorCode, FigmaTarget, ReadToolCall, SnapshotChunk, UpstreamResult,
    metadata::metadata_from_result, snapshot_chunk_from_result,
};

const LARGE_SUBTREE_THRESHOLD: usize = 200;
const MAX_PENDING_CALLS: usize = 4;

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
    source_version: Option<String>,
    snapshot_chunks: BTreeMap<usize, SnapshotChunk>,
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
            source_version: None,
            snapshot_chunks: BTreeMap::new(),
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
        if self.metadata.is_some() && self.queued.is_empty() {
            self.completed = true;
            return Ok(CollectorStep::Complete(Box::new(CollectedParts {
                target: self.request.target.clone(),
                scope: self.request.scope,
                metadata: self.metadata.take().unwrap_or(Value::Null),
                snapshot_chunks: std::mem::take(&mut self.snapshot_chunks)
                    .into_values()
                    .collect(),
                variables: None,
                styles: None,
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
        }
    }

    fn accept_metadata(&mut self, result: UpstreamResult) -> Result<(), DevupError> {
        let document = metadata_from_result(&result)?;
        if document.file_key != self.request.target.file_key {
            return Err(invalid_call("Figma metadata의 file key가 요청과 다릅니다."));
        }
        self.source_version = document.version.clone();
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

fn invalid_call(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupFigmaHandoffInvalid, message, false)
}
