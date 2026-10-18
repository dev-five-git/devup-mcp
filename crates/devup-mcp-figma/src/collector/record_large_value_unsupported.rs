impl CollectorSession {

    fn record_large_value_unsupported(
        &mut self,
        options: &LargeValueReadOptions,
        error_code: &str,
        error_detail: Option<&str>,
    ) -> Result<(), DevupError> {
        let key = (options.node_id.clone(), options.field.clone());
        let assembler = self
            .large_values
            .remove(&key)
            .ok_or_else(|| invalid_call("large value assembler is missing."))?;
        if let Some((request, _)) = self.chunked_assets.remove(&key) {
            self.record_asset_failure(
                request,
                if error_detail.is_some_and(|detail| detail.contains("DEVUP_ASSET_TIMEOUT:")) {
                    "DEVUP_ASSET_TIMEOUT"
                } else {
                    "DEVUP_ASSET_EXPORT_FAILED"
                },
                Some(format!(
                    "reading the export back in fragments failed: {}",
                    error_detail.unwrap_or(error_code)
                )),
            );
            return Ok(());
        }
        let descriptor = assembler.descriptor().clone();
        replace_descriptor(
            &mut self.snapshot_chunks,
            &descriptor,
            json!({
                "$truncated": "unsupported-by-upstream",
                "byteLength": descriptor.byte_length
            }),
        )?;
        if let Some(chunk) = self
            .snapshot_chunks
            .values_mut()
            .find(|chunk| chunk.nodes.iter().any(|node| node.id == descriptor.node_id))
            && let Some(node) = chunk
                .nodes
                .iter_mut()
                .find(|node| node.id == descriptor.node_id)
        {
            node.field_errors
                .insert(descriptor.field.clone(), error_code.to_owned());
            chunk.diagnostics.push(crate::Diagnostic {
                code: error_code.to_owned(),
                message:
                    "Figma upstream could not re-read the large field, so an explicit marker was kept."
                        .to_owned(),
                node_id: Some(descriptor.node_id),
                severity: Some(crate::DiagnosticSeverity::Warning),
                property: Some(descriptor.field),
                fallback: Some("unsupported-by-upstream".to_owned()),
                recoverable: Some(false),
                ..crate::Diagnostic::default()
            });
        }
        Ok(())
    }
}