impl CollectorSession {

    fn record_snapshot_chunk(
        &mut self,
        order: usize,
        chunk: SnapshotChunk,
    ) -> Result<(), DevupError> {
        let descriptors = descriptors_in_chunk(&chunk)?;
        let version = chunk.version.clone();
        self.snapshot_chunks.insert(order, chunk);
        for descriptor in descriptors {
            let key = (descriptor.node_id.clone(), descriptor.field.clone());
            if self.large_values.contains_key(&key) {
                return Err(invalid_call(
                    "Duplicate Figma large value descriptor for the same field.",
                ));
            }
            let options = LargeValueReadOptions::from_descriptor(
                &descriptor,
                version.clone(),
                descriptor.cursor.next_offset,
            );
            self.large_values.insert(
                key,
                LargeValueAssembler::new(
                    self.request.target.file_key.clone(),
                    version.clone(),
                    descriptor,
                )?,
            );
            self.enqueue(
                ReadToolCall::large_value(&self.request.target.file_key, options.clone()),
                Some(options.node_id),
                CallKind::LargeValue,
            );
        }
        Ok(())
    }
}