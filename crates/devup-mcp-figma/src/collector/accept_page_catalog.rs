impl CollectorSession {

    fn accept_page_catalog(
        &mut self,
        planned: &PlannedCall,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let catalog = snapshot_chunk_from_result(&result)?;
        if catalog.file_key != planned.expected_file_key {
            return Err(invalid_call(
                "Figma page catalog file key does not match the request.",
            ));
        }
        if catalog.root_ids.is_empty() {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "Figma page catalog is empty.",
                false,
            ));
        }
        let options = self
            .request
            .search
            .clone()
            .ok_or_else(|| invalid_call("Collected a page catalog without search options."))?;
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
}