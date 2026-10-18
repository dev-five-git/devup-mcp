impl CollectorSession {

    fn accept_projection(
        &mut self,
        planned: &PlannedCall,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let chunk = snapshot_chunk_from_result(&result)?;
        if chunk.file_key != planned.expected_file_key {
            return Err(invalid_call(
                "Figma projection file key does not match the request.",
            ));
        }
        let expected_node_id = planned
            .expected_node_id
            .as_deref()
            .ok_or_else(|| invalid_call("Figma projection expected node ID is missing."))?;
        if !chunk.nodes.iter().any(|node| node.id == expected_node_id) {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "anchor node not found in the Figma projection.",
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
}