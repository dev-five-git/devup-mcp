impl CollectorSession {

    fn accept_fast_multi_root(
        &mut self,
        planned: &PlannedCall,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let ReadToolCall::Snapshot {
            script: BuiltinScript::MultiRootSnapshotEnvelope,
            root_ids: Some(root_ids),
            snapshot: options,
            ..
        } = &planned.call
        else {
            return Err(invalid_call("multi-root snapshot call format is invalid."));
        };
        let requested_offset = options.as_ref().map_or(0, |options| options.offset);
        let payload = match decode_fast_multi_snapshot(&result, &self.request.target, root_ids) {
            Ok(payload) => payload,
            Err(error) if requested_offset == 0 && fast_call_fallback_allowed(&error) => {
                self.fallback_multi_root_batch(planned, fallback_category(&error))?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let mut chunk = payload.snapshot;
        let cursor = take_snapshot_cursor(&mut chunk)?;
        if let Some(cursor) = &cursor {
            if cursor.offset != requested_offset
                || requested_offset.checked_add(chunk.nodes.len()) != Some(cursor.next_offset)
                || cursor.next_offset > cursor.total_nodes
                || cursor.complete != (cursor.next_offset == cursor.total_nodes)
                || (!cursor.complete && cursor.next_offset <= requested_offset)
            {
                return Err(invalid_call(
                    "Figma multi-root snapshot cursor does not match the requested node range.",
                ));
            }
        } else if requested_offset != 0 {
            return Err(invalid_call(
                "Figma multi-root continuation is missing its snapshot cursor.",
            ));
        }
        if let (Some(existing), Some(incoming)) = (&self.source_version, &chunk.version)
            && existing != incoming
        {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaVersionChanged,
                "The Figma file version changed during multi-root collection.",
                true,
            ));
        }
        if chunk.version.is_some() {
            self.source_version = chunk.version.clone();
        }
        self.fast_multi_has_large_values |= !descriptors_in_chunk(&chunk)?.is_empty();
        merge_fast_resources(&mut self.fast_multi_resources, payload.resources)?;
        self.stats.transport = if self.section_fallback_roots.is_empty() {
            if requested_offset > 0
                || self.stats.transport == "text-paginated"
                || cursor.as_ref().is_some_and(|cursor| !cursor.complete)
            {
                "text-paginated"
            } else {
                payload.stats.transport
            }
        } else {
            "hybrid-multi-root-cursor"
        }
        .to_owned();
        self.stats.raw_bytes = self.stats.raw_bytes.saturating_add(payload.stats.raw_bytes);
        self.stats.wire_bytes = self
            .stats
            .wire_bytes
            .saturating_add(payload.stats.wire_bytes);
        self.stats.envelope_chunks = self
            .stats
            .envelope_chunks
            .saturating_add(payload.stats.chunk_count);
        self.record_snapshot_chunk(order, chunk)?;
        if let Some(cursor) = cursor.filter(|cursor| !cursor.complete) {
            // Keep this Section and this exact root set: another sibling may
            // have a different cursor in flight. Never cache a first page as
            // a complete design while its vector operands remain unread.
            let mut continuation = planned.call.clone();
            if let ReadToolCall::Snapshot { snapshot, .. } = &mut continuation {
                let mut options = options.clone().unwrap_or_default();
                options.offset = cursor.next_offset;
                *snapshot = Some(options);
            }
            self.enqueue(
                continuation,
                planned.expected_node_id.clone(),
                CallKind::FastMultiRoot,
            );
        }
        Ok(())
    }
}