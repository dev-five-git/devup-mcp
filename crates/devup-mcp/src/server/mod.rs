pub mod artifacts;
pub mod handoff;
mod tools;

use std::sync::Arc;

use async_trait::async_trait;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{ErrorCode as McpErrorCode, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use serde_json::{Map, Value, json};

use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, RootLayout, generate_component},
    theme::{ThemeScope, generate_devup_json, variable_snapshot_from_result},
};
use devup_mcp_figma::{
    AuthStatus, CollectedParts, CollectedPayload, CollectionRequest, CollectionScope,
    CollectorSession, CollectorStep, CompletenessState, CredentialStore, DevupError, ErrorCode,
    ExploreOptions, ExploreReadOptions, FigmaTarget, FigmaUpstream, KeyringCredentialStore,
    OAuthManager, RemoteFigmaClient, ResourceScope, SearchOptions, SearchReadOptions, SourcePolicy,
    SystemBrowser, TargetKind, classify_target, explore_snapshot, fallback_allowed_for_error,
    search_snapshot,
};

use artifacts::{ArtifactLookup, ArtifactRequestKey, ArtifactStore};
use handoff::{HandoffStep, HandoffStore, PendingOperation};

pub use tools::{
    AuthInput, ContinueInput, FigmaExploreInput, FigmaExportInput, FigmaSearchInput,
    FigmaToJsonInput, FigmaToUiInput,
};

const FIGMA_ENDPOINT: &str = "https://mcp.figma.com/mcp";

#[async_trait]
pub trait DevupAuth: Send + Sync {
    async fn status(&self) -> Result<AuthStatus, DevupError>;
    async fn login(&self) -> Result<AuthStatus, DevupError>;
    async fn logout(&self) -> Result<AuthStatus, DevupError>;
}

#[async_trait]
impl<S: CredentialStore> DevupAuth for OAuthManager<S> {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        OAuthManager::status(self).await
    }

    async fn login(&self) -> Result<AuthStatus, DevupError> {
        OAuthManager::login(self, &SystemBrowser).await?;
        Ok(AuthStatus::Connected)
    }

    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        OAuthManager::logout(self).await?;
        Ok(AuthStatus::Disconnected)
    }
}

#[derive(Clone)]
pub struct Services {
    auth: Arc<dyn DevupAuth>,
    upstream: Arc<dyn FigmaUpstream>,
}

impl Services {
    pub fn new(auth: Arc<dyn DevupAuth>, upstream: Arc<dyn FigmaUpstream>) -> Self {
        Self { auth, upstream }
    }

    fn production() -> Self {
        let oauth = OAuthManager::with_endpoint(FIGMA_ENDPOINT, KeyringCredentialStore);
        let upstream = RemoteFigmaClient::new(oauth.clone());
        Self::new(Arc::new(oauth), Arc::new(upstream))
    }
}

#[derive(Clone)]
pub struct DevupServer {
    tool_router: ToolRouter<Self>,
    services: Services,
    handoffs: HandoffStore,
    artifacts: ArtifactStore,
}

impl DevupServer {
    pub fn new(services: Services) -> Self {
        Self {
            tool_router: Self::tool_router(),
            services,
            handoffs: HandoffStore::default(),
            artifacts: ArtifactStore::default(),
        }
    }
}

impl Default for DevupServer {
    fn default() -> Self {
        Self::new(Services::production())
    }
}

impl DevupServer {
    async fn start_operation(
        &self,
        operation: PendingOperation,
        request: CollectionRequest,
        policy: SourcePolicy,
        refresh: bool,
    ) -> Result<Value, DevupError> {
        let artifact_key = ArtifactRequestKey::from_collection(&request, policy);
        if !refresh && let Some(artifact) = self.artifacts.lookup(&artifact_key).await {
            return complete_operation(operation, &artifact.payload, "artifact", &artifact);
        }
        if policy == SourcePolicy::Host {
            return self.begin_handoff(operation, request, artifact_key).await;
        }

        let auth_status = self.services.auth.status().await?;
        if auth_status == AuthStatus::Disconnected {
            if policy == SourcePolicy::Auto {
                return self.begin_handoff(operation, request, artifact_key).await;
            }
            return Err(DevupError::with_details(
                ErrorCode::DevupAuthRequired,
                "Figma direct 연결을 사용하려면 devup_figma_auth login이 필요합니다.",
                false,
                json!({"source": "direct"}),
            ));
        }

        match self
            .artifacts
            .get_or_acquire(artifact_key.clone(), refresh, || async {
                CollectedPayload::try_from(self.run_direct(request.clone()).await?)
            })
            .await
        {
            Ok(artifact) => complete_operation(operation, &artifact.payload, "direct", &artifact),
            Err(error) if fallback_allowed_for_error(policy, &error) => {
                self.begin_handoff(operation, request, artifact_key).await
            }
            Err(error) => Err(error),
        }
    }

    async fn run_direct(&self, request: CollectionRequest) -> Result<CollectedParts, DevupError> {
        let mut collector = CollectorSession::new(request);
        loop {
            match collector.advance()? {
                CollectorStep::Call(planned) => {
                    let call_id = planned.id.clone();
                    match self.services.upstream.call_read_tool(planned.call).await {
                        Ok(result) => collector.accept(&call_id, result)?,
                        Err(error) if collector.reject(&call_id, &error)? => continue,
                        Err(error) => return Err(error),
                    }
                }
                CollectorStep::AwaitingResults => continue,
                CollectorStep::Complete(parts) => return Ok(*parts),
            }
        }
    }

    async fn begin_handoff(
        &self,
        operation: PendingOperation,
        request: CollectionRequest,
        artifact_key: ArtifactRequestKey,
    ) -> Result<Value, DevupError> {
        let session_id = self
            .handoffs
            .begin_with_artifact(
                operation,
                CollectorSession::new(request),
                Some(artifact_key),
            )
            .await?;
        let step = self.handoffs.next(&session_id).await?;
        self.handoff_step_to_value(step, "host").await
    }

    async fn handoff_step_to_value(
        &self,
        step: HandoffStep,
        source: &str,
    ) -> Result<Value, DevupError> {
        match step {
            HandoffStep::NeedsFigma {
                session_id,
                expires_at_epoch_seconds,
                calls,
            } => Ok(json!({
                "status": "needs_figma",
                "sessionId": session_id,
                "expiresAt": format_epoch_rfc3339(expires_at_epoch_seconds),
                "calls": calls,
                "resumeTool": "devup_figma_continue"
            })),
            HandoffStep::Complete { operation, parts } => {
                let PendingOperation::Artifact {
                    operation,
                    artifact_key,
                } = operation
                else {
                    return Err(DevupError::new(
                        ErrorCode::DevupFigmaHandoffInvalid,
                        "Figma handoff artifact key가 없습니다.",
                        false,
                    ));
                };
                let payload = CollectedPayload::try_from(*parts)?;
                let artifact = self.artifacts.insert(artifact_key, payload).await?;
                complete_operation(*operation, &artifact.payload, source, &artifact)
            }
        }
    }
}

#[tool_router]
impl DevupServer {
    #[tool(description = "Check, start, or clear Figma Remote MCP OAuth")]
    async fn devup_figma_auth(
        &self,
        Parameters(input): Parameters<AuthInput>,
    ) -> Result<Json<Value>, ErrorData> {
        let status = match input.action.as_str() {
            "status" => self.services.auth.status().await,
            "login" => self.services.auth.login().await,
            "logout" => self.services.auth.logout().await,
            _ => {
                return Err(to_mcp_error(DevupError::new(
                    ErrorCode::DevupAuthRequired,
                    "action은 status, login 또는 logout이어야 합니다.",
                    false,
                )));
            }
        }
        .map_err(to_mcp_error)?;
        Ok(Json(json!({ "status": status })))
    }

    #[tool(description = "Convert a Figma design link to deterministic DevupUI TypeScript")]
    async fn devup_figma_to_ui(
        &self,
        Parameters(input): Parameters<FigmaToUiInput>,
    ) -> Result<Json<Value>, ErrorData> {
        let target = FigmaTarget::parse(&input.url).map_err(to_mcp_error)?;
        target.node_id.as_ref().ok_or_else(|| {
            to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "UI 변환 링크에는 node-id가 필요합니다.",
                false,
            ))
        })?;
        let policy = parse_source_policy(&input.source_policy).map_err(to_mcp_error)?;
        let scope = parse_collection_scope(&input.scope).map_err(to_mcp_error)?;
        let root_layout = parse_root_layout(&input.root_layout).map_err(to_mcp_error)?;
        let mut request = CollectionRequest::new(target, scope);
        request.resource_scope = ResourceScope::Used;
        let result = self
            .start_operation(
                PendingOperation::ToUi {
                    component_name: input.component_name,
                    include_diagnostics: input.include_diagnostics,
                    root_layout,
                    output_path: input.output_path,
                },
                request,
                policy,
                false,
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(Json(result))
    }

    #[tool(description = "Convert Figma variables and styles to deterministic devup.json")]
    async fn devup_figma_to_json(
        &self,
        Parameters(input): Parameters<FigmaToJsonInput>,
    ) -> Result<Json<Value>, ErrorData> {
        let target = FigmaTarget::parse(&input.url).map_err(to_mcp_error)?;
        parse_scope(&input.scope).map_err(to_mcp_error)?;
        let policy = parse_source_policy(&input.source_policy).map_err(to_mcp_error)?;
        let collection_scope = parse_collection_scope(&input.scope).map_err(to_mcp_error)?;
        let mut request = CollectionRequest::new(target, collection_scope);
        if collection_scope == CollectionScope::File {
            request.resource_scope = ResourceScope::File;
            request.variables_only = true;
        } else {
            request.resource_scope = ResourceScope::Used;
        }
        let result = self
            .start_operation(
                PendingOperation::ToJson {
                    scope: input.scope,
                    include_diagnostics: input.include_diagnostics,
                    output_path: input.output_path,
                },
                request,
                policy,
                false,
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(Json(result))
    }

    #[tool(description = "Search Figma pages, sections, frames, and components by name")]
    async fn devup_figma_search(
        &self,
        Parameters(input): Parameters<FigmaSearchInput>,
    ) -> Result<Json<Value>, ErrorData> {
        let target = FigmaTarget::parse(&input.url).map_err(to_mcp_error)?;
        let policy = parse_source_policy(&input.source_policy).map_err(to_mcp_error)?;
        let mut request = CollectionRequest::new(target, CollectionScope::File);
        request.search = Some(SearchReadOptions {
            query: input.query.clone(),
            node_types: input.node_types.clone(),
            match_kind: input.match_kind.clone(),
            limit: input.limit,
        });
        let result = self
            .start_operation(
                PendingOperation::Search {
                    query: input.query,
                    node_types: input.node_types,
                    match_kind: input.match_kind,
                    limit: input.limit,
                },
                request,
                policy,
                false,
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(Json(result))
    }

    #[tool(description = "Explore screen candidates spatially related to a linked Figma node")]
    async fn devup_figma_explore(
        &self,
        Parameters(input): Parameters<FigmaExploreInput>,
    ) -> Result<Json<Value>, ErrorData> {
        let target = FigmaTarget::parse(&input.url).map_err(to_mcp_error)?;
        target.node_id.as_ref().ok_or_else(|| {
            to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "Figma 주변 화면 탐색에는 node-id가 필요합니다.",
                false,
            ))
        })?;
        if !(1..=100).contains(&input.limit) {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "탐색 limit은 1 이상 100 이하여야 합니다.",
                false,
            )));
        }
        let policy = parse_source_policy(&input.source_policy).map_err(to_mcp_error)?;
        let mut request = CollectionRequest::new(target, CollectionScope::Node);
        request.resource_scope = ResourceScope::None;
        request.explore = Some(ExploreReadOptions {
            projection_limit: input.limit.saturating_mul(4).clamp(50, 400),
            text_preview_limit: if input.include_text_preview { 160 } else { 0 },
        });
        let result = self
            .start_operation(
                PendingOperation::Explore { limit: input.limit },
                request,
                policy,
                false,
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(Json(result))
    }

    #[tool(description = "Continue a read-only Figma host handoff with an official MCP result")]
    async fn devup_figma_continue(
        &self,
        Parameters(input): Parameters<ContinueInput>,
    ) -> Result<Json<Value>, ErrorData> {
        self.handoffs
            .accept(&input.session_id, &input.call_id, input.result)
            .await
            .map_err(to_mcp_error)?;
        let step = self
            .handoffs
            .next(&input.session_id)
            .await
            .map_err(to_mcp_error)?;
        Ok(Json(
            self.handoff_step_to_value(step, "host")
                .await
                .map_err(to_mcp_error)?,
        ))
    }

    #[tool(description = "Acquire a Figma design once and project multiple DevupUI artifacts")]
    async fn devup_figma_export(
        &self,
        Parameters(input): Parameters<FigmaExportInput>,
    ) -> Result<Json<Value>, ErrorData> {
        validate_outputs(&input.outputs).map_err(to_mcp_error)?;
        let root_layout = parse_root_layout(&input.root_layout).map_err(to_mcp_error)?;
        parse_scope(&input.scope).map_err(to_mcp_error)?;

        if let Some(artifact_id) = input.artifact_id.as_deref() {
            if input.url.is_some() || input.refresh {
                return Err(to_mcp_error(DevupError::new(
                    ErrorCode::DevupFigmaHandoffInvalid,
                    "artifactId는 url 또는 refresh와 함께 사용할 수 없습니다.",
                    false,
                )));
            }
            let artifact = self.artifacts.get(artifact_id).await.ok_or_else(|| {
                to_mcp_error(DevupError::new(
                    ErrorCode::DevupFigmaHandoffExpired,
                    "Figma artifact가 없거나 만료되었습니다.",
                    true,
                ))
            })?;
            let result = complete_operation(
                PendingOperation::Export {
                    outputs: input.outputs,
                    component_name: input.component_name,
                    include_diagnostics: input.include_diagnostics,
                    root_layout,
                    scope: input.scope,
                    strict: input.strict,
                    output_paths: input.output_paths,
                    frame_ids: input.frame_ids,
                    all_screens: input.all_screens,
                },
                &artifact.payload,
                "artifact",
                &artifact,
            )
            .map_err(to_mcp_error)?;
            return Ok(Json(result));
        }

        let url = input.url.as_deref().ok_or_else(|| {
            to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaHandoffInvalid,
                "url 또는 artifactId 중 하나가 필요합니다.",
                false,
            ))
        })?;
        let target = FigmaTarget::parse(url).map_err(to_mcp_error)?;
        if input.outputs.iter().any(|output| output == "tsx") && target.node_id.is_none() {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "TSX export 링크에는 node-id가 필요합니다.",
                false,
            )));
        }
        let policy = parse_source_policy(&input.source_policy).map_err(to_mcp_error)?;
        let collection_scope = parse_collection_scope(&input.scope).map_err(to_mcp_error)?;
        let mut request = CollectionRequest::new(target, collection_scope);
        request.resource_scope = if collection_scope == CollectionScope::File {
            ResourceScope::File
        } else {
            ResourceScope::Used
        };
        request.variables_only = collection_scope == CollectionScope::File
            && input.outputs.iter().all(|output| output == "devupJson");
        let result = self
            .start_operation(
                PendingOperation::Export {
                    outputs: input.outputs,
                    component_name: input.component_name,
                    include_diagnostics: input.include_diagnostics,
                    root_layout,
                    scope: input.scope,
                    strict: input.strict,
                    output_paths: input.output_paths,
                    frame_ids: input.frame_ids,
                    all_screens: input.all_screens,
                },
                request,
                policy,
                input.refresh,
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(Json(result))
    }
}

fn complete_operation(
    operation: PendingOperation,
    payload: &CollectedPayload,
    source_kind: &str,
    artifact: &ArtifactLookup,
) -> Result<Value, DevupError> {
    let collection = payload.stats.clone();
    let completeness_report = payload.completeness_report();
    let status = match completeness_report.state {
        CompletenessState::Complete => "complete",
        CompletenessState::Partial => "partial",
        CompletenessState::Failed => "failed",
    };
    match operation {
        PendingOperation::ToUi {
            component_name,
            include_diagnostics,
            root_layout,
            output_path,
        } => {
            let node_id = payload.target.node_id.as_deref().ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupFigmaNodeNotFound,
                    "UI 변환 payload에는 node ID가 필요합니다.",
                    false,
                )
            })?;
            let output = generate_component(
                &payload.snapshot,
                node_id,
                &CodegenOptions {
                    component_name,
                    include_diagnostics,
                    inline_instances: true,
                    root_layout,
                    ..CodegenOptions::default()
                }
                .with_payload_tokens(payload),
            )?;
            let diagnostics = if include_diagnostics {
                output.diagnostics
            } else {
                Vec::new()
            };
            let written_path = output_path
                .as_deref()
                .map(|path| write_output(path, &output.tsx))
                .transpose()?;
            Ok(json!({
                "status": status,
                "tsx": output.tsx,
                "imports": output.imports,
                "usedTokens": output.used_tokens,
                "diagnostics": diagnostics,
                "outputPath": written_path,
                "completeness": payload.completeness,
                "completenessReport": &completeness_report,
                "rootLayout": root_layout,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "nodeId": node_id,
                    "version": payload.snapshot.version
                },
                "snapshot": {
                    "preservedNodeCount": payload.snapshot.nodes.len(),
                    "fieldErrorCount": payload.snapshot.nodes.values()
                        .map(|node| node.field_errors.len()).sum::<usize>()
                }
            }))
        }
        PendingOperation::ToJson {
            scope,
            include_diagnostics,
            output_path,
        } => {
            let result = payload.variables.as_ref().ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "Figma 변수/style 수집 결과가 없습니다.",
                    false,
                )
            })?;
            let variables = variable_snapshot_from_result(result)?;
            let output = generate_devup_json(&variables, parse_scope(&scope)?)?;
            let diagnostics = if include_diagnostics {
                output.diagnostics
            } else {
                Vec::new()
            };
            let written_path = output_path
                .as_deref()
                .map(|path| write_output(path, &output.json))
                .transpose()?;
            Ok(json!({
                "status": status,
                "devupJson": output.json,
                "counts": output.counts,
                "completeness": output.completeness,
                "completenessReport": &completeness_report,
                "conflicts": output.conflicts,
                "unresolvedVariables": output.unresolved_variables,
                "diagnostics": diagnostics,
                "outputPath": written_path,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "nodeId": payload.target.node_id,
                    "version": payload.snapshot.version
                }
            }))
        }
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
            Ok(json!({
                "status": status,
                "query": query,
                "count": matches.len(),
                "matches": matches,
                "completeness": payload.completeness,
                "completenessReport": &completeness_report,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "version": payload.snapshot.version
                }
            }))
        }
        PendingOperation::Explore { limit } => {
            let result = explore_snapshot(
                &payload.snapshot,
                &payload.target,
                &ExploreOptions { limit },
            )?;
            let count = result.candidates.len();
            Ok(json!({
                "status": status,
                "targetKind": result.target_kind,
                "anchor": result.anchor,
                "group": result.group,
                "count": count,
                "candidates": result.candidates,
                "truncated": result.truncated,
                "diagnostics": payload.snapshot.diagnostics,
                "completeness": payload.completeness,
                "completenessReport": &completeness_report,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "nodeId": payload.target.node_id,
                    "version": payload.snapshot.version
                }
            }))
        }
        PendingOperation::Export {
            outputs,
            component_name,
            include_diagnostics,
            root_layout,
            scope,
            strict,
            output_paths,
            frame_ids,
            all_screens,
        } => {
            if strict && completeness_report.state != CompletenessState::Complete {
                return Err(DevupError::with_details(
                    ErrorCode::DevupSnapshotUnsupported,
                    format!(
                        "strict export는 partial 또는 failed payload를 허용하지 않습니다: {status}"
                    ),
                    false,
                    json!({"completenessReport": completeness_report}),
                ));
            }

            let mut result = Map::new();
            result.insert("status".to_owned(), json!(status));
            result.insert("completeness".to_owned(), json!(payload.completeness));
            result.insert("completenessReport".to_owned(), json!(&completeness_report));
            result.insert("collection".to_owned(), json!(collection));
            result.insert("cache".to_owned(), artifact_metadata(artifact));
            result.insert(
                "source".to_owned(),
                json!({
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "nodeId": payload.target.node_id,
                    "version": payload.snapshot.version
                }),
            );
            let target_kind = classify_target(&payload.snapshot, &payload.target);
            result.insert("targetKind".to_owned(), json!(target_kind));

            if !frame_ids.is_empty() && all_screens {
                return Err(DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "frameIds와 allScreens는 동시에 사용할 수 없습니다.",
                    false,
                ));
            }
            if target_kind != TargetKind::Section && (!frame_ids.is_empty() || all_screens) {
                return Err(DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "frameIds와 allScreens는 Section artifact에서만 사용할 수 있습니다.",
                    false,
                ));
            }

            let section_candidates = if target_kind == TargetKind::Section
                && outputs.iter().any(|output| output == "tsx")
            {
                Some(
                    explore_snapshot(
                        &payload.snapshot,
                        &payload.target,
                        &ExploreOptions { limit: 100 },
                    )?
                    .candidates,
                )
            } else {
                None
            };
            if let Some(candidates) = &section_candidates
                && frame_ids.is_empty()
                && !all_screens
            {
                result.insert("status".to_owned(), json!("selection_required"));
                result.insert(
                    "selection".to_owned(),
                    json!({
                        "kind": "screen-frame",
                        "candidates": candidates,
                        "truncated": candidates.len() == 100
                    }),
                );
                return Ok(Value::Object(result));
            }

            let mut written_paths = Map::new();
            let mut section_tsx_projected = false;
            if let Some(candidates) = section_candidates {
                let by_id = candidates
                    .iter()
                    .map(|candidate| (candidate.node.node_id.as_str(), candidate))
                    .collect::<std::collections::BTreeMap<_, _>>();
                let selected = if all_screens {
                    candidates.iter().collect::<Vec<_>>()
                } else {
                    let mut seen = std::collections::BTreeSet::new();
                    frame_ids
                        .iter()
                        .map(|node_id| {
                            if !seen.insert(node_id.as_str()) {
                                return Err(DevupError::new(
                                    ErrorCode::DevupSnapshotUnsupported,
                                    format!("frameIds에 중복 node가 있습니다: {node_id}"),
                                    false,
                                ));
                            }
                            by_id.get(node_id.as_str()).copied().ok_or_else(|| {
                                DevupError::new(
                                    ErrorCode::DevupFigmaNodeNotFound,
                                    format!(
                                        "Section 내부 screen frame이 아니거나 존재하지 않습니다: {node_id}"
                                    ),
                                    false,
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?
                };
                let mut frames = Vec::with_capacity(selected.len());
                for (index, candidate) in selected.into_iter().enumerate() {
                    let frame_component_name = component_name.as_ref().map(|name| {
                        if frame_ids.len() <= 1 && !all_screens {
                            name.clone()
                        } else {
                            format!("{name}{}", index + 1)
                        }
                    });
                    let output = generate_component(
                        &payload.snapshot,
                        &candidate.node.node_id,
                        &CodegenOptions {
                            component_name: frame_component_name,
                            include_diagnostics,
                            inline_instances: true,
                            root_layout,
                            ..CodegenOptions::default()
                        }
                        .with_payload_tokens(payload),
                    )?;
                    let source_map = json!({
                        "version": 1,
                        "tsx": [],
                        "source": {
                            "fileKey": payload.target.file_key,
                            "rootNodeId": candidate.node.node_id,
                            "sourceVersion": payload.source_version
                        }
                    });
                    let mut frame = json!({
                        "nodeId": candidate.node.node_id,
                        "name": candidate.node.name,
                        "canonicalUrl": candidate.canonical_url,
                        "status": status,
                        "tsx": output.tsx,
                        "imports": output.imports,
                        "usedTokens": output.used_tokens,
                        "completenessReport": &completeness_report
                    });
                    if outputs.iter().any(|output| output == "sourceMap") {
                        frame["sourceMap"] = source_map;
                    }
                    if include_diagnostics {
                        frame["diagnostics"] = json!(output.diagnostics);
                    }
                    frames.push(frame);
                }
                result.insert("frames".to_owned(), Value::Array(frames));
                section_tsx_projected = true;
            }

            if outputs.iter().any(|output| output == "tsx") && !section_tsx_projected {
                let node_id = payload.target.node_id.as_deref().ok_or_else(|| {
                    DevupError::new(
                        ErrorCode::DevupFigmaNodeNotFound,
                        "TSX export payload에는 node ID가 필요합니다.",
                        false,
                    )
                })?;
                let output = generate_component(
                    &payload.snapshot,
                    node_id,
                    &CodegenOptions {
                        component_name,
                        include_diagnostics,
                        inline_instances: true,
                        root_layout,
                        ..CodegenOptions::default()
                    }
                    .with_payload_tokens(payload),
                )?;
                if let Some(path) = output_paths.get("tsx") {
                    written_paths.insert("tsx".to_owned(), json!(write_output(path, &output.tsx)?));
                }
                result.insert("tsx".to_owned(), json!(output.tsx));
                result.insert("imports".to_owned(), json!(output.imports));
                result.insert("usedTokens".to_owned(), json!(output.used_tokens));
                if include_diagnostics {
                    result.insert("diagnostics".to_owned(), json!(output.diagnostics));
                }
            }

            if outputs.iter().any(|output| output == "devupJson") {
                let variables = payload.variables.as_ref().ok_or_else(|| {
                    DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        "Figma 변수/style 수집 결과가 없습니다.",
                        false,
                    )
                })?;
                let variables = variable_snapshot_from_result(variables)?;
                let output = generate_devup_json(&variables, parse_scope(&scope)?)?;
                if let Some(path) = output_paths.get("devupJson") {
                    written_paths.insert(
                        "devupJson".to_owned(),
                        json!(write_output(path, &output.json)?),
                    );
                }
                result.insert("devupJson".to_owned(), json!(output.json));
                result.insert("themeCounts".to_owned(), json!(output.counts));
                result.insert("themeCompleteness".to_owned(), json!(output.completeness));
                result.insert("conflicts".to_owned(), json!(output.conflicts));
                result.insert(
                    "unresolvedVariables".to_owned(),
                    json!(output.unresolved_variables),
                );
                if include_diagnostics && !result.contains_key("diagnostics") {
                    result.insert("diagnostics".to_owned(), json!(output.diagnostics));
                }
            }

            if outputs.iter().any(|output| output == "rawSnapshot") {
                let raw = serde_json::to_value(&payload.snapshot).map_err(|error| {
                    DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        format!("raw snapshot을 직렬화할 수 없습니다: {error}"),
                        false,
                    )
                })?;
                if let Some(path) = output_paths.get("rawSnapshot") {
                    written_paths.insert(
                        "rawSnapshot".to_owned(),
                        json!(write_output(
                            path,
                            &serde_json::to_string_pretty(&raw).unwrap_or_default()
                        )?),
                    );
                }
                result.insert("rawSnapshot".to_owned(), raw);
            }

            if outputs.iter().any(|output| output == "sourceMap") && !section_tsx_projected {
                let source_map = json!({
                    "version": 1,
                    "tsx": [],
                    "devupJson": [],
                    "source": {
                        "fileKey": payload.target.file_key,
                        "rootNodeId": payload.target.node_id,
                        "sourceVersion": payload.source_version
                    }
                });
                if let Some(path) = output_paths.get("sourceMap") {
                    written_paths.insert(
                        "sourceMap".to_owned(),
                        json!(write_output(
                            path,
                            &serde_json::to_string_pretty(&source_map).unwrap_or_default()
                        )?),
                    );
                }
                result.insert("sourceMap".to_owned(), source_map);
            }

            if outputs.iter().any(|output| output == "assetManifest") {
                result.insert(
                    "assetManifest".to_owned(),
                    json!({"version": 1, "assets": [], "diagnostics": []}),
                );
            }
            result.insert("outputPaths".to_owned(), Value::Object(written_paths));
            Ok(Value::Object(result))
        }
        PendingOperation::Collect | PendingOperation::Artifact { .. } => Err(DevupError::new(
            ErrorCode::DevupFigmaHandoffInvalid,
            "내부 수집 operation은 MCP artifact로 완료할 수 없습니다.",
            false,
        )),
    }
}

fn artifact_metadata(artifact: &ArtifactLookup) -> Value {
    json!({
        "artifactId": artifact.artifact_id,
        "contentHash": artifact.content_hash,
        "cacheHit": artifact.cache_hit,
        "sizeBytes": artifact.size_bytes,
        "acquiredAt": format_epoch_rfc3339(artifact.created_at_epoch_seconds),
        "expiresAt": format_epoch_rfc3339(artifact.expires_at_epoch_seconds)
    })
}

fn validate_outputs(outputs: &[String]) -> Result<(), DevupError> {
    if outputs.is_empty() {
        return Err(DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "outputs는 하나 이상이어야 합니다.",
            false,
        ));
    }
    for output in outputs {
        if !matches!(
            output.as_str(),
            "tsx" | "devupJson" | "rawSnapshot" | "sourceMap" | "assetManifest"
        ) {
            return Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                format!("지원하지 않는 export output입니다: {output}"),
                false,
            ));
        }
    }
    Ok(())
}

fn parse_source_policy(policy: &str) -> Result<SourcePolicy, DevupError> {
    match policy {
        "auto" => Ok(SourcePolicy::Auto),
        "direct" => Ok(SourcePolicy::Direct),
        "host" => Ok(SourcePolicy::Host),
        _ => Err(DevupError::new(
            ErrorCode::DevupFigmaHostRequired,
            "sourcePolicy는 auto, direct 또는 host여야 합니다.",
            false,
        )),
    }
}

fn write_output(path: &str, contents: &str) -> Result<String, DevupError> {
    let path = std::path::PathBuf::from(path);
    if path.as_os_str().is_empty() || path.file_name().is_none() {
        return Err(DevupError::new(
            ErrorCode::DevupCodegenFailed,
            "outputPath는 파일 경로여야 합니다.",
            false,
        ));
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            DevupError::new(
                ErrorCode::DevupCodegenFailed,
                format!("outputPath 상위 폴더를 만들 수 없습니다: {error}"),
                false,
            )
        })?;
    }
    std::fs::write(&path, contents).map_err(|error| {
        DevupError::new(
            ErrorCode::DevupCodegenFailed,
            format!("artifact를 outputPath에 쓸 수 없습니다: {error}"),
            false,
        )
    })?;
    path.canonicalize()
        .unwrap_or(path)
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupCodegenFailed,
                "outputPath를 UTF-8 경로로 반환할 수 없습니다.",
                false,
            )
        })
}

fn parse_collection_scope(scope: &str) -> Result<CollectionScope, DevupError> {
    match scope {
        "node" => Ok(CollectionScope::Node),
        "page" => Ok(CollectionScope::Page),
        "file" => Ok(CollectionScope::File),
        _ => Err(DevupError::new(
            ErrorCode::DevupThemeConflict,
            "scope는 node, page 또는 file이어야 합니다.",
            false,
        )),
    }
}

fn parse_root_layout(root_layout: &str) -> Result<RootLayout, DevupError> {
    match root_layout {
        "standalone" => Ok(RootLayout::Standalone),
        "embedded" => Ok(RootLayout::Embedded),
        _ => Err(DevupError::new(
            ErrorCode::DevupThemeConflict,
            "rootLayout은 standalone 또는 embedded여야 합니다.",
            false,
        )),
    }
}

fn format_epoch_rfc3339(epoch_seconds: u64) -> String {
    let days = (epoch_seconds / 86_400) as i64;
    let seconds = epoch_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3_600;
    let minute = (seconds % 3_600) / 60;
    let second = seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn parse_scope(scope: &str) -> Result<ThemeScope, DevupError> {
    match scope {
        "node" => Ok(ThemeScope::Node),
        "page" => Ok(ThemeScope::Page),
        "file" => Ok(ThemeScope::File),
        _ => Err(DevupError::new(
            ErrorCode::DevupThemeConflict,
            "scope는 node, page 또는 file이어야 합니다.",
            false,
        )),
    }
}

fn to_mcp_error(error: DevupError) -> ErrorData {
    ErrorData::new(
        McpErrorCode::INTERNAL_ERROR,
        error.message,
        Some(json!({ "code": error.code, "retryable": error.retryable, "details": error.details })),
    )
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DevupServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Read Figma designs and generate DevupUI artifacts")
    }
}
