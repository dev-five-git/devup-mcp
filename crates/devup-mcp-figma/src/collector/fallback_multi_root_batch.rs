impl CollectorSession {

    fn fallback_multi_root_batch(
        &mut self,
        planned: &PlannedCall,
        reason: String,
    ) -> Result<(), DevupError> {
        let ReadToolCall::Snapshot {
            script: BuiltinScript::MultiRootSnapshotEnvelope,
            root_ids: Some(root_ids),
            ..
        } = &planned.call
        else {
            return Err(invalid_call("multi-root fallback call format is invalid."));
        };
        if root_ids.is_empty() {
            return Err(invalid_call("multi-root fallback has no selected roots."));
        }
        self.mark_section_legacy(reason);
        for root_id in root_ids {
            self.enqueue_section_legacy_root(root_id.clone());
        }
        Ok(())
    }
}