impl CollectorSession {

    fn accept_snapshot(
        &mut self,
        planned: &PlannedCall,
        order: usize,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let mut chunk = snapshot_chunk_from_result(&result)?;
        if chunk.file_key != planned.expected_file_key {
            return Err(invalid_call(
                "Figma snapshot file key does not match the request.",
            ));
        }
        if chunk.version != self.source_version {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaVersionChanged,
                "The Figma file version changed during collection.",
                true,
            ));
        }
        let pagination = match &planned.call {
            ReadToolCall::Snapshot {
                script: BuiltinScript::NodeSnapshot,
                snapshot: Some(options),
                ..
            } => Some(options.clone()),
            _ => None,
        };
        if let Some(options) = pagination
            && let Some(cursor) = take_snapshot_cursor(&mut chunk)?
        {
            let expected_next = options
                .offset
                .checked_add(chunk.nodes.len())
                .ok_or_else(|| invalid_call("Figma snapshot cursor offset overflowed."))?;
            if cursor.next_offset != expected_next || cursor.next_offset > cursor.total_nodes {
                return Err(invalid_call(
                    "Figma snapshot cursor does not match the collected node range.",
                ));
            }
            if cursor.complete != (cursor.next_offset >= cursor.total_nodes) {
                return Err(invalid_call(
                    "Figma snapshot cursor completion state does not match the node count.",
                ));
            }
            if !cursor.complete {
                if chunk.nodes.is_empty() {
                    return Err(invalid_call("Figma snapshot cursor did not advance."));
                }
                let node_id = planned.expected_node_id.clone().ok_or_else(|| {
                    invalid_call("Figma snapshot cursor root node ID is missing.")
                })?;
                self.enqueue(
                    ReadToolCall::snapshot_chunk(
                        &self.request.target.file_key,
                        &node_id,
                        SnapshotReadOptions {
                            offset: cursor.next_offset,
                            ..options
                        },
                    ),
                    Some(node_id),
                    CallKind::Snapshot,
                );
            }
        }
        self.record_snapshot_chunk(order, chunk)?;
        Ok(())
    }
}