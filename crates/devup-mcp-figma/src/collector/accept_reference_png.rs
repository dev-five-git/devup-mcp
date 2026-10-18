impl CollectorSession {

    fn accept_reference_png(
        &mut self,
        planned: &PlannedCall,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let ReadToolCall::Screenshot { file_key, node_id } = &planned.call else {
            return Err(invalid_call("reference PNG call format is invalid."));
        };
        if file_key != &planned.expected_file_key
            || planned.expected_node_id.as_deref() != Some(node_id.as_str())
        {
            return Err(invalid_call(
                "reference PNG call Figma target does not match the request.",
            ));
        }
        let note = result.raw["_meta"][crate::bridge::REFERENCE_PNG_NOTE_META_KEY]
            .as_str()
            .map(str::to_owned);
        let data_base64 = take_single_png_data(result.raw)?;
        if data_base64.len() > MAX_REFERENCE_PNG_BASE64_BYTES {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "Figma reference PNG exceeds the allowed size.",
                false,
            ));
        }
        let bytes = STANDARD
            .decode(data_base64.as_bytes())
            .map_err(|_| invalid_call("Figma reference PNG base64 is invalid."))?;
        if bytes.is_empty() || bytes.len() > MAX_REFERENCE_PNG_BYTES {
            return Err(invalid_call(
                "Figma reference PNG format or size is invalid.",
            ));
        }
        validate_reference_png(&bytes)?;
        self.reference_png = Some(ReferencePng {
            mime_type: "image/png".to_owned(),
            data_base64,
            byte_length: bytes.len(),
            sha256: Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            note,
        });
        Ok(())
    }
}