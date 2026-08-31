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
    AssetRequest, CredentialStore, DevupError, LargeValueReadOptions, OAuthManager, ResourceBatch,
    UpstreamFailureContext, UpstreamFailureKind, upstream_failure_error,
};

const DEFAULT_FIGMA_MCP_ENDPOINT: &str = "https://mcp.figma.com/mcp";
const MAX_SSE_EVENT_SIZE: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinScript {
    NodeSnapshot,
    FastSnapshotEnvelope,
    FastThemeEnvelope,
    PageCatalog,
    SearchSnapshot,
    VariableCatalog,
    LocalVariables,
    UsedResources,
    ExploreSnapshot,
    LargeValue,
    AssetExport,
}

impl BuiltinScript {
    fn source(self, node_id: &str, inputs: ScriptInputs<'_>) -> String {
        let node_id = serde_json::to_string(node_id).expect("node id serializes");
        let source = match self {
            Self::NodeSnapshot => include_str!("scripts/snapshot.js"),
            Self::FastSnapshotEnvelope => include_str!("scripts/fast_snapshot.js"),
            Self::FastThemeEnvelope => include_str!("scripts/fast_theme.js"),
            Self::PageCatalog => include_str!("scripts/page_catalog.js"),
            Self::SearchSnapshot => include_str!("scripts/search.js"),
            Self::VariableCatalog => include_str!("scripts/variable_catalog.js"),
            Self::LocalVariables => include_str!("scripts/variables.js"),
            Self::UsedResources => include_str!("scripts/used_resources.js"),
            Self::ExploreSnapshot => include_str!("scripts/explore.js"),
            Self::LargeValue => include_str!("scripts/large_value.js"),
            Self::AssetExport => include_str!("scripts/assets.js"),
        };
        let empty_resources = ResourceBatch {
            variable_ids: Vec::new(),
            styles: Vec::new(),
        };
        let resources = serde_json::to_string(inputs.resources.unwrap_or(&empty_resources))
            .expect("resource batch serializes");
        let search = serde_json::to_string(inputs.search.unwrap_or(&SearchReadOptions::default()))
            .expect("search options serialize");
        let explore =
            serde_json::to_string(inputs.explore.unwrap_or(&ExploreReadOptions::default()))
                .expect("explore options serialize");
        let snapshot =
            serde_json::to_string(inputs.snapshot.unwrap_or(&SnapshotReadOptions::default()))
                .expect("snapshot options serialize");
        let large_value =
            serde_json::to_string(inputs.large_value.unwrap_or(&LargeValueReadOptions {
                node_id: String::new(),
                field: String::new(),
                offset: 0,
                max_chunk_bytes: 1,
                byte_length: 0,
                sha256: String::new(),
                version: None,
            }))
            .expect("large value options serialize");
        let asset = inputs
            .asset
            .map(|(request, version)| {
                json!({
                    "assetId": request.asset_id,
                    "nodeId": request.node_id,
                    "field": request.field,
                    "imageHash": request.image_hash,
                    "format": request.format,
                    "scale": request.scale,
                    "version": version
                })
            })
            .unwrap_or_else(|| json!({}));
        let asset = serde_json::to_string(&asset).expect("asset options serialize");
        source
            .replace(
                "\"__DEVUP_LARGE_VALUE_HELPERS__\"",
                include_str!("scripts/large_value_helpers.js"),
            )
            .replace("\"__DEVUP_NODE_ID__\"", &node_id)
            .replace(
                "\"__DEVUP_PLUGIN_API_MANIFEST__\"",
                include_str!("plugin_api_manifest.json"),
            )
            .replace(
                "\"__DEVUP_TEXT_SEGMENT_MANIFEST__\"",
                include_str!("text_segment_manifest.json"),
            )
            .replace("\"__DEVUP_RESOURCE_BATCH__\"", &resources)
            .replace("\"__DEVUP_SEARCH__\"", &search)
            .replace("\"__DEVUP_EXPLORE__\"", &explore)
            .replace("\"__DEVUP_SNAPSHOT__\"", &snapshot)
            .replace("\"__DEVUP_LARGE_VALUE__\"", &large_value)
            .replace("\"__DEVUP_ASSET__\"", &asset)
    }
}

#[derive(Default)]
struct ScriptInputs<'a> {
    resources: Option<&'a ResourceBatch>,
    search: Option<&'a SearchReadOptions>,
    explore: Option<&'a ExploreReadOptions>,
    snapshot: Option<&'a SnapshotReadOptions>,
    large_value: Option<&'a LargeValueReadOptions>,
    asset: Option<(&'a AssetRequest, Option<&'a str>)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotReadOptions {
    pub offset: usize,
    pub max_payload_bytes: usize,
    pub max_field_bytes: usize,
}

impl Default for SnapshotReadOptions {
    fn default() -> Self {
        Self {
            offset: 0,
            max_payload_bytes: 12_000,
            max_field_bytes: 4_096,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchReadOptions {
    pub query: String,
    #[serde(default)]
    pub node_types: Vec<String>,
    pub match_kind: String,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExploreReadOptions {
    pub projection_limit: usize,
    pub text_preview_limit: usize,
}

impl Default for ExploreReadOptions {
    fn default() -> Self {
        Self {
            projection_limit: 200,
            text_preview_limit: 160,
        }
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
        snapshot: Option<SnapshotReadOptions>,
    },
    SearchSnapshot {
        file_key: String,
        node_id: String,
        options: SearchReadOptions,
    },
    PageCatalog {
        file_key: String,
    },
    ExploreSnapshot {
        file_key: String,
        node_id: String,
        options: ExploreReadOptions,
    },
    FastTheme {
        file_key: String,
    },
    LargeValue {
        file_key: String,
        options: LargeValueReadOptions,
    },
    AssetExport {
        file_key: String,
        version: Option<String>,
        request: Box<AssetRequest>,
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
        let snapshot = (script == BuiltinScript::NodeSnapshot).then(SnapshotReadOptions::default);
        Self::Snapshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
            script,
            resources: None,
            snapshot,
        }
    }

    pub fn snapshot_chunk(
        file_key: impl Into<String>,
        node_id: impl Into<String>,
        options: SnapshotReadOptions,
    ) -> Self {
        Self::Snapshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
            script: BuiltinScript::NodeSnapshot,
            resources: None,
            snapshot: Some(options),
        }
    }

    pub fn fast_snapshot(file_key: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self::Snapshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
            script: BuiltinScript::FastSnapshotEnvelope,
            resources: None,
            snapshot: None,
        }
    }

    pub fn fast_theme(file_key: impl Into<String>) -> Self {
        Self::FastTheme {
            file_key: file_key.into(),
        }
    }

    pub fn large_value(file_key: impl Into<String>, options: LargeValueReadOptions) -> Self {
        Self::LargeValue {
            file_key: file_key.into(),
            options,
        }
    }

    pub fn asset_export(
        file_key: impl Into<String>,
        version: Option<String>,
        request: AssetRequest,
    ) -> Self {
        Self::AssetExport {
            file_key: file_key.into(),
            version,
            request: Box::new(request),
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
            snapshot: None,
        }
    }

    pub fn used_resources(
        file_key: impl Into<String>,
        node_id: impl Into<String>,
        resources: ResourceBatch,
    ) -> Self {
        Self::Snapshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
            script: BuiltinScript::UsedResources,
            resources: Some(resources),
            snapshot: None,
        }
    }

    pub fn search_snapshot(
        file_key: impl Into<String>,
        node_id: impl Into<String>,
        options: SearchReadOptions,
    ) -> Self {
        Self::SearchSnapshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
            options,
        }
    }

    pub fn page_catalog(file_key: impl Into<String>) -> Self {
        Self::PageCatalog {
            file_key: file_key.into(),
        }
    }

    pub fn explore_snapshot(
        file_key: impl Into<String>,
        node_id: impl Into<String>,
        options: ExploreReadOptions,
    ) -> Self {
        Self::ExploreSnapshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
            options,
        }
    }

    pub fn tool_name(&self) -> &'static str {
        match self {
            Self::Metadata { .. } => "get_metadata",
            Self::VariableDefs { .. } => "get_variable_defs",
            Self::DesignContext { .. } => "get_design_context",
            Self::CodeConnectMap { .. } => "get_code_connect_map",
            Self::Screenshot { .. } => "get_screenshot",
            Self::Snapshot { .. }
            | Self::SearchSnapshot { .. }
            | Self::PageCatalog { .. }
            | Self::ExploreSnapshot { .. }
            | Self::FastTheme { .. }
            | Self::LargeValue { .. }
            | Self::AssetExport { .. } => "use_figma",
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
                snapshot,
            } => json!({
                "fileKey": file_key,
                "nodeId": node_id,
                "code": script.source(node_id, ScriptInputs {
                    resources: resources.as_ref(),
                    snapshot: snapshot.as_ref(),
                    ..ScriptInputs::default()
                })
            }),
            Self::SearchSnapshot {
                file_key,
                node_id,
                options,
            } => json!({
                "fileKey": file_key,
                "nodeId": node_id,
                "code": BuiltinScript::SearchSnapshot.source(node_id, ScriptInputs {
                    search: Some(options),
                    ..ScriptInputs::default()
                })
            }),
            Self::PageCatalog { file_key } => json!({
                "fileKey": file_key,
                "code": BuiltinScript::PageCatalog.source("", ScriptInputs::default())
            }),
            Self::ExploreSnapshot {
                file_key,
                node_id,
                options,
            } => json!({
                "fileKey": file_key,
                "nodeId": node_id,
                "code": BuiltinScript::ExploreSnapshot.source(node_id, ScriptInputs {
                    explore: Some(options),
                    ..ScriptInputs::default()
                })
            }),
            Self::FastTheme { file_key } => json!({
                "fileKey": file_key,
                "code": BuiltinScript::FastThemeEnvelope.source("", ScriptInputs::default())
            }),
            Self::LargeValue { file_key, options } => json!({
                "fileKey": file_key,
                "nodeId": options.node_id,
                "code": BuiltinScript::LargeValue.source(&options.node_id, ScriptInputs {
                    large_value: Some(options),
                    ..ScriptInputs::default()
                })
            }),
            Self::AssetExport {
                file_key,
                version,
                request,
            } => json!({
                "fileKey": file_key,
                "nodeId": request.node_id,
                "code": BuiltinScript::AssetExport.source(&request.node_id, ScriptInputs {
                    asset: Some((request, version.as_deref())),
                    ..ScriptInputs::default()
                })
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
