impl CollectorSession {

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
            invalid_call("No root node ID available for the used Figma resource batch.")
        })?;
        for batch in used_resource_batches(&refs, self.request.batch_budget)? {
            self.enqueue(
                ReadToolCall::used_resources(&self.request.target.file_key, &node_id, batch),
                Some(node_id.clone()),
                CallKind::UsedResourceBatch,
            );
        }
        self.used_resource_refs = Some(refs);
        Ok(())
    }
}