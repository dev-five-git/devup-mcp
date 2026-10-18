impl CollectorSession {

    fn record_asset_failure(
        &mut self,
        request: AssetRequest,
        error_code: &str,
        error_detail: Option<String>,
    ) {
        let error_detail = error_detail.map(|detail| detail.chars().take(500).collect::<String>());
        self.record_asset_diagnostic(&request, error_code, error_detail.as_deref());
        let source_kind = crate::assets::source_kind_of(&request).to_owned();
        self.asset_results.push(AssetManifestEntry {
            usage_aspect_ratio: request.usage_aspect_ratio,
            max_width: request.max_width,
            srcset: request.srcset,
            visible_scope_node_id: request.visible_scope_node_id.clone(),
            variants: Vec::new(),
            render_mode: request.render_mode,
            quality: request.quality,
            lossless: request.lossless,
            render_info: None,
            suggested_filename: None,
            asset_id: request.asset_id,
            node_id: request.node_id,
            field: request.field,
            source_kind,
            image_hash: request.image_hash,
            format: Some(request.format),
            scale: Some(request.scale),
            status: AssetStatus::Failed,
            byte_length: None,
            sha256: None,
            mime_type: None,
            data_base64: None,
            output_path: None,
            error_code: Some(error_code.to_owned()),
            error_detail,
            export_diagnostics: None,
            path: None,
        });
    }
}