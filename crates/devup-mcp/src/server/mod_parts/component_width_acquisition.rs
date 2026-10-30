impl DevupServer {
    async fn plan_component_widths(
        &self,
        input: &screen_plan::ScreenPlanInput,
        screens: &[screen_plan::PlanScreenInput],
        source: &mut Value,
    ) -> Result<(), DevupError> {
        if !input.screens.is_empty()
            && self.services.upstream.bridge_path_snapshot().await.is_none_or(|bridge| bridge.attached_files.len() != 1) {
            source["componentWidthEvidence"] = json!({"state":"unavailable","reason":"Caller screen summaries have no component identity; no unique Bridge file is attached."});
            return Ok(());
        }
        let target = self.bind_to_bridge(parse_link(input.url.as_deref())?, Unnamed::Selection,
            &json!({"tool":"devup_screen_plan","arguments":{"action":"draft"}})).await?;
        let mut snapshot = None;
        let mut payloads = Vec::new();
        for screen in screens.iter().filter(|screen| screen.visible != Some(false)) {
            let mut target = target.clone();
            target.node_id = Some(screen.node_id.clone());
            let request = CollectionRequest::new(target, CollectionScope::Node);
            let key = ArtifactRequestKey::from_collection(&request);
            let payload = CollectedPayload::try_from(self.run_direct(request, &PendingOperation::Collect, None).await?)?;
            let artifact = self.artifacts.insert(key, payload).await?;
            match &mut snapshot {
                None => snapshot = Some(artifact.payload.snapshot.clone()),
                Some(combined) => combined.nodes.extend(artifact.payload.snapshot.nodes.clone()),
            }
            payloads.push(artifact.artifact_id);
        }
        let Some(snapshot) = snapshot else { return Ok(()) };
        let roots = screens.iter().map(|screen| screen.node_id.clone()).collect::<Vec<_>>();
        let mut mismatches = screen_plan::component_width_mismatches(&snapshot, &roots);
        for mismatch in &mut mismatches {
            let mut references = Vec::new();
            let mut render_bytes = Vec::new();
            for field in ["nodeId", "correctNodeId"] {
                let Some(id) = mismatch[field].as_str() else { continue };
                let mut target = target.clone();
                target.node_id = Some(id.into());
                let mut request = CollectionRequest::new(target, CollectionScope::Node);
                request.asset_selections.push(devup_mcp_figma::AssetSelection {
                    asset_id: format!("{id}:node"), format: devup_mcp_figma::AssetFormat::Png,
                    scale: 1.0, render_mode: devup_mcp_figma::AssetRenderMode::Node,
                    usage_aspect_ratio: None, max_width: None, srcset: false,
                    visible_scope_node_id: None, quality: None, lossless: None,
                });
                let key = ArtifactRequestKey::from_collection(&request);
                match self.run_direct(request, &PendingOperation::Collect, None).await.and_then(CollectedPayload::try_from) {
                    Ok(payload) => {
                        let artifact = self.artifacts.insert(key, payload).await?;
                        for asset in &artifact.payload.assets {
                            if asset.node_id == id
                                && asset.status == devup_mcp_figma::AssetStatus::Exported
                                && asset.format == Some(devup_mcp_figma::AssetFormat::Png)
                                && asset.scale == Some(1.0)
                                && asset.render_info.as_ref().is_some_and(|info|info["representation"] == "native-node") {
                                references.push(json!({"nodeId":id,"artifactId":artifact.artifact_id,"assetId":asset.asset_id,"renderMode":"node","format":"png","scale":1}));
                                if let Some(encoded) = &asset.data_base64 {
                                    use base64::Engine as _;
                                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                                        render_bytes.push(bytes);
                                    }
                                }
                            }
                        }
                        payloads.push(artifact.artifact_id);
                    }
                    Err(error) => mismatch["renderError"] = json!(error.to_string()),
                }
            }
            mismatch["render"] = json!(if render_bytes.len() == 2 { "captured" } else { "unavailable" });
            mismatch["renderReferences"] = json!(references);
            mismatch["comparison"] = json!({"method":"drawn-width-majority","pixelComparison":"unverified","decision":"use expected master as one responsive component"});
            if let [suspect, correct] = render_bytes.as_slice() {
                match devup_mcp_visual::compare_png_bytes(correct, suspect, &devup_mcp_visual::CompareOptions::default(), false) {
                    Ok((report, _)) => {
                        let comparable = report.status != devup_mcp_visual::VisualStatus::InvalidDimensions;
                        mismatch["comparison"]["pixelComparison"] = serde_json::to_value(report).map_err(|error|DevupError::new(ErrorCode::DevupInvalidInput,error.to_string(),false))?;
                        mismatch["comparison"]["pixelVerdict"] = json!(if comparable { "compared" } else { "inconclusive" });
                        if !comparable {
                            mismatch["comparison"]["pixelReason"] = json!("the two renders have different dimensions, so no pixels were compared");
                        }
                    }
                    Err(error) => mismatch["comparison"]["renderError"] = json!(error.to_string()),
                }
            }
        }
        source["componentWidthArtifacts"] = json!(payloads);
        source["componentWidthMismatches"] = json!(mismatches);
        Ok(())
    }
}
