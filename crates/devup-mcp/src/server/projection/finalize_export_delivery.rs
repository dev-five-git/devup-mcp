use super::*;

pub(super) struct ExportDelivery<'a> {
    pub(super) payload: &'a CollectedPayload,
    pub(super) artifact: &'a ArtifactLookup,
    pub(super) artifact_store: &'a ArtifactStore,
    pub(super) output_policy: &'a OutputPolicy,
    pub(super) outputs: Vec<String>,
    pub(super) asset_names_per_node: bool,
    pub(super) strict: bool,
    pub(super) output_paths: BTreeMap<String, String>,
    pub(super) page_scaffold: Option<page_scaffold::PageScaffoldOptions>,
    pub(super) asset_captures: Vec<devup_mcp_figma::AssetSelection>,
    pub(super) web_ready_images: bool,
    pub(super) asset_output_paths: BTreeMap<String, String>,
    pub(super) asset_public_root: Option<std::path::PathBuf>,
    pub(super) response_selection: super::super::compact::ResponseSelection,
    pub(super) delivery: DeliveryMode,
    pub(super) result: Map<String, Value>,
    pub(super) recovery_arguments: Value,
    pub(super) selection_index: Option<devup_mcp_figma::SectionIndex>,
    pub(super) completeness_report: devup_mcp_figma::PayloadCompletenessReport,
    pub(super) scaffold_requested: bool,
    pub(super) manifest_requested: bool,
    pub(super) keep_tsx: bool,
    pub(super) node_summaries: Option<Value>,
    pub(super) written_paths: Map<String, Value>,
    pub(super) pending_text_outputs: BTreeMap<String, String>,
    pub(super) pending_binary_outputs: BTreeMap<String, Vec<u8>>,
    pub(super) pending_asset_manifest: Option<AssetManifest>,
    pub(super) asset_resource_outputs: Vec<ProjectedOutput>,
    pub(super) responsive_widths: Vec<f64>,
}

pub(super) async fn finalize_export_delivery(
    input: ExportDelivery<'_>,
) -> Result<Value, DevupError> {
    let ExportDelivery {
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
        mut asset_output_paths,
        mut asset_public_root,
        response_selection,
        delivery,
        mut result,
        recovery_arguments,
        selection_index,
        completeness_report,
        scaffold_requested,
        manifest_requested,
        keep_tsx,
        node_summaries,
        mut written_paths,
        mut pending_text_outputs,
        pending_binary_outputs,
        mut pending_asset_manifest,
        mut asset_resource_outputs,
        responsive_widths,
    } = input;
    if scaffold_requested
        && let Some(refusal) =
            page_scaffold::directory_refusal(&result, &output_paths, output_policy)?
    {
        return Ok(refusal);
    }
    if scaffold_requested && let Some(manifest) = &pending_asset_manifest {
        page_scaffold::prepare_assets(
            &result,
            &output_paths,
            manifest,
            &mut asset_output_paths,
            &mut asset_public_root,
            output_policy,
        )?;
    }
    let replacements = if let Some(manifest) = &mut pending_asset_manifest {
        reconcile_asset_paths(
            manifest,
            &mut asset_output_paths,
            asset_public_root.as_deref(),
            output_policy,
        )?
    } else {
        BTreeMap::new()
    };
    let owners = if let Some(index) = &selection_index {
        index.node_screen_ids.clone()
    } else {
        devup_mcp_figma::screen_owners(
            &payload.snapshot,
            payload.snapshot.roots.iter().map(String::as_str),
        )
    };
    // Share definitions only for labels present in these outputs. The
    // full label catalog remains documented and standalone maps carry it.
    if let Some(frames) = result.get("frames").and_then(Value::as_array)
        && frames.iter().any(|frame| frame.get("sourceMap").is_some())
    {
        let labels = frames
            .iter()
            .flat_map(|f| {
                f["outputResults"]
                    .as_object()
                    .into_iter()
                    .flat_map(|o| o.values())
            })
            .flat_map(|o| o["_sourceMap"].as_array().into_iter().flatten())
            .filter_map(|e| e["resolution"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let mut semantics = devup_mcp_devup_ui::provenance::resolution_semantics();
        semantics
            .as_object_mut()
            .unwrap()
            .remove("verificationAxis");
        semantics["relation"] =
            json!("Same node/property/screen/output: ABSOLUTE components.*.state is independent.");
        semantics["values"]
            .as_object_mut()
            .unwrap()
            .retain(|label, _| labels.contains(label.as_str()));
        result.insert("resolutionSemantics".into(), semantics);
    }
    if let Some(manifest) = &mut pending_asset_manifest {
        for asset in &mut manifest.assets {
            if let Some(path) = &asset.path {
                for variant in &mut asset.variants {
                    variant.path = Some(asset_policy::variant_path(path));
                }
            }
        }
        let css_map =
            asset_policy::css_map(&payload.snapshot, &manifest.assets, asset_names_per_node);
        for field in ["imageAudit", "imageFillAudit"] {
            if let Some(audit) = result.get_mut(field) {
                asset_policy::enrich_audit(audit, &css_map);
            }
        }
        if web_ready_images || outputs.iter().any(|o| o == "assetCssMap") {
            result.insert("assetCssMap".into(), css_map.clone());
        }
        for field in ["imageAudit", "assetCssMap"] {
            if output_paths.contains_key(field)
                && let Some(body) = result.get(field)
            {
                pending_text_outputs.insert(
                    field.into(),
                    serde_json::to_string_pretty(body).unwrap_or_default(),
                );
            }
        }
    }
    let mut value = Value::Object(std::mem::take(&mut result));
    rewrite_result_asset_references(&mut value, &replacements);
    audit_delivered_properties(&mut value);
    super::super::interactions::attach(&mut value, &payload.snapshot);
    attach_diagnostic_screens(&mut value, &owners);
    super::super::verdict_scope::attach(&mut value);
    attach_verification_scope(&mut value, &responsive_widths);
    if strict && value["mappingComplete"] == false {
        return Err(DevupError::with_details(
            ErrorCode::DevupSnapshotUnsupported,
            "strict export requires complete generated-property provenance.",
            false,
            json!({"quality":value["quality"],"projectionIssues":value["projectionIssues"]}),
        ));
    }
    result = value.as_object().expect("result object").clone();
    if outputs.iter().any(|o| o == "outline") {
        let outline =
            super::super::compact::outline(node_summaries.as_ref().unwrap(), &response_selection);
        if output_paths.contains_key("outline") {
            pending_text_outputs.insert(
                "outline".into(),
                serde_json::to_string_pretty(&outline).unwrap(),
            );
        }
        result.insert("outline".into(), outline);
    }
    for (name, contents) in &mut pending_text_outputs {
        if matches!(
            name.as_str(),
            "tsx" | "componentTsx" | "responsiveTsx" | "pageScaffold"
        ) {
            if let Some(code) = result.get(name).and_then(Value::as_str) {
                *contents = code.to_owned();
            }
        } else if name == "sourceMap" {
            *contents = serde_json::to_string_pretty(&result["sourceMap"]).unwrap_or_default();
        } else if matches!(name.as_str(), "rawSnapshot" | "rawPayload") {
            *contents = serde_json::to_string_pretty(&result[name]).unwrap_or_default();
        }
    }
    let mut planned_outputs = Vec::new();
    let mut supported_keys = Vec::new();
    if let Some(options) = &page_scaffold {
        let (key, planned) = page_scaffold::attach(
            &mut result,
            options,
            &output_paths,
            pending_asset_manifest.as_ref(),
            &asset_output_paths,
            output_policy,
            keep_tsx,
        )?;
        supported_keys.push(key);
        planned_outputs.extend(planned);
    }
    for field in [
        "tsx",
        "componentTsx",
        "responsiveTsx",
        "devupJson",
        "rawSnapshot",
        "rawPayload",
        "sourceMap",
        "designFingerprints",
        "referencePng",
        "imageAudit",
        "assetCssMap",
        "outline",
    ] {
        if result.contains_key(field) {
            supported_keys.push(field.to_owned());
        }
    }
    if let Some(frames) = result.get("frames").and_then(Value::as_array) {
        for frame in frames {
            for field in ["tsx", "componentTsx", "sourceMap"] {
                let Some(contents) = frame.get(field) else {
                    continue;
                };
                let key = format!("frame:{}:{field}", frame["nodeId"].as_str().unwrap());
                supported_keys.push(key.clone());
                if let Some(path) = output_paths.get(&key) {
                    let bytes = if let Some(code) = contents.as_str() {
                        code.as_bytes().to_vec()
                    } else {
                        serde_json::to_vec_pretty(contents).unwrap()
                    };
                    planned_outputs.push((key, output_policy.resolve(path)?, bytes));
                }
            }
        }
    }
    let path_diagnostics: Vec<_> = output_paths.keys().filter(|key| !supported_keys.contains(key)).map(|key| {
    json!({"code":"DEVUP_OUTPUT_PATH_UNSUPPORTED","key":key,"written":false,
        "message":"This key has no writable output in this projection. Use a supportedKeys entry; frame outputs require frame:<nodeId>:<output>."})
}).collect();
    result.insert("outputPathResults".into(), json!({"supportedKeys":supported_keys,
    "frameKeyFormat":if scaffold_requested { "frame:<nodeId>:<tsx|componentTsx|sourceMap|pageScaffold>" } else { "frame:<nodeId>:<tsx|componentTsx|sourceMap>" },
    "diagnostics":path_diagnostics,
    "note":"outputPaths reports committed writes only. Unsupported or unavailable keys do not write files; assetRequests use their own outputPath."}));
    if scaffold_requested {
        let kinds: Map<String, Value> = supported_keys
            .iter()
            .filter(|key| key.ends_with("pageScaffold"))
            .map(|key| (key.clone(), json!("directory")))
            .collect();
        result.get_mut("outputPathResults").unwrap()["pathKinds"] = json!(kinds);
    }
    for (output, contents) in pending_text_outputs {
        if let Some(path) = output_paths.get(&output) {
            planned_outputs.push((output, output_policy.resolve(path)?, contents.into_bytes()));
        }
    }
    for (output, bytes) in pending_binary_outputs {
        if let Some(path) = output_paths.get(&output) {
            planned_outputs.push((output, output_policy.resolve(path)?, bytes));
        }
    }
    if let Some(manifest) = pending_asset_manifest.as_ref() {
        for asset in &manifest.assets {
            if asset.status != AssetStatus::Exported {
                continue;
            }
            let Some(path) = asset_output_paths.get(&asset.asset_id) else {
                continue;
            };
            let data = asset.data_base64.as_deref().ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "The exported asset binary is not in the artifact.",
                    false,
                )
            })?;
            let bytes = STANDARD.decode(data.as_bytes()).map_err(|_| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "The exported asset binary base64 is invalid.",
                    false,
                )
            })?;
            planned_outputs.push((
                format!("asset:{}", asset.asset_id),
                output_policy.resolve(path)?,
                bytes,
            ));
            for variant in &asset.variants {
                let variant_path = asset_policy::variant_path(path);
                let bytes = STANDARD
                    .decode(variant.data_base64.as_deref().unwrap_or_default())
                    .map_err(|_| {
                        DevupError::new(
                            ErrorCode::DevupSnapshotUnsupported,
                            "Invalid variant base64.",
                            false,
                        )
                    })?;
                planned_outputs.push((
                    format!("asset:{}@2x", asset.asset_id),
                    output_policy.resolve(&variant_path)?,
                    bytes,
                ));
            }
        }
    }
    if page_scaffold.as_ref().is_some_and(|options| !options.write) {
        planned_outputs.clear();
    }
    let mut transaction = OutputTransaction::new();
    if page_scaffold
        .as_ref()
        .is_some_and(|options| !options.overwrite)
    {
        transaction.refuse_existing();
    }
    // Two nodes can be one picture - a logo drawn at three sizes
    // shares a file, as the plugin has it - so several assets resolve
    // to one path. That is one write, not a collision. Only differing
    // bytes under one name are a mistake, and that is worth refusing.
    // Two different drawings under one layer name land here too: the
    // plugin names an asset after its layer, and a snapshot cannot
    // tell two drawings of one name apart - only the exported bytes
    // can. The first is written and the rest are reported, so the
    // code still points at a file that exists and the caller learns
    // which names hide more than one picture.
    let fingerprint_path = planned_outputs
        .iter()
        .find(|(name, _, _)| name == "designFingerprints")
        .map(|(_, target, _)| target.display_path().to_path_buf());
    let mut staged_content: BTreeMap<String, String> = BTreeMap::new();
    let mut shared_names: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, target, bytes) in planned_outputs {
        let path = target.display_path().to_string_lossy().into_owned();
        let fingerprint = sha256_hex(&bytes);
        match staged_content.get(&path) {
            Some(staged) if *staged == fingerprint => {
                written_paths.insert(name, json!(path));
                continue;
            }
            Some(_)
                if scaffold_requested
                    || fingerprint_path.as_deref() == Some(target.display_path()) =>
            {
                return Err(DevupError::new(
                    ErrorCode::DevupInvalidInput,
                    format!("Output file collision: different outputs claim {path}."),
                    false,
                ));
            }
            Some(_) => {
                let asset = name.strip_prefix("asset:").unwrap_or(&name).to_owned();
                shared_names.entry(path).or_default().push(asset);
                continue;
            }
            None => {
                staged_content.insert(path.clone(), fingerprint);
            }
        }
        written_paths.insert(name.clone(), json!(path));
        transaction.stage(name, target, &bytes)?;
    }
    if manifest_requested && let Some(mut manifest) = pending_asset_manifest {
        asset_resource_outputs = projected_asset_outputs(&manifest)?;
        for asset in &mut manifest.assets {
            if asset.status != AssetStatus::Exported {
                continue;
            }
            let output_name = format!("asset:{}", asset.asset_id);
            let Some(path) = written_paths.get(&output_name).and_then(Value::as_str) else {
                continue;
            };
            asset.output_path = Some(path.to_owned());
            asset.data_base64 = None;
            for variant in &mut asset.variants {
                if let Some(path) = written_paths
                    .get(&format!("asset:{}@2x", asset.asset_id))
                    .and_then(Value::as_str)
                {
                    variant.output_path = Some(path.into());
                    variant.data_base64 = None;
                }
            }
        }
        for (path, others) in &shared_names {
            manifest.diagnostics.push(Diagnostic {
                code: "DEVUP_ASSET_NAME_SHARED".to_owned(),
                message: format!(
                    include_str!("../text/projection-project_operation_Diagnostic-6ab28a84.md"),
                    others.len(),
                    path = path
                ),
                severity: Some(DiagnosticSeverity::Warning),
                resource_kind: Some("asset".to_owned()),
                details: Some(json!({ "outputPath": path, "notWritten": others })),
                ..Diagnostic::default()
            });
        }
        let mut manifest = json!(manifest);
        if let Some(assets) = manifest["assets"].as_array_mut() {
            for asset in assets {
                let name = asset["nodeId"]
                    .as_str()
                    .and_then(|id| payload.snapshot.nodes.get(id))
                    .and_then(|node| node.typed_view().name());
                if let Some(name) = name {
                    asset["layerName"] = json!(name);
                }
            }
        }
        result.insert("assetManifest".to_owned(), manifest);
    }
    if web_ready_images {
        let captures = result["assetManifest"]["assets"].as_array().into_iter().flatten()
        .filter(|a| asset_captures.iter().any(|c| c.asset_id == a["assetId"].as_str().unwrap_or_default()))
        .map(|a| json!({"assetId":a["assetId"],"nodeId":a["nodeId"],"format":a["format"],"scale":a["scale"],
            "quality":a["quality"],"lossless":a["lossless"],"renderMode":a["renderMode"],"status":a["status"],
            "role":if a["sourceKind"] == "image-fill" {"descendant-image-fill"}else{"generated-code-native-raster-boundary"},
            "sha256":a["sha256"],"byteLength":a["byteLength"],"path":a["path"],"outputPath":a["outputPath"],"errorCode":a["errorCode"]})).collect::<Vec<_>>();
        let failed = captures
            .iter()
            .filter(|a| a["status"] != "exported")
            .count();
        result.insert("webReadyImages".into(), json!({"preset":"web-ready-images","version":1,"maxAssetsPerBatch":6,
        "total":asset_captures.len(),"exported":captures.len()-failed,"failed":failed,
        "complete":failed == 0 && captures.len() == asset_captures.len() && payload.failures.is_empty() && completeness_report.snapshot.state == devup_mcp_figma::CompletenessState::Complete,
        "collectionCompleteness":completeness_report.snapshot.state,
        "coverage":"all acquired in-scope IMAGE paints including hidden/failed dispositions; consult collectionCompleteness for acquisition gaps","defaultRenderMode":"original-source","quality":85,"requestedScale":2,"effectsAppliedBy":"css","cssMap":"asset-css-map.json",
        "plan":captures}));
    }
    if scaffold_requested {
        page_scaffold::committed_paths(&result, &mut written_paths);
    }
    result.insert("outputPaths".to_owned(), Value::Object(written_paths));
    for value in result.values_mut() {
        attach_diagnostic_screens(value, &owners);
    }
    let outline_only = !response_selection.active() && outputs.iter().all(|o| o == "outline");
    let compact_details = if let Some(summary) = &node_summaries {
        let selected = if outline_only {
            super::super::compact::outline_response(&mut result, summary)
        } else {
            super::super::compact::select(&mut result, summary, &response_selection)
        };
        selected.map_err(|mut error| {
        let mut recovery=recovery_arguments.clone();
        recovery.as_object_mut().unwrap().remove("path");
        recovery.as_object_mut().unwrap().remove("responseFields");
        if outputs.iter().all(|o|o=="outline") { recovery["responseLimit"]=json!(response_selection.response_limit.min(25)); }
        error.details=json!({"artifactId":artifact.artifact_id,"maxSelectedBytes":65536,
            "nextAction":{"tool":"devup_figma_export","arguments":recovery,
                "how":"Read original outputs as bounded resources, or select a narrower path or lower responseLimit against this same artifact. Scalar values are never silently truncated."}});
        error
    })?
    } else {
        None
    };
    let projected_outputs = projected_outputs_from_result(&result)?;
    let mut projected_outputs = projected_outputs;
    projected_outputs.extend(asset_resource_outputs);
    projected_outputs.extend(compact_details);
    let mut result = Value::Object(result);
    let attachment = apply_delivery(
        &mut result,
        if response_selection.active() || outline_only {
            DeliveryMode::Resource
        } else {
            delivery
        },
        artifact_store,
        artifact,
        projected_outputs,
        Some(&recovery_arguments),
    )
    .await
    .map_err(|error| super::super::delivery::with_inline_recovery(error, recovery_arguments))?;
    if let Err(error) = transaction.commit() {
        rollback_delivery(artifact_store, artifact, attachment).await;
        return Err(error);
    }
    commit_delivery(attachment);
    if let Some(path) = result["outputPaths"].get("designFingerprints").cloned() {
        result["outputPathResults"]["committed"] = json!({"designFingerprints":path});
    }
    Ok(result)
}
