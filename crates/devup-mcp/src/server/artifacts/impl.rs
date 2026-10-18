

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
                missing_artifact_details(&state, artifact_id),
            )
        }).and_then(|artifact| {
            if artifact.capabilities.kind == ArtifactKind::Measurement {
                Err(DevupError::with_details(ErrorCode::DevupFigmaHandoffInvalid,"A measurement-only artifact cannot supply export outputs; acquire a design export by explicit URL.",false,json!({"capabilities":artifact.capabilities,"artifactId":artifact.artifact_id})))
            } else {Ok(artifact)}
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
                    asset_metadata: output.asset_metadata,
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
        let Some(entry) = state.entries.get(artifact_id) else {
            let mut error = resource_missing("artifact");
            merge_details(&mut error, missing_artifact_details(&state, artifact_id));
            return Err(error);
        };
        if entry.expires_at <= now {
            return Err(DevupError::with_details(
                ErrorCode::DevupFigmaHandoffExpired,
                "The resource artifact has expired. Re-collect it from the original URL.",
                true,
                json!({"artifactState":"expired","recoveryArguments":recovery_arguments(entry)}),
            ));
        }
        state.access_sequence = state.access_sequence.saturating_add(1);
        let access = state.access_sequence;
        let entry = state
            .entries
            .get_mut(artifact_id)
            .ok_or_else(|| resource_missing("artifact"))?;
        let remaining_ttl_seconds = entry.expires_at.saturating_sub(now);
        let Some(output) = entry.outputs.get(output_id) else {
            let mut error = resource_missing("output");
            merge_details(
                &mut error,
                json!({"artifactState":"live","remainingTtlSeconds":remaining_ttl_seconds}),
            );
            return Err(error);
        };
        let bytes = if let Some(index) = index {
            let Some((start, end)) = output.ranges.get(index).copied() else {
                let mut error = resource_missing("chunk");
                merge_details(
                    &mut error,
                    json!({"artifactState":"live","chunkIndex":index,
                        "chunkCount":output.ranges.len(),"manifestUri":output.manifest.manifest_uri}),
                );
                return Err(error);
            };
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
        if request_key.web_ready_images {
            capabilities.asset_captures = payload
                .assets
                .iter()
                .filter_map(|a| {
                    Some(AssetSelection {
                        usage_aspect_ratio: a.usage_aspect_ratio,
                        max_width: a.max_width,
                        srcset: a.srcset,
                        visible_scope_node_id: a.visible_scope_node_id.clone(),
                        asset_id: a.asset_id.clone(),
                        format: a.format?,
                        scale: a.scale?,
                        render_mode: a.render_mode,
                        quality: a.quality,
                        lossless: a.lossless,
                    })
                })
                .collect();
            capabilities.asset_capture_count = capabilities.asset_captures.len();
        }
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

/// Canonical URL and the last selection: enough to reacquire the same capture,
/// with no payload, credential or original private URL label retained.
fn recovery_arguments(entry: &Entry) -> serde_json::Value {
    let key = &entry.request_key;
    let url = devup_mcp_figma::FigmaTarget {
        file_key: key.file_key.clone(),
        node_id: None,
        branch_key: key.branch_key.clone(),
    }
    .link(key.node_id.as_deref());
    if let Some(ids) = &key.measurement_node_ids {
        return json!({"url":url,"nodeIds":ids});
    }
    let mut args = json!({"url":url,"refresh":true});
    if let Some(selection) = &entry.capabilities.section_selection {
        args["frameIds"] = json!(selection.frame_ids);
        args["allScreens"] = json!(selection.all_screens);
    }
    args
}

/// What this process can say about an id it does not hold: its tombstone if
/// it once held it, and otherwise whether the id was even issued here.
fn missing_artifact_details(state: &StoreState, artifact_id: &str) -> serde_json::Value {
    if let Some(tombstone) = state.recovery.get(artifact_id) {
        return tombstone.clone();
    }
    if issued_by_another_process(artifact_id) {
        json!({"artifactState":"issued-by-another-process"})
    } else {
        json!({"artifactState":"unknown"})
    }
}

fn merge_details(error: &mut DevupError, extra: serde_json::Value) {
    if let (Some(details), serde_json::Value::Object(extra)) =
        (error.details.as_object_mut(), extra)
    {
        details.extend(extra);
    }
}

fn remove_entry(state: &mut StoreState, artifact_id: &str) {
    if let Some(entry) = state.entries.remove(artifact_id) {
        let args = recovery_arguments(&entry);
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

/// How many leading bytes of every artifactId name the process that issued it.
///
/// Artifacts live only in the memory of the process that collected them, so an
/// id this store does not hold is either one it forgot or one another process
/// issued: the server that ran before a restart, or a second devup-mcp session.
/// Only the first is this process's to explain, and the stamp tells them apart
/// without writing anything to disk. The id keeps its 32-byte, 43-character
/// shape, so resource URIs do not change.
const PROCESS_TAG_BYTES: usize = 6;

fn process_tag() -> &'static [u8; PROCESS_TAG_BYTES] {
    static TAG: OnceLock<[u8; PROCESS_TAG_BYTES]> = OnceLock::new();
    TAG.get_or_init(|| {
        let mut tag = [0_u8; PROCESS_TAG_BYTES];
        rand::rng().fill_bytes(&mut tag);
        tag
    })
}

/// True only for a well-formed id stamped by a different process. A malformed
/// id is not claimed to come from anywhere.
pub(crate) fn issued_by_another_process(artifact_id: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(artifact_id)
        .ok()
        .filter(|bytes| bytes.len() == 32)
        .is_some_and(|bytes| bytes[..PROCESS_TAG_BYTES] != process_tag()[..])
}

fn unique_id(entries: &BTreeMap<String, Entry>) -> String {
    loop {
        let mut bytes = [0_u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        bytes[..PROCESS_TAG_BYTES].copy_from_slice(process_tag());
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