impl CollectorSession {

    fn mark_section_legacy(&mut self, reason: String) {
        self.stats.transport = if self.fast_multi_resources.is_some() {
            "hybrid-multi-root-cursor"
        } else {
            "legacy-multi-root-cursor"
        }
        .to_owned();
        self.stats.fallback_used = true;
        self.stats.fallback_reason.get_or_insert(reason);
    }
}