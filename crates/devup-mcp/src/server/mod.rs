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
    AuthStatus, BuiltinScript, CredentialStore, DevupError, ErrorCode, FigmaTarget, FigmaUpstream,
    KeyringCredentialStore, OAuthManager, ReadToolCall, RemoteFigmaClient, SystemBrowser,
    merge_chunks, snapshot_chunk_from_result,
};

pub use tools::{AuthInput, FigmaToJsonInput, FigmaToUiInput};

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
}

impl DevupServer {
    pub fn new(services: Services) -> Self {
        Self {
            tool_router: Self::tool_router(),
            services,
        }
    }

    async fn ensure_authenticated(&self) -> Result<(), ErrorData> {
        if self.services.auth.status().await.map_err(to_mcp_error)? == AuthStatus::Disconnected {
            self.services.auth.login().await.map_err(to_mcp_error)?;
        }
        Ok(())
    }
}

impl Default for DevupServer {
    fn default() -> Self {
        Self::new(Services::production())
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
        let node_id = target.node_id.clone().ok_or_else(|| {
            to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "UI 변환 링크에는 node-id가 필요합니다.",
                false,
            ))
        })?;
        self.ensure_authenticated().await?;
        let result = self
            .services
            .upstream
            .call_read_tool(ReadToolCall::snapshot(
                &target.file_key,
                &node_id,
                BuiltinScript::NodeSnapshot,
            ))
            .await
            .map_err(to_mcp_error)?;
        let chunk = snapshot_chunk_from_result(&result).map_err(to_mcp_error)?;
        let snapshot = merge_chunks(vec![chunk]).map_err(to_mcp_error)?;
        let output = generate_component(
            &snapshot,
            &node_id,
            &CodegenOptions {
                component_name: input.component_name,
                include_diagnostics: input.include_diagnostics,
            },
        )
        .map_err(to_mcp_error)?;
        let diagnostics = if input.include_diagnostics {
            output.diagnostics
        } else {
            Vec::new()
        };
        Ok(Json(json!({
            "tsx": output.tsx,
            "imports": output.imports,
            "usedTokens": output.used_tokens,
            "diagnostics": diagnostics,
            "source": { "fileKey": target.file_key, "nodeId": node_id, "version": snapshot.version },
            "snapshot": {
                "preservedNodeCount": snapshot.nodes.len(),
                "fieldErrorCount": snapshot.nodes.values().map(|node| node.field_errors.len()).sum::<usize>()
            }
        })))
    }

    #[tool(description = "Convert Figma variables and styles to deterministic devup.json")]
    async fn devup_figma_to_json(
        &self,
        Parameters(input): Parameters<FigmaToJsonInput>,
    ) -> Result<Json<Value>, ErrorData> {
        let target = FigmaTarget::parse(&input.url).map_err(to_mcp_error)?;
        let scope = parse_scope(&input.scope).map_err(to_mcp_error)?;
        self.ensure_authenticated().await?;
        let result = self
            .services
            .upstream
            .call_read_tool(ReadToolCall::snapshot(
                &target.file_key,
                target.node_id.as_deref().unwrap_or("0:0"),
                BuiltinScript::LocalVariables,
            ))
            .await
            .map_err(to_mcp_error)?;
        let variables = variable_snapshot_from_result(&result).map_err(to_mcp_error)?;
        let output = generate_devup_json(&variables, scope).map_err(to_mcp_error)?;
        let diagnostics = if input.include_diagnostics {
            output.diagnostics
        } else {
            Vec::new()
        };
        Ok(Json(json!({
            "devupJson": output.json,
            "counts": output.counts,
            "completeness": output.completeness,
            "diagnostics": diagnostics,
            "source": { "fileKey": target.file_key, "nodeId": target.node_id }
        })))
    }
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
