use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Cursor,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ImageDecoder, ImageError, Limits, codecs::png::PngDecoder};
use rmcp::model::{CallToolResult, ContentBlock};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::large_values::{
    LargeValueResult, descriptors_in_chunk, large_value_from_result, replace_descriptor,
};
use crate::{
    AssetManifestEntry, AssetRequest, AssetSelection, AssetStatus, BatchLimits, BuiltinScript,
    DevupError, ErrorCode, ExploreReadOptions, FigmaTarget, LargeValueAssembler,
    LargeValueReadOptions, RawNode, ReadToolCall, ResourceBatch, ResourceScope, ResourceStyleRef,
    SNAPSHOT_CURSOR_ID, SearchReadOptions, SectionIndex, SnapshotChunk, SnapshotCursor,
    SnapshotReadOptions, UnresolvedResource, UpstreamResult, UsedResourceRefs,
    asset_export_from_result, build_section_index, collect_used_resource_refs,
    decode_fast_multi_snapshot, decode_fast_snapshot, decode_fast_theme, merge_chunks,
    metadata::{MetadataResult, metadata_from_result_for_target},
    plan_batches, read_snapshot_cursor, resolve_asset_selections, snapshot_chunk_from_result,
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

const MAX_REFERENCE_PNG_BYTES: usize = 16 * 1024 * 1024;
const MAX_REFERENCE_PNG_BASE64_BYTES: usize = MAX_REFERENCE_PNG_BYTES.div_ceil(3) * 4;
const MAX_REFERENCE_PNG_DIMENSION: u32 = 8_192;
const MAX_REFERENCE_PNG_DECODED_BYTES: usize = 64 * 1024 * 1024;

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
    pub section: Option<SectionReadOptions>,
    pub cached_section_index: Option<SectionIndex>,
    pub asset_selections: Vec<AssetSelection>,
    pub reference_png: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionReadOptions {
    #[serde(default)]
    pub frame_ids: Vec<String>,
    #[serde(default)]
    pub all_screens: bool,
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
            section: None,
            cached_section_index: None,
            asset_selections: Vec::new(),
            reference_png: false,
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
    pub assets: Vec<AssetManifestEntry>,
    pub reference_png: Option<ReferencePng>,
    pub failures: Vec<ScreenFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenFailure {
    pub node_id: String,
    pub error_code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferencePng {
    pub mime_type: String,
    pub data_base64: String,
    pub byte_length: usize,
    pub sha256: String,
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
            // Text (optionally paginated) is the default, primary path now;
            // "legacy-cursor" only ever appears once a fast call actually
            // falls back (see `restart_legacy`).
            transport: "text".to_owned(),
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
    Call(Box<PlannedCall>),
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
    SectionIndex,
    FastMultiRoot,
    LargeValue,
    Asset,
    Screenshot,
}

#[derive(Debug, Clone)]
struct PendingCall {
    planned: PlannedCall,
    kind: CallKind,
    order: usize,
}

#[derive(Debug, Clone)]
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
    large_values: BTreeMap<(String, String), LargeValueAssembler>,
    asset_results: Vec<AssetManifestEntry>,
    assets_scheduled: bool,
    reference_png: Option<ReferencePng>,
    reference_png_scheduled: bool,
    stats: CollectionStats,
    fast_attempted: bool,
    section_index: Option<SectionIndex>,
    section_selected_roots: Vec<String>,
    fast_multi_resources: Option<UpstreamResult>,
    fast_multi_has_large_values: bool,
    /// Resources merged across rounds of the paginated single-root fast
    /// snapshot (`accept_fast_snapshot`). Distinct from `fast_multi_resources`,
    /// which is scoped to Section multi-root batching; the two paths are
    /// mutually exclusive (`fast_path_eligible` requires `section.is_none()`).
    fast_snapshot_resources: Option<UpstreamResult>,
    fast_snapshot_has_large_values: bool,
    fast_snapshot_rounds: usize,
    section_fallback_roots: BTreeSet<String>,
    screen_failures: Vec<ScreenFailure>,
    next_id: usize,
    completed: bool,
}

impl CollectorSession {
    pub fn new(request: CollectionRequest) -> Self {
        let assets_scheduled = request.asset_selections.is_empty();
        let reference_png_scheduled = !request.reference_png;
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
            large_values: BTreeMap::new(),
            asset_results: Vec::new(),
            assets_scheduled,
            reference_png: None,
            reference_png_scheduled,
            stats: CollectionStats::default(),
            fast_attempted: false,
            section_index: None,
            section_selected_roots: Vec::new(),
            fast_multi_resources: None,
            fast_multi_has_large_values: false,
            fast_snapshot_resources: None,
            fast_snapshot_has_large_values: false,
            fast_snapshot_rounds: 0,
            section_fallback_roots: BTreeSet::new(),
            screen_failures: Vec::new(),
            next_id: 0,
            completed: false,
        }
    }

    pub fn stats(&self) -> &CollectionStats {
        &self.stats
    }

    pub fn advance(&mut self) -> Result<CollectorStep, DevupError> {
        if self.completed {
            return Err(invalid_call("완료된 Figma 수집 session입니다."));
        }
        if self.section_index.is_none()
            && let Some(index) = self.request.cached_section_index.take()
        {
            self.prepare_cached_section_index(index)?;
            return self.advance();
        }
        if self.metadata.is_none() && self.pending.is_empty() && self.queued.is_empty() {
            if self.request.section.is_some() && self.section_index.is_none() {
                let node_id =
                    self.request.target.node_id.clone().ok_or_else(|| {
                        invalid_call("Figma Section index에는 node ID가 필요합니다.")
                    })?;
                self.enqueue(
                    ReadToolCall::section_index(&self.request.target.file_key, &node_id),
                    Some(node_id),
                    CallKind::SectionIndex,
                );
                return self.advance();
            }
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
        let pending_limit = if self.request.section.is_some() {
            2
        } else {
            MAX_PENDING_CALLS
        };
        if self.pending.len() < pending_limit
            && let Some(call) = self.queued.pop_front()
        {
            self.pending.insert(call.planned.id.clone(), call.clone());
            self.stats.figma_tool_calls = self.stats.figma_tool_calls.saturating_add(1);
            return Ok(CollectorStep::Call(Box::new(call.planned)));
        }
        if !self.pending.is_empty() {
            return Ok(CollectorStep::AwaitingResults);
        }
        if self.request.section.is_some()
            && !self.section_selected_roots.is_empty()
            && self.variables.is_none()
            && self.section_fallback_roots.is_empty()
            && !self.fast_multi_has_large_values
            && let Some(resources) = self.fast_multi_resources.take()
        {
            self.variables = Some(resources);
        }
        if self.metadata.is_some()
            && !self.assets_scheduled
            && !self.snapshot_chunks.is_empty()
            && self.queued.is_empty()
        {
            let snapshot = merge_chunks(self.snapshot_chunks.values().cloned().collect())?;
            let requests = resolve_asset_selections(&snapshot, &self.request.asset_selections)?;
            self.assets_scheduled = true;
            for request in requests {
                self.enqueue(
                    ReadToolCall::asset_export(
                        &self.request.target.file_key,
                        self.source_version.clone(),
                        request.clone(),
                    ),
                    Some(request.node_id.clone()),
                    CallKind::Asset,
                );
            }
            if !self.queued.is_empty() {
                return self.advance();
            }
        }
        if self.metadata.is_some()
            && !self.reference_png_scheduled
            && !self.snapshot_chunks.is_empty()
            && self.queued.is_empty()
        {
            let node_id = self.request.target.node_id.clone().ok_or_else(|| {
                invalid_call("Figma reference PNG 수집에는 node ID가 필요합니다.")
            })?;
            self.reference_png_scheduled = true;
            self.enqueue(
                ReadToolCall::screenshot(&self.request.target.file_key, &node_id),
                Some(node_id),
                CallKind::Screenshot,
            );
            return self.advance();
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
            let mut result = Some(merged.result);
            if !self.section_fallback_roots.is_empty()
                && !self.fast_multi_has_large_values
                && let Some(fast_resources) = self.fast_multi_resources.take()
            {
                let mut combined = Some(fast_resources);
                merge_fast_resources(
                    &mut combined,
                    result
                        .take()
                        .ok_or_else(|| invalid_call("fallback resource 결과가 없습니다."))?,
                )?;
                result = combined;
            }
            self.variables = result;
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
                let chunks = std::mem::take(&mut self.snapshot_chunks)
                    .into_values()
                    .collect();
                self.restore_section_visual_order(chunks)?
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
                assets: std::mem::take(&mut self.asset_results),
                reference_png: self.reference_png.take(),
                failures: std::mem::take(&mut self.screen_failures),
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
            CallKind::FastSnapshot => {
                self.accept_fast_snapshot(&pending.planned, pending.order, result)
            }
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
            CallKind::SectionIndex => {
                self.accept_section_index(&pending.planned, pending.order, result)
            }
            CallKind::FastMultiRoot => {
                self.accept_fast_multi_root(&pending.planned, pending.order, result)
            }
            CallKind::LargeValue => self.accept_large_value(&pending.planned, result),
            CallKind::Asset => self.accept_asset(&pending.planned, result),
            CallKind::Screenshot => self.accept_reference_png(&pending.planned, result),
        }
    }

    pub fn reject(&mut self, call_id: &str, error: &DevupError) -> Result<bool, DevupError> {
        let Some(pending) = self.pending.get(call_id) else {
            return Err(invalid_call(
                "알 수 없거나 이미 처리한 Figma call ID입니다.",
            ));
        };
        if pending.kind == CallKind::FastSnapshot && is_section_target_error(error) {
            let pending = self
                .pending
                .remove(call_id)
                .ok_or_else(|| invalid_call("fast Section probe call이 없습니다."))?;
            self.consumed.insert(call_id.to_owned());
            let node_id = pending
                .planned
                .expected_node_id
                .ok_or_else(|| invalid_call("fast Section probe의 node ID가 없습니다."))?;
            self.request.section = Some(SectionReadOptions {
                frame_ids: Vec::new(),
                all_screens: false,
            });
            self.enqueue(
                ReadToolCall::section_index(&self.request.target.file_key, &node_id),
                Some(node_id),
                CallKind::SectionIndex,
            );
            return Ok(true);
        }
        if pending.kind == CallKind::Asset {
            let pending = self
                .pending
                .remove(call_id)
                .ok_or_else(|| invalid_call("asset call이 없습니다."))?;
            self.consumed.insert(call_id.to_owned());
            let ReadToolCall::AssetExport { request, .. } = pending.planned.call else {
                return Err(invalid_call("asset call 형식이 올바르지 않습니다."));
            };
            self.record_asset_failure(*request, "DEVUP_ASSET_EXPORT_FAILED");
            return Ok(true);
        }
        if pending.kind == CallKind::LargeValue {
            let pending = self
                .pending
                .remove(call_id)
                .ok_or_else(|| invalid_call("large value call이 없습니다."))?;
            self.consumed.insert(call_id.to_owned());
            let ReadToolCall::LargeValue { options, .. } = pending.planned.call else {
                return Err(invalid_call("large value call 형식이 올바르지 않습니다."));
            };
            self.record_large_value_unsupported(&options, "DEVUP_FIELD_UNSUPPORTED_BY_UPSTREAM")?;
            return Ok(true);
        }
        if pending.kind == CallKind::Snapshot
            && pending
                .planned
                .expected_node_id
                .as_ref()
                .is_some_and(|node_id| self.section_fallback_roots.contains(node_id))
        {
            let pending = self
                .pending
                .remove(call_id)
                .ok_or_else(|| invalid_call("Section legacy call이 없습니다."))?;
            self.consumed.insert(call_id.to_owned());
            let node_id = pending
                .planned
                .expected_node_id
                .ok_or_else(|| invalid_call("Section legacy call의 node ID가 없습니다."))?;
            self.section_fallback_roots.remove(&node_id);
            self.screen_failures.push(ScreenFailure {
                node_id,
                error_code: error.code,
                message: error.message.clone(),
                retryable: error.retryable,
            });
            return Ok(true);
        }
        if !matches!(
            pending.kind,
            CallKind::FastSnapshot | CallKind::FastTheme | CallKind::FastMultiRoot
        ) {
            return Ok(false);
        }
        if !fast_call_fallback_allowed(error) {
            return Ok(false);
        }
        let pending = self
            .pending
            .remove(call_id)
            .ok_or_else(|| invalid_call("fast call이 없습니다."))?;
        self.consumed.insert(call_id.to_owned());
        if pending.kind == CallKind::FastMultiRoot {
            self.fallback_multi_root_batch(&pending.planned, fallback_category(error))?;
            return Ok(true);
        }
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
            && self.request.section.is_none()
    }

    fn fast_theme_eligible(&self) -> bool {
        self.request.scope == CollectionScope::File
            && self.request.resource_scope == ResourceScope::File
            && self.request.variables_only
            && !self.request.metadata_only
            && !self.request.include_context
            && self.request.search.is_none()
            && self.request.explore.is_none()
            && self.request.section.is_none()
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
            "transport": payload.stats.transport,
            "collectionCount": payload.resources.raw["collections"]
                .as_array().map_or(0, Vec::len),
            "variableCount": payload.resources.raw["variables"]
                .as_array().map_or(0, Vec::len),
            "styleCount": payload.resources.raw["styles"]
                .as_array().map_or(0, Vec::len)
        }));
        self.source_version = payload.source_version;
        self.stats.transport = payload.stats.transport.to_owned();
        self.stats.raw_bytes = payload.stats.raw_bytes;
        self.stats.wire_bytes = payload.stats.wire_bytes;
        self.stats.envelope_chunks = payload.stats.chunk_count;
        self.variables = Some(payload.resources);
        Ok(())
    }

    fn accept_fast_snapshot(
        &mut self,
        planned: &PlannedCall,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let payload = match decode_fast_snapshot(&result, &self.request.target) {
            Ok(payload) => payload,
            Err(error) => {
                if let Ok(chunk) = snapshot_chunk_from_result(&result)
                    && chunk.root_ids.as_slice()
                        == [self.request.target.node_id.as_deref().unwrap_or_default()]
                    && chunk.nodes.iter().any(|node| {
                        node.id == self.request.target.node_id.as_deref().unwrap_or_default()
                            && node.node_type == "SECTION"
                            && node.typed_view().value("projectionTruncated").is_some()
                    })
                {
                    self.request.section = Some(SectionReadOptions {
                        frame_ids: Vec::new(),
                        all_screens: false,
                    });
                    return self.accept_section_index(planned, order, result);
                }
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
        self.root_node_id = Some(root_id.clone());
        self.source_version = payload.snapshot.version.clone();
        self.metadata_root_ids = payload.snapshot.root_ids.clone();
        self.fast_snapshot_rounds = self.fast_snapshot_rounds.saturating_add(1);
        self.stats.raw_bytes = self.stats.raw_bytes.saturating_add(payload.stats.raw_bytes);
        self.stats.wire_bytes = self
            .stats
            .wire_bytes
            .saturating_add(payload.stats.wire_bytes);
        let has_large_values = !descriptors_in_chunk(&payload.snapshot)?.is_empty();
        if has_large_values {
            self.fast_snapshot_has_large_values = true;
        } else {
            merge_fast_resources(&mut self.fast_snapshot_resources, payload.resources)?;
        }

        let mut chunk = payload.snapshot;
        // The script always appends a `__DEVUP_SNAPSHOT_CURSOR__` marker node
        // (same convention as the legacy cursor snapshot) reporting whether
        // more pages remain; `take_snapshot_cursor` strips it and returns
        // that state. A missing marker (only possible for hand-built,
        // pre-pagination-shaped payloads) is treated as a single complete
        // page. `record_snapshot_chunk` then stores this page's real nodes
        // and enqueues any large-value follow-ups they declared.
        let total_nodes = chunk.nodes.len();
        let cursor = take_snapshot_cursor(&mut chunk)?.unwrap_or(SnapshotCursor {
            offset: 0,
            next_offset: total_nodes,
            complete: true,
            total_nodes,
        });
        self.record_snapshot_chunk(order, chunk)?;

        if cursor.complete {
            self.stats.transport = if self.fast_snapshot_rounds > 1 {
                "text-paginated"
            } else {
                "text"
            }
            .to_owned();
            self.stats.envelope_chunks = 0;
            self.variables = (!self.fast_snapshot_has_large_values)
                .then(|| self.fast_snapshot_resources.take())
                .flatten();
            self.metadata = Some(json!({
                "transport": &self.stats.transport,
                "rootId": root_id,
                "nodeCount": cursor.total_nodes,
                "pageCount": self.fast_snapshot_rounds
            }));
        } else {
            self.enqueue(
                ReadToolCall::fast_snapshot_page(
                    &self.request.target.file_key,
                    &root_id,
                    SnapshotReadOptions {
                        offset: cursor.next_offset,
                        ..SnapshotReadOptions::default()
                    },
                ),
                Some(root_id),
                CallKind::FastSnapshot,
            );
        }
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
        self.large_values.clear();
        self.fast_snapshot_resources = None;
        self.fast_snapshot_has_large_values = false;
        self.fast_snapshot_rounds = 0;
        self.section_fallback_roots.clear();
        self.screen_failures.clear();
        self.asset_results.clear();
        self.assets_scheduled = self.request.asset_selections.is_empty();
        self.reference_png = None;
        self.reference_png_scheduled = !self.request.reference_png;
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
        self.record_snapshot_chunk(order, chunk)?;
        Ok(())
    }

    fn accept_section_index(
        &mut self,
        planned: &PlannedCall,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let chunk = snapshot_chunk_from_result(&result)?;
        if chunk.file_key != planned.expected_file_key {
            return Err(invalid_call(
                "Figma Section index의 file key가 요청과 다릅니다.",
            ));
        }
        let section_id = planned
            .expected_node_id
            .as_deref()
            .ok_or_else(|| invalid_call("Figma Section index의 node ID가 없습니다."))?;
        if chunk.root_ids.as_slice() != [section_id]
            || !chunk.nodes.iter().any(|node| node.id == section_id)
        {
            return Err(invalid_call(
                "Figma Section index가 요청한 Section과 일치하지 않습니다.",
            ));
        }
        let snapshot = merge_chunks(vec![chunk.clone()])?;
        let index = build_section_index(&snapshot, &self.request.target)?;
        let options = self
            .request
            .section
            .clone()
            .ok_or_else(|| invalid_call("Section read options가 없습니다."))?;
        self.source_version = index.source_version.clone();
        self.root_node_id = Some(section_id.to_owned());
        self.metadata_root_ids = chunk.root_ids.clone();
        self.metadata = Some(json!({
            "transport": "section-index-v1",
            "sectionIndex": &index
        }));
        self.section_index = Some(index.clone());

        if options.frame_ids.is_empty() && !options.all_screens {
            self.request.resource_scope = ResourceScope::None;
            self.variables = Some(empty_used_resources());
            self.record_snapshot_chunk(order, chunk)?;
            return Ok(());
        }

        let selected = index.select(&options.frame_ids, options.all_screens)?;
        let batches = plan_batches(&index, &selected, BatchLimits::default())?;
        self.section_selected_roots = selected.clone();
        self.root_node_id = selected.first().cloned();
        self.metadata_root_ids = selected.clone();
        self.metadata = Some(json!({
            "transport": "section-multi-root-v1",
            "sectionIndex": &index,
            "selectedRootIds": &selected,
            "batchCount": batches.len()
        }));
        for batch in batches {
            if batch.oversized {
                self.mark_section_legacy("oversized-section-root".to_owned());
                for root_id in batch.root_ids {
                    self.enqueue_section_legacy_root(root_id);
                }
            } else {
                for root_id in batch.root_ids {
                    self.enqueue(
                        ReadToolCall::multi_root_snapshot(
                            &self.request.target.file_key,
                            section_id,
                            vec![root_id],
                        ),
                        Some(section_id.to_owned()),
                        CallKind::FastMultiRoot,
                    );
                }
            }
        }
        Ok(())
    }

    fn prepare_cached_section_index(&mut self, index: SectionIndex) -> Result<(), DevupError> {
        if index.file_key != self.request.target.file_key
            || self.request.target.node_id.as_deref() != Some(index.section.node_id.as_str())
        {
            return Err(invalid_call(
                "cached Section index가 요청한 Section과 일치하지 않습니다.",
            ));
        }
        let options = self
            .request
            .section
            .clone()
            .ok_or_else(|| invalid_call("cached Section index에 선택 설정이 없습니다."))?;
        let selected = index.select(&options.frame_ids, options.all_screens)?;
        let batches = plan_batches(&index, &selected, BatchLimits::default())?;
        self.source_version = index.source_version.clone();
        self.section_selected_roots = selected.clone();
        self.root_node_id = selected.first().cloned();
        self.metadata_root_ids = selected.clone();
        self.metadata = Some(json!({
            "transport": "section-multi-root-v1",
            "sectionIndex": &index,
            "selectedRootIds": &selected,
            "batchCount": batches.len(),
            "indexCacheHit": true
        }));
        self.section_index = Some(index);
        let section_id = self
            .request
            .target
            .node_id
            .clone()
            .ok_or_else(|| invalid_call("cached Section index의 Section ID가 없습니다."))?;
        for batch in batches {
            if batch.oversized {
                self.mark_section_legacy("oversized-section-root".to_owned());
                for root_id in batch.root_ids {
                    self.enqueue_section_legacy_root(root_id);
                }
            } else {
                for root_id in batch.root_ids {
                    self.enqueue(
                        ReadToolCall::multi_root_snapshot(
                            &self.request.target.file_key,
                            &section_id,
                            vec![root_id],
                        ),
                        Some(section_id.clone()),
                        CallKind::FastMultiRoot,
                    );
                }
            }
        }
        Ok(())
    }

    fn accept_fast_multi_root(
        &mut self,
        planned: &PlannedCall,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let ReadToolCall::Snapshot {
            script: BuiltinScript::MultiRootSnapshotEnvelope,
            root_ids: Some(root_ids),
            ..
        } = &planned.call
        else {
            return Err(invalid_call(
                "multi-root snapshot call 형식이 올바르지 않습니다.",
            ));
        };
        let payload = match decode_fast_multi_snapshot(&result, &self.request.target, root_ids) {
            Ok(payload) => payload,
            Err(error) if fast_call_fallback_allowed(&error) => {
                self.fallback_multi_root_batch(planned, fallback_category(&error))?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if let (Some(existing), Some(incoming)) = (&self.source_version, &payload.snapshot.version)
            && existing != incoming
        {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaVersionChanged,
                "multi-root 수집 중 Figma 파일 버전이 변경되었습니다.",
                true,
            ));
        }
        if payload.snapshot.version.is_some() {
            self.source_version = payload.snapshot.version.clone();
        }
        self.fast_multi_has_large_values |= !descriptors_in_chunk(&payload.snapshot)?.is_empty();
        merge_fast_resources(&mut self.fast_multi_resources, payload.resources)?;
        self.stats.transport = if self.section_fallback_roots.is_empty() {
            payload.stats.transport
        } else {
            "hybrid-multi-root-cursor"
        }
        .to_owned();
        self.stats.raw_bytes = self.stats.raw_bytes.saturating_add(payload.stats.raw_bytes);
        self.stats.wire_bytes = self
            .stats
            .wire_bytes
            .saturating_add(payload.stats.wire_bytes);
        self.stats.envelope_chunks = self
            .stats
            .envelope_chunks
            .saturating_add(payload.stats.chunk_count);
        self.record_snapshot_chunk(order, payload.snapshot)?;
        Ok(())
    }

    fn fallback_multi_root_batch(
        &mut self,
        planned: &PlannedCall,
        reason: String,
    ) -> Result<(), DevupError> {
        let ReadToolCall::Snapshot {
            script: BuiltinScript::MultiRootSnapshotEnvelope,
            root_ids: Some(root_ids),
            ..
        } = &planned.call
        else {
            return Err(invalid_call(
                "multi-root fallback call 형식이 올바르지 않습니다.",
            ));
        };
        if root_ids.is_empty() {
            return Err(invalid_call(
                "multi-root fallback에 선택된 root가 없습니다.",
            ));
        }
        self.mark_section_legacy(reason);
        for root_id in root_ids {
            self.enqueue_section_legacy_root(root_id.clone());
        }
        Ok(())
    }

    fn mark_section_legacy(&mut self, reason: String) {
        self.stats.transport = if self.fast_multi_resources.is_some() {
            "hybrid-multi-root-cursor"
        } else {
            "legacy-multi-root-cursor"
        }
        .to_owned();
        self.stats.fallback_used = true;
        self.stats.fallback_reason.get_or_insert(reason);
    }

    fn enqueue_section_legacy_root(&mut self, root_id: String) {
        if !self.section_fallback_roots.insert(root_id.clone()) {
            return;
        }
        self.enqueue(
            ReadToolCall::snapshot(
                &self.request.target.file_key,
                &root_id,
                BuiltinScript::NodeSnapshot,
            ),
            Some(root_id),
            CallKind::Snapshot,
        );
    }

    fn restore_section_visual_order(
        &self,
        chunks: Vec<SnapshotChunk>,
    ) -> Result<Vec<SnapshotChunk>, DevupError> {
        if self.section_selected_roots.is_empty() || chunks.is_empty() {
            return Ok(chunks);
        }
        let snapshot = merge_chunks(chunks)?;
        let observed = snapshot.roots.iter().collect::<BTreeSet<_>>();
        let failed = self
            .screen_failures
            .iter()
            .map(|failure| failure.node_id.as_str())
            .collect::<BTreeSet<_>>();
        if self
            .section_selected_roots
            .iter()
            .any(|root_id| !observed.contains(root_id) && !failed.contains(root_id.as_str()))
        {
            return Err(invalid_call(
                "Section snapshot에 선택된 root가 모두 포함되지 않았습니다.",
            ));
        }
        Ok(vec![SnapshotChunk {
            file_key: snapshot.file_key,
            version: snapshot.version,
            root_ids: self
                .section_selected_roots
                .iter()
                .filter(|root_id| observed.contains(*root_id))
                .cloned()
                .collect(),
            nodes: snapshot.nodes.into_values().collect(),
            diagnostics: snapshot.diagnostics,
        }])
    }

    fn record_snapshot_chunk(
        &mut self,
        order: usize,
        chunk: SnapshotChunk,
    ) -> Result<(), DevupError> {
        let descriptors = descriptors_in_chunk(&chunk)?;
        let version = chunk.version.clone();
        self.snapshot_chunks.insert(order, chunk);
        for descriptor in descriptors {
            let key = (descriptor.node_id.clone(), descriptor.field.clone());
            if self.large_values.contains_key(&key) {
                return Err(invalid_call(
                    "동일한 Figma large value descriptor가 중복되었습니다.",
                ));
            }
            let options = LargeValueReadOptions::from_descriptor(
                &descriptor,
                version.clone(),
                descriptor.cursor.next_offset,
            );
            self.large_values.insert(
                key,
                LargeValueAssembler::new(
                    self.request.target.file_key.clone(),
                    version.clone(),
                    descriptor,
                )?,
            );
            self.enqueue(
                ReadToolCall::large_value(&self.request.target.file_key, options.clone()),
                Some(options.node_id),
                CallKind::LargeValue,
            );
        }
        Ok(())
    }

    fn accept_large_value(
        &mut self,
        planned: &PlannedCall,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let ReadToolCall::LargeValue { options, .. } = &planned.call else {
            return Err(invalid_call("large value call 형식이 올바르지 않습니다."));
        };
        let result = large_value_from_result(&result)?;
        if let LargeValueResult::Unsupported(unsupported) = result {
            if unsupported.file_key != planned.expected_file_key
                || unsupported.version != options.version
                || unsupported.node_id != options.node_id
                || unsupported.field != options.field
                || unsupported.byte_length != options.byte_length
                || unsupported.sha256 != options.sha256
                || unsupported.error_code != "DEVUP_FIELD_UNSUPPORTED_BY_UPSTREAM"
            {
                return Err(invalid_call(
                    "large value unsupported 응답이 요청과 일치하지 않습니다.",
                ));
            }
            return self.record_large_value_unsupported(options, &unsupported.error_code);
        }
        let LargeValueResult::Fragment(fragment) = result else {
            unreachable!()
        };
        if fragment.offset != options.offset {
            return Err(invalid_call(
                "large value fragment offset이 요청과 일치하지 않습니다.",
            ));
        }
        let key = (options.node_id.clone(), options.field.clone());
        let next_offset = fragment.next_offset;
        let complete = fragment.complete;
        let assembler = self
            .large_values
            .get_mut(&key)
            .ok_or_else(|| invalid_call("large value assembler가 없습니다."))?;
        assembler.push(fragment)?;
        if complete {
            let assembler = self
                .large_values
                .remove(&key)
                .ok_or_else(|| invalid_call("large value assembler가 없습니다."))?;
            let descriptor = assembler.descriptor().clone();
            let value = assembler.finish()?;
            replace_descriptor(&mut self.snapshot_chunks, &descriptor, value)?;
        } else {
            if next_offset <= options.offset {
                return Err(invalid_call("large value cursor가 진행되지 않았습니다."));
            }
            let descriptor = assembler.descriptor().clone();
            let next = LargeValueReadOptions::from_descriptor(
                &descriptor,
                options.version.clone(),
                next_offset,
            );
            self.enqueue(
                ReadToolCall::large_value(&self.request.target.file_key, next.clone()),
                Some(next.node_id),
                CallKind::LargeValue,
            );
        }
        Ok(())
    }

    fn record_large_value_unsupported(
        &mut self,
        options: &LargeValueReadOptions,
        error_code: &str,
    ) -> Result<(), DevupError> {
        let key = (options.node_id.clone(), options.field.clone());
        let assembler = self
            .large_values
            .remove(&key)
            .ok_or_else(|| invalid_call("large value assembler가 없습니다."))?;
        let descriptor = assembler.descriptor().clone();
        replace_descriptor(
            &mut self.snapshot_chunks,
            &descriptor,
            json!({
                "$truncated": "unsupported-by-upstream",
                "byteLength": descriptor.byte_length
            }),
        )?;
        if let Some(chunk) = self
            .snapshot_chunks
            .values_mut()
            .find(|chunk| chunk.nodes.iter().any(|node| node.id == descriptor.node_id))
            && let Some(node) = chunk
                .nodes
                .iter_mut()
                .find(|node| node.id == descriptor.node_id)
        {
            node.field_errors
                .insert(descriptor.field.clone(), error_code.to_owned());
            chunk.diagnostics.push(crate::Diagnostic {
                code: error_code.to_owned(),
                message:
                    "Figma upstream에서 큰 필드를 다시 읽을 수 없어 명시적 marker를 유지했습니다."
                        .to_owned(),
                node_id: Some(descriptor.node_id),
                severity: Some(crate::DiagnosticSeverity::Warning),
                property: Some(descriptor.field),
                fallback: Some("unsupported-by-upstream".to_owned()),
                recoverable: Some(false),
                ..crate::Diagnostic::default()
            });
        }
        Ok(())
    }

    fn accept_asset(
        &mut self,
        planned: &PlannedCall,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let ReadToolCall::AssetExport {
            version, request, ..
        } = &planned.call
        else {
            return Err(invalid_call("asset call 형식이 올바르지 않습니다."));
        };
        let exported = asset_export_from_result(
            &result,
            &planned.expected_file_key,
            version.as_deref(),
            request,
        )?;
        if exported.status == AssetStatus::Failed {
            let error_code = exported
                .error_code
                .clone()
                .unwrap_or_else(|| "DEVUP_ASSET_EXPORT_FAILED".to_owned());
            self.record_asset_diagnostic(request, &error_code);
        }
        self.asset_results.push(exported);
        Ok(())
    }

    fn accept_reference_png(
        &mut self,
        planned: &PlannedCall,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let ReadToolCall::Screenshot { file_key, node_id } = &planned.call else {
            return Err(invalid_call("reference PNG call 형식이 올바르지 않습니다."));
        };
        if file_key != &planned.expected_file_key
            || planned.expected_node_id.as_deref() != Some(node_id.as_str())
        {
            return Err(invalid_call(
                "reference PNG call의 Figma 대상이 요청과 다릅니다.",
            ));
        }
        let data_base64 = take_single_png_data(result.raw)?;
        if data_base64.len() > MAX_REFERENCE_PNG_BASE64_BYTES {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "Figma reference PNG가 허용 크기를 초과했습니다.",
                false,
            ));
        }
        let bytes = STANDARD
            .decode(data_base64.as_bytes())
            .map_err(|_| invalid_call("Figma reference PNG의 base64가 올바르지 않습니다."))?;
        if bytes.is_empty() || bytes.len() > MAX_REFERENCE_PNG_BYTES {
            return Err(invalid_call(
                "Figma reference PNG의 형식 또는 크기가 올바르지 않습니다.",
            ));
        }
        validate_reference_png(&bytes)?;
        self.reference_png = Some(ReferencePng {
            mime_type: "image/png".to_owned(),
            data_base64,
            byte_length: bytes.len(),
            sha256: Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        });
        Ok(())
    }

    fn record_asset_failure(&mut self, request: AssetRequest, error_code: &str) {
        self.record_asset_diagnostic(&request, error_code);
        self.asset_results.push(AssetManifestEntry {
            asset_id: request.asset_id,
            node_id: request.node_id,
            field: request.field,
            source_kind: if request.image_hash.is_some() {
                "image-fill".to_owned()
            } else {
                "vector-node".to_owned()
            },
            image_hash: request.image_hash,
            format: Some(request.format),
            scale: Some(request.scale),
            status: AssetStatus::Failed,
            byte_length: None,
            sha256: None,
            mime_type: None,
            data_base64: None,
            output_path: None,
            error_code: Some(error_code.to_owned()),
        });
    }

    fn record_asset_diagnostic(&mut self, request: &AssetRequest, error_code: &str) {
        if let Some(chunk) = self
            .snapshot_chunks
            .values_mut()
            .find(|chunk| chunk.nodes.iter().any(|node| node.id == request.node_id))
        {
            chunk.diagnostics.push(crate::Diagnostic {
                code: error_code.to_owned(),
                message: "요청한 Figma asset을 export하지 못해 layout 출력은 유지했습니다."
                    .to_owned(),
                node_id: Some(request.node_id.clone()),
                severity: Some(crate::DiagnosticSeverity::Warning),
                property: Some(request.field.clone()),
                resource_kind: Some("asset".to_owned()),
                resource_id: Some(request.asset_id.clone()),
                fallback: Some("layout-only".to_owned()),
                recoverable: Some(true),
                ..crate::Diagnostic::default()
            });
        }
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
        self.record_snapshot_chunk(order, chunk)?;
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
        let chunks = self
            .snapshot_chunks
            .values()
            .filter(|chunk| {
                self.section_fallback_roots.is_empty()
                    || self.fast_multi_resources.is_none()
                    || self.fast_multi_has_large_values
                    || chunk
                        .root_ids
                        .iter()
                        .any(|root_id| self.section_fallback_roots.contains(root_id))
            })
            .cloned()
            .collect::<Vec<_>>();
        let refs = collect_used_resource_refs(&chunks);
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

fn take_single_png_data(value: Value) -> Result<String, DevupError> {
    let result = serde_json::from_value::<CallToolResult>(value)
        .map_err(|_| invalid_call("Figma screenshot 응답 형식이 올바르지 않습니다."))?;
    if result.is_error == Some(true) || result.content.len() != 1 {
        return Err(invalid_call(
            "Figma screenshot 응답에는 image/png content가 정확히 하나 있어야 합니다.",
        ));
    }
    let content = result
        .content
        .into_iter()
        .next()
        .ok_or_else(|| invalid_call("Figma screenshot 응답에 image/png content가 없습니다."))?;
    let ContentBlock::Image(image) = content else {
        return Err(invalid_call(
            "Figma screenshot 응답에는 image/png content가 정확히 하나 있어야 합니다.",
        ));
    };
    if image.mime_type != "image/png" {
        return Err(invalid_call(
            "Figma screenshot 응답의 MIME 형식이 image/png가 아닙니다.",
        ));
    }
    Ok(image.data)
}

fn validate_reference_png(bytes: &[u8]) -> Result<(), DevupError> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_REFERENCE_PNG_DIMENSION);
    limits.max_image_height = Some(MAX_REFERENCE_PNG_DIMENSION);
    limits.max_alloc = Some(MAX_REFERENCE_PNG_DECODED_BYTES as u64);
    let decoder =
        PngDecoder::with_limits(Cursor::new(bytes), limits).map_err(reference_png_decode_error)?;
    let (width, height) = decoder.dimensions();
    let decoded_bytes = usize::try_from(decoder.total_bytes()).map_err(|_| {
        DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "Figma reference PNG의 decoded 크기가 허용 범위를 초과했습니다.",
            false,
        )
    })?;
    if width == 0
        || height == 0
        || width > MAX_REFERENCE_PNG_DIMENSION
        || height > MAX_REFERENCE_PNG_DIMENSION
        || decoded_bytes > MAX_REFERENCE_PNG_DECODED_BYTES
    {
        return Err(DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "Figma reference PNG의 dimensions 또는 decoded 크기가 허용 범위를 초과했습니다.",
            false,
        ));
    }
    let mut pixels = vec![0_u8; decoded_bytes];
    decoder
        .read_image(&mut pixels)
        .map_err(reference_png_decode_error)
}

fn reference_png_decode_error(error: ImageError) -> DevupError {
    if matches!(error, ImageError::Limits(_)) {
        DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "Figma reference PNG의 dimensions 또는 decoded 크기가 허용 범위를 초과했습니다.",
            false,
        )
    } else {
        invalid_call("Figma reference PNG 데이터가 손상되었습니다.")
    }
}

fn take_snapshot_cursor(chunk: &mut SnapshotChunk) -> Result<Option<SnapshotCursor>, DevupError> {
    let Some(cursor) = read_snapshot_cursor(&chunk.nodes)
        .map_err(|message| invalid_call(message.korean_message()))?
    else {
        return Ok(None);
    };
    chunk.nodes.retain(|node| node.id != SNAPSHOT_CURSOR_ID);
    Ok(Some(cursor))
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

fn is_section_target_error(error: &DevupError) -> bool {
    error.message.contains("DEVUP_TARGET_IS_SECTION")
        || error
            .details
            .to_string()
            .contains("DEVUP_TARGET_IS_SECTION")
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

fn empty_used_resources() -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "collections": [],
            "variables": [],
            "styles": [],
            "usedRemoteVariables": [],
            "usedVariableIds": [],
            "usedStyleIds": [],
            "localComplete": false,
            "usedRemoteComplete": true,
            "unresolved": []
        }),
    }
}

fn merge_fast_resources(
    existing: &mut Option<UpstreamResult>,
    incoming: UpstreamResult,
) -> Result<(), DevupError> {
    let Some(current) = existing else {
        *existing = Some(incoming);
        return Ok(());
    };
    let current = current
        .raw
        .as_object_mut()
        .ok_or_else(|| invalid_call("기존 multi-root resource 형식이 올바르지 않습니다."))?;
    let incoming = incoming
        .raw
        .as_object()
        .ok_or_else(|| invalid_call("multi-root resource 형식이 올바르지 않습니다."))?;
    for field in ["collections", "variables", "styles", "usedRemoteVariables"] {
        let mut values = BTreeMap::<String, Value>::new();
        for value in current
            .get(field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(
                incoming
                    .get(field)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten(),
            )
        {
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_call("multi-root resource ID가 없습니다."))?;
            if let Some(previous) = values.get(id)
                && previous != value
            {
                return Err(DevupError::new(
                    ErrorCode::DevupFigmaVersionChanged,
                    "multi-root resource 내용이 batch 사이에서 달라졌습니다.",
                    true,
                ));
            }
            values.insert(id.to_owned(), value.clone());
        }
        current.insert(
            field.to_owned(),
            Value::Array(values.into_values().collect()),
        );
    }
    for field in ["usedVariableIds", "usedStyleIds"] {
        let values = current
            .get(field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(
                incoming
                    .get(field)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten(),
            )
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid_call("multi-root resource ID 형식이 올바르지 않습니다."))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        current.insert(
            field.to_owned(),
            Value::Array(values.into_iter().map(Value::String).collect()),
        );
    }
    let unresolved = current
        .get("unresolved")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            incoming
                .get("unresolved")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .map(|value| serde_json::to_string(value).map(|key| (key, value.clone())))
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|_| invalid_call("multi-root unresolved resource를 직렬화할 수 없습니다."))?;
    current.insert(
        "unresolved".to_owned(),
        Value::Array(unresolved.into_values().collect()),
    );
    for field in ["localComplete", "usedRemoteComplete"] {
        let merged = current.get(field).and_then(Value::as_bool).unwrap_or(false)
            && incoming
                .get(field)
                .and_then(Value::as_bool)
                .unwrap_or(false);
        current.insert(field.to_owned(), Value::Bool(merged));
    }
    Ok(())
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
