impl CollectorSession {

    fn accept_large_value(
        &mut self,
        planned: &PlannedCall,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let ReadToolCall::LargeValue { options, .. } = &planned.call else {
            return Err(invalid_call("large value call format is invalid."));
        };
        let result = large_value_from_result(&result)?;
        if let LargeValueResult::Unsupported(unsupported) = result {
            if unsupported.file_key != planned.expected_file_key
                || unsupported.version != options.version
                || unsupported.node_id != options.node_id
                || unsupported.field != options.field
                || unsupported.byte_length != options.byte_length
                || unsupported.sha256 != options.sha256
                || unsupported.error_code != "DEVUP_FIELD_UNSUPPORTED_BY_UPSTREAM"
            {
                return Err(invalid_call(
                    "large value unsupported response does not match the request.",
                ));
            }
            return self.record_large_value_unsupported(
                options,
                &unsupported.error_code,
                unsupported.error_detail.as_deref(),
            );
        }
        let LargeValueResult::Fragment(fragment) = result else {
            unreachable!()
        };
        if fragment.offset != options.offset {
            return Err(invalid_call(
                "large value fragment offset does not match the request.",
            ));
        }
        let key = (options.node_id.clone(), options.field.clone());
        let next_offset = fragment.next_offset;
        let complete = fragment.complete;
        let assembler = self
            .large_values
            .get_mut(&key)
            .ok_or_else(|| invalid_call("large value assembler is missing."))?;
        assembler.push(fragment)?;
        if complete {
            let assembler = self
                .large_values
                .remove(&key)
                .ok_or_else(|| invalid_call("large value assembler is missing."))?;
            if let Some((request, diagnostics)) = self.chunked_assets.remove(&key) {
                // The fragments were an SVG or PNG export, not a field: the
                // bytes are the asset, and go where a one-answer export would.
                let bytes = match assembler.finish_bytes() {
                    Ok(bytes) => bytes,
                    Err(error) if request.format == crate::AssetFormat::Webp => {
                        self.record_asset_failure(
                            request,
                            "DEVUP_ASSET_WEBP_POSTPROCESS_FAILED",
                            Some(error.message),
                        );
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                };
                let mut asset = match exported_asset_from_bytes(&request, &bytes) {
                    Ok(asset) => asset,
                    Err(error) if request.format == crate::AssetFormat::Webp => {
                        self.record_asset_failure(
                            request,
                            "DEVUP_ASSET_WEBP_POSTPROCESS_FAILED",
                            Some(error.message),
                        );
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                };
                if let Some(mut observed) = diagnostics.as_ref().and_then(|d| d.render_info.clone())
                {
                    if request.format == crate::AssetFormat::Webp {
                        // Preserve postprocessing evidence and measure the final bytes,
                        // not the PNG fragments from which they were encoded.
                        if let Some(info) = &asset.render_info {
                            for key in ["representation", "compression", "transcoded"] {
                                observed[key] = info[key].clone();
                            }
                        }
                    }
                    let output = asset
                        .data_base64
                        .as_deref()
                        .and_then(|s| STANDARD.decode(s).ok());
                    asset.render_info = Some(crate::asset_render_info::measured_info(
                        request.render_mode,
                        request.format,
                        request.scale,
                        Some(observed),
                        output.as_deref(),
                    ));
                }
                asset.export_diagnostics = diagnostics;
                if let Some(diagnostics) = &mut asset.export_diagnostics {
                    diagnostics.render_info = asset.render_info.clone();
                }
                self.retain_asset(asset);
                return Ok(());
            }
            let descriptor = assembler.descriptor().clone();
            let value = assembler.finish()?;
            replace_descriptor(&mut self.snapshot_chunks, &descriptor, value)?;
        } else {
            if next_offset <= options.offset {
                return Err(invalid_call("large value cursor did not advance."));
            }
            let descriptor = assembler.descriptor().clone();
            let next = LargeValueReadOptions::from_descriptor(
                &descriptor,
                options.version.clone(),
                next_offset,
            );
            self.enqueue(
                ReadToolCall::large_value(&self.request.target.file_key, next.clone()),
                Some(next.node_id),
                CallKind::LargeValue,
            );
        }
        Ok(())
    }
}