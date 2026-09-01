use std::{
    collections::BTreeMap,
    future::Future,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use devup_mcp_figma::{
    AssetSelection, CollectedPayload, CollectionRequest, CollectionScope, DevupError, ErrorCode,
    ExploreReadOptions, ResourceScope, SearchReadOptions, SectionReadOptions, SourcePolicy,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, watch};

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
    source_policy: SourcePolicy,
}

impl ArtifactRequestKey {
    pub fn from_collection(request: &CollectionRequest, source_policy: SourcePolicy) -> Self {
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
            section: request.section.clone(),
            asset_selections: request.asset_selections.clone(),
            source_policy,
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
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactCapabilities {
    pub kind: ArtifactKind,
    pub collection_scope: CollectionScope,
    pub resource_scope: ResourceScope,
    pub asset_capture_count: usize,
    #[serde(skip)]
    asset_captures: Vec<AssetSelection>,
}

impl ArtifactCapabilities {
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
    pub size_bytes: usize,
    pub cache_hit: bool,
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

#[derive(Debug)]
struct Entry {
    key_digest: String,
    content_hash: String,
    created_at: u64,
    expires_at: u64,
    size_bytes: usize,
    last_access: u64,
    capabilities: ArtifactCapabilities,
    payload: Arc<CollectedPayload>,
}

type AcquisitionResult = Result<ArtifactLookup, DevupError>;

#[derive(Default)]
struct StoreState {
    entries: BTreeMap<String, Entry>,
    key_index: BTreeMap<String, String>,
    in_flight: BTreeMap<String, watch::Receiver<Option<AcquisitionResult>>>,
    total_bytes: usize,
    access_sequence: u64,
}

#[derive(Clone)]
pub struct ArtifactStore {
    state: Arc<Mutex<StoreState>>,
    clock: Arc<dyn ArtifactClock>,
    limits: ArtifactLimits,
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
        let (owner, mut receiver, sender) = {
            let now = self.clock.now_epoch_seconds();
            let mut state = self.state.lock().await;
            self.prune_expired(&mut state, now);
            if !refresh && let Some(hit) = lookup_by_key(&mut state, &key_digest, true) {
                return Ok(hit);
            }
            if let Some(receiver) = state.in_flight.get(&key_digest) {
                (false, receiver.clone(), None)
            } else {
                let (sender, receiver) = watch::channel(None);
                state.in_flight.insert(key_digest.clone(), receiver.clone());
                (true, receiver, Some(sender))
            }
        };

        if !owner {
            receiver
                .wait_for(Option::is_some)
                .await
                .map_err(|_| acquisition_cancelled())?;
            let mut result = receiver
                .borrow()
                .clone()
                .ok_or_else(acquisition_cancelled)?;
            if let Ok(hit) = &mut result {
                hit.cache_hit = true;
            }
            return result;
        }

        let result = match acquire().await {
            Ok(payload) => {
                self.insert_with_digest(key_digest.clone(), capabilities, payload)
                    .await
            }
            Err(error) => Err(error),
        };
        {
            let mut state = self.state.lock().await;
            state.in_flight.remove(&key_digest);
        }
        if let Some(sender) = sender {
            let _ = sender.send(Some(result.clone()));
        }
        result
    }

    pub async fn insert(
        &self,
        key: ArtifactRequestKey,
        payload: CollectedPayload,
    ) -> Result<ArtifactLookup, DevupError> {
        self.insert_with_digest(key.digest(), key.capabilities(), payload)
            .await
    }

    pub async fn get(&self, artifact_id: &str) -> Option<ArtifactLookup> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        touch_entry(&mut state, artifact_id, true)
    }

    pub async fn lookup(&self, key: &ArtifactRequestKey) -> Option<ArtifactLookup> {
        let now = self.clock.now_epoch_seconds();
        let mut state = self.state.lock().await;
        self.prune_expired(&mut state, now);
        lookup_by_key(&mut state, &key.digest(), true)
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

    async fn insert_with_digest(
        &self,
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
                format!("Figma artifact를 직렬화할 수 없습니다: {error}"),
                false,
            )
        })?;
        if bytes.len() > self.limits.max_entry_bytes
            || bytes.len() > self.limits.max_total_bytes
            || self.limits.max_entries == 0
        {
            return Err(DevupError::with_details(
                ErrorCode::DevupFigmaResponseTooLarge,
                "Figma artifact가 메모리 캐시 한도를 초과했습니다.",
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
            || state.total_bytes.saturating_add(bytes.len()) > self.limits.max_total_bytes
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
        state.total_bytes = state.total_bytes.saturating_add(bytes.len());
        state
            .key_index
            .insert(key_digest.clone(), artifact_id.clone());
        state.entries.insert(
            artifact_id.clone(),
            Entry {
                key_digest,
                content_hash: content_hash.clone(),
                created_at: now,
                expires_at,
                size_bytes: bytes.len(),
                last_access,
                capabilities: capabilities.clone(),
                payload: payload.clone(),
            },
        );
        Ok(ArtifactLookup {
            artifact_id,
            content_hash,
            created_at_epoch_seconds: now,
            expires_at_epoch_seconds: expires_at,
            size_bytes: bytes.len(),
            cache_hit: false,
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
        }
    }
}

fn lookup_by_key(
    state: &mut StoreState,
    key_digest: &str,
    cache_hit: bool,
) -> Option<ArtifactLookup> {
    let artifact_id = state.key_index.get(key_digest)?.clone();
    touch_entry(state, &artifact_id, cache_hit)
}

fn touch_entry(
    state: &mut StoreState,
    artifact_id: &str,
    cache_hit: bool,
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
        size_bytes: entry.size_bytes,
        cache_hit,
        capabilities: entry.capabilities.clone(),
        payload: entry.payload.clone(),
    })
}

fn remove_entry(state: &mut StoreState, artifact_id: &str) {
    if let Some(entry) = state.entries.remove(artifact_id) {
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

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn acquisition_cancelled() -> DevupError {
    DevupError::new(
        ErrorCode::DevupFigmaDirectUnavailable,
        "동일 Figma artifact 수집이 완료되기 전에 취소되었습니다.",
        true,
    )
}
