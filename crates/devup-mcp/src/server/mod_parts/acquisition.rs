

impl DevupServer {
    async fn start_operation(
        &self,
        operation: PendingOperation,
        request: CollectionRequest,
        refresh: bool,
    ) -> Result<Value, DevupError> {
        if matches!(operation, PendingOperation::Export { .. }) {
            return self.start_asset_job(operation, request, refresh).await;
        }
        self.start_operation_scoped(operation, request, refresh, None)
            .await
    }

    /// As [`Self::start_operation`], plus the one shaping step that cannot be
    /// done by the tool handler on its own: narrowing search matches to the
    /// node the caller linked. Deciding whether a match sits inside that node
    /// needs the collected snapshot's parent chain, and this is the last place
    /// that still holds the snapshot.
    async fn start_operation_scoped(
        &self,
        operation: PendingOperation,
        request: CollectionRequest,
        refresh: bool,
        scope: Option<&SearchScope>,
    ) -> Result<Value, DevupError> {
        self.start_operation_scoped_tracked(operation, request, refresh, scope, None)
            .await
    }

    async fn start_operation_scoped_tracked(
        &self,
        operation: PendingOperation,
        request: CollectionRequest,
        refresh: bool,
        scope: Option<&SearchScope>,
        job: Option<&asset_jobs::AssetJob>,
    ) -> Result<Value, DevupError> {
        let artifact_key = ArtifactRequestKey::from_collection(&request);
        if !refresh && let Some(artifact) = self.artifacts.lookup(&artifact_key).await {
            if let Some(job) = job {
                job.captured(&artifact.payload.assets);
            }
            let response = complete_operation(
                operation,
                &artifact.payload,
                "artifact",
                &artifact,
                &self.output_policy,
                &self.artifacts,
            )
            .await?;
            return Ok(apply_search_scope(
                response,
                &artifact.payload.snapshot,
                artifact.capabilities.collection_scope,
                scope,
            ));
        }
        if !refresh
            && let Some(artifact) = self.artifacts.lookup_related_explore(&artifact_key).await
        {
            if let Some(job) = job {
                job.captured(&artifact.payload.assets);
            }
            let response = complete_operation(
                operation,
                &artifact.payload,
                "artifact",
                &artifact,
                &self.output_policy,
                &self.artifacts,
            )
            .await?;
            return Ok(apply_search_scope(
                response,
                &artifact.payload.snapshot,
                artifact.capabilities.collection_scope,
                scope,
            ));
        }
        // The bridge is asked first, because it is the path that spends no
        // allowance and needs no login at all. Demanding the token before
        // looking at it inverted that: someone who had attached the plugin
        // precisely to stay off the metered path was told to go and authorize
        // the metered path first, and the collection never started — on a file
        // every read of which the plugin was sitting there ready to serve.
        //
        // A login is required only when no plugin is holding *this* file open.
        // A read inside the collection that the bridge cannot serve still
        // refuses on its own, where the reason is specific to that read,
        // rather than here where it would condemn the whole export.
        if !self
            .services
            .upstream
            .serves_without_credentials(&request.target.file_key)
            .await
        {
            let auth_status = self.services.auth.status().await?;
            if auth_status == AuthStatus::Disconnected {
                return Err(DevupError::with_details(
                    ErrorCode::DevupAuthRequired,
                    include_str!("../text/mod-start_operation_scoped_tracked-ec551baf.md"),
                    false,
                    json!({
                        "source": "direct",
                        "bridgeServesFile": false,
                        "preferredPath": "bridge",
                        "fileKey": request.target.file_key,
                    }),
                ));
            }
        }

        // Each tracked job owns its collector. Artifact single-flight would
        // make another output-path job wait on a paused collector it cannot resume.
        let acquisition = if job.is_some() {
            let payload = CollectedPayload::try_from(
                self.run_direct(request.clone(), &operation, job).await?,
            )?;
            // Only deduplicate publication once this collector has finished.
            // A ready payload cannot strand another job behind a paused read,
            // and the winning artifact's resource URIs remain valid.
            self.artifacts
                .get_or_acquire(artifact_key.clone(), refresh, || async { Ok(payload) })
                .await
        } else {
            self.artifacts
                .get_or_acquire(artifact_key.clone(), refresh, || async {
                    CollectedPayload::try_from(
                        self.run_direct(request.clone(), &operation, None).await?,
                    )
                })
                .await
        };
        match acquisition {
            Ok(artifact) => {
                if let Some(job) = job {
                    job.captured(&artifact.payload.assets);
                }
                let response = complete_operation(
                    operation,
                    &artifact.payload,
                    "direct",
                    &artifact,
                    &self.output_policy,
                    &self.artifacts,
                )
                .await?;
                Ok(apply_search_scope(
                    response,
                    &artifact.payload.snapshot,
                    artifact.capabilities.collection_scope,
                    scope,
                ))
            }
            Err(error) => Err(error),
        }
    }

    /// A collection is a burst: a Section of any size spends five to seventeen
    /// calls back to back, and Figma meters by the minute. So a large enough
    /// target outruns its own allowance partway through, and the refusal used
    /// to end the whole collection — discarding every call already spent and
    /// returning nothing, which is the worst of both: the allowance is gone and
    /// there is no result to show for it. Waiting is what the refusal asks for.
    /// It is marked retryable and often carries the exact number of seconds.
    ///
    /// Bounded, because an allowance that is genuinely exhausted must still be
    /// reported rather than waited on forever: three attempts, each waiting
    /// what upstream asked for, or a widening guess when it did not say.
    async fn call_waiting_out_a_spent_allowance(
        &self,
        call: ReadToolCall,
        attempt_timeout: Option<std::time::Duration>,
    ) -> Result<UpstreamResult, DevupError> {
        const ATTEMPTS: u32 = 3;
        const LONGEST_WAIT: u64 = 90;

        // A call already banked costs no allowance, so it is answered ahead of
        // the pacer rather than queued behind it. This is what lets a capture
        // too large for one day's allowance finish across several: the reads
        // an earlier attempt paid for are replayed, and only what is still
        // missing is spent on.
        if let Some(raw) = self.services.call_cache.get(&call) {
            return Ok(UpstreamResult { raw });
        }

        // Only a read bound for the metered path is paced. One the Devup Bridge
        // plugin answers spends none of Figma's allowance, and holding it to
        // that pace made a second export on the same server sit out most of a
        // minute for an allowance it was not using.
        let metered = self.services.upstream.is_metered(&call).await;
        let mut attempt = 1;
        loop {
            // Before the call, not after the refusal: a collection that paces
            // itself under the ceiling rarely has to be waited out at all.
            if metered {
                self.services.pacer.acquire().await;
            }
            // Rate-limit pacing is deliberate waiting, not a stalled upstream
            // read. Bound each actual attempt without cancelling the retry policy.
            let read = self.services.upstream.call_read_tool(call.clone());
            let result = if let Some(timeout) = attempt_timeout {
                tokio::time::timeout(timeout, read).await.unwrap_or_else(|_| Err(DevupError::with_details(
                    ErrorCode::DevupFigmaDirectUnavailable,
                    "Export job upstream read timed out; accepted reads are retained. Resume the job to retry the pending read.", true,
                    json!({"stage":"upstream-read","timeoutSeconds":timeout.as_secs()}))))
            } else {
                read.await
            };
            let error = match result {
                Ok(result) => {
                    // Only an answer is banked. A refusal arrives as a
                    // *successful* MCP call — `isError` in the body, or the
                    // sentence alone — and banking one meant every later
                    // attempt replayed it at once, from disk, without reaching
                    // Figma: twenty-eight refusals overnight and a fresh token
                    // refused, all one cached refusal. The classification
                    // that decides retries decides this too, so the two cannot
                    // disagree.
                    if operation::upstream_error(&result.raw).is_none() {
                        self.services.call_cache.put(&call, &result.raw);
                    }
                    return Ok(result);
                }
                Err(error) => error,
            };
            if error.code != ErrorCode::DevupFigmaRateLimited {
                return Err(error);
            }
            // The ceiling was reached at a rate the pacer thought was safe, so
            // its picture is what is wrong. Correct it before anything else —
            // including before giving up, because whatever runs next inherits
            // the same window and would otherwise walk into the same refusal.
            self.services.pacer.penalise();
            if attempt >= ATTEMPTS {
                return Err(error);
            }
            let asked_for = error
                .details
                .get("retryAfterSeconds")
                .and_then(serde_json::Value::as_u64);
            let wait = asked_for
                .unwrap_or(u64::from(attempt) * 20)
                .min(LONGEST_WAIT);
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            attempt += 1;
        }
    }

    async fn run_direct(
        &self,
        request: CollectionRequest,
        operation: &PendingOperation,
        job: Option<&asset_jobs::AssetJob>,
    ) -> Result<CollectedParts, DevupError> {
        let budget = match operation {
            PendingOperation::Export {
                outputs,
                all_screens: true,
                ..
            } => Some(outputs),
            _ => None,
        };
        if let (Some(outputs), Some(index)) = (budget, &request.cached_section_index) {
            validation::validate_export_budget(&index.select(&[], true)?, outputs)?;
        }
        let target = request.target.clone();
        // 리소스를 몇 개씩 묶어 물을지는 이 수집을 실어 나를 전송이 정한다.
        // 공식 MCP 는 응답을 자르므로 8개씩 끊지만, 자르지 않는 전송은 한 번에
        // 다 물어도 되고 그만큼 왕복이 사라진다.
        let mut request = request;
        request.batch_budget = self.services.upstream.batch_budget();
        let mut collector = CollectorSession::new(request);
        loop {
            if let Some(job) = job {
                job.progress(&collector);
            }
            match collector.advance()? {
                CollectorStep::Call(planned) => {
                    let call_id = planned.id.clone();
                    let asset_read = planned.call.is_asset_read();
                    let section_index_call = matches!(
                        &planned.call,
                        ReadToolCall::Snapshot {
                            script: devup_mcp_figma::BuiltinScript::SectionIndex,
                            ..
                        }
                    );
                    let upstream_result = if let Some(job) = job {
                        job.call(self, planned.call).await
                    } else {
                        self.call_waiting_out_a_spent_allowance(planned.call, None)
                            .await
                    };
                    match upstream_result {
                        // A Section target is not a failed call — the script
                        // throws, and MCP delivers that as a successful result
                        // carrying `isError`. Handing it to `accept` made the
                        // collector look for snapshot data that was never
                        // there and report "snapshot data not found", hiding
                        // the one thing the caller needed to know. Rejecting
                        // it lets the collector switch to the section index
                        // and answer with the screens inside, which is what
                        // the collector has always done.
                        Ok(result) if operation::is_section_error_result(&result.raw) => {
                            let error = DevupError::new(
                                ErrorCode::DevupSnapshotUnsupported,
                                "DEVUP_TARGET_IS_SECTION",
                                false,
                            );
                            if !collector.reject(&call_id, &error)? {
                                return Err(error);
                            }
                        }
                        // Every other upstream refusal arrives the same way.
                        // Report what upstream said instead of letting the
                        // collector misread the response as missing data.
                        Ok(result) => match operation::upstream_error(&result.raw) {
                            Some(mut error) => {
                                if !error.details.is_object() {
                                    error.details = json!({"upstreamDetails":error.details});
                                }
                                // A key only the bridge routes by is not the
                                // file's, so it is not reported as one.
                                error.details["fileKey"] = if target.is_bridge_only() {
                                    Value::Null
                                } else {
                                    json!(target.file_key)
                                };
                                if error.details.get("nodeId").is_none() {
                                    error.details["nodeId"] = json!(planned.expected_node_id);
                                }
                                error.details["stage"] = json!(if section_index_call {
                                    "section-index"
                                } else {
                                    "plugin-execution"
                                });
                                if !collector.reject(&call_id, &error)? {
                                    return Err(error);
                                }
                            }
                            None => {
                                if section_index_call && let Some(outputs) = budget {
                                    let chunk =
                                        devup_mcp_figma::snapshot_chunk_from_result(&result)?;
                                    let snapshot = devup_mcp_figma::merge_chunks(vec![chunk])?;
                                    let index =
                                        devup_mcp_figma::build_section_index(&snapshot, &target)?;
                                    validation::validate_export_budget(
                                        &index.select(&[], true)?,
                                        outputs,
                                    )?;
                                }
                                if asset_read {
                                    static ENCODERS: std::sync::OnceLock<
                                        std::sync::Arc<tokio::sync::Semaphore>,
                                    > = std::sync::OnceLock::new();
                                    let permit = ENCODERS
                                        .get_or_init(|| {
                                            std::sync::Arc::new(tokio::sync::Semaphore::new(2))
                                        })
                                        .clone()
                                        .acquire_owned()
                                        .await
                                        .map_err(|_| {
                                            DevupError::new(
                                                ErrorCode::DevupInvalidInput,
                                                "Asset postprocessing is unavailable.",
                                                false,
                                            )
                                        })?;
                                    let (returned, accepted) = tokio::task::spawn_blocking(move || {
                                        let _permit = permit;
                                        let accepted = collector.accept(&call_id, result);
                                        (collector, accepted)
                                    }).await.map_err(|_| DevupError::new(ErrorCode::DevupInvalidInput, "Asset postprocessing worker failed; repeat the request.", false))?;
                                    collector = returned;
                                    accepted?;
                                } else {
                                    collector.accept(&call_id, result)?;
                                }
                            }
                        },
                        Err(error) if collector.reject(&call_id, &error)? => continue,
                        Err(error) => return Err(error),
                    }
                }
                CollectorStep::AwaitingResults => continue,
                CollectorStep::Complete(parts) => return Ok(*parts),
            }
        }
    }

    /// Both paths to Figma as they stand right now.
    /// `devup_figma_explore` without the tool envelope, so `devup_screen_plan`
    /// lists screens through the same read rather than a second one.
    async fn explore(&self, input: FigmaExploreInput) -> Result<Value, DevupError> {
        if let Some(hints) = &input.screenshot_observations {
            hints.validate()?;
        }
        let target = self
            .bind_to_bridge(
                parse_link(input.url.as_deref())?,
                Unnamed::Selection,
                &json!({ "tool": "devup_figma_explore", "arguments": { "limit": input.limit,"screenshotObservations":input.screenshot_observations } }),
            )
            .await
            ?;
        target.node_id.as_ref().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "Exploring neighboring Figma screens requires a node-id.",
                false,
            )
        })?;
        if !(1..=MATCH_CEILING).contains(&input.limit) {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "The explore limit must be between 1 and 100 inclusive.",
                false,
            ));
        }
        let requested_target = target.clone();
        let mut request = CollectionRequest::new(target, CollectionScope::Node);
        request.resource_scope = ResourceScope::None;
        request.explore = Some(ExploreReadOptions {
            // Fixed, not derived from `limit`. Derived, it made `limit` two
            // things at once - how many screens come back, and how much of the
            // page the collection script is allowed to walk - and the second
            // one is why limit 33 could answer with fewer screens than limit
            // 30. Asking for the script's own ceiling every time also means
            // one anchor's projection can be reused for a neighbour's, since
            // every explore of a file now reads the same amount.
            projection_limit: EXPLORE_PROJECTION_LIMIT,
            text_preview_limit: if input.include_text_preview
                || input.screenshot_observations.is_some()
            {
                160
            } else {
                0
            },
        });
        let result = self
            .start_operation(
                PendingOperation::Explore {
                    // The projection ranks up to the ceiling and `limit` cuts
                    // the ranked list afterwards, in `apply_explore_limit`, so
                    // that what was found and what was returned are both known
                    // and can be reported apart.
                    limit: MATCH_CEILING,
                    target: requested_target,
                },
                request,
                input.refresh,
            )
            .await?;
        let mut result = result;
        screenshot_observations::rank(&mut result, input.screenshot_observations.as_ref());
        Ok(apply_explore_limit(result, input.limit))
    }

    /// The screens `devup_screen_plan draft` plans: the caller's list from an
    /// earlier explore, or one explore of the linked Section or selection.
    async fn plan_screens(
        &self,
        input: &screen_plan::ScreenPlanInput,
    ) -> Result<(Vec<screen_plan::PlanScreenInput>, Value), DevupError> {
        if !input.screens.is_empty() {
            return Ok((
                input.screens.clone(),
                json!({"from": "caller", "url": input.url,
                    "nodeId": input.url.as_deref().and_then(|url| FigmaTarget::parse(url).ok()).and_then(|target| target.node_id)}),
            ));
        }
        let explored = self
            .explore(FigmaExploreInput {
                screenshot_observations: None,
                url: input.url.clone(),
                limit: MATCH_CEILING,
                include_text_preview: false,
                refresh: input.refresh,
            })
            .await?;
        let candidates = explored["candidates"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let screens_only = candidates
            .iter()
            .any(|candidate| candidate["node"]["kind"] == "screen");
        let screens = candidates
            .iter()
            .map(|candidate| &candidate["node"])
            .filter(|node| !screens_only || node["kind"] == "screen")
            .filter_map(screen_plan::screen_from_explore)
            .collect();
        // Nodes beside the screens - a logo, a frame named og - are the
        // favicon and share image candidates the site questions offer.
        let assets = candidates
            .iter()
            .map(|candidate| &candidate["node"])
            .filter(|node| screens_only && node["kind"] != "screen")
            .filter_map(|node| {
                Some(json!({"nodeId": node["nodeId"].as_str()?, "name": node["name"].as_str()?}))
            })
            .collect::<Vec<_>>();
        Ok((
            screens,
            json!({
                "assetCandidates": assets,
                "from": "devup_figma_explore",
                "url": input.url,
                "nodeId": explored["anchor"]["nodeId"],
                "anchorName": explored["anchor"]["name"],
                "anchorBreadcrumb": explored["anchor"]["breadcrumb"],
                "nodePage": explored["nodePage"],
                "targetKind": explored["targetKind"],
                "fileKey": explored["source"]["fileKey"],
                "truncation": explored["truncation"],
            }),
        ))
    }

    async fn connection_report(&self) -> Result<Value, DevupError> {
        let status = self.services.auth.status().await?;
        let direct = self.services.auth.direct_path_snapshot().await?;
        let bridge = self.services.upstream.bridge_path_snapshot().await;
        Ok(diagnostics::connection_report(
            status,
            &direct,
            bridge.as_ref(),
        ))
    }

    /// Binds a call made without url, or with `figma-bridge://current`, to the
    /// one attached Devup Bridge plugin: its file, and the node selected in
    /// Figma when the call names none.
    ///
    /// A call without url used to be refused outright as "url or artifactId
    /// is required" while a plugin sat attached and ready, and the one agent
    /// that met it invented `/design/bridge/bridge` to get past the field.
    /// With no plugin, or several, there is no one file the call can mean,
    /// and the refusal says what would make one - `retry` is the call to make
    /// then.
    async fn bind_to_bridge(
        &self,
        mut target: FigmaTarget,
        unnamed: Unnamed,
        retry: &Value,
    ) -> Result<FigmaTarget, DevupError> {
        if target.file_key != BRIDGE_CURRENT_KEY {
            return Ok(target);
        }
        let bridge = self.services.upstream.bridge_path_snapshot().await;
        let attached = bridge
            .as_ref()
            .map_or(&[][..], |bridge| bridge.attached_files.as_slice());
        let mut file = match attached {
            [file] => file.clone(),
            [] => {
                let direct_available = self.services.auth.status().await? == AuthStatus::Connected;
                return Err(DevupError::with_details(
                    ErrorCode::DevupFigmaHandoffInvalid,
                    include_str!("../text/mod-bind_to_bridge-3d006854.md"),
                    false,
                    json!({
                        "stage": "target-resolution",
                        "bridge": {
                            "listening": bridge
                                .as_ref()
                                .is_some_and(|bridge| bridge.role == BridgeRole::Host),
                            "role": bridge.as_ref().map(|bridge| match bridge.role {
                                BridgeRole::Host => "host",
                                BridgeRole::Relay => "relay",
                                BridgeRole::Connecting => "connecting",
                                BridgeRole::Unavailable => "unavailable",
                            }),
                            "issue": bridge
                                .as_ref()
                                .and_then(|bridge| bridge.issue.as_ref())
                                .map(|issue| issue.code()),
                            "port": bridge.as_ref().and_then(|bridge| bridge.port),
                            "attachedFiles": [],
                        },
                        "directAvailable": direct_available,
                        "nextAction": diagnostics::ways_to_open_a_path(
                            bridge.as_ref(),
                            direct_available,
                            retry,
                        ),
                    }),
                ));
            }
            several => {
                return Err(DevupError::with_details(
                    ErrorCode::DevupFigmaHandoffInvalid,
                    "No url was given and several Devup Bridge plugins are attached, so which file \
                     is meant is ambiguous.",
                    false,
                    json!({
                        "stage": "target-resolution",
                        "nextAction": diagnostics::several_plugins(several, retry),
                    }),
                ));
            }
        };
        target.file_key.clone_from(&file.target_key);
        if target.node_id.is_some() || unnamed == Unnamed::File {
            return Ok(target);
        }
        let last_delivered = file.context.clone();
        let refreshed = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.services
                .upstream
                .refresh_bridge_context(&file.target_key),
        )
        .await;
        match refreshed {
            Ok(Ok(context)) => file.context = context,
            _ => {
                let mut unnamed_file = file.clone();
                unnamed_file.context.selection = None;
                let mut error = selection_refusal(&file,
                    "Current Bridge selection could not be refreshed. Last-known context is diagnostic only; no node was exported.".into(),name_the_node(&unnamed_file,retry));
                error.details["selectionFreshness"] = json!({"state":"refresh-failed","current":null,"lastKnown":last_delivered,
                    "nextStep":"Reload an up-to-date Bridge plugin or pass an explicit node link; the last-known node is never selected automatically."});
                return Err(error);
            }
        }
        let context = &file.context;
        match (context.single_selection(), unnamed) {
            (Some(node), Unnamed::SelectedSection)
                if node
                    .node_type
                    .as_deref()
                    .is_some_and(|node_type| node_type != "SECTION") =>
            {
                let mut call = retry.clone();
                if let Some(first) = retry["arguments"]["frameIds"].get(0) {
                    call["arguments"]["frameIds"] = json!([first]);
                }
                call["how"] = json!(
                    "Select the Section in Figma and repeat the call, or export one frame per call: a single frameId and no url, once for each frame."
                );
                Err(selection_refusal(
                    &file,
                    format!(
                        "frameIds and allScreens without url choose screens inside the Section \
                         selected in Figma, and the selection is a {}.",
                        node.node_type.as_deref().unwrap_or("node")
                    ),
                    call,
                ))
            }
            (Some(node), _) => {
                target.node_id = Some(node.id.clone());
                Ok(target)
            }
            (None, Unnamed::SelectionOrFile)
                if context.selection.as_ref().is_some_and(Vec::is_empty) =>
            {
                Ok(target)
            }
            (None, _) => {
                let mut error = selection_refusal(
                    &file,
                    match &context.selection {
                        None => include_str!("../text/mod-bind_to_bridge-6dea64b3.md").to_owned(),
                        Some(selection) if selection.is_empty() => {
                            "No url or node was given and nothing is selected in Figma. Select the \
                         node in Figma, or name it."
                                .to_owned()
                        }
                        Some(selection) => format!(
                            "No url or node was given and {} nodes are selected in Figma. Select \
                         exactly one, or name the one you mean.",
                            context.selection_count.unwrap_or(selection.len())
                        ),
                    },
                    name_the_node(&file, retry),
                );
                error.details["selectionFreshness"] =
                    json!({"state":"current","current":file.context,"lastKnown":last_delivered});
                if context.selection.as_ref().is_some_and(Vec::is_empty)
                    && let Some(page) = &context.current_page
                {
                    let observations = retry["arguments"]
                        .get("screenshotObservations")
                        .filter(|v| !v.is_null())
                        .and_then(|v| {
                            serde_json::from_value::<
                                    screenshot_observations::ScreenshotObservations,
                                >(v.clone())
                                .ok()
                        });
                    let anchor = FigmaTarget {
                        node_id: Some(page.id.clone()),
                        ..target.clone()
                    };
                    error.details["exploreSuggestion"] = json!({"tool":"devup_figma_explore","arguments":{"url":anchor.link(Some(&page.id)),"limit":10},
                            "basis":"current-page-context","screenshotMatch":"unverified","evidenceLimit":"No screenshot was inspected. Candidates use actual Figma page structure and geometry only."});
                    if let Some(hints) = &observations {
                        error.details["exploreSuggestion"]["arguments"]["screenshotObservations"] =
                            json!(hints);
                        error.details["exploreSuggestion"]["basis"] =
                            json!("caller-observations-and-current-page-evidence");
                    }
                    let call = ReadToolCall::explore_snapshot(
                        &target.file_key,
                        &page.id,
                        ExploreReadOptions {
                            projection_limit: EXPLORE_PROJECTION_LIMIT,
                            text_preview_limit: if observations.is_some() { 160 } else { 0 },
                        },
                    );
                    if let Ok(Ok(read)) = tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        self.services.upstream.call_read_tool(call),
                    )
                    .await
                        && let Ok(chunk) = devup_mcp_figma::snapshot_chunk_from_result(&read)
                        && let Ok(snapshot) = devup_mcp_figma::merge_chunks(vec![chunk])
                        && let Ok(candidates) = devup_mcp_figma::explore_snapshot(
                            &snapshot,
                            &anchor,
                            &devup_mcp_figma::ExploreOptions {
                                limit: MATCH_CEILING,
                            },
                        )
                    {
                        error.details["exploreSuggestion"]["candidates"] =
                            json!(candidates.candidates);
                        error.details["exploreSuggestion"]["projectionTruncated"] =
                            json!(candidates.truncated);
                        screenshot_observations::rank(
                            &mut error.details["exploreSuggestion"],
                            observations.as_ref(),
                        );
                        let found = error.details["exploreSuggestion"]["candidates"]
                            .as_array()
                            .map_or(0, Vec::len);
                        error.details["exploreSuggestion"]["foundCandidates"] = json!(found);
                        error.details["exploreSuggestion"]["candidatesTruncated"] =
                            json!(found > 10);
                        if let Some(candidates) =
                            error.details["exploreSuggestion"]["candidates"].as_array_mut()
                        {
                            candidates.truncate(10);
                        }
                    }
                }
                Err(error)
            }
        }
    }
}

/// The call that names the node a selection could not: the first selected
/// one when anything is selected, a placeholder to replace otherwise.
fn name_the_node(file: &devup_mcp_figma::AttachedFile, retry: &Value) -> Value {
    let node = file
        .context
        .selection
        .as_deref()
        .and_then(<[_]>::first)
        .map(|node| node.id.as_str());
    let mut call = retry.clone();
    call["arguments"]["url"] =
        json!(FigmaTarget::bridge_current().link(Some(node.unwrap_or("<node-id>"))));
    if node.is_none() {
        call["requiredArguments"] = json!(["url"]);
    }
    call["how"] = json!(
        "Select one node in Figma and repeat the call, or name the node with figma-bridge://current?node-id=<id> as url."
    );
    call
}

/// Which node a call bound to the bridge means when it names none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unnamed {
    /// No node: the whole file, as a search reads it.
    File,
    /// The one node selected in Figma: an export's target, explore's anchor.
    Selection,
    /// The selected node when exactly one is selected, else the whole file:
    /// a file-scope export that needs no node.
    SelectionOrFile,
    /// The selected Section, whose screens frameIds or allScreens choose.
    SelectedSection,
}

/// The refusal for a selection that does not name the node a call needs,
/// carrying the call that would.
fn selection_refusal(
    file: &devup_mcp_figma::AttachedFile,
    message: String,
    call: Value,
) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaHandoffInvalid,
        message,
        false,
        json!({
            "stage": "target-resolution",
            "file": {
                "fileName": file.file_name,
                "currentPage": file.context.current_page,
                "selection": file.context.selection,
                "selectionCount": file.context.selection_count,
                "lastKnownSelection":file.context.last_known_selection,
                "lastKnownPage":file.context.last_known_page,
                "observedAt":file.context.observed_at,
            },
            "nextAction": call,
        }),
    )
}

/// Every `devup_figma_*` tool response is a JSON object whose exact shape
/// varies per operation, `status`, and `delivery` mode (see README). Rather
/// than pin a schema per branch that would need constant re-syncing, this
/// declares only what the MCP spec requires of `outputSchema` — root type
/// `object` (SEP-2106) — with no constraint on properties.
///
/// This replaces `schema_for_output::<serde_json::Value>()`, whose root
/// schema had no `type` field at all: schemars maps `Value` to the boolean
/// JSON Schema `true`, and `into_root_schema_for` normalizes that to the
/// empty object `{}` before `outputSchema` strips `title`/`description`,
/// leaving a schema that satisfies "is an object" as a JSON value but not
/// the MCP-mandated `"type": "object"` marker.
fn permissive_object_output_schema() -> Arc<JsonObject> {
    let mut schema = JsonObject::new();
    schema.insert("type".to_owned(), json!("object"));
    Arc::new(schema)
}

/// What `devup_figma_search` was pointed at, and how much of it to answer
/// with. `node_id` is read from the URL rather than from a parameter of its
/// own: a caller who links a node has already said where to look, and a
/// search that ignores that says nothing about having ignored it.
struct SearchScope {
    node_id: Option<String>,
    limit: usize,
    /// The same file with no node linked - the call that widens the search.
    file_url: String,
}

/// Whether `node_id` is `ancestor_id` itself or sits beneath it, following the
/// parent chain the collection recorded.
///
/// The search projection keeps each match's ancestors up to its page, so the
/// chain of anything that matched is present even when the ancestors did not
/// match themselves. A node with no recorded chain to `ancestor_id` is out of
/// scope, which is the answer wanted when the linked node holds nothing.
fn is_within(snapshot: &Snapshot, node_id: &str, ancestor_id: &str) -> bool {
    if node_id == ancestor_id {
        return true;
    }
    let parent_of = |id: &str| {
        snapshot
            .nodes
            .get(id)
            .and_then(|node| node.typed_view().string("parentId"))
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut current = parent_of(node_id);
    while let Some(parent) = current {
        if parent == ancestor_id {
            return true;
        }
        if !seen.insert(parent.to_owned()) {
            return false;
        }
        current = parent_of(parent);
    }
    false
}

/// Cuts the ranked match list down to what the caller linked and says so.
///
/// Report the stored artifact capability for both fresh and cached reads.
fn apply_search_scope(
    mut response: Value,
    snapshot: &Snapshot,
    collection_scope: CollectionScope,
    scope: Option<&SearchScope>,
) -> Value {
    let Some(scope) = scope else {
        return response;
    };
    let Some(Value::Array(matches)) = response.get_mut("matches").map(Value::take) else {
        return response;
    };
    let scanned = matches.len();
    let node_id = match scope.node_id.as_deref() {
        None => {
            let mut kept = matches;
            kept.truncate(scope.limit);
            response["count"] = json!(kept.len());
            response["matches"] = Value::Array(kept);
            response["scope"] = json!({
                "kind": "file",
                "nodeId": Value::Null,
                "collectionScope": collection_scope,
                "scannedMatches": scanned,
                "note": "The URL carried no node-id, so the whole file was searched. \
                         Put the node you care about in the URL to search only inside it.",
            });
            return response;
        }
        Some(node_id) => node_id,
    };

    let (mut inside, outside): (Vec<Value>, Vec<Value>) = matches.into_iter().partition(|entry| {
        entry["nodeId"]
            .as_str()
            .is_some_and(|id| is_within(snapshot, id, node_id))
    });
    let found = inside.len();
    inside.truncate(scope.limit);
    let mut scope_report = json!({
        "kind": "node",
        "nodeId": node_id,
        "collectionScope": collection_scope,
        "scannedMatches": scanned,
        "matchedInScope": found,
        "returned": inside.len(),
        "excludedOutOfScope": outside.len(),
        "candidatesTruncated": found > inside.len(),
        "note": "The URL carried a node-id, so only matches inside that node are \
                 returned. Remove the node-id to search elsewhere in the file.",
    });
    if inside.is_empty() {
        scope_report["nextAction"] = json!({
            "reason": "Nothing inside the linked node matched. Search the whole file \
                       with the node-id removed to look beyond this scope.",
            "example": {
                "url": scope.file_url,
                "note": "The same call with the node-id removed from the URL.",
            },
        });
        if !outside.is_empty() {
            let mut elsewhere = outside;
            elsewhere.truncate(scope.limit);
            response["outOfScopeMatches"] = Value::Array(elsewhere);
        }
    }
    response["count"] = json!(inside.len());
    response["matches"] = Value::Array(inside);
    response["scope"] = scope_report;
    response
}
