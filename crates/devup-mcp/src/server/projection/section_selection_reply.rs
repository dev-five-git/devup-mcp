use super::*;

pub(super) struct SectionSelection<'a> {
    pub(super) payload: &'a CollectedPayload,
    pub(super) artifact: &'a ArtifactLookup,
    pub(super) artifact_store: &'a ArtifactStore,
    pub(super) outputs: &'a Vec<String>,
    pub(super) previous_design_fingerprints: &'a Option<String>,
    pub(super) component_name: &'a Option<String>,
    pub(super) include_diagnostics: bool,
    pub(super) root_layout: devup_mcp_devup_ui::codegen::RootLayout,
    pub(super) asset_names_per_node: bool,
    pub(super) scope: &'a String,
    pub(super) theme_options: &'a theme_export::ThemeExportOptions,
    pub(super) strict: bool,
    pub(super) output_paths: &'a BTreeMap<String, String>,
    pub(super) page_scaffold: &'a Option<page_scaffold::PageScaffoldOptions>,
    pub(super) frame_ids: &'a Vec<String>,
    pub(super) all_screens: bool,
    pub(super) asset_captures: &'a Vec<devup_mcp_figma::AssetSelection>,
    pub(super) web_ready_images: bool,
    pub(super) asset_output_paths: &'a BTreeMap<String, String>,
    pub(super) asset_public_root: &'a Option<std::path::PathBuf>,
    pub(super) delivery: DeliveryMode,
    pub(super) result: Map<String, Value>,
    pub(super) recovery_arguments: &'a Value,
    pub(super) candidates: &'a Vec<devup_mcp_figma::ExploreCandidate>,
    pub(super) candidates_truncated: bool,
    pub(super) completeness_report: &'a devup_mcp_figma::PayloadCompletenessReport,
    pub(super) scaffold_requested: bool,
}

pub(super) async fn section_selection_reply(
    input: SectionSelection<'_>,
) -> Result<Value, DevupError> {
    let SectionSelection {
        payload,
        artifact,
        artifact_store,
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
        asset_captures,
        web_ready_images,
        asset_output_paths,
        asset_public_root,
        delivery,
        mut result,
        recovery_arguments,
        candidates,
        candidates_truncated,
        completeness_report,
        scaffold_requested,
    } = input;
    // Both candidate producers report actual list truncation.
    // Reaching the limit alone does not mean candidates were omitted.
    let truncated = candidates_truncated;
    let (screens, explicit_candidates): (Vec<_>, Vec<_>) = candidates
        .iter()
        .partition(|candidate| candidate.node.is_screen_candidate());
    let quality = OutputQuality {
        acquisition: acquisition_quality(completeness_report, false),
        projection: projection_quality(false, &[]),
        theme: theme_quality(false, 0, 0),
        assets: assets_quality(false, &[], &[]),
    };
    result.insert("status".to_owned(), json!("selection_required"));
    if let Some(summary) = result.get_mut("assetSummary") {
        summary["manifestIncluded"] = json!(false);
    }
    result.insert("quality".to_owned(), json!(quality));
    result.insert(
        "selection".to_owned(),
        json!({
            "kind": "screen-frame",
            "status": if truncated { "partial" } else { "complete" },
            "count": screens.len(),
            "candidates": screens,
            "explicitCandidates": explicit_candidates,
            "truncated": truncated
        }),
    );
    result.insert(
        "nextAction".to_owned(),
        json!({
            "why": "This is a Section candidate list. Screen artifacts have not been exported yet.",
            "how": include_str!("../text/projection-project_operation-2b397164.md"),
            "doNot": "Do not try to collect the whole Section at once."
        }),
    );
    // An example built from this call's own artifact and a real
    // candidate, so the next step is a call to run rather than a
    // shape to assemble.
    if let Some(candidate) = screens.first().or_else(|| explicit_candidates.first()) {
        result
            .get_mut("nextAction")
            .expect("nextAction was inserted")["example"] = json!({
            "tool": "devup_figma_export",
            "arguments": {
                "artifactId": artifact.artifact_id,
                "frameIds": [candidate.node.node_id],
                "outputs": outputs,
                "delivery": "resource",
                "componentName": component_name,
                "includeDiagnostics": include_diagnostics,
                "rootLayout": if root_layout == devup_mcp_devup_ui::codegen::RootLayout::Embedded { "embedded" } else { "standalone" },
                "assetNamesPerNode": asset_names_per_node,
                "scope": scope,
                "devupJsonMode":if theme_options.mode.is_empty() {"full"} else {&theme_options.mode},"projectRoot":theme_options.project_root,
                "strict": strict,
                "debug": outputs.iter().any(|output| super::super::validation::DIAGNOSIS_OUTPUTS.contains(&output.as_str())),
                "outputPaths": output_paths,
                "assetPublicRoot": asset_public_root,
                "assetRequests": asset_captures.iter().map(|capture| json!({
                    "assetId":capture.asset_id,"format":capture.format,"scale":capture.scale,"quality":capture.quality,"lossless":capture.lossless,"renderMode":capture.render_mode,"usageAspectRatio":capture.usage_aspect_ratio,"maxWidth":capture.max_width,"srcset":capture.srcset,"visibleScopeNodeId":capture.visible_scope_node_id,
                    "outputPath":asset_output_paths.get(&capture.asset_id)
                })).collect::<Vec<_>>()
            }
        });
        let arguments = result.get_mut("nextAction").expect("nextAction")["example"]["arguments"]
            .as_object_mut()
            .expect("example arguments");
        if web_ready_images {
            arguments.remove("assetRequests");
            arguments.insert("preset".into(), json!("web-ready-images"));
        }
        if let Some(previous) = &previous_design_fingerprints {
            arguments.insert("previousDesignFingerprints".into(), json!(previous));
        }
        if let Some(options) = &page_scaffold {
            arguments.insert("pageScaffold".into(), json!(options));
        }
        arguments.insert("scope".into(), json!("node"));
        if outputs.iter().any(|output| output == "referencePng") {
            arguments.remove("artifactId");
            arguments.remove("frameIds");
            arguments.insert("url".into(), json!(candidate.canonical_url));
        }
    } else {
        result.get_mut("nextAction").expect("nextAction")["example"] = json!({
            "tool":"devup_figma_explore", "arguments":{
                "url":payload.target.link(payload.target.node_id.as_deref()),
                "limit":100
            }
        });
    }
    let output_count = outputs
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        .max(1);
    let png = outputs.iter().any(|s| s == "referencePng");
    let batch_size = if png {
        1
    } else {
        (12 / output_count).clamp(1, 6)
    };
    let template = result["nextAction"]["example"]["arguments"].clone();
    let batches: Vec<_> = screens
        .chunks(batch_size)
        .map(|batch| {
            let mut args = template.clone();
            if png {
                args["url"] = json!(batch[0].canonical_url);
            } else {
                args["frameIds"] = json!(batch.iter().map(|c| &c.node.node_id).collect::<Vec<_>>());
            }
            json!({"tool":"devup_figma_export","arguments":args})
        })
        .collect();
    result.get_mut("nextAction").unwrap()["maxFramesPerCall"] = json!(batch_size);
    result.get_mut("nextAction").unwrap()["batches"] = json!(batches);
    for category in ["candidates", "explicitCandidates"] {
        if let Some(list) = result.get_mut("selection").unwrap()[category].as_array_mut() {
            for candidate in list {
                if candidate.get("textPreviewState").is_none() {
                    candidate["textPreviewState"] = json!("unknown-in-cached-snapshot");
                }
                candidate["nextAction"] = json!({"tool":"devup_figma_export","arguments":{
                "url":candidate["canonicalUrl"],"outputs":["tsx"],"scope":"node","delivery":"resource"}});
            }
        }
    }
    if scaffold_requested {
        page_scaffold::selection_guidance(&mut result);
    }
    responsive_groups::completion(&mut result, frame_ids, all_screens);
    let projected = projected_outputs_from_result(&result)?;
    let mut response = Value::Object(result);
    let attachment = apply_delivery(
        &mut response,
        delivery,
        artifact_store,
        artifact,
        projected,
        Some(recovery_arguments),
    )
    .await?;
    commit_delivery(attachment);
    Ok(response)
}
