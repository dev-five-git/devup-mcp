async fn project_operation(
    mut operation: PendingOperation,
    payload: &CollectedPayload,
    source_kind: &str,
    artifact: &ArtifactLookup,
    output_policy: &OutputPolicy,
    artifact_store: &ArtifactStore,
) -> Result<Value, DevupError> {
    let collection = if artifact.cache_hit {
        CollectionStats::default()
    } else {
        payload.stats.clone()
    };
    let completeness_report = payload.completeness_report();
    if source_kind == "artifact"
        && let PendingOperation::Export {
            frame_ids,
            all_screens,
            ..
        } = &mut operation
        && frame_ids.is_empty()
        && !*all_screens
        && let Some(saved) = &artifact.capabilities.section_selection
    {
        frame_ids.clone_from(&saved.frame_ids);
        *all_screens = saved.all_screens;
    }
    match operation {
        PendingOperation::Search {
            query,
            node_types,
            match_kind,
            limit,
        } => {
            let matches = search_snapshot(
                &payload.snapshot,
                &payload.target,
                &SearchOptions {
                    query: query.clone(),
                    node_types,
                    match_kind,
                    limit,
                },
            )?;
            let quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, true),
                projection: projection_quality(false, &[]),
                theme: theme_quality(false, 0, 0),
                assets: assets_quality(false, &[], &[]),
            };
            let mut response = json!({
                "status": quality.status(),
                "quality": quality,
                "query": query,
                "count": matches.len(),
                "matches": matches,
                "completeness": payload.completeness,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "version": payload.snapshot.version
                }
            });
            attach_completeness_report(&mut response, quality, &completeness_report, false);
            Ok(response)
        }
        PendingOperation::Explore { limit, target } => {
            let result = explore_snapshot(&payload.snapshot, &target, &ExploreOptions { limit })?;
            let count = result.candidates.len();
            let quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, true),
                projection: projection_quality(false, &[]),
                theme: theme_quality(false, 0, 0),
                assets: assets_quality(false, &[], &[]),
            };
            let mut response = json!({
                "status": quality.status(),
                "quality": quality,
                "targetKind": result.target_kind,
                "anchor": result.anchor,
                "group": result.group,
                "count": count,
                "candidates": result.candidates,
                "truncated": result.truncated,
                "diagnostics": payload.snapshot.diagnostics,
                "completeness": payload.completeness,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": target.file_key,
                    "nodeId": target.node_id,
                    "version": payload.snapshot.version
                }
            });
            attach_completeness_report(&mut response, quality, &completeness_report, false);
            Ok(response)
        }
        PendingOperation::Export {
            outputs,
            previous_design_fingerprints,
            component_name,
            include_diagnostics,
            root_layout,
            asset_names_per_node,
            scope,
            theme_options,
            strict,
            output_paths,
            page_scaffold,
            frame_ids,
            all_screens,
            mut asset_captures,
            web_ready_images,
            mut asset_output_paths,
            asset_public_root,
            fields,
            response_selection,
            delivery,
            width_policy,
        } => {
            // Source selection can turn a nodeId target into its IMAGE fill.
            // Keep requested file destinations and capture reporting attached
            // to the resolved asset rather than the convenience node alias.
            for capture in &mut asset_captures {
                if capture.format == devup_mcp_figma::AssetFormat::Auto {
                    let resolved = devup_mcp_figma::resolve_asset_selections(&payload.snapshot, std::slice::from_ref(capture))?.remove(0);
                    capture.format = resolved.format;
                    capture.quality = resolved.quality;
                    capture.render_mode = resolved.render_mode;
                    if let Some(path) = asset_output_paths.get_mut(&capture.asset_id) {
                        *path = std::path::Path::new(path).with_extension(resolved.format.extension()).to_string_lossy().into_owned();
                    }
                }
                if !matches!(capture.render_mode, devup_mcp_figma::AssetRenderMode::OriginalSource | devup_mcp_figma::AssetRenderMode::Auto) {
                    continue;
                }
                let Some(asset) = payload.assets.iter().find(|asset| {
                    asset.render_mode.is_original() && asset.field.starts_with("fills/")
                        && (asset.asset_id == capture.asset_id
                            || capture.asset_id == format!("{}:node", asset.node_id))
                }) else { continue };
                if capture.asset_id != asset.asset_id {
                    if let Some(path) = asset_output_paths.remove(&capture.asset_id) {
                        asset_output_paths.insert(asset.asset_id.clone(), path);
                    }
                    capture.asset_id = asset.asset_id.clone();
                }
                capture.render_mode = asset.render_mode;
            }
            if web_ready_images {
                asset_captures = payload
                    .assets
                    .iter()
                    .filter_map(|a| {
                        Some(devup_mcp_figma::AssetSelection {
                            usage_aspect_ratio: a.usage_aspect_ratio,
                            max_width: a.max_width,
                            srcset: a.srcset,
                            visible_scope_node_id: a.visible_scope_node_id.clone(),
                            asset_id: a.asset_id.clone(),
                            format: a.format?,
                            scale: a.scale?,
                            render_mode: a.render_mode,
                            quality: a.quality,
                            lossless: a.lossless,
                        })
                    })
                    .collect();
                if let Some(root) = &asset_public_root {
                    for capture in &asset_captures {
                        let name = format!("{}.webp", sha256_hex(capture.asset_id.as_bytes()));
                        asset_output_paths.insert(
                            capture.asset_id.clone(),
                            root.join("images")
                                .join("web-ready")
                                .join(name)
                                .display()
                                .to_string(),
                        );
                    }
                }
            }
            let asset_policies =
                asset_policy::policies(&payload.snapshot, &payload.assets, asset_names_per_node);
            let selected_fields = super::field_select::expand(&fields);
            response_selection.validate()?;
            let compact_roots = if frame_ids.is_empty() {
                &payload.snapshot.roots
            } else {
                &frame_ids
            };
            let node_summaries = (response_selection.active()
                || outputs.iter().any(|o| o == "outline"))
            .then(|| super::compact::node_summaries(&payload.snapshot, compact_roots));
            page_scaffold::validate(page_scaffold.as_ref(), &outputs)?;
            super::validation::validate_width_policy_nodes(
                width_policy.as_ref(),
                &payload.snapshot,
            )?;
            let previous_fingerprints = super::design_drift::validate_request(
                &outputs,
                previous_design_fingerprints.as_deref(),
            )?;
            let design_fingerprints = outputs
                .iter()
                .any(|output| {
                    matches!(
                        output.as_str(),
                        "sourceMap" | "designFingerprints" | "designChanges"
                    )
                })
                .then(|| devup_mcp_devup_ui::provenance::design_fingerprints(&payload.snapshot));
            let scaffold_requested = page_scaffold.is_some();
            if (scaffold_requested
                || outputs
                    .iter()
                    .any(|o| matches!(o.as_str(), "tsx" | "componentTsx" | "responsiveTsx")))
                && payload
                    .assets
                    .iter()
                    .any(|a| a.render_mode == devup_mcp_figma::AssetRenderMode::Fill)
            {
                return Err(DevupError::new(
                    ErrorCode::DevupInvalidInput,
                    "Legacy fill captures remain separate from code generation; use original-source for CSS projection or renderMode node for native assets.",
                    false,
                ));
            }
            if (scaffold_requested
                || outputs
                    .iter()
                    .any(|o| matches!(o.as_str(), "tsx" | "componentTsx" | "responsiveTsx")))
                && payload.assets.iter().any(|a| {
                    a.render_mode == devup_mcp_figma::AssetRenderMode::Visible
                        && a.render_info.as_ref().is_some_and(|i| {
                            i["captureNodeId"].as_str() != Some(a.node_id.as_str())
                        })
                })
            {
                return Err(DevupError::new(
                    ErrorCode::DevupInvalidInput,
                    "Visible ancestor captures cannot replace leaf code at leaf dimensions; request assetManifest or explicitly generate the selected parent.",
                    false,
                ));
            }
            let keep_tsx = outputs.iter().any(|o| o == "tsx");
            if outputs.iter().any(|output| output == "sourceMap")
                && !outputs.iter().any(|output| {
                    matches!(
                        output.as_str(),
                        "tsx" | "componentTsx" | "devupJson" | "pageScaffold"
                    )
                })
            {
                return Err(unavailable_output(
                    payload,
                    "sourceMap",
                    include_str!("../text/projection-project_operation-648fa6f6.md"),
                ));
            }
            // Capture the accepted projection before fields are consumed by
            // codegen/file staging. Resource retry reuses this acquisition.
            let mut recovery_arguments = json!({
                "artifactId":artifact.artifact_id,"outputs":outputs,
                "componentName":component_name,"includeDiagnostics":include_diagnostics,
                "rootLayout":match root_layout { devup_mcp_devup_ui::codegen::RootLayout::Standalone => "standalone", devup_mcp_devup_ui::codegen::RootLayout::Embedded => "embedded" },
                "assetNamesPerNode":asset_names_per_node,"scope":scope,"strict":strict,
                "devupJsonMode":if theme_options.mode.is_empty() {"full"} else {&theme_options.mode},"projectRoot":theme_options.project_root,
                "outputPaths":output_paths,"frameIds":frame_ids,"allScreens":all_screens,
                "debug":outputs.iter().any(|o| matches!(o.as_str(), "rawSnapshot" | "rawPayload")),
                "delivery":"resource",
                "assetRequests":asset_captures.iter().map(|a| json!({"assetId":a.asset_id,"format":a.format,"scale":a.scale,"quality":a.quality,"lossless":a.lossless,"renderMode":a.render_mode,"usageAspectRatio":a.usage_aspect_ratio,"maxWidth":a.max_width,"srcset":a.srcset,"visibleScopeNodeId":a.visible_scope_node_id,
                    "outputPath":asset_output_paths.get(&a.asset_id)})).collect::<Vec<_>>()
            });
            if let Some(previous) = &previous_design_fingerprints {
                recovery_arguments["previousDesignFingerprints"] = json!(previous);
            }
            for (key, value) in serde_json::to_value(&response_selection)
                .unwrap()
                .as_object()
                .unwrap()
            {
                recovery_arguments[key] = value.clone();
            }
            if web_ready_images {
                recovery_arguments
                    .as_object_mut()
                    .unwrap()
                    .remove("assetRequests");
                recovery_arguments["preset"] = json!("web-ready-images");
            }
            if let Some(options) = &page_scaffold {
                recovery_arguments["pageScaffold"] = json!(options);
            }
            if let Some(root) = &asset_public_root {
                recovery_arguments["assetPublicRoot"] = json!(root);
            }
            if let Some(policy) = &width_policy {
                recovery_arguments["widthPolicy"] =
                    json!({"root": "fluid", "anchors": policy.anchors});
            }
            let mut result = Map::new();
            // Not restated by quality: this grades how far token resolution
            // reached - whether external library variables were covered - where
            // quality.theme only says whether what was resolved conflicts.
            result.insert("completeness".to_owned(), json!(payload.completeness));
            result.insert("collection".to_owned(), json!(collection));
            result.insert("cache".to_owned(), artifact_metadata(artifact));
            result.insert(
                "sectionChecklist".into(),
                super::section_checklist::checklist(payload, compact_roots),
            );
            let mut failures = payload
                .failures
                .iter()
                .map(|failure| json!(failure))
                .collect::<Vec<_>>();
            let mut warnings = Vec::new();
            for asset in payload
                .assets
                .iter()
                .filter(|asset| asset.status == AssetStatus::Failed)
            {
                let mut failure = json!({"assetId":asset.asset_id,"nodeId":asset.node_id,
                    "errorCode":asset.error_code,"message":"Asset export failed; inspect errorDetail before retrying.",
                    "retryable":asset.error_code.as_deref() == Some("DEVUP_ASSET_TIMEOUT")});
                if let Some(detail) = &asset.error_detail {
                    failure["errorDetail"] = json!(detail);
                    if let Some(hint) = devup_mcp_figma::figma_unreachable_hint(detail) {
                        failure["hint"] = json!(hint);
                    }
                }
                if asset.error_code.as_deref() == Some("DEVUP_ASSET_NODE_HIDDEN") {
                    warnings.push(failure);
                } else {
                    failures.push(failure);
                }
            }
            result.insert("failures".to_owned(), json!(failures));
            if !warnings.is_empty() {
                result.insert("warnings".to_owned(), json!(warnings));
            }
            result.insert(
                "source".to_owned(),
                json!({
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "nodeId": payload.target.node_id,
                    "version": payload.snapshot.version
                }),
            );
            let payload_section_index = section_index_from_payload(payload);
            let target_kind = if payload_section_index.is_some() {
                TargetKind::Section
            } else {
                classify_target(&payload.snapshot, &payload.target)
            };
            result.insert("targetKind".to_owned(), json!(target_kind));
            let selection_index = if target_kind == TargetKind::Section {
                let mut index = match &payload_section_index {
                    Some(index) => index.clone(),
                    None => {
                        devup_mcp_figma::build_section_index(&payload.snapshot, &payload.target)?
                    }
                };
                for (id, owner) in devup_mcp_figma::screen_owners(
                    &payload.snapshot,
                    index.candidates.iter().map(|c| c.node_id.as_str()),
                ) {
                    if owner.is_some() || !index.node_screen_ids.contains_key(&id) {
                        index.node_screen_ids.insert(id, owner);
                    }
                }
                Some(index)
            } else {
                None
            };
            let manifest_requested = outputs.iter().any(|output| output == "assetManifest");
            image_fill_audit::attach_image_fill_audit(
                &mut result,
                payload,
                &completeness_report,
                &outputs,
                asset_names_per_node,
            );
            result.insert(
                "assetSummary".to_owned(),
                asset_summary(
                    payload,
                    artifact,
                    &payload.snapshot.roots,
                    manifest_requested,
                ),
            );

            if !frame_ids.is_empty() && all_screens {
                return Err(DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "frameIds and allScreens cannot be used together.",
                    false,
                ));
            }
            if target_kind != TargetKind::Section && (!frame_ids.is_empty() || all_screens) {
                return Err(DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "frameIds and allScreens can only be used on a Section artifact.",
                    false,
                ));
            }

            let mut candidates_truncated = false;
            let section_candidates = if target_kind == TargetKind::Section
                && outputs.iter().any(|output| {
                    matches!(
                        output.as_str(),
                        "tsx"
                            | "componentTsx"
                            | "responsiveTsx"
                            | "pageScaffold"
                            | "assetManifest"
                            | "imageAudit"
                            | "designFingerprints"
                            | "designChanges"
                    )
                }) {
                Some(if let Some(index) = &payload_section_index {
                    candidates_truncated = index.truncated;
                    index
                        .candidates
                        .iter()
                        .map(|candidate| {
                            let mut explored = section_candidate_as_explore(candidate);
                            explored.node.kind =
                                devup_mcp_figma::classify_explore_node(&explored.node);
                            explored.selection_reasons = if explored.node.is_screen_candidate() {
                                vec!["screen-like".into(), "inside-section".into()]
                            } else {
                                vec!["explicit-selection-only".into(), "inside-section".into()]
                            };
                            explored
                        })
                        .collect()
                } else {
                    let explored = explore_snapshot(
                        &payload.snapshot,
                        &payload.target,
                        &ExploreOptions { limit: 100 },
                    )?;
                    candidates_truncated = explored.candidates_truncated;
                    explored.candidates
                })
            } else {
                None
            };
            if let Some(candidates) = &section_candidates {
                result.insert(
                    "responsiveGroups".into(),
                    json!(responsive_groups::discover(
                        candidates,
                        &artifact.artifact_id
                    )),
                );
            } else {
                let groups = responsive_groups::from_roots(payload, &artifact.artifact_id);
                if !groups.is_empty() {
                    result.insert("responsiveGroups".into(), json!(groups));
                }
            }
            if let Some(candidates) = &section_candidates
                && frame_ids.is_empty()
                && !all_screens
            {
                return section_selection_reply::section_selection_reply(
                    section_selection_reply::SectionSelection {
                        payload,
                        artifact,
                        artifact_store,
                        outputs: &outputs,
                        previous_design_fingerprints: &previous_design_fingerprints,
                        component_name: &component_name,
                        include_diagnostics,
                        root_layout,
                        asset_names_per_node,
                        scope: &scope,
                        theme_options: &theme_options,
                        strict,
                        output_paths: &output_paths,
                        page_scaffold: &page_scaffold,
                        frame_ids: &frame_ids,
                        all_screens,
                        asset_captures: &asset_captures,
                        web_ready_images,
                        asset_output_paths: &asset_output_paths,
                        asset_public_root: &asset_public_root,
                        delivery,
                        result,
                        recovery_arguments: &recovery_arguments,
                        candidates,
                        candidates_truncated,
                        completeness_report: &completeness_report,
                        scaffold_requested,
                    },
                )
                .await;
            }
            let written_paths = Map::new();
            let mut section_tsx_projected = false;
            let mut selected_projection_roots = None;
            let mut tsx_source_map = None;
            let mut generated_output_target = "devupJson";
            let mut devup_json_source_map = None;
            let mut projection_diagnostics = Vec::new();
            let mut fidelity_reports = Vec::new();
            let mut theme_conflict_count = 0;
            let mut theme_unresolved_count = 0;
            let mut pending_text_outputs = std::collections::BTreeMap::new();
            if output_paths.contains_key("imageAudit")
                && let Some(audit) = result.get("imageAudit")
            {
                pending_text_outputs.insert(
                    "imageAudit".into(),
                    serde_json::to_string_pretty(audit).unwrap_or_default(),
                );
            }
            if let Some(current) = &design_fingerprints {
                super::design_drift::attach(
                    &outputs,
                    previous_fingerprints.as_ref(),
                    current,
                    &mut result,
                    &mut pending_text_outputs,
                );
            }
            let mut pending_binary_outputs = std::collections::BTreeMap::new();
            let mut pending_asset_manifest = None;
            let asset_resource_outputs = Vec::new();
            (
                result,
                failures,
                projection_diagnostics,
                fidelity_reports,
                selected_projection_roots,
                section_tsx_projected,
            ) = project_section_frames::project_section_frames(
                project_section_frames::SectionFrames {
                    annotate_node_ids: theme_options
                        .project_root
                        .as_deref()
                        .is_some_and(super::screen_plan::node_ids_enabled),
                    failures,
                    design_fingerprints: &design_fingerprints,
                    payload,
                    artifact,
                    outputs: &outputs,
                    component_name: &component_name,
                    include_diagnostics,
                    root_layout,
                    asset_names_per_node,
                    frame_ids: &frame_ids,
                    all_screens,
                    width_policy: &width_policy,
                    result,
                    selection_index: &selection_index,
                    section_candidates,
                    completeness_report: &completeness_report,
                    asset_policies: &asset_policies,
                    selected_fields: &selected_fields,
                    scaffold_requested,
                    manifest_requested,
                    section_tsx_projected,
                    selected_projection_roots,
                    projection_diagnostics,
                    fidelity_reports,
                },
            )?;
            let component_name_for_components = component_name.clone();
            let (next_result, next_diagnostics, next_text_outputs, responsive_widths) =
                project_responsive_output::project_responsive_output(
                    project_responsive_output::ResponsiveOutput {
                        failures: &mut failures,
                        payload,
                        outputs: &outputs,
                        include_diagnostics,
                        root_layout,
                        asset_names_per_node,
                        theme_options: &theme_options,
                        output_paths: &output_paths,
                        result,
                        asset_policies: &asset_policies,
                        selected_projection_roots,
                        projection_diagnostics,
                        pending_text_outputs,
                        component_name_for_components: &component_name_for_components,
                    },
                )?;
            result = next_result;
            projection_diagnostics = next_diagnostics;
            pending_text_outputs = next_text_outputs;
            (
                result,
                projection_diagnostics,
                fidelity_reports,
                pending_text_outputs,
                tsx_source_map,
                generated_output_target,
                devup_json_source_map,
                theme_conflict_count,
                theme_unresolved_count,
                warnings,
            ) = project_node_and_theme_outputs::project_node_and_theme_outputs(
                project_node_and_theme_outputs::NodeAndThemeOutputs {
                    payload,
                    outputs: &outputs,
                    component_name,
                    include_diagnostics,
                    root_layout,
                    asset_names_per_node,
                    scope: &scope,
                    theme_options: &theme_options,
                    strict,
                    output_paths: &output_paths,
                    width_policy: &width_policy,
                    result,
                    failures: &mut failures,
                    warnings,
                    asset_policies: &asset_policies,
                    scaffold_requested,
                    keep_tsx,
                    section_tsx_projected,
                    tsx_source_map,
                    generated_output_target,
                    devup_json_source_map,
                    projection_diagnostics,
                    fidelity_reports,
                    theme_conflict_count,
                    theme_unresolved_count,
                    pending_text_outputs,
                    component_name_for_components: &component_name_for_components,
                },
            )?;
            if outputs.iter().any(|output| output == "rawSnapshot") {
                let raw = serde_json::to_value(&payload.snapshot).map_err(|error| {
                    DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        format!("Cannot serialize the raw snapshot: {error}"),
                        false,
                    )
                })?;
                let mut raw = raw;
                super::field_select::prune_nodes(&mut raw, &selected_fields);
                if output_paths.contains_key("rawSnapshot") {
                    pending_text_outputs.insert(
                        "rawSnapshot".to_owned(),
                        serde_json::to_string_pretty(&raw).unwrap_or_default(),
                    );
                }
                result.insert("rawSnapshot".to_owned(), raw);
            }

            // The whole collection, not only its node tree. A snapshot kept on
            // its own can be converted, but not the way the server converts
            // it: the token names come from the variables and styles collected
            // beside it, and without them a `$gray200` fill comes out as the
            // tail of its variable ID and a `typography="h4"` as five font
            // props. Captures kept as fixtures need the resources too, so
            // this writes them. The reference PNG is left out — it is large,
            // binary, and has its own output.
            if outputs.iter().any(|output| output == "rawPayload") {
                let mut without_png = payload.clone();
                without_png.reference_png = None;
                let raw = serde_json::to_value(&without_png).map_err(|error| {
                    DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        format!("Cannot serialize the raw payload: {error}"),
                        false,
                    )
                })?;
                let mut raw = raw;
                super::field_select::prune_nodes(&mut raw, &selected_fields);
                if output_paths.contains_key("rawPayload") {
                    pending_text_outputs.insert(
                        "rawPayload".to_owned(),
                        serde_json::to_string_pretty(&raw).unwrap_or_default(),
                    );
                }
                result.insert("rawPayload".to_owned(), raw);
            }

            if outputs.iter().any(|output| output == "sourceMap") && !section_tsx_projected {
                let rasterized_assets = tsx_source_map
                    .as_ref()
                    .map(|m| m.rasterized_assets())
                    .unwrap_or_default();
                let mut tsx_entries: Vec<Value> = tsx_source_map
                    .map(|source_map| source_map.property_entries())
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|entry| serde_json::to_value(entry).ok())
                    .collect();
                super::field_select::filter_entries(&mut tsx_entries, &selected_fields);
                let source_map = json!({
                    "version": 2,
                    "rasterizedAssets":rasterized_assets,
                    "designFingerprints":design_fingerprints,
                    "resolutionSemantics":devup_mcp_devup_ui::provenance::resolution_semantics(),
                    "tsx": tsx_entries,
                    "devupJson": devup_json_source_map
                        .map(|source_map| source_map.entries)
                        .unwrap_or_default(),
                    "source": {
                        "fileKey": vouched_file_key(payload),
                        "rootNodeId": payload.target.node_id,
                        "sourceVersion": payload.source_version,
                        "generatedOutput":generated_output_target,"mappingKind":"node-field-property"
                    }
                });
                if output_paths.contains_key("sourceMap") {
                    pending_text_outputs.insert(
                        "sourceMap".to_owned(),
                        serde_json::to_string_pretty(&source_map).unwrap_or_default(),
                    );
                }
                result.insert("sourceMap".to_owned(), source_map);
            }

            if outputs.iter().any(|output| output == "referencePng") {
                let reference = payload.reference_png.as_ref().ok_or_else(|| unavailable_output(payload, "referencePng",
                    "No reference PNG was collected. Re-collect a single screen URL with referencePng in outputs."))?;
                let bytes = STANDARD
                    .decode(reference.data_base64.as_bytes())
                    .map_err(|_| {
                        DevupError::new(
                            ErrorCode::DevupSnapshotUnsupported,
                            "The artifact reference PNG base64 is invalid.",
                            false,
                        )
                    })?;
                if bytes.len() != reference.byte_length
                    || Sha256::digest(&bytes)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                        != reference.sha256
                {
                    return Err(DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        "The artifact reference PNG length or hash does not match.",
                        false,
                    ));
                }
                if output_paths.contains_key("referencePng") {
                    pending_binary_outputs.insert("referencePng".to_owned(), bytes);
                }
                let mut inline = json!(reference);
                if let Some(note) = inline
                    .as_object_mut()
                    .and_then(|inline| inline.remove("note"))
                {
                    result.insert("referencePngNote".to_owned(), note);
                }
                result.insert("referencePng".to_owned(), inline);
            }

            (pending_asset_manifest, warnings) = prepare_asset_manifest::prepare_asset_manifest(
                prepare_asset_manifest::AssetManifestPreparation {
                    payload,
                    outputs: &outputs,
                    asset_names_per_node,
                    asset_captures: &asset_captures,
                    web_ready_images,
                    warnings,
                    scaffold_requested,
                    manifest_requested,
                    pending_asset_manifest,
                },
            )?;
            (result, failures, pending_text_outputs) =
                exclude_hidden_asset_outputs::exclude_hidden_asset_outputs(
                    exclude_hidden_asset_outputs::HiddenAssetOutputs {
                        failures,
                        payload,
                        asset_names_per_node,
                        result,
                        pending_text_outputs,
                    },
                )?;
            (result,) = summarize_projection_diagnostics::summarize_projection_diagnostics(
                summarize_projection_diagnostics::ProjectionDiagnostics {
                    payload,
                    outputs: &outputs,
                    include_diagnostics,
                    root_layout,
                    asset_names_per_node,
                    scope: &scope,
                    strict,
                    frame_ids: &frame_ids,
                    all_screens,
                    asset_captures: &asset_captures,
                    delivery,
                    result,
                    failures,
                    warnings,
                    completeness_report: &completeness_report,
                    section_tsx_projected,
                    projection_diagnostics,
                    fidelity_reports,
                    theme_conflict_count,
                    theme_unresolved_count,
                    component_name_for_components: &component_name_for_components,
                    target_kind,
                },
            )?;
            return finalize_export_delivery::finalize_export_delivery(
                finalize_export_delivery::ExportDelivery {
                    payload,
                    artifact,
                    artifact_store,
                    output_policy,
                    outputs,
                    asset_names_per_node,
                    strict,
                    output_paths,
                    page_scaffold,
                    asset_captures,
                    web_ready_images,
                    asset_output_paths,
                    asset_public_root,
                    response_selection,
                    delivery,
                    result,
                    recovery_arguments,
                    selection_index,
                    completeness_report,
                    scaffold_requested,
                    manifest_requested,
                    keep_tsx,
                    node_summaries,
                    written_paths,
                    pending_text_outputs,
                    pending_binary_outputs,
                    pending_asset_manifest,
                    asset_resource_outputs,
                    responsive_widths,
                },
            )
            .await;
        }
        PendingOperation::Collect | PendingOperation::Artifact { .. } => Err(DevupError::new(
            ErrorCode::DevupFigmaHandoffInvalid,
            "An internal collect operation cannot be completed from an MCP artifact.",
            false,
        )),
    }
}
