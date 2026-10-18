impl CollectorSession {

    fn fast_path_eligible(&self) -> bool {
        self.request.scope == CollectionScope::Node
            && self.request.resource_scope == ResourceScope::Used
            && self.request.target.node_id.is_some()
            && !self.request.include_context
            && !self.request.metadata_only
            && !self.request.variables_only
            && self.request.search.is_none()
            && self.request.explore.is_none()
            && self.request.section.is_none()
    }
}