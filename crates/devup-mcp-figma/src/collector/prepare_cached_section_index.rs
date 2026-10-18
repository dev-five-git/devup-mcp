impl CollectorSession {

    fn prepare_cached_section_index(&mut self, index: SectionIndex) -> Result<(), DevupError> {
        if index.file_key != self.request.target.file_key
            || self.request.target.node_id.as_deref() != Some(index.section.node_id.as_str())
        {
            return Err(invalid_call(
                "cached Section index does not match the requested Section.",
            ));
        }
        let options = self
            .request
            .section
            .clone()
            .ok_or_else(|| invalid_call("cached Section index has no selection options."))?;
        // Compact indexes only retain ownership for IDs queried during that
        // request. A newly requested descendant needs a fresh index query.
        if options.frame_ids.iter().any(|id| {
            !index
                .candidates
                .iter()
                .any(|candidate| candidate.node_id == *id)
                && !index.node_screen_ids.contains_key(id)
        }) {
            return Ok(());
        }
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
            .ok_or_else(|| invalid_call("cached Section index Section ID is missing."))?;
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
}