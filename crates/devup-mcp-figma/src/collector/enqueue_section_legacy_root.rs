impl CollectorSession {

    fn enqueue_section_legacy_root(&mut self, root_id: String) {
        if !self.section_fallback_roots.insert(root_id.clone()) {
            return;
        }
        self.enqueue(
            ReadToolCall::snapshot(
                &self.request.target.file_key,
                &root_id,
                BuiltinScript::NodeSnapshot,
            ),
            Some(root_id),
            CallKind::Snapshot,
        );
    }
}