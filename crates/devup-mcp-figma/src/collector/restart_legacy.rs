impl CollectorSession {

    fn restart_legacy(&mut self, reason: String) {
        self.queued.clear();
        self.pending.clear();
        self.metadata = None;
        self.metadata_nodes.clear();
        self.metadata_root_ids.clear();
        self.root_node_id = None;
        self.source_version = None;
        self.snapshot_chunks.clear();
        self.variable_catalog = None;
        self.used_resource_refs = None;
        self.variable_batches.clear();
        self.variables = None;
        self.large_values.clear();
        self.fast_snapshot_resources = None;
        self.fast_snapshot_has_large_values = false;
        self.fast_snapshot_rounds = 0;
        self.section_fallback_roots.clear();
        self.screen_failures.clear();
        self.asset_results.clear();
        self.retained_asset_bytes = 0;
        self.asset_byte_budget_exhausted = false;
        self.assets_scheduled =
            self.request.asset_selections.is_empty() && !self.request.web_ready_images;
        self.asset_plan_initialized = false;
        self.asset_plan.clear();
        self.reference_png = None;
        self.reference_png_scheduled = !self.request.reference_png;
        self.stats.transport = "legacy-cursor".to_owned();
        self.stats.fallback_used = true;
        self.stats.fallback_reason = Some(reason);
        self.stats.node_count = 0;
        self.stats.variable_count = 0;
        self.stats.style_count = 0;
        self.stats.raw_bytes = 0;
        self.stats.wire_bytes = 0;
        self.stats.envelope_chunks = 0;
    }
}