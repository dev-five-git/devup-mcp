use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    BuiltinScript, DevupError, ErrorCode, ExploreReadOptions, FigmaTarget, RawNode, ReadToolCall,
    ResourceBatch, ResourceScope, ResourceStyleRef, SearchReadOptions, SnapshotChunk,
    UnresolvedResource, UpstreamResult, UsedResourceRefs, collect_used_resource_refs,
    metadata::{MetadataResult, metadata_from_result_for_target},
    snapshot_chunk_from_result,
    variables::{
        VariableBatchResult, VariableCatalog, batch_from_result, catalog_from_result,
        merge_used_resource_results, merge_variable_results,
    },
};

const LARGE_SUBTREE_THRESHOLD: usize = 200;
const MAX_PENDING_CALLS: usize = 4;
const VARIABLE_BATCH_SIZE: usize = 8;
const STYLE_BATCH_SIZE: usize = 8;
// Consumer relations can be huge. Compact, bounded fragments are expanded
// back to the exhaustive shape in Rust without dropping any relation.
const STYLE_CONSUMER_BATCH_SIZE: usize = 320;

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
    pub resource_scope: ResourceScope,
    pub include_context: bool,
    pub metadata_only: bool,
    pub variables_only: bool,
    pub search: Option<SearchReadOptions>,
    pub explore: Option<ExploreReadOptions>,
}

impl CollectionRequest {
    pub fn new(target: FigmaTarget, scope: CollectionScope) -> Self {
        Self {
            target,
            scope,
            resource_scope: ResourceScope::None,
            include_context: false,
            metadata_only: false,
            variables_only: false,
            search: None,
            explore: None,
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
    PageCatalog,
    Snapshot,
    VariableCatalog,
    VariableBatch,
    UsedResourceBatch,
    Explore,
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
    metadata_nodes: BTreeMap<String, RawNode>,
    metadata_root_ids: Vec<String>,
    root_node_id: Option<String>,
    source_version: Option<String>,
    snapshot_chunks: BTreeMap<usize, SnapshotChunk>,
    variable_catalog: Option<VariableCatalog>,
    used_resource_refs: Option<UsedResourceRefs>,
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
            metadata_nodes: BTreeMap::new(),
            metadata_root_ids: Vec::new(),
            root_node_id: None,
            source_version: None,
            snapshot_chunks: BTreeMap::new(),
            variable_catalog: None,
            used_resource_refs: None,
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
            if let Some(options) = self.request.explore.clone() {
                let node_id = self.request.target.node_id.clone().ok_or_else(|| {
                    invalid_call("Figma 주변 화면 탐색에는 node ID가 필요합니다.")
                })?;
                self.enqueue(
                    ReadToolCall::explore_snapshot(
                        &self.request.target.file_key,
                        &node_id,
                        options,
                    ),
                    Some(node_id),
                    CallKind::Explore,
                );
                return self.advance();
            }
            if self.request.search.is_some() {
                self.enqueue(
                    ReadToolCall::page_catalog(&self.request.target.file_key),
                    None,
                    CallKind::PageCatalog,
                );
                return self.advance();
            }
            let node_id = match self.request.scope {
                CollectionScope::File => None,
                CollectionScope::Node | CollectionScope::Page => {
                    self.request.target.node_id.as_deref()
                }
            };
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
            && self.request.resource_scope == ResourceScope::File
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
            && self.request.resource_scope == ResourceScope::File
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
            )?);
        }
        if self.metadata.is_some()
            && self.request.resource_scope == ResourceScope::Used
            && self.used_resource_refs.is_none()
            && self.variables.is_none()
            && self.queued.is_empty()
        {
            self.enqueue_used_resource_batches()?;
            if !self.queued.is_empty() {
                return self.advance();
            }
        }
        if self.metadata.is_some()
            && self.request.resource_scope == ResourceScope::Used
            && self.used_resource_refs.is_some()
            && self.variables.is_none()
            && self.queued.is_empty()
        {
            let refs = self
                .used_resource_refs
                .take()
                .ok_or_else(|| invalid_call("사용된 Figma 리소스 참조가 없습니다."))?;
            let merged = merge_used_resource_results(
                &refs,
                std::mem::take(&mut self.variable_batches).into_values(),
            )?;
            self.record_unresolved_diagnostics(&refs, &merged.unresolved);
            self.variables = Some(merged.result);
        }
        if self.metadata.is_some() && self.queued.is_empty() {
            self.completed = true;
            let snapshot_chunks = if self.request.metadata_only || self.request.variables_only {
                vec![SnapshotChunk {
                    file_key: self.request.target.file_key.clone(),
                    version: self.source_version.clone(),
                    root_ids: std::mem::take(&mut self.metadata_root_ids),
                    nodes: std::mem::take(&mut self.metadata_nodes)
                        .into_values()
                        .collect(),
                    diagnostics: Vec::new(),
                }]
            } else {
                std::mem::take(&mut self.snapshot_chunks)
                    .into_values()
                    .collect()
            };
            return Ok(CollectorStep::Complete(Box::new(CollectedParts {
                target: self.request.target.clone(),
                scope: self.request.scope,
                metadata: self.metadata.take().unwrap_or(Value::Null),
                snapshot_chunks,
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
            CallKind::Metadata => self.accept_metadata(&pending.planned, result),
            CallKind::PageCatalog => self.accept_page_catalog(&pending.planned, result),
            CallKind::Snapshot => self.accept_snapshot(&pending.planned, pending.order, result),
            CallKind::VariableCatalog => self.accept_variable_catalog(result),
            CallKind::VariableBatch => {
                let batch = batch_from_result(&result)?;
                self.enqueue_style_consumer_batches(&batch)?;
                self.variable_batches.insert(pending.order, batch);
                Ok(())
            }
            CallKind::UsedResourceBatch => {
                let batch = batch_from_result(&result)?;
                self.variable_batches.insert(pending.order, batch);
                Ok(())
            }
            CallKind::Explore => self.accept_explore(&pending.planned, pending.order, result),
        }
    }

    fn accept_page_catalog(
        &mut self,
        planned: &PlannedCall,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let catalog = snapshot_chunk_from_result(&result)?;
        if catalog.file_key != planned.expected_file_key {
            return Err(invalid_call(
                "Figma page catalog의 file key가 요청과 다릅니다.",
            ));
        }
        if catalog.root_ids.is_empty() {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "Figma page catalog가 비어 있습니다.",
                false,
            ));
        }
        let options = self
            .request
            .search
            .clone()
            .ok_or_else(|| invalid_call("검색 설정 없이 page catalog를 수집했습니다."))?;
        self.metadata = Some(result.raw);
        self.root_node_id = catalog.root_ids.first().cloned();
        self.source_version = catalog.version.clone();
        self.metadata_root_ids = catalog.root_ids.clone();
        for page in catalog.nodes {
            self.metadata_nodes.insert(page.id.clone(), page);
        }
        for page_id in catalog.root_ids {
            self.enqueue(
                ReadToolCall::search_snapshot(
                    &self.request.target.file_key,
                    &page_id,
                    options.clone(),
                ),
                Some(page_id),
                CallKind::Snapshot,
            );
        }
        Ok(())
    }

    fn accept_explore(
        &mut self,
        planned: &PlannedCall,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let chunk = snapshot_chunk_from_result(&result)?;
        if chunk.file_key != planned.expected_file_key {
            return Err(invalid_call(
                "Figma 탐색 projection의 file key가 요청과 다릅니다.",
            ));
        }
        let expected_node_id = planned
            .expected_node_id
            .as_deref()
            .ok_or_else(|| invalid_call("Figma 탐색 projection의 expected node ID가 없습니다."))?;
        if !chunk.nodes.iter().any(|node| node.id == expected_node_id) {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "Figma 탐색 projection에서 anchor node를 찾지 못했습니다.",
                false,
            ));
        }
        self.source_version = chunk.version.clone();
        self.root_node_id = Some(expected_node_id.to_owned());
        self.metadata_root_ids = chunk.root_ids.clone();
        self.metadata = Some(result.raw);
        self.snapshot_chunks.insert(order, chunk);
        Ok(())
    }

    fn accept_metadata(
        &mut self,
        planned: &PlannedCall,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let metadata = metadata_from_result_for_target(
            &result,
            &self.request.target.file_key,
            planned.expected_node_id.as_deref(),
        )?;
        if let MetadataResult::TopLevelPages(pages) = metadata {
            if planned.expected_node_id.is_some() {
                return Err(invalid_call(
                    "page metadata 요청에 top-level page 목록이 반환되었습니다.",
                ));
            }
            self.record_metadata(result.raw);
            self.root_node_id = pages.first().map(|page| page.id.clone());
            for page in &pages {
                self.record_metadata_node(page);
                if !self.metadata_root_ids.contains(&page.id) {
                    self.metadata_root_ids.push(page.id.clone());
                }
            }
            if let Some(options) = self.request.search.clone() {
                for page in pages {
                    self.enqueue(
                        ReadToolCall::search_snapshot(
                            &self.request.target.file_key,
                            &page.id,
                            options.clone(),
                        ),
                        Some(page.id),
                        CallKind::Snapshot,
                    );
                }
                return Ok(());
            }
            if self.request.variables_only {
                return Ok(());
            }
            for page in pages {
                self.enqueue(
                    ReadToolCall::metadata(&self.request.target.file_key, Some(&page.id)),
                    Some(page.id),
                    CallKind::Metadata,
                );
            }
            return Ok(());
        }
        let MetadataResult::Document(document) = metadata else {
            unreachable!("top-level page metadata is handled above")
        };
        if document.file_key != self.request.target.file_key {
            return Err(invalid_call("Figma metadata의 file key가 요청과 다릅니다."));
        }
        if let (Some(existing), Some(incoming)) = (&self.source_version, &document.version)
            && existing != incoming
        {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaVersionChanged,
                "metadata 수집 중 Figma 파일 버전이 변경되었습니다.",
                true,
            ));
        }
        if document.version.is_some() {
            self.source_version = document.version.clone();
        }
        if self.root_node_id.is_none() {
            self.root_node_id = Some(document.root_id.clone());
        }
        if !self.metadata_root_ids.contains(&document.root_id) {
            self.metadata_root_ids.push(document.root_id.clone());
        }
        for node in &document.nodes {
            self.record_metadata_node(node);
        }
        self.record_metadata(result.raw);
        let root = document.root().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "Figma metadata에서 대상 node를 찾지 못했습니다.",
                false,
            )
        })?;
        if let Some(options) = self.request.search.clone() {
            self.enqueue(
                ReadToolCall::search_snapshot(&self.request.target.file_key, &root.id, options),
                Some(root.id.clone()),
                CallKind::Snapshot,
            );
            return Ok(());
        }
        if self.request.metadata_only || self.request.variables_only {
            return Ok(());
        }
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

    fn record_metadata(&mut self, value: Value) {
        self.metadata = Some(match self.metadata.take() {
            None => value,
            Some(Value::Array(mut values)) => {
                values.push(value);
                Value::Array(values)
            }
            Some(existing) => Value::Array(vec![existing, value]),
        });
    }

    fn record_metadata_node(&mut self, node: &crate::metadata::MetadataNode) {
        let mut fields = Map::new();
        if let Some(name) = &node.name {
            fields.insert("name".to_owned(), Value::String(name.clone()));
        }
        fields.insert(
            "childrenIds".to_owned(),
            Value::Array(
                node.children_ids
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        fields.insert(
            "descendantCount".to_owned(),
            Value::from(node.descendant_count),
        );
        self.metadata_nodes.insert(
            node.id.clone(),
            RawNode {
                id: node.id.clone(),
                node_type: node.node_type.clone(),
                fields,
                extra: Map::new(),
                field_errors: BTreeMap::new(),
            },
        );
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
        for variable_ids in catalog.variable_ids.chunks(VARIABLE_BATCH_SIZE) {
            self.enqueue(
                ReadToolCall::resource_batch(
                    &self.request.target.file_key,
                    &node_id,
                    ResourceBatch {
                        variable_ids: variable_ids.to_vec(),
                        styles: Vec::new(),
                    },
                ),
                Some(node_id.clone()),
                CallKind::VariableBatch,
            );
        }
        for styles in catalog.styles.chunks(STYLE_BATCH_SIZE) {
            self.enqueue(
                ReadToolCall::resource_batch(
                    &self.request.target.file_key,
                    &node_id,
                    ResourceBatch {
                        variable_ids: Vec::new(),
                        styles: styles.to_vec(),
                    },
                ),
                Some(node_id.clone()),
                CallKind::VariableBatch,
            );
        }
        self.variable_catalog = Some(catalog);
        Ok(())
    }

    fn enqueue_used_resource_batches(&mut self) -> Result<(), DevupError> {
        let refs =
            collect_used_resource_refs(&self.snapshot_chunks.values().cloned().collect::<Vec<_>>());
        let node_id = self.root_node_id.clone().ok_or_else(|| {
            invalid_call("사용된 Figma 리소스 batch에 사용할 root node ID가 없습니다.")
        })?;
        for variable_ids in refs.variable_ids.chunks(VARIABLE_BATCH_SIZE) {
            self.enqueue(
                ReadToolCall::used_resources(
                    &self.request.target.file_key,
                    &node_id,
                    ResourceBatch {
                        variable_ids: variable_ids.to_vec(),
                        styles: Vec::new(),
                    },
                ),
                Some(node_id.clone()),
                CallKind::UsedResourceBatch,
            );
        }
        for styles in refs.styles.chunks(STYLE_BATCH_SIZE) {
            self.enqueue(
                ReadToolCall::used_resources(
                    &self.request.target.file_key,
                    &node_id,
                    ResourceBatch {
                        variable_ids: Vec::new(),
                        styles: styles.to_vec(),
                    },
                ),
                Some(node_id.clone()),
                CallKind::UsedResourceBatch,
            );
        }
        self.used_resource_refs = Some(refs);
        Ok(())
    }

    fn record_unresolved_diagnostics(
        &mut self,
        refs: &UsedResourceRefs,
        unresolved: &[UnresolvedResource],
    ) {
        let unresolved = unresolved
            .iter()
            .map(|resource| (resource.kind, resource.id.as_str()))
            .collect::<BTreeSet<_>>();
        for occurrence in &refs.occurrences {
            if !unresolved.contains(&(occurrence.resource_kind, occurrence.resource_id.as_str())) {
                continue;
            }
            let diagnostic = crate::Diagnostic {
                code: "DEVUP_RESOURCE_UNRESOLVED".to_owned(),
                message: format!(
                    "Figma 리소스를 확인할 수 없어 raw 값으로 대체했습니다: field={}, resourceId={}",
                    occurrence.field, occurrence.resource_id
                ),
                node_id: Some(occurrence.node_id.clone()),
            };
            if let Some(chunk) = self
                .snapshot_chunks
                .values_mut()
                .find(|chunk| chunk.nodes.iter().any(|node| node.id == occurrence.node_id))
            {
                chunk.diagnostics.push(diagnostic);
            }
        }
    }

    fn enqueue_style_consumer_batches(
        &mut self,
        batch: &VariableBatchResult,
    ) -> Result<(), DevupError> {
        let node_id = self.root_node_id.clone().ok_or_else(|| {
            invalid_call("Figma style consumer 수집에 사용할 root node ID가 없습니다.")
        })?;
        for style in &batch.styles {
            let Some(object) = style.as_object() else {
                return Err(invalid_call("Figma style batch 형식이 올바르지 않습니다."));
            };
            let Some(consumer_count) = object.get("$consumerCount").and_then(Value::as_u64) else {
                continue;
            };
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_call("Figma style ID가 없습니다."))?;
            let style_type = object
                .get("styleType")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_call("Figma style type이 없습니다."))?;
            for start in (0..consumer_count as usize).step_by(STYLE_CONSUMER_BATCH_SIZE) {
                let end = (start + STYLE_CONSUMER_BATCH_SIZE).min(consumer_count as usize);
                self.enqueue(
                    ReadToolCall::resource_batch(
                        &self.request.target.file_key,
                        &node_id,
                        ResourceBatch {
                            variable_ids: Vec::new(),
                            styles: vec![ResourceStyleRef {
                                id: id.to_owned(),
                                style_type: style_type.to_owned(),
                                consumer_start: Some(start),
                                consumer_end: Some(end),
                            }],
                        },
                    ),
                    Some(node_id.clone()),
                    CallKind::VariableBatch,
                );
            }
        }
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
