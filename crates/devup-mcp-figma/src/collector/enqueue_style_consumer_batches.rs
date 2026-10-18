impl CollectorSession {

    fn enqueue_style_consumer_batches(
        &mut self,
        batch: &VariableBatchResult,
    ) -> Result<(), DevupError> {
        let node_id = self.root_node_id.clone().ok_or_else(|| {
            invalid_call("No root node ID available for Figma style consumer collection.")
        })?;
        for style in &batch.styles {
            let Some(object) = style.as_object() else {
                return Err(invalid_call("Figma style batch format is invalid."));
            };
            let Some(consumer_count) = object.get("$consumerCount").and_then(Value::as_u64) else {
                continue;
            };
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_call("Figma style ID is missing."))?;
            let style_type = object
                .get("styleType")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_call("Figma style type is missing."))?;
            for start in (0..consumer_count as usize).step_by(STYLE_CONSUMER_BATCH_SIZE) {
                let end = (start + STYLE_CONSUMER_BATCH_SIZE).min(consumer_count as usize);
                self.enqueue(
                    ReadToolCall::resource_batch(
                        &self.request.target.file_key,
                        &node_id,
                        ResourceBatch {
                            variable_ids: Vec::new(),
                            styles: vec![ResourceStyleRef {
                                id: id.to_owned(),
                                style_type: style_type.to_owned(),
                                consumer_start: Some(start),
                                consumer_end: Some(end),
                            }],
                        },
                    ),
                    Some(node_id.clone()),
                    CallKind::VariableBatch,
                );
            }
        }
        Ok(())
    }
}