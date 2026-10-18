impl CollectorSession {

    pub fn advance(&mut self) -> Result<CollectorStep, DevupError> {
        if self.completed {
            return Err(invalid_call(
                "This Figma collection session is already complete.",
            ));
        }
        if self.section_index.is_none()
            && let Some(index) = self.request.cached_section_index.take()
        {
            self.prepare_cached_section_index(index)?;
            return self.advance();
        }
        if self.metadata.is_none() && self.pending.is_empty() && self.queued.is_empty() {
            if self.request.section.is_some() && self.section_index.is_none() {
                let node_id = self
                    .request
                    .target
                    .node_id
                    .clone()
                    .ok_or_else(|| invalid_call("Figma Section index requires a node ID."))?;
                let mut call = ReadToolCall::section_index(&self.request.target.file_key, &node_id);
                if let ReadToolCall::Snapshot { root_ids, .. } = &mut call {
                    *root_ids = self.request.section.as_ref().map(|s| s.frame_ids.clone());
                }
                self.enqueue(call, Some(node_id), CallKind::SectionIndex);
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
                    invalid_call("Figma nearby-screen exploration requires a node ID.")
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
            if let Some(options) = self.request.search.clone() {
                if self.request.scope == CollectionScope::Node {
                    let node_id = self.request.target.node_id.clone().ok_or_else(|| {
                        invalid_call("Node-scoped Figma search requires a node ID.")
                    })?;
                    self.enqueue(
                        ReadToolCall::search_snapshot(
                            &self.request.target.file_key,
                            &node_id,
                            options,
                        ),
                        Some(node_id),
                        CallKind::ScopedSearch,
                    );
                    return self.advance();
                }
                self.enqueue(
                    ReadToolCall::page_catalog(&self.request.target.file_key),
                    None,
                    CallKind::PageCatalog,
                );
                return self.advance();
            }
            if self.fast_path_eligible() && !self.fast_attempted {
                let node_id = self
                    .request
                    .target
                    .node_id
                    .clone()
                    .ok_or_else(|| invalid_call("Figma fast snapshot requires a node ID."))?;
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
            if !self.asset_plan_initialized {
                self.asset_plan_initialized = true;
                if self.request.web_ready_images {
                    let census = crate::discover_web_ready_assets(&snapshot);
                    self.request.asset_selections = census
                        .iter()
                        .map(|a| AssetSelection {
                            usage_aspect_ratio: None,
                            max_width: snapshot
                                .nodes
                                .get(&a.node_id)
                                .and_then(|n| n.typed_view().number("width"))
                                .filter(|w| w.is_finite() && *w > 0.0)
                                .map(|w| (w * 2.0).ceil().clamp(1.0, 8192.0) as u32),
                            srcset: false,
                            visible_scope_node_id: None,
                            asset_id: a.asset_id.clone(),
                            format: crate::AssetFormat::Webp,
                            scale: 2.0,
                            render_mode: crate::AssetRenderMode::OriginalSource,
                            quality: Some(85),
                            lossless: Some(false),
                        })
                        .collect();
                    for (index, asset) in census.iter().enumerate() {
                        if asset.image_hash.is_some() && index < 4096 {
                            continue;
                        }
                        let mut failure = asset.clone();
                        failure.status = AssetStatus::Failed;
                        if index >= 4096 {
                            failure.error_code = Some("DEVUP_ASSET_PLAN_LIMIT".into());
                            failure.error_detail=Some("Preset scheduling is bounded to4096 IMAGE sources; recollect failed IDs in a smaller scope.".into());
                        } else if asset.image_hash.is_none() {
                            failure.error_code = Some("DEVUP_ORIGINAL_SOURCE_UNAVAILABLE".into());
                            failure.error_detail = Some(
                                "The acquired IMAGE paint has no resolvable source hash.".into(),
                            );
                        }
                        failure.render_mode = crate::AssetRenderMode::OriginalSource;
                        failure.format = Some(crate::AssetFormat::Webp);
                        failure.scale = Some(2.0);
                        failure.max_width = self.request.asset_selections[index].max_width;
                        failure.quality = Some(85);
                        failure.lossless = Some(false);
                        self.asset_results.push(failure);
                    }
                }
                let selections = self
                    .request
                    .asset_selections
                    .iter()
                    .filter(|s| !self.asset_results.iter().any(|a| a.asset_id == s.asset_id))
                    .cloned()
                    .collect::<Vec<_>>();
                for batch in selections.chunks(6) {
                    self.asset_plan
                        .extend(resolve_asset_selections(&snapshot, batch)?);
                }
            }
            // A later batch is scheduled only after every call/fragment in this
            // batch has settled. The ordinary pending-call bound still applies.
            if self.asset_byte_budget_exhausted {
                while let Some(request) = self.asset_plan.pop_front() {
                    self.record_asset_failure(request, "DEVUP_ASSET_TOTAL_BYTES_EXCEEDED", Some("The 128 MiB retained asset budget was reached. Export failed IDs in smaller custom batches or select a smaller frame.".into()));
                }
            }
            let mut nodes = BTreeSet::new();
            let mut batch = Vec::new();
            let remaining = self.asset_plan.len();
            for _ in 0..remaining {
                let request = self.asset_plan.pop_front().unwrap();
                if batch.len() < 6 && nodes.insert(request.node_id.clone()) {
                    batch.push(request);
                } else {
                    self.asset_plan.push_back(request);
                }
            }
            self.assets_scheduled = self.asset_plan.is_empty();
            for request in batch {
                if self.asset_byte_budget_exhausted {
                    self.record_asset_failure(request, "DEVUP_ASSET_TOTAL_BYTES_EXCEEDED", Some("The 128 MiB retained asset budget was reached. Export the failed asset IDs in smaller custom batches or select a smaller frame.".into()));
                    continue;
                }
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
                invalid_call("Figma reference PNG collection requires a node ID.")
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
            && self.file_local_resources.is_none()
            && self.variables.is_none()
            && self.queued.is_empty()
        {
            let node_id = self.root_node_id.clone().ok_or_else(|| {
                invalid_call("No root node ID available for Figma variable collection.")
            })?;
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
                .ok_or_else(|| invalid_call("Figma variable catalog is missing."))?;
            let local = merge_variable_results(
                catalog,
                std::mem::take(&mut self.variable_batches).into_values(),
            )?;
            // The catalog is the file's own variables and styles. The library
            // ones its nodes use, and those its variables alias, were left
            // out, so a theme of the whole file knew only the local half: a
            // local spacing aliasing a library primitive had no value. They
            // are read as a used-resource read reads them and joined to it.
            let chunks = self.snapshot_chunks.values().cloned().collect::<Vec<_>>();
            let refs = library_refs(&local, collect_used_resource_refs(&chunks));
            if refs.variable_ids.is_empty() && refs.styles.is_empty() {
                self.variables = Some(join_used_resources(local, None));
            } else {
                let node_id = self.root_node_id.clone().ok_or_else(|| {
                    invalid_call("No root node ID available for the used Figma resource batch.")
                })?;
                for batch in used_resource_batches(&refs, self.request.batch_budget)? {
                    self.enqueue(
                        ReadToolCall::used_resources(
                            &self.request.target.file_key,
                            &node_id,
                            batch,
                        ),
                        Some(node_id.clone()),
                        CallKind::UsedResourceBatch,
                    );
                }
                self.used_resource_refs = Some(refs);
                self.file_local_resources = Some(local);
                return self.advance();
            }
        }
        if self.metadata.is_some()
            && self.request.resource_scope == ResourceScope::File
            && self.file_local_resources.is_some()
            && self.variables.is_none()
            && self.queued.is_empty()
        {
            let refs = self
                .used_resource_refs
                .take()
                .ok_or_else(|| invalid_call("Used Figma resource references are missing."))?;
            let local = self
                .file_local_resources
                .take()
                .ok_or_else(|| invalid_call("The file's own Figma resources are missing."))?;
            let merged = merge_used_resource_results(
                &refs,
                std::mem::take(&mut self.variable_batches).into_values(),
            )?;
            self.record_unresolved_diagnostics(&refs, &merged.unresolved);
            self.variables = Some(join_used_resources(local, Some(merged.result)));
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
                .ok_or_else(|| invalid_call("Used Figma resource references are missing."))?;
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
                        .ok_or_else(|| invalid_call("fallback resource result is missing."))?,
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
}