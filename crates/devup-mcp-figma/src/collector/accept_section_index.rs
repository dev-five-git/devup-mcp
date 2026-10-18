impl CollectorSession {

    fn accept_section_index(
        &mut self,
        planned: &PlannedCall,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let chunk = snapshot_chunk_from_result(&result)?;
        if chunk.file_key != planned.expected_file_key {
            return Err(invalid_call(
                "Figma Section index file key does not match the request.",
            ));
        }
        let section_id = planned
            .expected_node_id
            .as_deref()
            .ok_or_else(|| invalid_call("Figma Section index node ID is missing."))?;
        if chunk.root_ids.as_slice() != [section_id]
            || !chunk.nodes.iter().any(|node| node.id == section_id)
        {
            return Err(invalid_call(
                "Figma Section index does not match the requested Section.",
            ));
        }
        let snapshot = merge_chunks(vec![chunk.clone()])?;
        let index = build_section_index(&snapshot, &self.request.target)?;
        let options = self
            .request
            .section
            .clone()
            .ok_or_else(|| invalid_call("Section read options are missing."))?;
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
}