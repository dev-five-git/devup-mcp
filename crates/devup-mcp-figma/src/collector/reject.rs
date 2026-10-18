impl CollectorSession {

    pub fn reject(&mut self, call_id: &str, error: &DevupError) -> Result<bool, DevupError> {
        let Some(pending) = self.pending.get(call_id) else {
            return Err(invalid_call("Unknown or already-handled Figma call ID."));
        };
        if pending.kind == CallKind::FastSnapshot && is_section_target_error(error) {
            let pending = self
                .pending
                .remove(call_id)
                .ok_or_else(|| invalid_call("fast Section probe call is missing."))?;
            self.consumed.insert(call_id.to_owned());
            let node_id = pending
                .planned
                .expected_node_id
                .ok_or_else(|| invalid_call("fast Section probe node ID is missing."))?;
            self.request.section = Some(SectionReadOptions {
                frame_ids: Vec::new(),
                all_screens: false,
            });
            self.enqueue(
                ReadToolCall::section_index(&self.request.target.file_key, &node_id),
                Some(node_id),
                CallKind::SectionIndex,
            );
            return Ok(true);
        }
        if pending.kind == CallKind::Asset {
            let pending = self
                .pending
                .remove(call_id)
                .ok_or_else(|| invalid_call("asset call is missing."))?;
            self.consumed.insert(call_id.to_owned());
            let ReadToolCall::AssetExport { request, .. } = pending.planned.call else {
                return Err(invalid_call("asset call format is invalid."));
            };
            self.record_asset_failure(
                *request,
                if error.message.contains("DEVUP_ASSET_TIMEOUT:")
                    || error.details["reason"] == "timeout"
                {
                    "DEVUP_ASSET_TIMEOUT"
                } else {
                    "DEVUP_ASSET_EXPORT_FAILED"
                },
                Some(error.message.clone()),
            );
            return Ok(true);
        }
        if pending.kind == CallKind::LargeValue {
            let pending = self
                .pending
                .remove(call_id)
                .ok_or_else(|| invalid_call("large value call is missing."))?;
            self.consumed.insert(call_id.to_owned());
            let ReadToolCall::LargeValue { options, .. } = pending.planned.call else {
                return Err(invalid_call("large value call format is invalid."));
            };
            self.record_large_value_unsupported(
                &options,
                "DEVUP_FIELD_UNSUPPORTED_BY_UPSTREAM",
                Some(&error.message),
            )?;
            return Ok(true);
        }
        if pending.kind == CallKind::Snapshot
            && pending
                .planned
                .expected_node_id
                .as_ref()
                .is_some_and(|node_id| self.section_fallback_roots.contains(node_id))
        {
            let pending = self
                .pending
                .remove(call_id)
                .ok_or_else(|| invalid_call("Section legacy call is missing."))?;
            self.consumed.insert(call_id.to_owned());
            let node_id = pending
                .planned
                .expected_node_id
                .ok_or_else(|| invalid_call("Section legacy call node ID is missing."))?;
            self.section_fallback_roots.remove(&node_id);
            self.screen_failures.push(ScreenFailure {
                node_id,
                error_code: error.code,
                message: error.message.clone(),
                retryable: error.retryable,
            });
            return Ok(true);
        }
        if !matches!(
            pending.kind,
            CallKind::FastSnapshot | CallKind::FastTheme | CallKind::FastMultiRoot
        ) {
            return Ok(false);
        }
        if !fast_call_fallback_allowed(error) {
            return Ok(false);
        }
        // A continuation already has accepted pages and possibly field reads
        // in flight. Restarting at zero would mix them with a second capture.
        if pending.kind == CallKind::FastMultiRoot
            && matches!(&pending.planned.call, ReadToolCall::Snapshot {
                snapshot: Some(options), ..
            } if options.offset > 0)
        {
            return Ok(false);
        }
        let pending = self
            .pending
            .remove(call_id)
            .ok_or_else(|| invalid_call("fast call is missing."))?;
        self.consumed.insert(call_id.to_owned());
        if pending.kind == CallKind::FastMultiRoot {
            self.fallback_multi_root_batch(&pending.planned, fallback_category(error))?;
            return Ok(true);
        }
        self.restart_legacy(fallback_category(error));
        Ok(true)
    }
}