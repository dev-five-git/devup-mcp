use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    BuiltinScript, DevupError, ErrorCode, ExploreReadOptions, FigmaTarget, RawNode, ReadToolCall,
    ResourceBatch, ResourceScope, ResourceStyleRef, SearchReadOptions, SnapshotChunk,
    SnapshotReadOptions, UnresolvedResource, UpstreamResult, UsedResourceRefs,
    collect_used_resource_refs, decode_fast_snapshot, decode_fast_theme,
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
const USED_RESOURCE_BATCH_ITEMS: usize = 64;
const USED_RESOURCE_BATCH_BYTES: usize = 12_000;
// Consumer relations can be huge. Compact, bounded fragments are expanded
// back to the exhaustive shape in Rust without dropping any relation.
const STYLE_CONSUMER_BATCH_SIZE: usize = 320;
const SNAPSHOT_CURSOR_ID: &str = "__DEVUP_SNAPSHOT_CURSOR__";

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
    pub stats: CollectionStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionStats {
    pub figma_tool_calls: usize,
    pub transport: String,
    pub fallback_used: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub node_count: usize,
    pub variable_count: usize,
    pub style_count: usize,
    pub raw_bytes: usize,
    pub wire_bytes: usize,
    pub envelope_chunks: usize,
}

impl Default for CollectionStats {
    fn default() -> Self {
        Self {
            figma_tool_calls: 0,
            transport: "legacy-cursor".to_owned(),
            fallback_used: false,
            fallback_reason: None,
            node_count: 0,
            variable_count: 0,
            style_count: 0,
            raw_bytes: 0,
            wire_bytes: 0,
            envelope_chunks: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum CollectorStep {
    Call(PlannedCall),
    AwaitingResults,
    Complete(Box<CollectedParts>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallKind {
    FastSnapshot,
    FastTheme,
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
    stats: CollectionStats,
    fast_attempted: bool,
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
            stats: CollectionStats::default(),
            fast_attempted: false,
            next_id: 0,
            completed: false,
        }
    }

    pub fn advance(&mut self) -> Result<CollectorStep, DevupError> {
        if self.completed {
            return Err(invalid_call("완료된 Figma 수집 session입니다."));
        }
        if self.metadata.is_none() && self.pending.is_empty() && self.queued.is_empty() {
            if self.fast_theme_eligible() && !self.fast_attempted {
                self.fast_attempted = true;
                self.enqueue(
                    ReadToolCall::fast_theme(&self.request.target.file_key),
                    None,
                    CallKind::FastTheme,
                );
                return self.advance();
            }
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
            if self.fast_path_eligible() && !self.fast_attempted {
                let node_id =
                    self.request.target.node_id.clone().ok_or_else(|| {
                        invalid_call("Figma fast snapshot에는 node ID가 필요합니다.")
                    })?;
                self.fast_attempted = true;
                self.enqueue(
                    ReadToolCall::fast_snapshot(&self.request.target.file_key, &node_id),
                    Some(node_id),
                    CallKind::FastSnapshot,
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
            self.stats.figma_tool_calls = self.stats.figma_tool_calls.saturating_add(1);
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
            self.finish_stats(&snapshot_chunks);
            return Ok(CollectorStep::Complete(Box::new(CollectedParts {
                target: self.request.target.clone(),
                scope: self.request.scope,
                metadata: self.metadata.take().unwrap_or(Value::Null),
                snapshot_chunks,
                variables: self.variables.clone(),
                styles: self.variables.clone(),
                source_version: self.source_version.clone(),
                stats: self.stats.clone(),
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
            CallKind::FastSnapshot => self.accept_fast_snapshot(pending.order, result),
            CallKind::FastTheme => self.accept_fast_theme(result),
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

    pub fn reject(&mut self, call_id: &str, error: &DevupError) -> Result<bool, DevupError> {
        let Some(pending) = self.pending.get(call_id) else {
            return Err(invalid_call(
                "알 수 없거나 이미 처리한 Figma call ID입니다.",
            ));
        };
        if !matches!(pending.kind, CallKind::FastSnapshot | CallKind::FastTheme) {
            return Ok(false);
        }
        if !fast_call_fallback_allowed(error) {
            return Ok(false);
        }
        self.pending.remove(call_id);
        self.consumed.insert(call_id.to_owned());
        self.restart_legacy(fallback_category(error));
        Ok(true)
    }

    fn fast_path_eligible(&self) -> bool {
        self.request.scope == CollectionScope::Node
            && self.request.resource_scope == ResourceScope::Used
            && self.request.target.node_id.is_some()
            && !self.request.include_context
            && !self.request.metadata_only
            && !self.request.variables_only
            && self.request.search.is_none()
            && self.request.explore.is_none()
    }

    fn fast_theme_eligible(&self) -> bool {
        self.request.scope == CollectionScope::File
            && self.request.resource_scope == ResourceScope::File
            && self.request.variables_only
            && !self.request.metadata_only
            && !self.request.include_context
            && self.request.search.is_none()
            && self.request.explore.is_none()
    }

    fn accept_fast_theme(&mut self, result: UpstreamResult) -> Result<(), DevupError> {
        let payload = match decode_fast_theme(&result, &self.request.target.file_key) {
            Ok(payload) => payload,
            Err(error) => {
                self.restart_legacy(fallback_category(&error));
                return Ok(());
            }
        };
        self.metadata = Some(json!({
            "transport": "png-theme-envelope-v1",
            "collectionCount": payload.resources.raw["collections"]
                .as_array().map_or(0, Vec::len),
            "variableCount": payload.resources.raw["variables"]
                .as_array().map_or(0, Vec::len),
            "styleCount": payload.resources.raw["styles"]
                .as_array().map_or(0, Vec::len)
        }));
        self.source_version = payload.source_version;
        self.stats.transport = "png-theme-envelope-v1".to_owned();
        self.stats.raw_bytes = payload.stats.raw_bytes;
        self.stats.wire_bytes = payload.stats.wire_bytes;
        self.stats.envelope_chunks = payload.stats.chunk_count;
        self.variables = Some(payload.resources);
        Ok(())
    }

    fn accept_fast_snapshot(
        &mut self,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let payload = match decode_fast_snapshot(&result, &self.request.target) {
            Ok(payload) => payload,
            Err(error) => {
                self.restart_legacy(fallback_category(&error));
                return Ok(());
            }
        };
        let root_id = self
            .request
            .target
            .node_id
            .clone()
            .ok_or_else(|| invalid_call("Figma fast snapshot에는 node ID가 필요합니다."))?;
        self.metadata = Some(json!({
            "transport": "png-envelope-v1",
            "rootId": root_id,
            "nodeCount": payload.snapshot.nodes.len()
        }));
        self.root_node_id = Some(root_id);
        self.source_version = payload.snapshot.version.clone();
        self.metadata_root_ids = payload.snapshot.root_ids.clone();
        self.stats.transport = "png-envelope-v1".to_owned();
        self.stats.raw_bytes = payload.stats.raw_bytes;
        self.stats.wire_bytes = payload.stats.wire_bytes;
        self.stats.envelope_chunks = payload.stats.chunk_count;
        self.snapshot_chunks.insert(order, payload.snapshot);
        self.variables = Some(payload.resources);
        Ok(())
    }

    fn restart_legacy(&mut self, reason: String) {
        self.queued.clear();
        self.pending.clear();
        self.metadata = None;
        self.metadata_nodes.clear();
        self.metadata_root_ids.clear();
        self.root_node_id = None;
        self.source_version = None;
        self.snapshot_chunks.clear();
        self.variable_catalog = None;
        self.used_resource_refs = None;
        self.variable_batches.clear();
        self.variables = None;
        self.stats.transport = "legacy-cursor".to_owned();
        self.stats.fallback_used = true;
        self.stats.fallback_reason = Some(reason);
        self.stats.node_count = 0;
        self.stats.variable_count = 0;
        self.stats.style_count = 0;
        self.stats.raw_bytes = 0;
        self.stats.wire_bytes = 0;
        self.stats.envelope_chunks = 0;
    }

    fn finish_stats(&mut self, chunks: &[SnapshotChunk]) {
        self.stats.node_count = chunks
            .iter()
            .flat_map(|chunk| chunk.nodes.iter().map(|node| node.id.as_str()))
            .collect::<BTreeSet<_>>()
            .len();
        if let Some(resources) = &self.variables {
            self.stats.variable_count = resource_count(&resources.raw, "variables");
            self.stats.style_count = resource_count(&resources.raw, "styles");
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
        let mut chunk = snapshot_chunk_from_result(&result)?;
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
        let pagination = match &planned.call {
            ReadToolCall::Snapshot {
                script: BuiltinScript::NodeSnapshot,
                snapshot: Some(options),
                ..
            } => Some(options.clone()),
            _ => None,
        };
        if let Some(options) = pagination
            && let Some(cursor) = take_snapshot_cursor(&mut chunk)?
        {
            let expected_next = options
                .offset
                .checked_add(chunk.nodes.len())
                .ok_or_else(|| invalid_call("Figma snapshot cursor offset이 넘쳤습니다."))?;
            if cursor.next_offset != expected_next || cursor.next_offset > cursor.total_nodes {
                return Err(invalid_call(
                    "Figma snapshot cursor가 수집한 node 범위와 일치하지 않습니다.",
                ));
            }
            if cursor.complete != (cursor.next_offset >= cursor.total_nodes) {
                return Err(invalid_call(
                    "Figma snapshot cursor의 완료 상태가 node 수와 일치하지 않습니다.",
                ));
            }
            if !cursor.complete {
                if chunk.nodes.is_empty() {
                    return Err(invalid_call("Figma snapshot cursor가 진행되지 않았습니다."));
                }
                let node_id = planned.expected_node_id.clone().ok_or_else(|| {
                    invalid_call("Figma snapshot cursor의 root node ID가 없습니다.")
                })?;
                self.enqueue(
                    ReadToolCall::snapshot_chunk(
                        &self.request.target.file_key,
                        &node_id,
                        SnapshotReadOptions {
                            offset: cursor.next_offset,
                            ..options
                        },
                    ),
                    Some(node_id),
                    CallKind::Snapshot,
                );
            }
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
        for batch in used_resource_batches(&refs)? {
            self.enqueue(
                ReadToolCall::used_resources(&self.request.target.file_key, &node_id, batch),
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
                severity: Some(crate::DiagnosticSeverity::Warning),
                property: Some(occurrence.field.clone()),
                resource_kind: Some(
                    match occurrence.resource_kind {
                        crate::ResourceKind::Variable => "variable",
                        crate::ResourceKind::Style => "style",
                    }
                    .to_owned(),
                ),
                resource_id: Some(occurrence.resource_id.clone()),
                fallback: Some("raw-value".to_owned()),
                recoverable: Some(true),
                ..crate::Diagnostic::default()
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

#[derive(Debug, Clone, Copy)]
struct SnapshotCursor {
    next_offset: usize,
    complete: bool,
    total_nodes: usize,
}

fn take_snapshot_cursor(chunk: &mut SnapshotChunk) -> Result<Option<SnapshotCursor>, DevupError> {
    let positions = chunk
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| (node.id == SNAPSHOT_CURSOR_ID).then_some(index))
        .collect::<Vec<_>>();
    let Some(&position) = positions.first() else {
        return Ok(None);
    };
    if positions.len() != 1 {
        return Err(invalid_call(
            "Figma snapshot 응답에 cursor가 중복되었습니다.",
        ));
    }
    let cursor = chunk.nodes.remove(position);
    if cursor.node_type != "DEVUP_INTERNAL" {
        return Err(invalid_call(
            "Figma snapshot cursor 형식이 올바르지 않습니다.",
        ));
    }
    let cursor = cursor.typed_view();
    let next_offset = cursor
        .value("nextOffset")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| invalid_call("Figma snapshot cursor의 nextOffset이 없습니다."))?;
    let complete = cursor
        .bool("complete")
        .ok_or_else(|| invalid_call("Figma snapshot cursor의 complete가 없습니다."))?;
    let total_nodes = cursor
        .value("totalNodes")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| invalid_call("Figma snapshot cursor의 totalNodes가 없습니다."))?;
    Ok(Some(SnapshotCursor {
        next_offset,
        complete,
        total_nodes,
    }))
}

fn invalid_call(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupFigmaHandoffInvalid, message, false)
}

fn fallback_category(error: &DevupError) -> String {
    error
        .details
        .get("category")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{:?}", error.code))
}

fn fast_call_fallback_allowed(error: &DevupError) -> bool {
    matches!(
        error.code,
        ErrorCode::DevupFigmaDirectUnavailable
            | ErrorCode::DevupFigmaCatalogRejected
            | ErrorCode::DevupFigmaResponseTooLarge
            | ErrorCode::DevupSnapshotUnsupported
            | ErrorCode::DevupFigmaHandoffInvalid
    )
}

fn resource_count(value: &Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

#[derive(Debug, Clone)]
enum UsedResourceItem {
    Variable(String),
    Style(ResourceStyleRef),
}

fn used_resource_batches(refs: &UsedResourceRefs) -> Result<Vec<ResourceBatch>, DevupError> {
    let items = refs
        .variable_ids
        .iter()
        .cloned()
        .map(UsedResourceItem::Variable)
        .chain(refs.styles.iter().cloned().map(UsedResourceItem::Style));
    let mut batches = Vec::new();
    let mut current = ResourceBatch {
        variable_ids: Vec::new(),
        styles: Vec::new(),
    };

    for item in items {
        let mut candidate = current.clone();
        add_used_resource(&mut candidate, item.clone());
        if used_resource_batch_fits(&candidate) {
            current = candidate;
            continue;
        }
        if current.variable_ids.is_empty() && current.styles.is_empty() {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "단일 Figma 리소스 ID가 안전한 batch 크기를 초과했습니다.",
                false,
            ));
        }
        batches.push(current);
        current = ResourceBatch {
            variable_ids: Vec::new(),
            styles: Vec::new(),
        };
        add_used_resource(&mut current, item);
        if !used_resource_batch_fits(&current) {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "단일 Figma 리소스 ID가 안전한 batch 크기를 초과했습니다.",
                false,
            ));
        }
    }
    if !current.variable_ids.is_empty() || !current.styles.is_empty() {
        batches.push(current);
    }
    Ok(batches)
}

fn add_used_resource(batch: &mut ResourceBatch, item: UsedResourceItem) {
    match item {
        UsedResourceItem::Variable(id) => batch.variable_ids.push(id),
        UsedResourceItem::Style(style) => batch.styles.push(style),
    }
}

fn used_resource_batch_fits(batch: &ResourceBatch) -> bool {
    batch.variable_ids.len() + batch.styles.len() <= USED_RESOURCE_BATCH_ITEMS
        && serde_json::to_vec(batch).is_ok_and(|bytes| bytes.len() <= USED_RESOURCE_BATCH_BYTES)
}
