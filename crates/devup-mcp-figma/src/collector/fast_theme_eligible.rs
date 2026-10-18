impl CollectorSession {

    fn fast_theme_eligible(&self) -> bool {
        self.request.scope == CollectionScope::File
            && self.request.resource_scope == ResourceScope::File
            && self.request.variables_only
            && !self.request.metadata_only
            && !self.request.include_context
            && self.request.search.is_none()
            && self.request.explore.is_none()
            && self.request.section.is_none()
    }
}