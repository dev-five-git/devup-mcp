use std::{
    cmp::Reverse,
    collections::BTreeMap,
    future::Future,
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use devup_mcp_figma::{
    AssetSelection, CollectedPayload, CollectionRequest, CollectionScope, DevupError, ErrorCode,
    ExploreReadOptions, ResourceScope, SearchReadOptions, SectionReadOptions,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OwnedMutexGuard, watch};

use super::delivery::{ProjectedOutput, RESOURCE_CHUNK_BYTES};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRequestKey {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    measurement_node_ids: Option<Vec<String>>,
    file_key: String,
    node_id: Option<String>,
    branch_key: Option<String>,
    scope: CollectionScope,
    resource_scope: ResourceScope,
    include_context: bool,
    metadata_only: bool,
    variables_only: bool,
    search: Option<SearchReadOptions>,
    explore: Option<ExploreReadOptions>,
    section: Option<SectionReadOptions>,
    asset_selections: Vec<AssetSelection>,
    web_ready_images: bool,
    reference_png: bool,
}

impl ArtifactRequestKey {
    pub fn from_collection(request: &CollectionRequest) -> Self {
        let mut section = request.section.clone();
        if let Some(section) = &mut section {
            section.frame_ids.sort();
            section.frame_ids.dedup();
        }
        let mut asset_selections = request.asset_selections.clone();
        asset_selections
            .sort_by_cached_key(|selection| serde_json::to_string(selection).unwrap_or_default());
        asset_selections.dedup();
        Self {
            measurement_node_ids: None,
            file_key: request.target.file_key.clone(),
            node_id: request.target.node_id.clone(),
            branch_key: request.target.branch_key.clone(),
            scope: request.scope,
            resource_scope: request.resource_scope,
            include_context: request.include_context,
            metadata_only: request.metadata_only,
            variables_only: request.variables_only,
            search: request.search.clone(),
            explore: request.explore.clone(),
            section,
            asset_selections,
            web_ready_images: request.web_ready_images,
            reference_png: request.reference_png,
        }
    }

    fn digest(&self) -> String {
        sha256_hex(&serde_json::to_vec(self).unwrap_or_default())
    }

    fn capabilities(&self) -> ArtifactCapabilities {
        let kind = if self.measurement_node_ids.is_some() {
            ArtifactKind::Measurement
        } else if self.search.is_some() {
            ArtifactKind::Search
        } else if self.explore.is_some() {
            ArtifactKind::Explore
        } else if self
            .section
            .as_ref()
            .is_some_and(|section| section.frame_ids.is_empty() && !section.all_screens)
        {
            ArtifactKind::SectionIndex
        } else if self.variables_only {
            ArtifactKind::ThemeOnly
        } else {
            ArtifactKind::Design
        };
        ArtifactCapabilities {
            kind,
            collection_scope: self.scope,
            resource_scope: self.resource_scope,
            asset_capture_count: self.asset_selections.len(),
            web_ready_images: self.web_ready_images,
            asset_captures: self.asset_selections.clone(),
            reference_png: self.reference_png,
            section_selection: self.section.clone(),
        }
    }

    pub(super) fn measurement(target: devup_mcp_figma::FigmaTarget, ids: Vec<String>) -> Self {
        let mut request = CollectionRequest::new(target, CollectionScope::Node);
        request.resource_scope = ResourceScope::None;
        let mut key = Self::from_collection(&request);
        key.measurement_node_ids = Some(ids);
        key
    }

    fn can_serve_explore(&self, requested: &Self) -> bool {
        let (Some(cached_options), Some(requested_options)) =
            (self.explore.as_ref(), requested.explore.as_ref())
        else {
            return false;
        };
        if cached_options.text_preview_limit != requested_options.text_preview_limit
            || cached_options.projection_limit < requested_options.projection_limit
        {
            return false;
        }
        let mut cached_scope = self.clone();
        let mut requested_scope = requested.clone();
        cached_scope.node_id = None;
        requested_scope.node_id = None;
        cached_scope.explore = None;
        requested_scope.explore = None;
        cached_scope == requested_scope
    }

    fn explore_projection_limit(&self) -> usize {
        self.explore
            .as_ref()
            .map_or(usize::MAX, |options| options.projection_limit)
    }

    fn explore_reuse_kind(&self, requested: &Self) -> Option<CacheReuseKind> {
        self.can_serve_explore(requested).then(|| {
            let same_node = self.node_id == requested.node_id;
            let same_projection =
                self.explore_projection_limit() == requested.explore_projection_limit();
            match (same_node, same_projection) {
                (true, true) => CacheReuseKind::Exact,
                (true, false) => CacheReuseKind::Superset,
                (false, true) => CacheReuseKind::RelatedNode,
                (false, false) => CacheReuseKind::RelatedNodeSuperset,
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    Measurement,
    Design,
    ThemeOnly,
    Search,
    Explore,
    SectionIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheReuseKind {
    Miss,
    Exact,
    RelatedNode,
    Superset,
    RelatedNodeSuperset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactCapabilities {
    pub kind: ArtifactKind,
    pub collection_scope: CollectionScope,
    pub resource_scope: ResourceScope,
    pub asset_capture_count: usize,
    pub web_ready_images: bool,
    pub reference_png: bool,
    /// Last validated Section selection; projection callers restore this when no selection is supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_selection: Option<SectionReadOptions>,
    #[serde(skip)]
    asset_captures: Vec<AssetSelection>,
}

impl ArtifactCapabilities {
    /// Validate exact asset ID, format and scale coverage before projecting a cached artifact.
    pub fn validate_asset_captures(&self, requested: &[AssetSelection]) -> Result<(), DevupError> {
        let missing = requested
            .iter()
            .filter(|capture| !self.asset_captures.contains(capture))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        Err(DevupError::with_details(
            ErrorCode::DevupFigmaHandoffInvalid,
            "This artifact was collected without the requested asset captures. Remove artifactId and call again with the original url and assetRequests.",
            false,
            json!({"missingAssetCaptures": missing, "assetCaptureCount": self.asset_capture_count}),
        ))
    }

    pub fn supports_asset_captures(&self, requested: &[AssetSelection]) -> bool {
        requested
            .iter()
            .all(|capture| self.asset_captures.contains(capture))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ArtifactLimits {
    pub ttl: Duration,
    pub max_entries: usize,
    pub max_entry_bytes: usize,
    pub max_total_bytes: usize,
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(10 * 60),
            max_entries: 8,
            // One collection may fill the whole cache: a scope:file collection of a
            // 5,543-node test file is 63.6 MB, and refusing it failed the export,
            // while the entries it evicts cost only a re-collection.
            max_entry_bytes: 384 * 1024 * 1024,
            max_total_bytes: 384 * 1024 * 1024,
        }
    }
}

pub trait ArtifactClock: Send + Sync {
    fn now_epoch_seconds(&self) -> u64;
}

#[derive(Debug)]
struct SystemClock;

impl ArtifactClock for SystemClock {
    fn now_epoch_seconds(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactLookup {
    pub artifact_id: String,
    pub content_hash: String,
    pub created_at_epoch_seconds: u64,
    pub expires_at_epoch_seconds: u64,
    pub age_seconds: u64,
    pub remaining_ttl_seconds: u64,
    pub size_bytes: usize,
    pub cache_hit: bool,
    pub reuse_kind: CacheReuseKind,
    pub capabilities: ArtifactCapabilities,
    pub payload: Arc<CollectedPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactStoreStats {
    pub entry_count: usize,
    pub total_bytes: usize,
    pub max_entries: usize,
    pub max_total_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachedOutputManifest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_metadata: Option<Value>,
    pub artifact_id: String,
    pub output_id: String,
    pub name: String,
    pub mime_type: String,
    pub raw_bytes: usize,
    pub sha256: String,
    pub chunk_count: usize,
    pub chunk_bytes: usize,
    pub chunk_uris: Vec<String>,
    pub is_binary: bool,
    pub manifest_uri: String,
    pub expires_at_epoch_seconds: u64,
}

#[derive(Debug)]
struct AttachedOutput {
    manifest: AttachedOutputManifest,
    bytes: Arc<[u8]>,
    ranges: Vec<(usize, usize)>,
    allocation_bytes: usize,
}

#[derive(Debug)]
struct Entry {
    key_digest: String,
    request_key: ArtifactRequestKey,
    selection_bytes: usize,
    content_hash: String,
    created_at: u64,
    expires_at: u64,
    size_bytes: usize,
    last_access: u64,
    capabilities: ArtifactCapabilities,
    payload: Arc<CollectedPayload>,
    outputs: BTreeMap<String, AttachedOutput>,
    projections: BTreeMap<String, Vec<String>>,
}

type AcquisitionResult = Result<ArtifactLookup, DevupError>;

#[derive(Default)]
struct StoreState {
    entries: BTreeMap<String, Entry>,
    key_index: BTreeMap<String, String>,
    in_flight: BTreeMap<String, watch::Receiver<Option<AcquisitionResult>>>,
    in_flight_keys: BTreeMap<String, ArtifactRequestKey>,
    total_bytes: usize,
    recovery: BTreeMap<String, serde_json::Value>,
    access_sequence: u64,
}

#[derive(Debug, Clone, Copy)]
enum InFlightWait {
    Exact,
    Compatible(CacheReuseKind),
}

#[derive(Clone)]
pub struct ArtifactStore {
    state: Arc<Mutex<StoreState>>,
    clock: Arc<dyn ArtifactClock>,
    limits: ArtifactLimits,
}

pub struct OutputReservation {
    state: Option<OwnedMutexGuard<StoreState>>,
    artifact_id: String,
    projection_key: String,
    staged: Vec<AttachedOutput>,
    manifests: Vec<AttachedOutputManifest>,
    allocation: usize,
    created: bool,
    max_total_bytes: usize,
}

impl OutputReservation {
    pub fn manifests(&self) -> &[AttachedOutputManifest] {
        &self.manifests
    }

    pub fn commit(mut self) -> Vec<AttachedOutputManifest> {
        let Some(mut state) = self.state.take() else {
            return self.manifests;
        };
        if !self.created {
            return self.manifests;
        }
        while state.total_bytes.saturating_add(self.allocation) > self.max_total_bytes {
            let lru_id = state
                .entries
                .iter()
                .filter(|(id, _)| id.as_str() != self.artifact_id)
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(id, _)| id.clone())
                .expect("output reservation validated enough evictable capacity");
            remove_entry(&mut state, &lru_id);
        }
        state.access_sequence = state.access_sequence.saturating_add(1);
        let access = state.access_sequence;
        let entry = state
            .entries
            .get_mut(&self.artifact_id)
            .expect("reserved artifact remains pinned by the store lock");
        entry.last_access = access;
        entry.size_bytes = entry.size_bytes.saturating_add(self.allocation);
        let ids = self
            .staged
            .iter()
            .map(|output| output.manifest.output_id.clone())
            .collect::<Vec<_>>();
        for output in self.staged.drain(..) {
            entry
                .outputs
                .insert(output.manifest.output_id.clone(), output);
        }
        entry.projections.insert(self.projection_key, ids);
        state.total_bytes = state.total_bytes.saturating_add(self.allocation);
        self.manifests
    }

    pub fn rollback(self) {}
}

impl Default for ArtifactStore {
    fn default() -> Self {
        Self::with_limits(ArtifactLimits::default())
    }
}