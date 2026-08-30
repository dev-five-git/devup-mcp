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
use serde_json::{Value, json};

use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    theme::{ThemeScope, generate_devup_json, variable_snapshot_from_result},
};
use devup_mcp_figma::{
    AuthStatus, CollectedParts, CollectedPayload, CollectionRequest, CollectionScope,
    CollectorSession, CollectorStep, CredentialStore, DevupError, ErrorCode, FigmaTarget,
    FigmaUpstream, KeyringCredentialStore, OAuthManager, RemoteFigmaClient, SearchOptions,
    SourcePolicy, SystemBrowser, fallback_allowed_for_error, search_snapshot,
};

use handoff::{HandoffStep, HandoffStore, PendingOperation};

pub use tools::{AuthInput, ContinueInput, FigmaSearchInput, FigmaToJsonInput, FigmaToUiInput};

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
}

impl DevupServer {
    pub fn new(services: Services) -> Self {
        Self {
            tool_router: Self::tool_router(),
            services,
            handoffs: HandoffStore::default(),
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
    ) -> Result<Value, DevupError> {
        if policy == SourcePolicy::Host {
            return self.begin_handoff(operation, request).await;
        }

        let auth_status = self.services.auth.status().await?;
        if auth_status == AuthStatus::Disconnected {
            if policy == SourcePolicy::Auto {
                return self.begin_handoff(operation, request).await;
            }
            return Err(DevupError::with_details(
                ErrorCode::DevupAuthRequired,
                "Figma direct 연결을 사용하려면 devup_figma_auth login이 필요합니다.",
                false,
                json!({"source": "direct"}),
            ));
        }

        match self.run_direct(request.clone()).await {
            Ok(parts) => complete_operation(operation, parts, "direct"),
            Err(error) if fallback_allowed_for_error(policy, &error) => {
                self.begin_handoff(operation, request).await
            }
            Err(error) => Err(error),
        }
    }

    async fn run_direct(&self, request: CollectionRequest) -> Result<CollectedParts, DevupError> {
        let mut collector = CollectorSession::new(request);
        loop {
            match collector.advance()? {
                CollectorStep::Call(planned) => {
                    let result = self.services.upstream.call_read_tool(planned.call).await?;
                    collector.accept(&planned.id, result)?;
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
    ) -> Result<Value, DevupError> {
        let session_id = self
            .handoffs
            .begin(operation, CollectorSession::new(request))
            .await?;
        let step = self.handoffs.next(&session_id).await?;
        handoff_step_to_value(step, "host")
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
        let request = CollectionRequest::new(target, scope);
        let result = self
            .start_operation(
                PendingOperation::ToUi {
                    component_name: input.component_name,
                    include_diagnostics: input.include_diagnostics,
                    output_path: input.output_path,
                },
                request,
                policy,
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
        request.include_variables = true;
        let result = self
            .start_operation(
                PendingOperation::ToJson {
                    scope: input.scope,
                    include_diagnostics: input.include_diagnostics,
                    output_path: input.output_path,
                },
                request,
                policy,
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
        let request = CollectionRequest::new(target, CollectionScope::File);
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
            handoff_step_to_value(step, "host").map_err(to_mcp_error)?,
        ))
    }
}

fn handoff_step_to_value(step: HandoffStep, source: &str) -> Result<Value, DevupError> {
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
        HandoffStep::Complete { operation, parts } => complete_operation(operation, *parts, source),
    }
}

fn complete_operation(
    operation: PendingOperation,
    parts: CollectedParts,
    source_kind: &str,
) -> Result<Value, DevupError> {
    let payload = CollectedPayload::try_from(parts)?;
    match operation {
        PendingOperation::ToUi {
            component_name,
            include_diagnostics,
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
                    ..CodegenOptions::default()
                }
                .with_payload_tokens(&payload),
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
                "status": "complete",
                "tsx": output.tsx,
                "imports": output.imports,
                "usedTokens": output.used_tokens,
                "diagnostics": diagnostics,
                "outputPath": written_path,
                "completeness": payload.completeness,
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
                "status": "complete",
                "devupJson": output.json,
                "counts": output.counts,
                "completeness": output.completeness,
                "diagnostics": diagnostics,
                "outputPath": written_path,
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
                "status": "complete",
                "query": query,
                "count": matches.len(),
                "matches": matches,
                "completeness": payload.completeness,
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "version": payload.snapshot.version
                }
            }))
        }
        PendingOperation::Collect => Err(DevupError::new(
            ErrorCode::DevupFigmaHandoffInvalid,
            "내부 수집 operation은 MCP artifact로 완료할 수 없습니다.",
            false,
        )),
    }
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
