use std::{
    cmp::Reverse,
    collections::BTreeMap,
    future::Future,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use devup_mcp_figma::{
    AssetSelection, CollectedPayload, CollectionRequest, CollectionScope, DevupError, ErrorCode,
    ExploreReadOptions, ResourceScope, SearchReadOptions, SectionReadOptions,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OwnedMutexGuard, watch};

use super::delivery::{ProjectedOutput, RESOURCE_CHUNK_BYTES};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRequestKey {
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
        asset_selections.sort_by(|left, right| {
            (&left.asset_id, left.format, left.scale).cmp(&(
                &right.asset_id,
                right.format,
                right.scale,
            ))
        });
        asset_selections.dedup();
        Self {
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
            reference_png: request.reference_png,
        }
    }

    fn digest(&self) -> String {
        sha256_hex(&serde_json::to_vec(self).unwrap_or_default())
    }

    fn capabilities(&self) -> ArtifactCapabilities {
        let kind = if self.search.is_some() {
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
            asset_captures: self.asset_selections.clone(),
            reference_png: self.reference_png,
            section_selection: self.section.clone(),
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactCapabilities {
    pub kind: ArtifactKind,
    pub collection_scope: CollectionScope,
    pub resource_scope: ResourceScope,
    pub asset_capture_count: usize,
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
            max_entry_bytes: 32 * 1024 * 1024,
            max_total_bytes: 128 * 1024 * 1024,
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

impl ArtifactStore {
    pub fn with_limits(limits: ArtifactLimits) -> Self {
        Self::with_clock(Arc::new(SystemClock), limits)
    }

    pub fn with_clock(clock: Arc<dyn ArtifactClock>, limits: ArtifactLimits) -> Self {
        Self {
            state: Arc::new(Mutex::new(StoreState::default())),
            clock,
            limits,
        }
    }

    pub async fn get_or_acquire<F, Fut>(
        &self,
        key: ArtifactRequestKey,
        refresh: bool,
        acquire: F,
    ) -> Result<ArtifactLookup, DevupError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<CollectedPayload, DevupError>>,
    {
        let key_digest = key.digest();
        let capabilities = key.capabilities();
        if refresh {
            let payload = acquire().await?;
            return self
                .insert_with_digest(key, key_digest, capabilities, payload)
                .await;
        }
        let mut acquire = Some(acquire);
        loop {
            let (owner, mut receiver, sender, wait_digest, wait_kind) = {
                let now = self.clock.now_epoch_seconds();
                let mut state = self.state.lock().await;
                self.prune_expired(&mut state, now);
                if let Some(hit) = lookup_by_key(&mut state, &key_digest, now, true) {
                    return Ok(hit);
                }
                if let Some(receiver) = state.in_flight.get(&key_digest) {
                    (
                        false,
                        receiver.clone(),
                        None,
                        key_digest.clone(),
                        InFlightWait::Exact,
                    )
                } else if let Some((compatible_digest, receiver, reuse_kind)) = state
                    .in_flight_keys
                    .iter()
                    // The receiver is taken here, from the same read of the
                    // same lock that picked the key, rather than looked up
                    // again below. `in_flight` and `in_flight_keys` are meant
                    // to hold the same digests — they are written together and
                    // erased together — but that is an agreement between three
                    // separate sites, one of which only runs when a waiter
                    // notices a cancelled owner. Reading the key without its
                    // receiver leaves somewhere for the two to disagree, and
                    // what a disagreement would have cost is the process: this
                    // is the shared store every collection goes through. A
                    // candidate that has no receiver simply is not a candidate.
                    .filter_map(|(digest, in_flight_key)| {
                        let receiver = state.in_flight.get(digest)?;
                        in_flight_key.explore_reuse_kind(&key).map(|reuse_kind| {
                            (
                                digest.clone(),
                                receiver.clone(),
                                reuse_kind,
                                in_flight_key.explore_projection_limit(),
                            )
                        })
                    })
                    .min_by_key(|(_, _, _, projection_limit)| *projection_limit)
                    .map(|(digest, receiver, reuse_kind, _)| (digest, receiver, reuse_kind))
                {
                    (
                        false,
                        receiver,
                        None,
                        compatible_digest,
                        InFlightWait::Compatible(reuse_kind),
                    )
                } else {
                    let (sender, receiver) = watch::channel(None);
                    state.in_flight.insert(key_digest.clone(), receiver.clone());
                    state.in_flight_keys.insert(key_digest.clone(), key.clone());
                    (
                        true,
                        receiver,
                        Some(sender),
                        key_digest.clone(),
                        InFlightWait::Exact,
                    )
                }
            };

            if !owner {
                if receiver.wait_for(Option::is_some).await.is_err() {
                    let mut state = self.state.lock().await;
                    if state
                        .in_flight
                        .get(&wait_digest)
                        .is_some_and(|current| current.same_channel(&receiver))
                    {
                        state.in_flight.remove(&wait_digest);
                        state.in_flight_keys.remove(&wait_digest);
                    }
                    continue;
                }
                let mut result = receiver
                    .borrow()
                    .clone()
                    .ok_or_else(acquisition_cancelled)?;
                match (wait_kind, &mut result) {
                    (InFlightWait::Exact, Ok(hit)) => {
                        hit.cache_hit = true;
                        hit.reuse_kind = CacheReuseKind::Exact;
                    }
                    (InFlightWait::Compatible(reuse_kind), Ok(hit))
                        if key.node_id.as_ref().is_none_or(|node_id| {
                            hit.payload.snapshot.nodes.contains_key(node_id)
                        }) =>
                    {
                        let now = self.clock.now_epoch_seconds();
                        hit.cache_hit = true;
                        hit.reuse_kind = reuse_kind;
                        hit.age_seconds = now.saturating_sub(hit.created_at_epoch_seconds);
                        hit.remaining_ttl_seconds =
                            hit.expires_at_epoch_seconds.saturating_sub(now);
                    }
                    (InFlightWait::Exact, Err(_)) => {}
                    (InFlightWait::Compatible(_), _) => continue,
                }
                return result;
            }

            let acquire = acquire.take().ok_or_else(acquisition_cancelled)?;
            let result = match acquire().await {
                Ok(payload) => {
                    self.insert_with_digest(
                        key.clone(),
                        key_digest.clone(),
                        capabilities.clone(),
                        payload,
                    )
                    .await
                }
                Err(error) => Err(error),
            };
            {
                let mut state = self.state.lock().await;
                state.in_flight.remove(&key_digest);
                state.in_flight_keys.remove(&key_digest);
            }
            if let Some(sender) = sender {
                let _ = sender.send(Some(result.clone()));
            }
            return result;
        }
    }

    pub async fn insert(
        &self,
        key: ArtifactRequestKey,
        payload: CollectedPayload,
    ) -> Result<ArtifactLookup, DevupError> {
        self.insert_with_digest(key.clone(), key.digest(), key.capabilities(), payload)
            .await
    }

    /// Remember a selection after the caller has validated it against the Section candidates.
    /// This updates projection state only: the original acquisition key and capture capabilities stay intact.
    pub async fn remember_section_selection(
        &self,
        artifact_id: &str,
        selection: SectionReadOptions,
    ) -> Result<ArtifactLookup, DevupError> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        let entry = state
            .entries
            .get(artifact_id)
            .ok_or_else(resource_expired)?;
        if (entry.capabilities.section_selection.is_none()
            && entry.capabilities.kind != ArtifactKind::SectionIndex)
            || (selection.all_screens && !selection.frame_ids.is_empty())
            || (!selection.all_screens && selection.frame_ids.is_empty())
            || selection
                .frame_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != selection.frame_ids.len()
        {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaHandoffInvalid,
                "A Section selection must contain distinct frameIds or allScreens:true.",
                false,
            ));
        }
        let selection_bytes = serde_json::to_vec(&selection)
            .map_err(|error| {
                DevupError::new(
                    ErrorCode::DevupFigmaHandoffInvalid,
                    error.to_string(),
                    false,
                )
            })?
            .len();
        let previous_bytes = entry.selection_bytes;
        let size_bytes = entry
            .size_bytes
            .saturating_sub(previous_bytes)
            .saturating_add(selection_bytes);
        let total_bytes = state
            .total_bytes
            .saturating_sub(previous_bytes)
            .saturating_add(selection_bytes);
        if size_bytes > self.limits.max_entry_bytes || total_bytes > self.limits.max_total_bytes {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "The Section selection exceeded the artifact memory limit.",
                false,
            ));
        }
        let entry = state
            .entries
            .get_mut(artifact_id)
            .ok_or_else(resource_expired)?;
        entry.capabilities.section_selection = Some(selection);
        entry.selection_bytes = selection_bytes;
        entry.size_bytes = size_bytes;
        state.total_bytes = total_bytes;
        touch_entry(&mut state, artifact_id, now, CacheReuseKind::Exact)
            .ok_or_else(resource_expired)
    }

    pub async fn get_for_export(&self, artifact_id: &str) -> Result<ArtifactLookup, DevupError> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        touch_entry(&mut state, artifact_id, now, CacheReuseKind::Exact).ok_or_else(|| {
            DevupError::with_details(
                ErrorCode::DevupFigmaHandoffExpired,
                "The Figma artifact is missing or expired.",
                true,
                state
                    .recovery
                    .get(artifact_id)
                    .cloned()
                    .unwrap_or_else(|| json!({"artifactState":"unknown"})),
            )
        })
    }

    pub async fn get(&self, artifact_id: &str) -> Option<ArtifactLookup> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        touch_entry(&mut state, artifact_id, now, CacheReuseKind::Exact)
    }

    pub async fn lookup(&self, key: &ArtifactRequestKey) -> Option<ArtifactLookup> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        lookup_by_key(&mut state, &key.digest(), now, true)
    }

    pub async fn lookup_related_explore(&self, key: &ArtifactRequestKey) -> Option<ArtifactLookup> {
        let requested_node_id = key.node_id.as_deref()?;
        key.explore.as_ref()?;
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        let (artifact_id, reuse_kind) = state
            .entries
            .iter()
            .filter_map(|(artifact_id, entry)| {
                entry
                    .request_key
                    .explore_reuse_kind(key)
                    .map(|reuse_kind| (artifact_id, entry, reuse_kind))
            })
            .filter(|(_, entry, _)| entry.payload.snapshot.nodes.contains_key(requested_node_id))
            .min_by_key(|(_, entry, _)| {
                (
                    entry.request_key.explore_projection_limit(),
                    Reverse(entry.last_access),
                )
            })
            .map(|(artifact_id, _, reuse_kind)| (artifact_id.clone(), reuse_kind))?;
        touch_entry(&mut state, &artifact_id, now, reuse_kind)
    }

    pub async fn stats(&self) -> ArtifactStoreStats {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        ArtifactStoreStats {
            entry_count: state.entries.len(),
            total_bytes: state.total_bytes,
            max_entries: self.limits.max_entries,
            max_total_bytes: self.limits.max_total_bytes,
        }
    }

    pub async fn attach_outputs(
        &self,
        artifact_id: &str,
        projection_key: &str,
        outputs: Vec<ProjectedOutput>,
    ) -> Result<Vec<AttachedOutputManifest>, DevupError> {
        Ok(self
            .reserve_outputs(artifact_id, projection_key, outputs)
            .await?
            .commit())
    }

    pub async fn reserve_outputs(
        &self,
        artifact_id: &str,
        projection_key: &str,
        outputs: Vec<ProjectedOutput>,
    ) -> Result<OutputReservation, DevupError> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.clone().lock_owned().await;
        self.prune_expired(&mut state, now);
        if let Some(ids) = state
            .entries
            .get(artifact_id)
            .and_then(|entry| entry.projections.get(projection_key))
            .cloned()
        {
            let entry = state
                .entries
                .get(artifact_id)
                .ok_or_else(resource_expired)?;
            let manifests = ids
                .iter()
                .map(|id| {
                    entry
                        .outputs
                        .get(id)
                        .map(|output| output.manifest.clone())
                        .ok_or_else(resource_expired)
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(OutputReservation {
                state: Some(state),
                artifact_id: artifact_id.to_owned(),
                projection_key: projection_key.to_owned(),
                staged: Vec::new(),
                manifests,
                allocation: 0,
                created: false,
                max_total_bytes: self.limits.max_total_bytes,
            });
        }
        let expires_at = state
            .entries
            .get(artifact_id)
            .map(|entry| entry.expires_at)
            .ok_or_else(resource_expired)?;
        let mut staged = Vec::with_capacity(outputs.len());
        let mut staged_ids = BTreeMap::new();
        for output in outputs {
            if output.name.is_empty()
                || !output
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
                || output.mime_type.is_empty()
            {
                return Err(DevupError::new(
                    ErrorCode::DevupFigmaHandoffInvalid,
                    "The resource output name or MIME type is invalid.",
                    false,
                ));
            }
            let output_id = output.resource_id;
            if !valid_output_id(&output_id)
                || staged_ids.contains_key(&output_id)
                || state
                    .entries
                    .get(artifact_id)
                    .is_some_and(|entry| entry.outputs.contains_key(&output_id))
            {
                return Err(DevupError::new(
                    ErrorCode::DevupFigmaHandoffInvalid,
                    "The resource output ID is duplicated or invalid.",
                    true,
                ));
            }
            staged_ids.insert(output_id.clone(), ());
            let raw_bytes = output.bytes.len();
            let ranges = output_chunk_ranges(&output.bytes, output.is_binary)?;
            let encoded_bytes = if output.is_binary {
                raw_bytes.div_ceil(3).saturating_mul(4)
            } else {
                0
            };
            let allocation_bytes = raw_bytes.saturating_add(encoded_bytes);
            let manifest_uri =
                format!("devup://artifact/{artifact_id}/outputs/{output_id}/manifest");
            staged.push(AttachedOutput {
                manifest: AttachedOutputManifest {
                    artifact_id: artifact_id.to_owned(),
                    output_id: output_id.clone(),
                    name: output.name,
                    mime_type: output.mime_type,
                    raw_bytes,
                    sha256: sha256_hex(&output.bytes),
                    chunk_uris: (0..ranges.len())
                        .map(|index| {
                            format!(
                                "devup://artifact/{artifact_id}/outputs/{output_id}/chunks/{index}"
                            )
                        })
                        .collect(),
                    chunk_count: ranges.len(),
                    chunk_bytes: RESOURCE_CHUNK_BYTES,
                    is_binary: output.is_binary,
                    manifest_uri,
                    expires_at_epoch_seconds: expires_at,
                },
                bytes: Arc::from(output.bytes),
                ranges,
                allocation_bytes,
            });
        }
        let allocation = staged
            .iter()
            .try_fold(0_usize, |sum, output| {
                sum.checked_add(output.allocation_bytes)
            })
            .ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupFigmaResponseTooLarge,
                    "The resource allocation size exceeded the safe range.",
                    false,
                )
            })?;
        let entry_size = state
            .entries
            .get(artifact_id)
            .map(|entry| entry.size_bytes)
            .ok_or_else(resource_expired)?;
        if entry_size.saturating_add(allocation) > self.limits.max_entry_bytes
            || allocation > self.limits.max_total_bytes
        {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "The resource output exceeded the artifact memory limit.",
                false,
            ));
        }
        let retained_bytes = state
            .entries
            .get(artifact_id)
            .map(|entry| entry.size_bytes)
            .ok_or_else(resource_expired)?;
        if retained_bytes.saturating_add(allocation) > self.limits.max_total_bytes {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "The resource output exceeded the total memory limit.",
                false,
            ));
        }
        let manifests = staged
            .iter()
            .map(|output| output.manifest.clone())
            .collect::<Vec<_>>();
        Ok(OutputReservation {
            state: Some(state),
            artifact_id: artifact_id.to_owned(),
            projection_key: projection_key.to_owned(),
            staged,
            manifests,
            allocation,
            created: true,
            max_total_bytes: self.limits.max_total_bytes,
        })
    }

    pub async fn detach_projection(&self, artifact_id: &str, projection_key: &str) -> bool {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        let removed_allocation = {
            let Some(entry) = state.entries.get_mut(artifact_id) else {
                return false;
            };
            let Some(ids) = entry.projections.remove(projection_key) else {
                return false;
            };
            let allocation = ids
                .into_iter()
                .filter_map(|id| entry.outputs.remove(&id))
                .map(|output| output.allocation_bytes)
                .sum::<usize>();
            entry.size_bytes = entry.size_bytes.saturating_sub(allocation);
            allocation
        };
        state.total_bytes = state.total_bytes.saturating_sub(removed_allocation);
        true
    }

    pub(crate) async fn read_resource_output(
        &self,
        artifact_id: &str,
        output_id: &str,
        index: Option<usize>,
    ) -> Result<(AttachedOutputManifest, Vec<u8>), DevupError> {
        let mut state = self.state.lock().await;
        let now = self.clock.now_epoch_seconds();
        let entry = state
            .entries
            .get(artifact_id)
            .ok_or_else(|| resource_missing("artifact"))?;
        if entry.expires_at <= now {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaHandoffExpired,
                "The resource artifact has expired. Re-collect it from the original URL.",
                true,
            ));
        }
        state.access_sequence = state.access_sequence.saturating_add(1);
        let access = state.access_sequence;
        let entry = state
            .entries
            .get_mut(artifact_id)
            .ok_or_else(|| resource_missing("artifact"))?;
        let output = entry
            .outputs
            .get(output_id)
            .ok_or_else(|| resource_missing("output"))?;
        let bytes = if let Some(index) = index {
            let (start, end) = output
                .ranges
                .get(index)
                .copied()
                .ok_or_else(|| resource_missing("chunk"))?;
            output.bytes[start..end].to_vec()
        } else {
            Vec::new()
        };
        let manifest = output.manifest.clone();
        entry.last_access = access;
        Ok((manifest, bytes))
    }

    pub async fn read_output_chunk(
        &self,
        artifact_id: &str,
        output_id: &str,
        index: usize,
    ) -> Option<Vec<u8>> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        state.access_sequence = state.access_sequence.saturating_add(1);
        let access = state.access_sequence;
        let entry = state.entries.get_mut(artifact_id)?;
        entry.last_access = access;
        let output = entry.outputs.get(output_id)?;
        let (start, end) = *output.ranges.get(index)?;
        Some(output.bytes[start..end].to_vec())
    }

    pub async fn output_manifest(
        &self,
        artifact_id: &str,
        output_id: &str,
    ) -> Option<AttachedOutputManifest> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        state.access_sequence = state.access_sequence.saturating_add(1);
        let access = state.access_sequence;
        let entry = state.entries.get_mut(artifact_id)?;
        entry.last_access = access;
        entry
            .outputs
            .get(output_id)
            .map(|output| output.manifest.clone())
    }

    pub async fn output_manifests(&self) -> Vec<AttachedOutputManifest> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        state
            .entries
            .values()
            .flat_map(|entry| entry.outputs.values())
            .map(|output| output.manifest.clone())
            .collect()
    }

    async fn insert_with_digest(
        &self,
        request_key: ArtifactRequestKey,
        key_digest: String,
        mut capabilities: ArtifactCapabilities,
        payload: CollectedPayload,
    ) -> Result<ArtifactLookup, DevupError> {
        if payload.metadata.get("sectionIndex").is_some()
            && payload.metadata.get("selectedRootIds").is_none()
        {
            capabilities.kind = ArtifactKind::SectionIndex;
        }
        let bytes = serde_json::to_vec(&payload).map_err(|error| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                format!("Cannot serialize the Figma artifact: {error}"),
                false,
            )
        })?;
        let selection_bytes = capabilities
            .section_selection
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|error| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    error.to_string(),
                    false,
                )
            })?
            .map_or(0, |bytes| bytes.len());
        let size_bytes = bytes.len().saturating_add(selection_bytes);
        if size_bytes > self.limits.max_entry_bytes
            || size_bytes > self.limits.max_total_bytes
            || self.limits.max_entries == 0
        {
            return Err(DevupError::with_details(
                ErrorCode::DevupFigmaResponseTooLarge,
                "The Figma artifact exceeded the memory cache limit.",
                false,
                json!({"artifactBytes": bytes.len()}),
            ));
        }
        let now = self.clock.now_epoch_seconds();
        let content_hash = sha256_hex(&bytes);
        let payload = Arc::new(payload);
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        if let Some(previous_id) = state.key_index.remove(&key_digest) {
            remove_entry(&mut state, &previous_id);
        }
        while state.entries.len() >= self.limits.max_entries
            || state.total_bytes.saturating_add(size_bytes) > self.limits.max_total_bytes
        {
            let Some(lru_id) = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            remove_entry(&mut state, &lru_id);
        }
        state.access_sequence = state.access_sequence.saturating_add(1);
        let last_access = state.access_sequence;
        let artifact_id = unique_id(&state.entries);
        let expires_at = now.saturating_add(self.limits.ttl.as_secs());
        state.total_bytes = state.total_bytes.saturating_add(size_bytes);
        state
            .key_index
            .insert(key_digest.clone(), artifact_id.clone());
        state.entries.insert(
            artifact_id.clone(),
            Entry {
                key_digest,
                request_key,
                selection_bytes,
                content_hash: content_hash.clone(),
                created_at: now,
                expires_at,
                size_bytes,
                last_access,
                capabilities: capabilities.clone(),
                payload: payload.clone(),
                outputs: BTreeMap::new(),
                projections: BTreeMap::new(),
            },
        );
        Ok(ArtifactLookup {
            artifact_id,
            content_hash,
            created_at_epoch_seconds: now,
            expires_at_epoch_seconds: expires_at,
            age_seconds: 0,
            remaining_ttl_seconds: self.limits.ttl.as_secs(),
            size_bytes,
            cache_hit: false,
            reuse_kind: CacheReuseKind::Miss,
            capabilities,
            payload,
        })
    }

    fn prune_expired(&self, state: &mut StoreState, now: u64) {
        let ids = state
            .entries
            .iter()
            .filter_map(|(id, entry)| (entry.expires_at <= now).then_some(id.clone()))
            .collect::<Vec<_>>();
        for id in ids {
            remove_entry(state, &id);
            if let Some(recovery) = state.recovery.get_mut(&id) {
                recovery["artifactState"] = json!("expired");
            }
        }
    }
}

fn lookup_by_key(
    state: &mut StoreState,
    key_digest: &str,
    now: u64,
    cache_hit: bool,
) -> Option<ArtifactLookup> {
    let artifact_id = state.key_index.get(key_digest)?.clone();
    let reuse_kind = if cache_hit {
        CacheReuseKind::Exact
    } else {
        CacheReuseKind::Miss
    };
    touch_entry(state, &artifact_id, now, reuse_kind)
}

fn touch_entry(
    state: &mut StoreState,
    artifact_id: &str,
    now: u64,
    reuse_kind: CacheReuseKind,
) -> Option<ArtifactLookup> {
    state.access_sequence = state.access_sequence.saturating_add(1);
    let last_access = state.access_sequence;
    let entry = state.entries.get_mut(artifact_id)?;
    entry.last_access = last_access;
    Some(ArtifactLookup {
        artifact_id: artifact_id.to_owned(),
        content_hash: entry.content_hash.clone(),
        created_at_epoch_seconds: entry.created_at,
        expires_at_epoch_seconds: entry.expires_at,
        age_seconds: now.saturating_sub(entry.created_at),
        remaining_ttl_seconds: entry.expires_at.saturating_sub(now),
        size_bytes: entry.size_bytes,
        cache_hit: reuse_kind != CacheReuseKind::Miss,
        reuse_kind,
        capabilities: entry.capabilities.clone(),
        payload: entry.payload.clone(),
    })
}

fn remove_entry(state: &mut StoreState, artifact_id: &str) {
    if let Some(entry) = state.entries.remove(artifact_id) {
        // Bounded tombstones retain no payload, credentials or original private URL label.
        // Canonical URL and the last selection are enough to reacquire the same capture.
        let key = &entry.request_key;
        let mut url = if let Some(branch) = &key.branch_key {
            format!("https://www.figma.com/branch/{}/{branch}", key.file_key)
        } else {
            format!("https://www.figma.com/design/{}", key.file_key)
        };
        if let Some(node) = &key.node_id {
            url.push_str(&format!("?node-id={}", node.replace(':', "-")));
        }
        let mut args = json!({"url":url,"refresh":true});
        if let Some(selection) = &entry.capabilities.section_selection {
            args["frameIds"] = json!(selection.frame_ids);
            args["allScreens"] = json!(selection.all_screens);
        }
        if state.recovery.len() >= 64 {
            let oldest = state
                .recovery
                .iter()
                .min_by_key(|(_, v)| v["createdAt"].as_u64().unwrap_or(0))
                .map(|(id, _)| id.clone());
            if let Some(id) = oldest {
                state.recovery.remove(&id);
            }
        }
        state.recovery.insert(artifact_id.into(), json!({"artifactState":"evicted","recoveryArguments":args,"createdAt":entry.created_at}));
        state.total_bytes = state.total_bytes.saturating_sub(entry.size_bytes);
        if state.key_index.get(&entry.key_digest) == Some(&artifact_id.to_owned()) {
            state.key_index.remove(&entry.key_digest);
        }
    }
}

fn unique_id(entries: &BTreeMap<String, Entry>) -> String {
    loop {
        let mut bytes = [0_u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let id = URL_SAFE_NO_PAD.encode(bytes);
        if !entries.contains_key(&id) {
            return id;
        }
    }
}

fn valid_output_id(value: &str) -> bool {
    value.len() == 22
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn output_chunk_ranges(bytes: &[u8], is_binary: bool) -> Result<Vec<(usize, usize)>, DevupError> {
    let text = (!is_binary)
        .then(|| std::str::from_utf8(bytes))
        .transpose()
        .map_err(|_| {
            DevupError::new(
                ErrorCode::DevupFigmaHandoffInvalid,
                "A text resource output must be UTF-8.",
                false,
            )
        })?;
    if bytes.is_empty() {
        return Ok(vec![(0, 0)]);
    }
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let mut end = start.saturating_add(RESOURCE_CHUNK_BYTES).min(bytes.len());
        if let Some(text) = text {
            while end > start && !text.is_char_boundary(end) {
                end -= 1;
            }
            if end == start {
                return Err(DevupError::new(
                    ErrorCode::DevupFigmaHandoffInvalid,
                    "Cannot compute the text resource chunk boundary.",
                    false,
                ));
            }
        }
        ranges.push((start, end));
        start = end;
    }
    Ok(ranges)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn acquisition_cancelled() -> DevupError {
    DevupError::new(
        ErrorCode::DevupFigmaDirectUnavailable,
        "Collection of the same Figma artifact was cancelled before it completed.",
        true,
    )
}

fn resource_missing(kind: &str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaHandoffInvalid,
        format!("The resource {kind} was not found. It may have been removed from the cache."),
        false,
        json!({"reason": "not_found", "resourceKind": kind}),
    )
}

fn resource_expired() -> DevupError {
    DevupError::new(
        ErrorCode::DevupFigmaHandoffExpired,
        "The resource artifact is missing or expired.",
        true,
    )
}

#[cfg(test)]
pub(crate) mod w3_tests {
    use super::*;
    use devup_mcp_figma::{
        AssetFormat, CollectionStats, FigmaTarget, PayloadCompleteness, Snapshot,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Default)]
    pub(crate) struct Clock(pub AtomicU64);
    impl ArtifactClock for Clock {
        fn now_epoch_seconds(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }
    pub(crate) fn request() -> CollectionRequest {
        CollectionRequest::new(
            FigmaTarget {
                file_key: "fixture".into(),
                node_id: Some("1:1".into()),
                branch_key: None,
            },
            CollectionScope::Node,
        )
    }
    pub(crate) fn payload() -> CollectedPayload {
        CollectedPayload {
            target: request().target,
            scope: CollectionScope::Node,
            metadata: json!({}),
            snapshot: Snapshot {
                file_key: "fixture".into(),
                version: None,
                roots: vec!["1:1".into()],
                nodes: BTreeMap::new(),
                diagnostics: vec![],
            },
            variables: None,
            styles: None,
            completeness: PayloadCompleteness::ResolvedValuesOnly,
            source_version: None,
            stats: CollectionStats::default(),
            assets: vec![],
            reference_png: None,
            failures: vec![],
        }
    }

    /// Settles the question W3 left open about `ArtifactRequestKey::digest`.
    ///
    /// It ends in `unwrap_or_default()`, so a serialisation failure would not
    /// raise anything — it would hand back the digest of the *empty* byte
    /// string, and every key that failed would land on that one entry. Two
    /// unrelated collections would then read each other's design.
    ///
    /// It cannot fail today. The key is `String`, `Option<String>`, `bool`,
    /// `usize`, `u8`, three fieldless enums and `Vec`s of those, every one of
    /// them `#[derive(Serialize)]`; `serde_json` fails on a non-string map
    /// key, a hand-written `Serialize` that returns an error, or a writer that
    /// errors, and the writer here is a `Vec<u8>`. There is no float, so not
    /// even a non-finite one to argue about. So the fallback stays.
    ///
    /// What it stays subject to is a field added later that *can* fail — at
    /// which point the collapse is silent. This fills every field and asserts
    /// each one still reaches a digest of its own, none of them the empty one.
    #[test]
    fn w3_every_field_of_a_request_key_reaches_its_digest() {
        let populated = || {
            let mut request = request();
            request.resource_scope = ResourceScope::Used;
            request.include_context = true;
            request.metadata_only = true;
            request.variables_only = true;
            request.reference_png = true;
            request.search = Some(SearchReadOptions {
                query: "STORY-F-PROOFREAD".into(),
                node_types: vec!["FRAME".into()],
                match_kind: "normalized".into(),
                limit: 20,
            });
            request.explore = Some(ExploreReadOptions {
                projection_limit: 50,
                text_preview_limit: 10,
            });
            request.section = Some(SectionReadOptions {
                frame_ids: vec!["2:2".into()],
                all_screens: false,
            });
            request.asset_selections = vec![AssetSelection {
                asset_id: "asset".into(),
                format: AssetFormat::Png,
                scale: 2,
            }];
            request
        };

        let mut branch_key = populated();
        branch_key.target.branch_key = Some("branch".into());
        let mut file_key = populated();
        file_key.target.file_key = "other".into();
        let mut node_id = populated();
        node_id.target.node_id = Some("9:9".into());
        let mut scope = populated();
        scope.scope = CollectionScope::File;
        let mut resource_scope = populated();
        resource_scope.resource_scope = ResourceScope::File;
        let mut include_context = populated();
        include_context.include_context = false;
        let mut metadata_only = populated();
        metadata_only.metadata_only = false;
        let mut variables_only = populated();
        variables_only.variables_only = false;
        let mut reference_png = populated();
        reference_png.reference_png = false;
        let mut search = populated();
        search.search = None;
        let mut explore = populated();
        explore.explore = None;
        let mut section = populated();
        section.section = None;
        let mut asset_format = populated();
        asset_format.asset_selections[0].format = AssetFormat::Svg;
        let mut asset_scale = populated();
        asset_scale.asset_selections[0].scale = 3;
        let mut asset_id = populated();
        asset_id.asset_selections[0].asset_id = "elsewhere".into();

        let empty = sha256_hex(&[]);
        let digests = [
            populated(),
            branch_key,
            file_key,
            node_id,
            scope,
            resource_scope,
            include_context,
            metadata_only,
            variables_only,
            reference_png,
            search,
            explore,
            section,
            asset_format,
            asset_scale,
            asset_id,
        ]
        .iter()
        .map(|request| ArtifactRequestKey::from_collection(request).digest())
        .collect::<Vec<_>>();

        for digest in &digests {
            assert_ne!(
                digest, &empty,
                "a key that serialised to nothing would share one cache entry with every other such key"
            );
        }
        let distinct = digests.iter().collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            distinct.len(),
            digests.len(),
            "each field has to reach the digest, or two different collections share an artifact"
        );
    }

    #[tokio::test]
    async fn w3_selection_is_available_after_artifact_id_lookup() {
        let store = ArtifactStore::default();
        let mut request = request();
        request.section = Some(SectionReadOptions {
            frame_ids: vec!["2:2".into(), "3:3".into()],
            all_screens: false,
        });
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request), payload())
            .await
            .unwrap();
        let restored = store.get(&artifact.artifact_id).await.unwrap();
        let metadata = serde_json::to_value(&restored.capabilities).unwrap();
        assert_eq!(
            metadata["sectionSelection"]["frameIds"],
            json!(["2:2", "3:3"])
        );
    }

    #[tokio::test]
    async fn w3_last_selection_survives_lookup_without_changing_acquisition_key() {
        let store = ArtifactStore::default();
        let mut request = request();
        request.section = Some(SectionReadOptions {
            frame_ids: vec![],
            all_screens: false,
        });
        let key = ArtifactRequestKey::from_collection(&request);
        let artifact = store.insert(key.clone(), payload()).await.unwrap();
        store
            .remember_section_selection(
                &artifact.artifact_id,
                SectionReadOptions {
                    frame_ids: vec!["3:3".into()],
                    all_screens: false,
                },
            )
            .await
            .unwrap();
        let restored = store.lookup(&key).await.unwrap();
        assert_eq!(restored.artifact_id, artifact.artifact_id);
        assert_eq!(
            serde_json::to_value(&restored.capabilities).unwrap()["sectionSelection"]["frameIds"],
            json!(["3:3"])
        );
        store
            .remember_section_selection(
                &artifact.artifact_id,
                SectionReadOptions {
                    frame_ids: vec![],
                    all_screens: true,
                },
            )
            .await
            .unwrap();
        let restored = store.get(&artifact.artifact_id).await.unwrap();
        assert_eq!(
            serde_json::to_value(&restored.capabilities).unwrap()["sectionSelection"]["allScreens"],
            true
        );
    }

    #[tokio::test]
    async fn w3_unselected_section_index_remembers_its_first_selection() {
        let store = ArtifactStore::default();
        let mut payload = payload();
        payload.metadata = json!({"sectionIndex": {}});
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request()), payload)
            .await
            .unwrap();
        assert_eq!(artifact.capabilities.kind, ArtifactKind::SectionIndex);
        let selection = SectionReadOptions {
            frame_ids: vec!["2:2".into()],
            all_screens: false,
        };
        store
            .remember_section_selection(&artifact.artifact_id, selection.clone())
            .await
            .unwrap();
        assert_eq!(
            store
                .get(&artifact.artifact_id)
                .await
                .unwrap()
                .capabilities
                .section_selection,
            Some(selection)
        );
    }

    #[tokio::test]
    async fn r8_expired_branch_url_preserves_target_and_selection() {
        let clock = Arc::new(Clock::default());
        let store = ArtifactStore::with_clock(
            clock.clone(),
            ArtifactLimits {
                ttl: Duration::from_secs(1),
                ..Default::default()
            },
        );
        let mut request = request();
        request.target.branch_key = Some("Branch123".into());
        request.section = Some(SectionReadOptions {
            frame_ids: vec!["2:2".into()],
            all_screens: false,
        });
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request), payload())
            .await
            .unwrap();
        clock.0.store(1, Ordering::SeqCst);
        let error = store
            .get_for_export(&artifact.artifact_id)
            .await
            .unwrap_err();
        let arguments = &error.details["recoveryArguments"];
        let recovered =
            devup_mcp_figma::FigmaTarget::parse(arguments["url"].as_str().unwrap()).unwrap();
        assert_eq!(recovered, request.target);
        assert_eq!(arguments["frameIds"], json!(["2:2"]));
    }

    #[tokio::test]
    async fn w3_expired_key_is_reacquired_at_ttl_boundary() {
        let clock = Arc::new(Clock::default());
        let store = ArtifactStore::with_clock(
            clock.clone(),
            ArtifactLimits {
                ttl: Duration::from_secs(10),
                ..ArtifactLimits::default()
            },
        );
        let key = ArtifactRequestKey::from_collection(&request());
        let first = store.insert(key.clone(), payload()).await.unwrap();
        clock.0.store(10, Ordering::SeqCst);
        assert!(store.lookup(&key).await.is_none());
        let expired = store.get_for_export(&first.artifact_id).await.unwrap_err();
        assert_eq!(expired.details["artifactState"], "expired");
        assert!(
            expired.details["recoveryArguments"]["url"]
                .as_str()
                .unwrap()
                .contains("figma.com/design/")
        );
        let unknown = ArtifactStore::default()
            .get_for_export(&first.artifact_id)
            .await
            .unwrap_err();
        assert_eq!(unknown.details["artifactState"], "unknown");
        assert!(unknown.details.get("recoveryArguments").is_none());
        let second = store
            .get_or_acquire(key, false, || async { Ok(payload()) })
            .await
            .unwrap();
        assert!(!second.cache_hit);
        assert_ne!(first.artifact_id, second.artifact_id);
    }

    #[tokio::test]
    async fn w3_invalid_or_oversized_selection_does_not_replace_previous_selection() {
        let store = ArtifactStore::with_limits(ArtifactLimits {
            max_entry_bytes: 2048,
            ..ArtifactLimits::default()
        });
        let mut request = request();
        request.section = Some(SectionReadOptions {
            frame_ids: vec!["2:2".into()],
            all_screens: false,
        });
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request), payload())
            .await
            .unwrap();
        let original_bytes = store.stats().await.total_bytes;
        for selection in [
            SectionReadOptions {
                frame_ids: vec![],
                all_screens: false,
            },
            SectionReadOptions {
                frame_ids: vec!["3:3".into()],
                all_screens: true,
            },
            SectionReadOptions {
                frame_ids: vec!["3:3".into(), "3:3".into()],
                all_screens: false,
            },
            SectionReadOptions {
                frame_ids: vec!["x".repeat(4096)],
                all_screens: false,
            },
        ] {
            assert!(
                store
                    .remember_section_selection(&artifact.artifact_id, selection)
                    .await
                    .is_err()
            );
            assert_eq!(
                store
                    .get(&artifact.artifact_id)
                    .await
                    .unwrap()
                    .capabilities
                    .section_selection,
                request.section
            );
            assert_eq!(store.stats().await.total_bytes, original_bytes);
        }
        let updated = store
            .remember_section_selection(
                &artifact.artifact_id,
                SectionReadOptions {
                    frame_ids: vec![],
                    all_screens: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.size_bytes, store.stats().await.total_bytes);
        assert_eq!(updated.content_hash, artifact.content_hash);
        assert_eq!(
            updated.expires_at_epoch_seconds,
            artifact.expires_at_epoch_seconds
        );
    }

    #[test]
    fn w3_asset_capture_validation_reports_only_missing_exact_variants() {
        let mut request = request();
        let captured = AssetSelection {
            asset_id: "2:2:node".into(),
            format: AssetFormat::Svg,
            scale: 1,
        };
        request.asset_selections = vec![captured.clone()];
        let capabilities = ArtifactRequestKey::from_collection(&request).capabilities();
        assert!(capabilities.validate_asset_captures(&[]).is_ok());
        assert!(
            capabilities
                .validate_asset_captures(std::slice::from_ref(&captured))
                .is_ok()
        );
        let missing = AssetSelection {
            scale: 2,
            ..captured.clone()
        };
        let error = capabilities
            .validate_asset_captures(&[captured, missing])
            .unwrap_err();
        assert_eq!(
            error.details["missingAssetCaptures"],
            json!([{"assetId":"2:2:node", "format":"svg", "scale":2}])
        );
        assert!(!error.retryable);
    }

    #[test]
    fn w3_missing_asset_capture_has_actionable_details() {
        let capabilities = ArtifactRequestKey::from_collection(&request()).capabilities();
        let selection = AssetSelection {
            asset_id: "2:2:node".into(),
            format: AssetFormat::Svg,
            scale: 1,
        };
        // The storage-layer validator will be consumed by W1's projection validation.
        let error = capabilities
            .validate_asset_captures(&[selection])
            .unwrap_err();
        assert!(
            error.message.contains("Remove artifactId")
                && error.message.contains("original url")
                && error.message.contains("assetRequests")
        );
        assert_eq!(
            error.details["missingAssetCaptures"][0]["assetId"],
            "2:2:node"
        );
    }
}
