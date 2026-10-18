impl CollectorSession {

    /// Validated per-asset acquisition results retained before final payload assembly.
    /// Binary data is deliberately not part of progress responses.
    pub fn asset_progress(&self) -> Vec<Value> {
        self.request.asset_selections.iter().map(|selection| {
            match self.asset_results.iter().find(|asset| asset.asset_id == selection.asset_id) {
                Some(asset) => serde_json::json!({"assetId":asset.asset_id,"status":asset.status,"format":asset.format,"scale":asset.scale,"quality":asset.quality,"lossless":asset.lossless,"usageAspectRatio":asset.usage_aspect_ratio,"maxWidth":asset.max_width,"srcset":asset.srcset,"visibleScopeNodeId":asset.visible_scope_node_id,
                    "byteLength":asset.byte_length,"sha256":asset.sha256,"errorCode":asset.error_code,"errorDetail":asset.error_detail,"exportDiagnostics":asset.export_diagnostics,"renderMode":asset.render_mode,"renderInfo":asset.render_info}),
                None => serde_json::json!({"assetId":selection.asset_id,"renderMode":selection.render_mode,"format":selection.format,"scale":selection.scale,"quality":selection.quality,"lossless":selection.lossless,"usageAspectRatio":selection.usage_aspect_ratio,"maxWidth":selection.max_width,"srcset":selection.srcset,"visibleScopeNodeId":selection.visible_scope_node_id,"status":"pending"}),
            }
        }).collect()
    }
}