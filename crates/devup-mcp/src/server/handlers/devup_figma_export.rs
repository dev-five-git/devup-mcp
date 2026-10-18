use super::*;

#[tool_router(router = devup_figma_export_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = interactions::output_schema()
    )]
    #[doc = include_str!("../text/devup-figma-export.md")]
    pub(super) async fn devup_figma_export(
        &self,
        Parameters(workflow): Parameters<FigmaExportWorkflowInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut input = workflow.input;
        if input.preset.as_deref() == Some("web-ready-images") {
            for output in ["assetManifest", "imageAudit"] {
                if !input.outputs.iter().any(|o| o == output) {
                    input.outputs.push(output.into());
                }
            }
        }
        // Before anything is validated or collected: a page write without a
        // confirmed screen plan is answered with the plan to make, and a
        // pageScaffold route comes from the plan when one covers the frame.
        if workflow.job_id.is_none()
            && let Some(project_root) = input.project_root.clone()
        {
            let mut frames = input.frame_ids.clone();
            if frames.is_empty() {
                let node = match (&input.url, &input.artifact_id) {
                    (Some(url), _) => FigmaTarget::parse(url).ok().and_then(|t| t.node_id),
                    (None, Some(id)) => self
                        .artifacts
                        .get(id)
                        .await
                        .and_then(|artifact| artifact.payload.target.node_id.clone()),
                    _ => None,
                };
                frames.extend(node);
            }
            let request = screen_plan::ExportRequest {
                project_root: &project_root,
                url: input.url.as_deref(),
                frames,
                all_screens: input.all_screens,
                scaffold_write: input.page_scaffold.as_ref().is_some_and(|s| s.write),
                scaffold_route: input.page_scaffold.as_ref().map(|s| s.route.as_str()),
                output_paths: input
                    .output_paths
                    .iter()
                    .filter(|(key, _)| !key.ends_with("pageScaffold"))
                    .map(|(_, path)| path.as_str())
                    .collect(),
            };
            match screen_plan::export_gate(&request).map_err(to_mcp_error)? {
                screen_plan::Gate::Open => {}
                screen_plan::Gate::Blocked(response) => return Ok(tool_result(response)),
                screen_plan::Gate::Route(route) => {
                    if let Some(scaffold) = input.page_scaffold.as_mut() {
                        scaffold.route = route;
                    }
                }
            }
        }
        projection::page_scaffold::validate(input.page_scaffold.as_ref(), &input.outputs)
            .map_err(to_mcp_error)?;
        validation::validate_asset_budget(&input.asset_requests).map_err(to_mcp_error)?;
        validation::validate_asset_render_outputs(&input.asset_requests, &input.outputs)
            .map_err(to_mcp_error)?;
        if input.page_scaffold.is_some() {
            validation::validate_asset_render_outputs(
                &input.asset_requests,
                &["pageScaffold".into()],
            )
            .map_err(to_mcp_error)?;
        }
        if let Some(id) = workflow.job_id.as_deref() {
            if input.url.is_some()
                || input.artifact_id.is_some()
                || !input.asset_requests.is_empty()
                || !input.output_paths.is_empty()
                || workflow.asset_public_root.is_some()
                || input.page_scaffold.is_some()
                || input.refresh
                || !input.frame_ids.is_empty()
                || input.all_screens
                || input.width_policy.is_some()
                || input.preset.is_some()
            {
                return Err(to_mcp_error(DevupError::new(
                    ErrorCode::DevupInvalidInput,
                    "jobId status/resume cannot change the original request or output paths.",
                    false,
                )));
            }
            let job = self.asset_jobs.get(id).map_err(to_mcp_error)?;
            match workflow.job_action.as_deref().unwrap_or("status") {
                "status" => {}
                "resume" => job.resume(),
                "cancel" => job.cancel(),
                _ => {
                    return Err(to_mcp_error(DevupError::new(
                        ErrorCode::DevupInvalidInput,
                        "jobAction must be status, resume or cancel.",
                        false,
                    )));
                }
            }
            // A collection of any size answers `in_progress` first, so this
            // poll — not the call that started it — is what hands the caller
            // their TSX. Checking it only on the synchronous returns left
            // every real export unchecked.
            let project_root = input.project_root.clone();
            // responseFields on a poll selects from this reply; without them
            // the reply carries compact per-asset rows.
            let selection = &input.response_selection;
            selection.validate().map_err(to_mcp_error)?;
            let result = job
                .wait_briefly(selection.active())
                .await
                .map_err(to_mcp_error)?;
            let result = with_project_checks(
                result,
                project_root.as_deref(),
                &skills::Lookup::new(
                    project_root.as_deref(),
                    self.output_policy.primary_root(),
                    self.client_name(),
                ),
            );
            if selection.active() {
                return compact::pick(&result, selection)
                    .map(tool_result)
                    .map_err(to_mcp_error);
            }
            return Ok(tool_result(result));
        }
        if workflow.job_action.is_some() {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupInvalidInput,
                "jobAction requires jobId.",
                false,
            )));
        }
        let web_ready_images = match input.preset.as_deref() {
            None => false,
            Some("web-ready-images") => true,
            Some(_) => {
                return Err(to_mcp_error(DevupError::new(
                    ErrorCode::DevupInvalidInput,
                    "preset must be web-ready-images.",
                    false,
                )));
            }
        };
        if web_ready_images && !input.asset_requests.is_empty() {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupInvalidInput,
                "web-ready-images cannot override custom assetRequests. Export the preset and custom requests separately; no requested work was started.",
                false,
            )));
        }
        if web_ready_images
            && !input
                .outputs
                .iter()
                .any(|o| o == "assetManifest" || o == "pageScaffold")
        {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupInvalidInput,
                "web-ready-images requires outputs including assetManifest or pageScaffold for reusable assets and completeness.",
                false,
            )));
        }
        let asset_public_root =
            validation::validate_public_root(workflow.asset_public_root.as_deref())
                .map_err(to_mcp_error)?;
        projection::theme_export::validate(
            &input.devup_json_mode,
            &input.outputs,
            input.project_root.as_deref(),
        )
        .map_err(to_mcp_error)?;
        if asset_public_root.is_some()
            && !web_ready_images
            && input.page_scaffold.is_none()
            && !input
                .asset_requests
                .iter()
                .any(|asset| asset.output_path.is_some())
        {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupInvalidInput,
                "assetPublicRoot requires an assetRequests outputPath in this call; saved output mappings are not part of an artifact.",
                false,
            )));
        }
        validation::validate_export_budget(&input.frame_ids, &input.outputs)
            .map_err(to_mcp_error)?;
        for path in input
            .output_paths
            .iter()
            .filter(|(key, _)| !key.ends_with("pageScaffold"))
            .map(|(_, path)| path)
            .chain(
                input
                    .asset_requests
                    .iter()
                    .filter_map(|asset| asset.output_path.as_ref()),
            )
        {
            let target = self.output_policy.resolve(path).map_err(to_mcp_error)?;
            if let Some(root) = &asset_public_root
                && input
                    .asset_requests
                    .iter()
                    .any(|asset| asset.output_path.as_ref() == Some(path))
            {
                validation::public_asset_url(root, target.display_path()).map_err(to_mcp_error)?;
            }
        }
        validate_outputs(&input.outputs, input.debug).map_err(to_mcp_error)?;
        input.response_selection.validate().map_err(to_mcp_error)?;
        if let Some(hints) = &input.screenshot_observations {
            hints.validate().map_err(to_mcp_error)?;
        }
        design_drift::validate_request(
            &input.outputs,
            input.previous_design_fingerprints.as_deref(),
        )
        .map_err(to_mcp_error)?;
        if !input.asset_requests.is_empty()
            && !input
                .outputs
                .iter()
                .any(|output| matches!(output.as_str(), "assetManifest" | "pageScaffold"))
        {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "Using assetRequests requires assetManifest in outputs.",
                false,
            )));
        }
        let reference_png_requested = input.outputs.iter().any(|output| output == "referencePng");
        if reference_png_requested && (!input.frame_ids.is_empty() || input.all_screens) {
            let target = if let Some(url) = &input.url {
                FigmaTarget::parse(url).ok()
            } else if let Some(id) = &input.artifact_id {
                self.artifacts
                    .get(id)
                    .await
                    .map(|a| a.payload.target.clone())
            } else {
                None
            };
            let url = target
                .as_ref()
                .zip(input.frame_ids.first())
                .map(|(target, id)| target.link(Some(id)));
            let mut arguments = json!(input);
            for key in ["artifactId", "frameIds", "allScreens", "url"] {
                arguments.as_object_mut().unwrap().remove(key);
            }
            arguments["scope"] = json!("node");
            if let Some(url) = &url {
                arguments["url"] = json!(url);
            }
            return Err(to_mcp_error(DevupError::with_details(
                ErrorCode::DevupSnapshotUnsupported,
                "referencePng can only be collected for a single Figma link target.",
                false,
                json!({"recoveryState":if url.is_some() {"available"} else {"manual-fix-required"},
                    "nextAction":{"tool":"devup_figma_export","arguments":arguments,
                        "requiredArguments":if url.is_some() {Vec::<&str>::new()} else {vec!["url"]},
                        "omitArguments":["artifactId","frameIds","allScreens"],
                        "how":include_str!("../text/mod-devup_figma_export-1fe13d5b.md")}}),
            )));
        }
        let root_layout = parse_root_layout(&input.root_layout).map_err(to_mcp_error)?;
        let width_policy =
            validation::parse_width_policy(input.width_policy.as_ref()).map_err(to_mcp_error)?;
        parse_scope(&input.scope).map_err(to_mcp_error)?;
        let delivery = input
            .delivery
            .parse::<DeliveryMode>()
            .map_err(to_mcp_error)?;

        if let Some(artifact_id) = input.artifact_id.as_deref() {
            if input.url.is_some() || input.refresh {
                return Err(to_mcp_error(DevupError::with_details(
                    ErrorCode::DevupFigmaHandoffInvalid,
                    "artifactId cannot be used together with url or refresh.",
                    false,
                    json!({"stage":"preflight","reason":"invalid-argument-combination",
                        "distinctFrom":"DEVUP_FIGMA_HANDOFF_EXPIRED means the artifactId itself is gone; this means the call mixed incompatible arguments.",
                        "nextAction":{"tool":"devup_figma_export",
                            "how":"Send either artifactId (reuse, no Figma call) or url (+ refresh:true to recollect), never both."}}),
                )));
            }
            let artifact = self
                .artifacts
                .get_for_export(artifact_id)
                .await
                .map_err(to_mcp_error)?;
            if web_ready_images && !artifact.capabilities.web_ready_images {
                return Err(to_mcp_error(DevupError::new(
                    ErrorCode::DevupFigmaHandoffInvalid,
                    "This artifact did not acquire web-ready-images. Repeat with the original url and preset to capture the complete image plan.",
                    false,
                )));
            }
            let (asset_selections, asset_output_paths) =
                parse_asset_requests(&input.asset_requests).map_err(to_mcp_error)?;
            if artifact.capabilities.kind == ArtifactKind::SectionIndex
                && (!input.frame_ids.is_empty() || input.all_screens)
            {
                let index = section_index_from_payload(&artifact.payload).ok_or_else(|| {
                    to_mcp_error(DevupError::new(
                        ErrorCode::DevupFigmaHandoffInvalid,
                        "The Section index artifact payload is invalid.",
                        false,
                    ))
                })?;
                let collection_scope =
                    parse_collection_scope(&input.scope).map_err(to_mcp_error)?;
                if collection_scope != CollectionScope::Node {
                    return Err(to_mcp_error(DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        "The Section Frame collection scope must be node.",
                        false,
                    )));
                }
                // A Section index already knows every candidate's name and
                // width, which is all a breakpoint family is decided on. Ask
                // here, where the answer costs nothing: the same refusal from
                // projection arrives only after the whole collection has run.
                if !input.frame_ids.is_empty()
                    && input.outputs.iter().any(|output| output == "responsiveTsx")
                {
                    let selected = input
                        .frame_ids
                        .iter()
                        .filter_map(|id| {
                            index
                                .candidates
                                .iter()
                                .find(|candidate| candidate.node_id == *id)
                        })
                        .map(|candidate| (candidate.name.as_str(), candidate.bounds.width))
                        .collect::<Vec<_>>();
                    // Only when every selected frame was found in the index.
                    // A frame the index does not describe leaves the question
                    // open, and an open question is collected, not refused.
                    if selected.len() == input.frame_ids.len()
                        && !devup_mcp_devup_ui::codegen::responsive::breakpoint_family_is_possible(
                            &selected,
                        )
                    {
                        let mut arguments = json!(input);
                        arguments["outputs"] = json!(["tsx"]);
                        arguments["frameIds"] = json!([input.frame_ids[0]]);
                        return Err(to_mcp_error(DevupError::with_details(
                            ErrorCode::DevupInvalidInput,
                            "responsiveTsx needs the selected frames to be one screen drawn at several widths, and these are not. Nothing was collected.",
                            false,
                            json!({
                                "output": "responsiveTsx",
                                "selectedFrames": selected
                                    .iter()
                                    .map(|(name, width)| json!({"name": name, "width": width}))
                                    .collect::<Vec<_>>(),
                                "recoveryState": "available",
                                "nextAction": {
                                    "tool": "devup_figma_export",
                                    "arguments": arguments,
                                    "why": "Each selected frame still converts on its own.",
                                    "how": "Request tsx one frame at a time, repeating for each frameId you selected.",
                                },
                            }),
                        )));
                    }
                }
                let mut request =
                    CollectionRequest::new(artifact.payload.target.clone(), collection_scope);
                request.resource_scope = ResourceScope::Used;
                request.asset_selections = asset_selections.clone();
                request.web_ready_images = web_ready_images;
                request.reference_png = reference_png_requested;
                request.section = Some(SectionReadOptions {
                    frame_ids: input.frame_ids.clone(),
                    all_screens: input.all_screens,
                });
                request.cached_section_index = Some(index);
                let result = self
                    .start_operation(
                        PendingOperation::Export {
                            outputs: input.outputs,
                            component_name: input.component_name,
                            include_diagnostics: input.include_diagnostics,
                            root_layout,
                            asset_names_per_node: input.asset_names_per_node,
                            scope: input.scope,
                            theme_options: projection::theme_export::ThemeExportOptions {
                                mode: input.devup_json_mode,
                                project_root: input.project_root.clone(),
                            },
                            strict: input.strict,
                            output_paths: input.output_paths,
                            page_scaffold: input.page_scaffold,
                            previous_design_fingerprints: input.previous_design_fingerprints,
                            frame_ids: input.frame_ids,
                            all_screens: input.all_screens,
                            asset_captures: asset_selections,
                            web_ready_images,
                            asset_output_paths,
                            asset_public_root: asset_public_root.clone(),
                            fields: input.fields.clone(),
                            response_selection: input.response_selection.clone(),
                            delivery,
                            width_policy: width_policy.clone(),
                        },
                        request,
                        false,
                    )
                    .await
                    .map_err(to_mcp_error)?;
                return Ok(tool_result(with_project_checks(
                    result,
                    input.project_root.as_deref(),
                    &skills::Lookup::new(
                        input.project_root.as_deref(),
                        self.output_policy.primary_root(),
                        self.client_name(),
                    ),
                )));
            }
            validate_artifact_projection(
                &artifact,
                &input.outputs,
                &input.scope,
                &asset_selections,
            )
            .map_err(to_mcp_error)?;
            let result = complete_operation(
                PendingOperation::Export {
                    outputs: input.outputs,
                    component_name: input.component_name,
                    include_diagnostics: input.include_diagnostics,
                    root_layout,
                    asset_names_per_node: input.asset_names_per_node,
                    scope: input.scope,
                    theme_options: projection::theme_export::ThemeExportOptions {
                        mode: input.devup_json_mode,
                        project_root: input.project_root.clone(),
                    },
                    strict: input.strict,
                    output_paths: input.output_paths,
                    page_scaffold: input.page_scaffold,
                    previous_design_fingerprints: input.previous_design_fingerprints,
                    frame_ids: input.frame_ids,
                    all_screens: input.all_screens,
                    asset_captures: asset_selections,
                    web_ready_images,
                    asset_output_paths,
                    asset_public_root: asset_public_root.clone(),
                    fields: input.fields.clone(),
                    response_selection: input.response_selection.clone(),
                    delivery,
                    width_policy: width_policy.clone(),
                },
                &artifact.payload,
                "artifact",
                &artifact,
                &self.output_policy,
                &self.artifacts,
            )
            .await
            .map_err(to_mcp_error)?;
            return Ok(tool_result(with_project_checks(
                result,
                input.project_root.as_deref(),
                &skills::Lookup::new(
                    input.project_root.as_deref(),
                    self.output_policy.primary_root(),
                    self.client_name(),
                ),
            )));
        }

        let collection_scope = parse_collection_scope(&input.scope).map_err(to_mcp_error)?;
        let mut target = parse_link(input.url.as_deref()).map_err(to_mcp_error)?;
        let mut frame_ids = input.frame_ids;
        // With no link, frameIds naming one frame is that frame - exactly what
        // its own link would say. Several are screens of the selected Section.
        if target.file_key == BRIDGE_CURRENT_KEY
            && target.node_id.is_none()
            && frame_ids.len() == 1
            && !input.all_screens
        {
            target.node_id = frame_ids.pop();
        }
        let wants_tsx = input.outputs.iter().any(|output| output == "tsx");
        let unnamed = if !frame_ids.is_empty() || input.all_screens {
            Unnamed::SelectedSection
        } else if collection_scope == CollectionScope::File && !wants_tsx {
            Unnamed::SelectionOrFile
        } else {
            Unnamed::Selection
        };
        let mut retry =
            json!({ "tool": "devup_figma_export", "arguments": { "outputs": input.outputs } });
        if let Some(hints) = &input.screenshot_observations {
            retry["arguments"]["screenshotObservations"] = json!(hints);
        }
        if !frame_ids.is_empty() {
            retry["arguments"]["frameIds"] = json!(frame_ids);
        }
        if input.all_screens {
            retry["arguments"]["allScreens"] = json!(true);
        }
        let target = self
            .bind_to_bridge(target, unnamed, &retry)
            .await
            .map_err(to_mcp_error)?;
        if wants_tsx && target.node_id.is_none() {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "A TSX export link requires a node-id.",
                false,
            )));
        }
        let mut request = CollectionRequest::new(target, collection_scope);
        let (asset_selections, asset_output_paths) =
            parse_asset_requests(&input.asset_requests).map_err(to_mcp_error)?;
        request.asset_selections = asset_selections.clone();
        request.web_ready_images = web_ready_images;
        request.reference_png = reference_png_requested;
        request.resource_scope = if collection_scope == CollectionScope::File {
            ResourceScope::File
        } else {
            ResourceScope::Used
        };
        request.variables_only = collection_scope == CollectionScope::File
            && input.outputs.iter().all(|output| output == "devupJson")
            && request.asset_selections.is_empty();
        if !frame_ids.is_empty() || input.all_screens {
            request.section = Some(SectionReadOptions {
                frame_ids: frame_ids.clone(),
                all_screens: input.all_screens,
            });
        }
        let operation = PendingOperation::Export {
            outputs: input.outputs,
            component_name: input.component_name,
            include_diagnostics: input.include_diagnostics,
            root_layout,
            asset_names_per_node: input.asset_names_per_node,
            scope: input.scope,
            theme_options: projection::theme_export::ThemeExportOptions {
                mode: input.devup_json_mode,
                project_root: input.project_root.clone(),
            },
            strict: input.strict,
            output_paths: input.output_paths,
            page_scaffold: input.page_scaffold,
            previous_design_fingerprints: input.previous_design_fingerprints,
            frame_ids,
            all_screens: input.all_screens,
            asset_captures: asset_selections,
            web_ready_images,
            asset_output_paths,
            asset_public_root: asset_public_root.clone(),
            fields: input.fields.clone(),
            response_selection: input.response_selection.clone(),
            delivery,
            width_policy: width_policy.clone(),
        };
        // A frame link plus frameIds is a precise request; refusing it because
        // the plugin wants the parent Section only makes the caller repeat
        // the ancestor lookup the plugin already did. Retry once, there.
        let retry_with = |error: &DevupError| section_retry(error, &operation, &request);
        let result = match self
            .start_operation(operation.clone(), request.clone(), input.refresh)
            .await
        {
            Err(error) => match retry_with(&error) {
                Some((operation, request, resolved)) => self
                    .start_operation(operation, request, input.refresh)
                    .await
                    .map(|mut value| {
                        if let Some(object) = value.as_object_mut() {
                            object.insert("autoResolvedSection".into(), resolved);
                        }
                        value
                    }),
                None => Err(error),
            },
            ok => ok,
        }
        .map_err(to_mcp_error)?;
        Ok(tool_result(with_project_checks(
            result,
            input.project_root.as_deref(),
            &skills::Lookup::new(
                input.project_root.as_deref(),
                self.output_policy.primary_root(),
                self.client_name(),
            ),
        )))
    }
}
