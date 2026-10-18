impl CollectorSession {

    fn accept_asset(
        &mut self,
        planned: &PlannedCall,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let ReadToolCall::AssetExport {
            version, request, ..
        } = &planned.call
        else {
            return Err(invalid_call("asset call format is invalid."));
        };
        if self.asset_byte_budget_exhausted {
            self.record_asset_failure(request.as_ref().clone(), "DEVUP_ASSET_TOTAL_BYTES_EXCEEDED", Some("The retained asset byte budget was reached; export this ID in a smaller batch.".into()));
            return Ok(());
        }
        let outcome = asset_export_from_result(
            &result,
            &planned.expected_file_key,
            version.as_deref(),
            request,
        );
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) if request.format == crate::AssetFormat::Webp => {
                self.record_asset_failure(
                    request.as_ref().clone(),
                    "DEVUP_ASSET_WEBP_POSTPROCESS_FAILED",
                    Some(error.message),
                );
                return Ok(());
            }
            Err(error) if request.render_mode == crate::AssetRenderMode::OriginalSource => {
                self.record_asset_failure(
                    request.as_ref().clone(),
                    "DEVUP_ASSET_SOURCE_POSTPROCESS_FAILED",
                    Some(error.message),
                );
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let exported = match outcome {
            AssetExportOutcome::Entry(entry) => *entry,
            AssetExportOutcome::Chunked(descriptor) => {
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
                    key.clone(),
                    LargeValueAssembler::new(
                        self.request.target.file_key.clone(),
                        version.clone(),
                        descriptor,
                    )?,
                );
                self.chunked_assets.insert(
                    key,
                    (
                        request.as_ref().clone(),
                        crate::assets::asset_export_diagnostics(&result),
                    ),
                );
                self.enqueue(
                    ReadToolCall::large_value(&self.request.target.file_key, options.clone()),
                    Some(options.node_id),
                    CallKind::LargeValue,
                );
                return Ok(());
            }
        };
        if exported.status == AssetStatus::Failed {
            let error_code = exported
                .error_code
                .clone()
                .unwrap_or_else(|| "DEVUP_ASSET_EXPORT_FAILED".to_owned());
            self.record_asset_diagnostic(request, &error_code, exported.error_detail.as_deref());
        }
        self.retain_asset(exported);
        Ok(())
    }
}