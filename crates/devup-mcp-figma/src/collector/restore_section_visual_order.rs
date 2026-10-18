impl CollectorSession {

    fn restore_section_visual_order(
        &self,
        chunks: Vec<SnapshotChunk>,
    ) -> Result<Vec<SnapshotChunk>, DevupError> {
        if self.section_selected_roots.is_empty() || chunks.is_empty() {
            return Ok(chunks);
        }
        let snapshot = merge_chunks(chunks)?;
        let observed = snapshot.roots.iter().collect::<BTreeSet<_>>();
        let failed = self
            .screen_failures
            .iter()
            .map(|failure| failure.node_id.as_str())
            .collect::<BTreeSet<_>>();
        if self
            .section_selected_roots
            .iter()
            .any(|root_id| !observed.contains(root_id) && !failed.contains(root_id.as_str()))
        {
            return Err(invalid_call(
                "Section snapshot does not include all selected roots.",
            ));
        }
        Ok(vec![SnapshotChunk {
            file_key: snapshot.file_key,
            version: snapshot.version,
            root_ids: self
                .section_selected_roots
                .iter()
                .filter(|root_id| observed.contains(*root_id))
                .cloned()
                .collect(),
            nodes: snapshot.nodes.into_values().collect(),
            diagnostics: snapshot.diagnostics,
        }])
    }
}