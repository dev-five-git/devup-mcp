use async_trait::async_trait;
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{
    CredentialStore, DevupError, OAuthManager, ResourceBatch, UpstreamFailureContext,
    UpstreamFailureKind, upstream_failure_error,
};

const DEFAULT_FIGMA_MCP_ENDPOINT: &str = "https://mcp.figma.com/mcp";
const MAX_SSE_EVENT_SIZE: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinScript {
    NodeSnapshot,
    VariableCatalog,
    LocalVariables,
}

impl BuiltinScript {
    fn source(self, node_id: &str, resources: Option<&ResourceBatch>) -> String {
        let node_id = serde_json::to_string(node_id).expect("node id serializes");
        let source = match self {
            Self::NodeSnapshot => include_str!("scripts/snapshot.js"),
            Self::VariableCatalog => include_str!("scripts/variable_catalog.js"),
            Self::LocalVariables => include_str!("scripts/variables.js"),
        };
        let empty_resources = ResourceBatch {
            variable_ids: Vec::new(),
            styles: Vec::new(),
        };
        let resources = serde_json::to_string(resources.unwrap_or(&empty_resources))
            .expect("resource batch serializes");
        source
            .replace("\"__DEVUP_NODE_ID__\"", &node_id)
            .replace(
                "\"__DEVUP_PLUGIN_API_MANIFEST__\"",
                include_str!("plugin_api_manifest.json"),
            )
            .replace("\"__DEVUP_RESOURCE_BATCH__\"", &resources)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReadToolCall {
    Metadata {
        file_key: String,
        node_id: Option<String>,
    },
    VariableDefs {
        file_key: String,
        node_id: String,
    },
    DesignContext {
        file_key: String,
        node_id: String,
    },
    CodeConnectMap {
        file_key: String,
        node_id: String,
    },
    Screenshot {
        file_key: String,
        node_id: String,
    },
    Snapshot {
        file_key: String,
        node_id: String,
        script: BuiltinScript,
        resources: Option<ResourceBatch>,
    },
}

impl ReadToolCall {
    pub fn metadata(file_key: impl Into<String>, node_id: Option<&str>) -> Self {
        Self::Metadata {
            file_key: file_key.into(),
            node_id: node_id.map(str::to_owned),
        }
    }

    pub fn variable_defs(file_key: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self::VariableDefs {
            file_key: file_key.into(),
            node_id: node_id.into(),
        }
    }

    pub fn design_context(file_key: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self::DesignContext {
            file_key: file_key.into(),
            node_id: node_id.into(),
        }
    }

    pub fn code_connect_map(file_key: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self::CodeConnectMap {
            file_key: file_key.into(),
            node_id: node_id.into(),
        }
    }

    pub fn screenshot(file_key: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self::Screenshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
        }
    }

    pub fn snapshot(
        file_key: impl Into<String>,
        node_id: impl Into<String>,
        script: BuiltinScript,
    ) -> Self {
        Self::Snapshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
            script,
            resources: None,
        }
    }

    pub fn resource_batch(
        file_key: impl Into<String>,
        node_id: impl Into<String>,
        resources: ResourceBatch,
    ) -> Self {
        Self::Snapshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
            script: BuiltinScript::LocalVariables,
            resources: Some(resources),
        }
    }

    pub fn tool_name(&self) -> &'static str {
        match self {
            Self::Metadata { .. } => "get_metadata",
            Self::VariableDefs { .. } => "get_variable_defs",
            Self::DesignContext { .. } => "get_design_context",
            Self::CodeConnectMap { .. } => "get_code_connect_map",
            Self::Screenshot { .. } => "get_screenshot",
            Self::Snapshot { .. } => "use_figma",
        }
    }

    pub fn arguments(&self) -> Map<String, Value> {
        let value = match self {
            Self::Metadata { file_key, node_id } => {
                json!({ "fileKey": file_key, "nodeId": node_id })
            }
            Self::VariableDefs { file_key, node_id }
            | Self::DesignContext { file_key, node_id }
            | Self::CodeConnectMap { file_key, node_id }
            | Self::Screenshot { file_key, node_id } => {
                json!({ "fileKey": file_key, "nodeId": node_id })
            }
            Self::Snapshot {
                file_key,
                node_id,
                script,
                resources,
            } => json!({
                "fileKey": file_key,
                "nodeId": node_id,
                "code": script.source(node_id, resources.as_ref())
            }),
        };
        value.as_object().cloned().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamResult {
    pub raw: Value,
}

#[async_trait]
pub trait FigmaUpstream: Send + Sync {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError>;
    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError>;
}

#[derive(Clone)]
pub struct RemoteFigmaClient<S: CredentialStore> {
    endpoint: String,
    oauth: OAuthManager<S>,
}

impl<S: CredentialStore> RemoteFigmaClient<S> {
    pub fn new(oauth: OAuthManager<S>) -> Self {
        Self {
            endpoint: DEFAULT_FIGMA_MCP_ENDPOINT.to_owned(),
            oauth,
        }
    }

    pub fn with_endpoint(endpoint: impl Into<String>, oauth: OAuthManager<S>) -> Self {
        Self {
            endpoint: endpoint.into(),
            oauth,
        }
    }

    async fn connect(
        &self,
    ) -> Result<rmcp::service::RunningService<rmcp::RoleClient, ()>, DevupError> {
        let token = self.oauth.access_token().await?;
        let config = StreamableHttpClientTransportConfig::with_uri(self.endpoint.clone())
            .auth_header(token.expose().to_owned())
            .max_sse_event_size(MAX_SSE_EVENT_SIZE)
            .reinit_on_expired_session(true);
        ().serve(StreamableHttpClientTransport::from_config(config))
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::Connect, error))
    }
}

#[async_trait]
impl<S: CredentialStore> FigmaUpstream for RemoteFigmaClient<S> {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        let client = self.connect().await?;
        let mut names = client
            .list_all_tools()
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::ListTools, error))?
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>();
        names.sort();
        client
            .cancel()
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::Connect, error))?;
        Ok(names)
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        let client = self.connect().await?;
        let available = client
            .list_all_tools()
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::ListTools, error))?;
        if !available.iter().any(|tool| tool.name == call.tool_name()) {
            client
                .cancel()
                .await
                .map_err(|error| map_upstream_error(UpstreamFailureContext::Connect, error))?;
            return Err(UpstreamFailureKind::CapabilityUnavailable.into_devup_error(None));
        }
        let result = client
            .call_tool(
                CallToolRequestParams::new(call.tool_name()).with_arguments(call.arguments()),
            )
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::CallTool, error))?;
        client
            .cancel()
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::Connect, error))?;
        let raw = serde_json::to_value(result)
            .map_err(|error| map_upstream_error(UpstreamFailureContext::Decode, error))?;
        Ok(UpstreamResult { raw })
    }
}

fn map_upstream_error<E: std::fmt::Display>(
    context: UpstreamFailureContext,
    error: E,
) -> DevupError {
    upstream_failure_error(context, None, &error.to_string())
}
