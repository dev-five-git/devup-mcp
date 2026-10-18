impl CollectorSession {

    fn accept_fast_snapshot(
        &mut self,
        planned: &PlannedCall,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let payload = match decode_fast_snapshot(&result, &self.request.target) {
            Ok(payload) => payload,
            Err(error) => {
                if let Ok(chunk) = snapshot_chunk_from_result(&result)
                    && chunk.root_ids.as_slice()
                        == [self.request.target.node_id.as_deref().unwrap_or_default()]
                    && chunk.nodes.iter().any(|node| {
                        node.id == self.request.target.node_id.as_deref().unwrap_or_default()
                            && node.node_type == "SECTION"
                            && node.typed_view().value("projectionTruncated").is_some()
                    })
                {
                    self.request.section = Some(SectionReadOptions {
                        frame_ids: Vec::new(),
                        all_screens: false,
                    });
                    return self.accept_section_index(planned, order, result);
                }
                self.restart_legacy(fallback_category(&error));
                return Ok(());
            }
        };
        let root_id = self
            .request
            .target
            .node_id
            .clone()
            .ok_or_else(|| invalid_call("Figma fast snapshot requires a node ID."))?;
        self.root_node_id = Some(root_id.clone());
        self.source_version = payload.snapshot.version.clone();
        self.metadata_root_ids = payload.snapshot.root_ids.clone();
        self.fast_snapshot_rounds = self.fast_snapshot_rounds.saturating_add(1);
        self.stats.raw_bytes = self.stats.raw_bytes.saturating_add(payload.stats.raw_bytes);
        self.stats.wire_bytes = self
            .stats
            .wire_bytes
            .saturating_add(payload.stats.wire_bytes);
        let has_large_values = !descriptors_in_chunk(&payload.snapshot)?.is_empty();
        if has_large_values {
            self.fast_snapshot_has_large_values = true;
        } else {
            merge_fast_resources(&mut self.fast_snapshot_resources, payload.resources)?;
        }

        let mut chunk = payload.snapshot;
        // The script always appends a `__DEVUP_SNAPSHOT_CURSOR__` marker node
        // (same convention as the legacy cursor snapshot) reporting whether
        // more pages remain; `take_snapshot_cursor` strips it and returns
        // that state. A missing marker (only possible for hand-built,
        // pre-pagination-shaped payloads) is treated as a single complete
        // page. `record_snapshot_chunk` then stores this page's real nodes
        // and enqueues any large-value follow-ups they declared.
        let total_nodes = chunk.nodes.len();
        let cursor = take_snapshot_cursor(&mut chunk)?.unwrap_or(SnapshotCursor {
            offset: 0,
            next_offset: total_nodes,
            complete: true,
            total_nodes,
        });
        self.record_snapshot_chunk(order, chunk)?;

        if cursor.complete {
            self.stats.transport = if self.fast_snapshot_rounds > 1 {
                "text-paginated"
            } else {
                "text"
            }
            .to_owned();
            self.stats.envelope_chunks = 0;
            self.variables = (!self.fast_snapshot_has_large_values)
                .then(|| self.fast_snapshot_resources.take())
                .flatten();
            self.metadata = Some(json!({
                "transport": &self.stats.transport,
                "rootId": root_id,
                "nodeCount": cursor.total_nodes,
                "pageCount": self.fast_snapshot_rounds
            }));
        } else {
            self.enqueue(
                ReadToolCall::fast_snapshot_page(
                    &self.request.target.file_key,
                    &root_id,
                    SnapshotReadOptions {
                        offset: cursor.next_offset,
                        ..SnapshotReadOptions::default()
                    },
                ),
                Some(root_id),
                CallKind::FastSnapshot,
            );
        }
        Ok(())
    }
}