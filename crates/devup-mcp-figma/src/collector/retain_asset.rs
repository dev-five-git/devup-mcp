impl CollectorSession {

    fn retain_asset(&mut self, mut asset: AssetManifestEntry) {
        if asset.status == AssetStatus::Exported {
            let bytes = asset
                .byte_length
                .unwrap_or_default()
                .saturating_add(asset.variants.iter().map(|v| v.byte_length).sum::<usize>());
            if self.retained_asset_bytes.saturating_add(bytes) > MAX_COLLECTED_ASSET_BYTES {
                self.asset_byte_budget_exhausted = true;
                asset.status = AssetStatus::Failed;
                asset.error_code = Some("DEVUP_ASSET_TOTAL_BYTES_EXCEEDED".into());
                asset.error_detail = Some("The 128 MiB retained asset budget was reached. Export failed IDs in smaller custom batches or select a smaller frame.".into());
                asset.data_base64 = None;
                asset.variants.clear();
                asset.byte_length = None;
                asset.sha256 = None;
                asset.mime_type = None;
                if let Some(info) = &mut asset.render_info {
                    info["representation"] = json!("not-exported");
                    info["renderedAs"] = json!("not-exported");
                    info["byteEvidence"] = Value::Null;
                    info["fidelity"] = json!("unverified");
                    info["effectsBakedIntoAsset"] = json!(false);
                }
                if let Some(diagnostics) = &mut asset.export_diagnostics {
                    diagnostics.render_info = asset.render_info.clone();
                }
            } else {
                self.retained_asset_bytes += bytes;
            }
        }
        self.asset_results.push(asset);
    }
}