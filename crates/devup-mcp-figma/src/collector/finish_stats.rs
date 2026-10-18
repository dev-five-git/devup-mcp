impl CollectorSession {

    fn finish_stats(&mut self, chunks: &[SnapshotChunk]) {
        self.stats.node_count = chunks
            .iter()
            .flat_map(|chunk| chunk.nodes.iter().map(|node| node.id.as_str()))
            .collect::<BTreeSet<_>>()
            .len();
        if let Some(resources) = &self.variables {
            self.stats.variable_count = resource_count(&resources.raw, "variables");
            self.stats.style_count = resource_count(&resources.raw, "styles");
        }
    }
}