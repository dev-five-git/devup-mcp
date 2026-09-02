pub mod artifacts;
pub mod delivery;
mod diagnostics;
pub mod handoff;
pub mod output;
mod project_context;
mod project_root;
mod projection;
mod quality;
pub mod resources;
mod stack_diff;
mod tools;
mod validation;

use std::sync::Arc;

use async_trait::async_trait;
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, ErrorCode as McpErrorCode, Implementation, JsonObject, ServerCapabilities,
        ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use serde_json::{Value, json};

use devup_mcp_devup_ui::theme::ThemeScope;
use devup_mcp_figma::{
    AuthStatus, ClientCredentialSource, ClientCredentials, CollectedParts, CollectedPayload,
    CollectionRequest, CollectionScope, CollectorSession, CollectorStep, CredentialStore,
    DevupError, DirectPathSnapshot, ErrorCode, ExploreCandidate, ExploreKind, ExploreNode,
    ExploreReadOptions, FigmaTarget, FigmaUpstream, KeyringClientCredentialStore,
    KeyringCredentialStore, OAuthManager, RemoteFigmaClient, ResourceScope, SearchReadOptions,
    SecretString, SectionCandidate, SectionIndex, SectionReadOptions, SourcePolicy, SystemBrowser,
    TokenState, fallback_allowed_for_error,
};

use artifacts::{ArtifactKind, ArtifactRequestKey, ArtifactStore};
use delivery::{DeliveryMode, tool_result};
use handoff::{HandoffStep, HandoffStore, PendingOperation};
use output::OutputPolicy;
use projection::complete_operation;
use validation::{
    parse_asset_requests, parse_collection_scope, parse_root_layout, parse_source_policy,
    validate_artifact_projection, validate_outputs,
};

pub use tools::{
    AuthInput, ContinueInput, FigmaAssetRequestInput, FigmaExploreInput, FigmaExportInput,
    FigmaSearchInput, FigmaToJsonInput, FigmaToUiInput, ProjectContextInput, StackDiffInput,
    UiValidateInput,
};

const FIGMA_ENDPOINT: &str = "https://mcp.figma.com/mcp";

#[async_trait]
pub trait DevupAuth: Send + Sync {
    async fn status(&self) -> Result<AuthStatus, DevupError>;
    async fn login(&self) -> Result<AuthStatus, DevupError>;
    async fn logout(&self) -> Result<AuthStatus, DevupError>;

    /// Backs `devup_figma_auth {"action":"doctor"}`'s `paths.direct`
    /// block. Default implementation derives a best-effort snapshot from
    /// `status()` alone so existing `DevupAuth` test doubles keep
    /// compiling without changes; `OAuthManager` overrides this with the
    /// real credential-source/token-freshness/callback-port measurement.
    async fn direct_path_snapshot(&self) -> Result<DirectPathSnapshot, DevupError> {
        let status = self.status().await?;
        Ok(DirectPathSnapshot {
            credential_source: ClientCredentialSource::default(),
            token_state: if status == AuthStatus::Connected {
                TokenState::Valid
            } else {
                TokenState::Absent
            },
            callback_port: None,
            callback_port_free: None,
        })
    }

    /// Backs `devup_figma_auth {"action":"configure"}`. Default
    /// implementation rejects: only auth backends that actually persist a
    /// client credential (namely `OAuthManager`) support this.
    async fn configure_client_credentials(
        &self,
        _client_id: String,
        _client_secret: Option<String>,
    ) -> Result<(), DevupError> {
        Err(DevupError::new(
            ErrorCode::DevupAuthRequired,
            "이 auth 백엔드는 client 자격증명 설정을 지원하지 않습니다.",
            false,
        ))
    }
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

    async fn direct_path_snapshot(&self) -> Result<DirectPathSnapshot, DevupError> {
        OAuthManager::direct_path_snapshot(self).await
    }

    async fn configure_client_credentials(
        &self,
        client_id: String,
        client_secret: Option<String>,
    ) -> Result<(), DevupError> {
        OAuthManager::configure_client_credentials(self, client_id, client_secret).await
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

    fn production(figma_direct: crate::FigmaDirectConfig) -> Self {
        let mut oauth = OAuthManager::with_endpoint(FIGMA_ENDPOINT, KeyringCredentialStore)
            .with_client_credential_store(Arc::new(KeyringClientCredentialStore));
        if figma_direct.callback_port.is_some() {
            oauth = oauth.with_callback_port(figma_direct.callback_port);
        }
        if let Some(client_id) = figma_direct.client_id {
            oauth = oauth.with_static_client_credentials(
                ClientCredentials {
                    client_id,
                    client_secret: figma_direct.client_secret.map(SecretString::new),
                },
                figma_direct.credential_source,
            );
        }
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
    output_policy: OutputPolicy,
}

impl DevupServer {
    pub fn new(services: Services) -> Self {
        Self::with_output_roots(
            services,
            vec![std::env::current_dir().expect("devup-mcp current directory")],
        )
        .expect("devup-mcp output root")
    }

    pub fn with_output_roots(
        services: Services,
        roots: Vec<std::path::PathBuf>,
    ) -> Result<Self, DevupError> {
        Ok(Self {
            tool_router: Self::tool_router(),
            services,
            handoffs: HandoffStore::default(),
            artifacts: ArtifactStore::default(),
            output_policy: OutputPolicy::from_roots(roots)?,
        })
    }

    pub fn production_with_config(
        roots: Vec<std::path::PathBuf>,
        figma_direct: crate::FigmaDirectConfig,
    ) -> Result<Self, DevupError> {
        Self::with_output_roots(Services::production(figma_direct), roots)
    }

    pub fn production_with_output_roots(
        roots: Vec<std::path::PathBuf>,
    ) -> Result<Self, DevupError> {
        Self::production_with_config(roots, crate::FigmaDirectConfig::default())
    }
}

impl Default for DevupServer {
    fn default() -> Self {
        Self::new(Services::production(crate::FigmaDirectConfig::default()))
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
            return complete_operation(
                operation,
                &artifact.payload,
                "artifact",
                &artifact,
                &self.output_policy,
                &self.artifacts,
            )
            .await;
        }
        if !refresh
            && let Some(artifact) = self.artifacts.lookup_related_explore(&artifact_key).await
        {
            return complete_operation(
                operation,
                &artifact.payload,
                "artifact",
                &artifact,
                &self.output_policy,
                &self.artifacts,
            )
            .await;
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
            Ok(artifact) => {
                complete_operation(
                    operation,
                    &artifact.payload,
                    "direct",
                    &artifact,
                    &self.output_policy,
                    &self.artifacts,
                )
                .await
            }
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
                collection,
            } => {
                let host_requirement = diagnostics::host_requirement().await;
                Ok(json!({
                    "status": "needs_figma",
                    "sessionId": session_id,
                    "expiresAt": format_epoch_rfc3339(expires_at_epoch_seconds),
                    "calls": calls,
                    "collection": collection,
                    "resumeTool": "devup_figma_continue",
                    "hostRequirement": host_requirement
                }))
            }
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
                complete_operation(
                    *operation,
                    &artifact.payload,
                    source,
                    &artifact,
                    &self.output_policy,
                    &self.artifacts,
                )
                .await
            }
        }
    }
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

#[tool_router]
impl DevupServer {
    #[tool(
        description = "Check, start, or clear Figma Remote MCP OAuth, or inject a pre-registered client credential to skip Dynamic Client Registration (action: status | login | logout | configure | doctor)",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_figma_auth(
        &self,
        Parameters(input): Parameters<AuthInput>,
    ) -> Result<CallToolResult, ErrorData> {
        if input.action == "doctor" {
            let status = self.services.auth.status().await.map_err(to_mcp_error)?;
            let direct = self
                .services
                .auth
                .direct_path_snapshot()
                .await
                .map_err(to_mcp_error)?;
            return Ok(tool_result(
                diagnostics::doctor_report(status, direct).await,
            ));
        }
        if input.action == "configure" {
            let client_id = input.client_id.ok_or_else(|| {
                to_mcp_error(DevupError::new(
                    ErrorCode::DevupInvalidInput,
                    "configure에는 clientId가 필요합니다.",
                    false,
                ))
            })?;
            self.services
                .auth
                .configure_client_credentials(client_id, input.client_secret)
                .await
                .map_err(to_mcp_error)?;
            return Ok(tool_result(json!({ "status": "configured" })));
        }
        let status = match input.action.as_str() {
            "status" => self.services.auth.status().await,
            "login" => self.services.auth.login().await,
            "logout" => self.services.auth.logout().await,
            _ => {
                return Err(to_mcp_error(DevupError::new(
                    ErrorCode::DevupAuthRequired,
                    "action은 status, login, logout, configure 또는 doctor여야 합니다.",
                    false,
                )));
            }
        }
        .map_err(to_mcp_error)?;
        Ok(tool_result(json!({ "status": status })))
    }

    #[tool(
        description = "Convert a Figma design link to deterministic DevupUI TypeScript only; use devup_figma_export when tokens or a source map are also needed, and never hand-interpret a handoff node tree",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_figma_to_ui(
        &self,
        Parameters(input): Parameters<FigmaToUiInput>,
    ) -> Result<CallToolResult, ErrorData> {
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
        let delivery = input
            .delivery
            .parse::<DeliveryMode>()
            .map_err(to_mcp_error)?;
        let mut request = CollectionRequest::new(target, scope);
        request.resource_scope = ResourceScope::Used;
        let result = self
            .start_operation(
                PendingOperation::ToUi {
                    component_name: input.component_name,
                    include_diagnostics: input.include_diagnostics,
                    root_layout,
                    output_path: input.output_path,
                    delivery,
                },
                request,
                policy,
                false,
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }

    #[tool(
        description = "Convert Figma variables and styles to deterministic devup.json",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_figma_to_json(
        &self,
        Parameters(input): Parameters<FigmaToJsonInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let target = FigmaTarget::parse(&input.url).map_err(to_mcp_error)?;
        parse_scope(&input.scope).map_err(to_mcp_error)?;
        let policy = parse_source_policy(&input.source_policy).map_err(to_mcp_error)?;
        let collection_scope = parse_collection_scope(&input.scope).map_err(to_mcp_error)?;
        let delivery = input
            .delivery
            .parse::<DeliveryMode>()
            .map_err(to_mcp_error)?;
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
                    delivery,
                },
                request,
                policy,
                false,
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }

    #[tool(
        description = "Search Figma pages, sections, frames, and components by name to locate the target before devup_figma_export",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_figma_search(
        &self,
        Parameters(input): Parameters<FigmaSearchInput>,
    ) -> Result<CallToolResult, ErrorData> {
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
        Ok(tool_result(result))
    }

    #[tool(
        description = "Explore screen candidates spatially related to a linked Figma node to locate the right screen before devup_figma_export",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_figma_explore(
        &self,
        Parameters(input): Parameters<FigmaExploreInput>,
    ) -> Result<CallToolResult, ErrorData> {
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
        let requested_target = target.clone();
        let mut request = CollectionRequest::new(target, CollectionScope::Node);
        request.resource_scope = ResourceScope::None;
        request.explore = Some(ExploreReadOptions {
            projection_limit: input.limit.saturating_mul(4).clamp(50, 400),
            text_preview_limit: if input.include_text_preview { 160 } else { 0 },
        });
        let result = self
            .start_operation(
                PendingOperation::Explore {
                    limit: input.limit,
                    target: requested_target,
                },
                request,
                policy,
                input.refresh,
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }

    #[tool(
        description = "Continue a read-only Figma host handoff: run calls[].tool with the exact arguments and pass its raw result unchanged; never substitute tools or fabricate envelope fields",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_figma_continue(
        &self,
        Parameters(input): Parameters<ContinueInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.handoffs
            .accept(&input.session_id, &input.call_id, input.result)
            .await
            .map_err(to_mcp_error)?;
        let step = self
            .handoffs
            .next(&input.session_id)
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(
            self.handoff_step_to_value(step, "host")
                .await
                .map_err(to_mcp_error)?,
        ))
    }

    #[tool(
        description = "Acquire a Figma design once and project tsx/devupJson/sourceMap/rawSnapshot together in one collection; the primary Figma-to-code entry point, preferred over devup_figma_to_ui for implementation",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_figma_export(
        &self,
        Parameters(input): Parameters<FigmaExportInput>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_outputs(&input.outputs).map_err(to_mcp_error)?;
        if !input.asset_requests.is_empty()
            && !input.outputs.iter().any(|output| output == "assetManifest")
        {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "assetRequests를 사용하려면 outputs에 assetManifest가 필요합니다.",
                false,
            )));
        }
        let reference_png_requested = input.outputs.iter().any(|output| output == "referencePng");
        if reference_png_requested && (!input.frame_ids.is_empty() || input.all_screens) {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "referencePng는 단일 Figma 링크 대상에서만 수집할 수 있습니다.",
                false,
            )));
        }
        let root_layout = parse_root_layout(&input.root_layout).map_err(to_mcp_error)?;
        parse_scope(&input.scope).map_err(to_mcp_error)?;
        let delivery = input
            .delivery
            .parse::<DeliveryMode>()
            .map_err(to_mcp_error)?;

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
            let (asset_selections, asset_output_paths) =
                parse_asset_requests(&input.asset_requests).map_err(to_mcp_error)?;
            if artifact.capabilities.kind == ArtifactKind::SectionIndex
                && (!input.frame_ids.is_empty() || input.all_screens)
            {
                let index = section_index_from_payload(&artifact.payload).ok_or_else(|| {
                    to_mcp_error(DevupError::new(
                        ErrorCode::DevupFigmaHandoffInvalid,
                        "Section index artifact payload가 올바르지 않습니다.",
                        false,
                    ))
                })?;
                let policy = parse_source_policy(&input.source_policy).map_err(to_mcp_error)?;
                let collection_scope =
                    parse_collection_scope(&input.scope).map_err(to_mcp_error)?;
                if collection_scope != CollectionScope::Node {
                    return Err(to_mcp_error(DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        "Section Frame 수집 scope는 node여야 합니다.",
                        false,
                    )));
                }
                let mut request =
                    CollectionRequest::new(artifact.payload.target.clone(), collection_scope);
                request.resource_scope = ResourceScope::Used;
                request.asset_selections = asset_selections.clone();
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
                            scope: input.scope,
                            strict: input.strict,
                            output_paths: input.output_paths,
                            frame_ids: input.frame_ids,
                            all_screens: input.all_screens,
                            asset_captures: asset_selections,
                            asset_output_paths,
                            delivery,
                        },
                        request,
                        policy,
                        false,
                    )
                    .await
                    .map_err(to_mcp_error)?;
                return Ok(tool_result(result));
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
                    scope: input.scope,
                    strict: input.strict,
                    output_paths: input.output_paths,
                    frame_ids: input.frame_ids,
                    all_screens: input.all_screens,
                    asset_captures: asset_selections,
                    asset_output_paths,
                    delivery,
                },
                &artifact.payload,
                "artifact",
                &artifact,
                &self.output_policy,
                &self.artifacts,
            )
            .await
            .map_err(to_mcp_error)?;
            return Ok(tool_result(result));
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
        let (asset_selections, asset_output_paths) =
            parse_asset_requests(&input.asset_requests).map_err(to_mcp_error)?;
        request.asset_selections = asset_selections.clone();
        request.reference_png = reference_png_requested;
        request.resource_scope = if collection_scope == CollectionScope::File {
            ResourceScope::File
        } else {
            ResourceScope::Used
        };
        request.variables_only = collection_scope == CollectionScope::File
            && input.outputs.iter().all(|output| output == "devupJson")
            && request.asset_selections.is_empty();
        if !input.frame_ids.is_empty() || input.all_screens {
            request.section = Some(SectionReadOptions {
                frame_ids: input.frame_ids.clone(),
                all_screens: input.all_screens,
            });
        }
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
                    asset_captures: asset_selections,
                    asset_output_paths,
                    delivery,
                },
                request,
                policy,
                input.refresh,
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }

    #[tool(
        description = "Read a project's real devup.json theme tokens, openapi.json endpoints/schemas, or Vespertide models/*.json tables/columns (scope: theme | api | db | all) — read-only, no session cache, never guesses",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_project_context(
        &self,
        Parameters(input): Parameters<ProjectContextInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = project_context::run(
            &input.scope,
            input.project_root.as_deref(),
            input.filter.as_deref(),
        )
        .await
        .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }

    #[tool(
        description = "Validate DevupUI TSX against a project's real devup.json: unknown $token references, hardcoded colors/lengths with a matching token, unknown props on Box/Flex/Text/Center/Grid/Image, and non-static values inside css()/globalCss()/keyframes() calls",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_ui_validate(
        &self,
        Parameters(input): Parameters<UiValidateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let theme_lookup = project_context::theme_for_validation(input.project_root.as_deref())
            .map_err(to_mcp_error)?;
        let report = devup_mcp_devup_ui::ui_validate::validate_devup_ui_tsx(
            &input.tsx,
            theme_lookup.theme.as_ref(),
            input.strict,
        );
        Ok(tool_result(json!({
            "ok": report.ok,
            "violations": report.violations,
            "checkedTokens": report.checked_tokens,
            "availableTokenCount": report.available_token_count,
            "themeAvailable": theme_lookup.theme.is_some(),
            "themeGuardrail": theme_lookup.guardrail,
        })))
    }

    #[tool(
        description = "Detect drift across the devup stack (vespertide model -> sea-orm entity -> vespera route -> openapi.json -> devup-api client); layers: db-entity | entity-route | route-openapi | openapi-client, omit for all. Text/JSON-based heuristics, not a compiler — every finding carries an explicit confidence",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_stack_diff(
        &self,
        Parameters(input): Parameters<StackDiffInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = stack_diff::run(input.project_root.as_deref(), &input.layers)
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }
}

fn section_index_from_payload(payload: &CollectedPayload) -> Option<SectionIndex> {
    serde_json::from_value(payload.metadata.get("sectionIndex")?.clone()).ok()
}

fn section_candidate_as_explore(candidate: &SectionCandidate) -> ExploreCandidate {
    ExploreCandidate {
        node: ExploreNode {
            node_id: candidate.node_id.clone(),
            name: candidate.name.clone(),
            node_type: candidate.node_type.clone(),
            bounds: candidate.bounds,
            child_count: candidate.direct_child_count,
            text_preview: String::new(),
            parent_id: candidate.parent_id.clone(),
            kind: ExploreKind::Screen,
            visible: candidate.visible,
            breadcrumb: candidate.breadcrumb.clone(),
            page_child_index: None,
        },
        canonical_url: candidate.canonical_url.clone(),
        score: 900,
        selection_reasons: candidate.selection_reasons.clone(),
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
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::new("devup-mcp", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "1. Figma 디자인을 코드로 옮기는 작업에서 devup-mcp가 1차 소스다. 다른 출처로 이를 대체하지 마라.\n\
             2. 구현이 목적이면 devup_figma_export를 먼저 호출하고 tsx, rawSnapshot, sourceMap을 함께 받아라.\n\
             3. get_design_context, 스크린샷, 시각적 추론은 검증 보조일 뿐이다. devup-mcp 출력을 덮어쓰지 마라.\n\
             4. 핸드오프로 받은 노드 트리를 직접 해석해 devup-ui 코드를 작성하지 마라. 좌표로 레이아웃을 추론하지 마라.\n\
             5. 핸드오프 단계에서는 요청된 도구를 요청된 arguments 그대로 실행하고, 원본 결과를 가공 없이 devup_figma_continue로 돌려줘라.\n\
             6. devup-mcp 호출이 실패하면 명시적으로 기록하라. 조용히 다른 방법으로 우회하지 마라.\n\
             7. 색상·간격·radius·타이포 같은 UI 수치를 추측하지 마라. 확보하지 못했으면 멈추고 보고하라.",
        )
    }

    async fn list_resources(
        &self,
        request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ListResourcesResult, ErrorData> {
        resources::list_output_resources(
            &self.artifacts,
            request
                .as_ref()
                .and_then(|request| request.cursor.as_deref()),
        )
        .await
        .map_err(to_mcp_error)
    }

    async fn list_resource_templates(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ListResourceTemplatesResult, ErrorData> {
        Ok(resources::resource_templates())
    }

    async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ReadResourceResponse, ErrorData> {
        resources::read_output_resource(&self.artifacts, &request.uri)
            .await
            .map(Into::into)
            .map_err(|_| ErrorData::resource_not_found("resource not found", None))
    }
}
