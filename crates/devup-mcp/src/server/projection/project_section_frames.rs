use super::*;

pub(super) struct SectionFrames<'a> {
    pub(super) design_fingerprints: &'a Option<BTreeMap<String, String>>,
    pub(super) payload: &'a CollectedPayload,
    pub(super) artifact: &'a ArtifactLookup,
    pub(super) outputs: &'a Vec<String>,
    pub(super) component_name: &'a Option<String>,
    pub(super) include_diagnostics: bool,
    pub(super) root_layout: devup_mcp_devup_ui::codegen::RootLayout,
    pub(super) asset_names_per_node: bool,
    pub(super) frame_ids: &'a Vec<String>,
    pub(super) all_screens: bool,
    pub(super) width_policy: &'a Option<devup_mcp_devup_ui::codegen::width::WidthPolicy>,
    pub(super) result: Map<String, Value>,
    pub(super) failures: Vec<Value>,
    pub(super) selection_index: &'a Option<devup_mcp_figma::SectionIndex>,
    pub(super) section_candidates: Option<Vec<devup_mcp_figma::ExploreCandidate>>,
    pub(super) completeness_report: &'a devup_mcp_figma::PayloadCompletenessReport,
    pub(super) asset_policies:
        &'a BTreeMap<String, devup_mcp_devup_ui::codegen::AssetRepresentationPolicy>,
    pub(super) selected_fields: &'a std::collections::BTreeSet<String>,
    pub(super) scaffold_requested: bool,
    pub(super) manifest_requested: bool,
    pub(super) section_tsx_projected: bool,
    pub(super) selected_projection_roots: Option<Vec<String>>,
    pub(super) projection_diagnostics: Vec<Diagnostic>,
    pub(super) fidelity_reports: Vec<devup_mcp_devup_ui::provenance::FidelityReport>,
}

type SectionFrameProjection = Result<
    (
        Map<String, Value>,
        Vec<Value>,
        Vec<Diagnostic>,
        Vec<devup_mcp_devup_ui::provenance::FidelityReport>,
        Option<Vec<String>>,
        bool,
    ),
    DevupError,
>;

pub(super) fn project_section_frames(input: SectionFrames<'_>) -> SectionFrameProjection {
    let SectionFrames {
        design_fingerprints,
        payload,
        artifact,
        outputs,
        component_name,
        include_diagnostics,
        root_layout,
        asset_names_per_node,
        frame_ids,
        all_screens,
        width_policy,
        mut result,
        mut failures,
        selection_index,
        section_candidates,
        completeness_report,
        asset_policies,
        selected_fields,
        scaffold_requested,
        manifest_requested,
        mut section_tsx_projected,
        mut selected_projection_roots,
        mut projection_diagnostics,
        mut fidelity_reports,
    } = input;
    if let Some(candidates) = section_candidates {
        let by_id = candidates
            .iter()
            .map(|candidate| (candidate.node.node_id.as_str(), candidate))
            .collect::<std::collections::BTreeMap<_, _>>();
        let failed_ids = payload
            .failures
            .iter()
            .map(|failure| failure.node_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let selected = if all_screens {
            candidates
                .iter()
                .filter(|candidate| candidate.node.is_screen_candidate())
                .filter(|candidate| !failed_ids.contains(candidate.node.node_id.as_str()))
                .collect::<Vec<_>>()
        } else {
            let requested = frame_ids
                .iter()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            if requested.len() != frame_ids.len() {
                return Err(DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "frameIds contains a duplicate node.",
                    false,
                ));
            }
            if requested
                .iter()
                .any(|node_id| !by_id.contains_key(*node_id))
            {
                selection_index
                    .as_ref()
                    .expect("Section index")
                    .select(frame_ids, false)?;
            }
            candidates
                .iter()
                .filter(|candidate| {
                    requested.contains(candidate.node.node_id.as_str())
                        && !failed_ids.contains(candidate.node.node_id.as_str())
                })
                .collect::<Vec<_>>()
        };
        if selected.is_empty() && failed_ids.is_empty() {
            return Err(unavailable_output(
                payload,
                "Section outputs",
                "No screen frames match this selection. Use explicit frameIds for a nonstandard frame, or explore another Section.",
            ));
        }
        selected_projection_roots = Some(
            selected
                .iter()
                .map(|candidate| candidate.node.node_id.clone())
                .collect::<Vec<_>>(),
        );
        if scaffold_requested && selected.len() != 1 {
            return Err(DevupError::new(
                ErrorCode::DevupInvalidInput,
                "pageScaffold requires one frameId for the caller-supplied route.",
                false,
            ));
        }
        const MAX_SECTION_FRAMES: usize = 6;
        if selected.len() > MAX_SECTION_FRAMES {
            return Err(DevupError::with_details(
                ErrorCode::DevupInvalidInput,
                "This Section projection exceeds the batch budget. Request at most 6 frameIds per call; reuse this artifactId for subsequent batches.",
                false,
                json!({"artifactId":artifact.artifact_id,"requestedFrameCount":selected.len(),
                "recommendedBatchSize":3,"maxFrameCount":MAX_SECTION_FRAMES,"maxFrameOutputUnits":12,
                "recommendedFrameIds":selected.iter().take(3).map(|c| &c.node.node_id).collect::<Vec<_>>(),
                "remainingFrameIds":selected.iter().skip(3).map(|c| &c.node.node_id).collect::<Vec<_>>() }),
            ));
        }
        super::super::validation::validate_export_budget(
            &selected
                .iter()
                .map(|candidate| candidate.node.node_id.clone())
                .collect::<Vec<_>>(),
            outputs,
        )?;
        let mut frames = Vec::with_capacity(selected.len());
        for (index, candidate) in selected.iter().enumerate() {
            let frame_component_name = component_name.as_ref().map(|name| {
                if selected.len() == 1 {
                    name.clone()
                } else {
                    format!("{name}{}", index + 1)
                }
            });
            let mut frame = json!({"nodeId":candidate.node.node_id,"name":candidate.node.name,"outputResults":{},
            "canonicalUrl":candidate.canonical_url});
            let before_failures = failures.len();
            let mut frame_diagnostics = Vec::new();
            for field in ["tsx", "componentTsx"] {
                if !outputs.iter().any(|output| output == field)
                    && !(scaffold_requested && field == "tsx")
                {
                    continue;
                }
                let output = generate_collected_component(
                    payload,
                    &candidate.node.node_id,
                    &CodegenOptions {
                        asset_policies: asset_policies.clone(),
                        component_name: frame_component_name.clone(),
                        include_diagnostics,
                        inline_instances: field == "tsx",
                        root_layout,
                        asset_names_per_node,
                        width_policy: width_policy.clone(),
                        ..CodegenOptions::default()
                    }
                    .with_payload_tokens(payload),
                );
                let mut output = match output {
                    Ok(output) => output,
                    Err(error) => {
                        failures.push(projection_failure(
                            payload,
                            &candidate.node.node_id,
                            field,
                            &error,
                        ));
                        continue;
                    }
                };
                scope_output(&mut output, field);
                frame["outputResults"][field] = output_result(&output);
                if frame.get("widthBehavior").is_none() {
                    frame["widthBehavior"] = devup_mcp_devup_ui::codegen::width::width_behavior(
                        &payload.snapshot,
                        &candidate.node.node_id,
                        &output,
                    );
                }
                frame_diagnostics.extend(output.diagnostics.iter().cloned());
                fidelity_reports.push(output.fidelity_report.clone());
                frame[field] = json!(output.tsx);
                attach_fidelity(&mut frame, &output.fidelity_report, include_diagnostics);
                if outputs.iter().any(|output| output == "sourceMap") {
                    let mut frame_entries: Vec<Value> = output
                        .source_map
                        .property_entries()
                        .into_iter()
                        .filter_map(|entry| serde_json::to_value(entry).ok())
                        .collect();
                    super::super::field_select::filter_entries(&mut frame_entries, selected_fields);
                    frame["sourceMap"] = json!({"version":output.source_map.version,
                    "rasterizedAssets":output.source_map.rasterized_assets(),
                    "designFingerprints":design_fingerprints,
                    "resolutionSemantics":{"axis":"mapping-method","dictionary":"/resolutionSemantics"},
                    "entries":frame_entries,"source":{"fileKey":vouched_file_key(payload),
                    "rootNodeId":candidate.node.node_id,"sourceVersion":payload.source_version,
                    "generatedOutput":field,"mappingKind":"node-field-property"}});
                }
            }
            frame["placementContracts"] = json!(
                frame_diagnostics
                    .iter()
                    .filter(|d| d.code == "DEVUP_CODEGEN_PLACEMENT_CONTRACT")
                    .collect::<Vec<_>>()
            );
            frame["projectionIssues"] = json!(
                frame_diagnostics
                    .iter()
                    .filter(
                        |d| d.fidelity_impact() != devup_mcp_figma::FidelityImpact::None
                            || d.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"
                    )
                    .collect::<Vec<_>>()
            );
            projection_diagnostics.extend(frame_diagnostics.iter().cloned());
            let mut frame_quality = OutputQuality {
                acquisition: acquisition_quality(completeness_report, false),
                projection: projection_quality(true, &frame_diagnostics),
                theme: theme_quality(false, 0, 0),
                assets: assets_quality(false, &[], &[]),
            };
            if failures.len() > before_failures {
                frame_quality.projection =
                    if frame.get("tsx").is_some() || frame.get("componentTsx").is_some() {
                        ProjectionQuality::Lossy
                    } else {
                        ProjectionQuality::Failed
                    };
            }
            frame["quality"] = json!(frame_quality);
            frame["status"] = json!(if failures.len() > before_failures {
                "partial"
            } else {
                frame_quality.status()
            });
            frame["assetSummary"] = asset_summary(
                payload,
                artifact,
                std::slice::from_ref(&candidate.node.node_id),
                manifest_requested,
            );
            frame["projectionEvidence"] = json!(
                frame_diagnostics
                    .iter()
                    .filter(|d| {
                        d.fidelity_impact() == devup_mcp_figma::FidelityImpact::None
                            && matches!(
                                d.code.as_str(),
                                "DEVUP_CODEGEN_NON_RENDERING_ASSET"
                                    | "DEVUP_CODEGEN_ABSOLUTE_VERIFIED"
                                    | "DEVUP_CODEGEN_LAYOUT_ACCOUNTED_FOR"
                                    | "DEVUP_CODEGEN_PROPERTY_EVIDENCE"
                            )
                    })
                    .collect::<Vec<_>>()
            );
            frame_quality.assets = asset_collection_quality(&frame["assetSummary"]);
            frame["quality"] = json!(frame_quality);
            frame["status"] = json!(if failures.len() > before_failures {
                "partial"
            } else {
                frame_quality.status()
            });
            attach_completeness_report(
                &mut frame,
                frame_quality,
                completeness_report,
                include_diagnostics,
            );
            if include_diagnostics {
                frame["diagnostics"] = json!(
                    frame_diagnostics
                        .iter()
                        .filter(|d| d.code != "DEVUP_CODEGEN_PROPERTY_EVIDENCE")
                        .collect::<Vec<_>>()
                );
            }
            frames.push(frame);
        }
        annotate_identical_frames(&mut frames, payload, asset_names_per_node);
        if let Some(roots) = &selected_projection_roots {
            result.insert(
                "assetSummary".to_owned(),
                asset_summary(payload, artifact, roots, manifest_requested),
            );
        }
        if outputs
            .iter()
            .any(|output| matches!(output.as_str(), "tsx" | "componentTsx" | "pageScaffold"))
        {
            result.insert("frames".to_owned(), Value::Array(frames));
        }
        section_tsx_projected = true;
    }
    Ok((
        result,
        failures,
        projection_diagnostics,
        fidelity_reports,
        selected_projection_roots,
        section_tsx_projected,
    ))
}
