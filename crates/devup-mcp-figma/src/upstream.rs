use std::{
    collections::BTreeSet,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

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
use tokio::sync::Mutex;

use super::{
    AssetRequest, CredentialStore, DevupError, LargeValueReadOptions, OAuthManager, ResourceBatch,
    UpstreamFailureContext, UpstreamFailureKind, upstream_failure_error,
};

const DEFAULT_FIGMA_MCP_ENDPOINT: &str = "https://mcp.figma.com/mcp";
const MAX_SSE_EVENT_SIZE: usize = 16 * 1024 * 1024;
const REMOTE_SESSION_TTL: Duration = Duration::from_secs(30);
const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const REMOTE_LIST_TOOLS_TIMEOUT: Duration = Duration::from_secs(30);
const REMOTE_CALL_TOOL_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const READ_ONLY_TOOL_NAMES: [&str; 6] = [
    "get_metadata",
    "get_variable_defs",
    "get_design_context",
    "get_code_connect_map",
    "get_screenshot",
    "use_figma",
];

type RunningFigmaClient = rmcp::service::RunningService<rmcp::RoleClient, ()>;

// Keep at most one initialized MCP session for 30 seconds. Its capability catalog is fetched
// once, intersected with the locally compiled ReadToolCall allowlist, and reused concurrently.
// A closed session, TTL expiry, or tools/call transport failure evicts it; the next request then
// reconnects with a freshly resolved OAuth token and re-verifies the catalog. No remote-only tool
// name can cross this boundary, even when the server advertises it.
#[derive(Clone)]
struct RemoteSessionCache {
    ttl: Duration,
    connect_timeout: Duration,
    list_tools_timeout: Duration,
    state: Arc<Mutex<Option<CachedRemoteSession>>>,
}

#[derive(Clone)]
struct RemoteSession {
    client: Arc<RunningFigmaClient>,
    capabilities: Arc<BTreeSet<String>>,
}

struct CachedRemoteSession {
    created_at: Instant,
    session: RemoteSession,
}

impl RemoteSessionCache {
    fn new(ttl: Duration) -> Self {
        Self::with_timeouts(ttl, REMOTE_CONNECT_TIMEOUT, REMOTE_LIST_TOOLS_TIMEOUT)
    }

    fn with_timeouts(
        ttl: Duration,
        connect_timeout: Duration,
        list_tools_timeout: Duration,
    ) -> Self {
        Self {
            ttl,
            connect_timeout,
            list_tools_timeout,
            state: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_or_connect<F, Fut>(&self, connect: F) -> Result<RemoteSession, DevupError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<RunningFigmaClient, DevupError>>,
    {
        let mut cached = self.state.lock().await;
        if let Some(current) = cached.as_ref()
            && current.created_at.elapsed() < self.ttl
            && !current.session.client.is_closed()
        {
            return Ok(current.session.clone());
        }
        let client = Arc::new(
            tokio::time::timeout(self.connect_timeout, connect())
                .await
                .map_err(|_| upstream_timeout_error(UpstreamFailureContext::Connect))??,
        );
        let capabilities = tokio::time::timeout(self.list_tools_timeout, client.list_all_tools())
            .await
            .map_err(|_| upstream_timeout_error(UpstreamFailureContext::ListTools))?
            .map_err(|error| map_upstream_error(UpstreamFailureContext::ListTools, error))?
            .into_iter()
            .map(|tool| tool.name.to_string())
            .filter(|name| is_read_only_tool_name(name))
            .collect::<BTreeSet<_>>();
        let session = RemoteSession {
            client,
            capabilities: Arc::new(capabilities),
        };
        *cached = Some(CachedRemoteSession {
            created_at: Instant::now(),
            session: session.clone(),
        });
        Ok(session)
    }

    async fn invalidate(&self, session: &RemoteSession) {
        let mut cached = self.state.lock().await;
        if cached
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&current.session.client, &session.client))
        {
            cached.take();
        }
    }

    async fn call_tool(
        &self,
        session: &RemoteSession,
        call: &ReadToolCall,
        timeout: Duration,
    ) -> Result<rmcp::model::CallToolResult, DevupError> {
        let result = tokio::time::timeout(
            timeout,
            session.client.call_tool(
                CallToolRequestParams::new(call.tool_name()).with_arguments(call.arguments()),
            ),
        )
        .await;
        match result {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => {
                self.invalidate(session).await;
                Err(map_upstream_error(UpstreamFailureContext::CallTool, error))
            }
            Err(_) => {
                self.invalidate(session).await;
                Err(upstream_timeout_error(UpstreamFailureContext::CallTool))
            }
        }
    }
}

fn is_read_only_tool_name(name: &str) -> bool {
    READ_ONLY_TOOL_NAMES.contains(&name)
}

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
    SectionIndex,
    MultiRootSnapshotEnvelope,
    LargeValue,
    AssetExport,
}

impl BuiltinScript {
    fn source(self, node_id: &str, inputs: ScriptInputs<'_>) -> String {
        let default_root_ids = [node_id.to_owned()];
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
            Self::SectionIndex => include_str!("scripts/section_index.js"),
            Self::MultiRootSnapshotEnvelope => include_str!("scripts/fast_snapshot.js"),
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
        let root_ids = serde_json::to_string(inputs.root_ids.unwrap_or(&default_root_ids))
            .expect("root ids serialize");
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
            .replace("\"__DEVUP_ROOT_IDS__\"", &root_ids)
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
    root_ids: Option<&'a [String]>,
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
        root_ids: Option<Vec<String>>,
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
            root_ids: None,
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
            root_ids: None,
        }
    }

    pub fn fast_snapshot(file_key: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self::fast_snapshot_page(file_key, node_id, SnapshotReadOptions::default())
    }

    /// A single round of the fast (text-paginated) node snapshot. `options.offset`
    /// selects the starting node index; the script dynamically packs as many
    /// nodes as fit under `options.max_payload_bytes` and reports a cursor for
    /// the next round via the standard `__DEVUP_SNAPSHOT_CURSOR__` marker node.
    pub fn fast_snapshot_page(
        file_key: impl Into<String>,
        node_id: impl Into<String>,
        options: SnapshotReadOptions,
    ) -> Self {
        Self::Snapshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
            script: BuiltinScript::FastSnapshotEnvelope,
            resources: None,
            snapshot: Some(options),
            root_ids: None,
        }
    }

    pub fn section_index(file_key: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self::Snapshot {
            file_key: file_key.into(),
            node_id: node_id.into(),
            script: BuiltinScript::SectionIndex,
            resources: None,
            snapshot: None,
            root_ids: None,
        }
    }

    pub fn multi_root_snapshot(
        file_key: impl Into<String>,
        section_id: impl Into<String>,
        root_ids: Vec<String>,
    ) -> Self {
        Self::Snapshot {
            file_key: file_key.into(),
            node_id: section_id.into(),
            script: BuiltinScript::MultiRootSnapshotEnvelope,
            resources: None,
            snapshot: None,
            root_ids: Some(root_ids),
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
            root_ids: None,
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
            root_ids: None,
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
            // These variants all route to the official `use_figma` tool, whose
            // schema is `{ fileKey, code, description, skillNames? }` with
            // `additionalProperties: false`. `nodeId` is NOT part of that
            // schema and must never appear here (real Figma MCP hosts reject
            // unknown properties); the node this call targets is tracked
            // separately in `PlannedCall::expected_node_id` and surfaced to
            // handoff consumers outside `arguments`, not inside it.
            Self::Snapshot {
                file_key,
                node_id,
                script,
                resources,
                snapshot,
                root_ids,
            } => json!({
                "fileKey": file_key,
                "description": self.description(),
                "code": script.source(node_id, ScriptInputs {
                    resources: resources.as_ref(),
                    snapshot: snapshot.as_ref(),
                    root_ids: root_ids.as_deref(),
                    ..ScriptInputs::default()
                })
            }),
            Self::SearchSnapshot {
                file_key,
                node_id,
                options,
            } => json!({
                "fileKey": file_key,
                "description": self.description(),
                "code": BuiltinScript::SearchSnapshot.source(node_id, ScriptInputs {
                    search: Some(options),
                    ..ScriptInputs::default()
                })
            }),
            Self::PageCatalog { file_key } => json!({
                "fileKey": file_key,
                "description": self.description(),
                "code": BuiltinScript::PageCatalog.source("", ScriptInputs::default())
            }),
            Self::ExploreSnapshot {
                file_key,
                node_id,
                options,
            } => json!({
                "fileKey": file_key,
                "description": self.description(),
                "code": BuiltinScript::ExploreSnapshot.source(node_id, ScriptInputs {
                    explore: Some(options),
                    ..ScriptInputs::default()
                })
            }),
            Self::FastTheme { file_key } => json!({
                "fileKey": file_key,
                "description": self.description(),
                "code": BuiltinScript::FastThemeEnvelope.source("", ScriptInputs::default())
            }),
            Self::LargeValue { file_key, options } => json!({
                "fileKey": file_key,
                "description": self.description(),
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
                "description": self.description(),
                "code": BuiltinScript::AssetExport.source(&request.node_id, ScriptInputs {
                    asset: Some((request, version.as_deref())),
                    ..ScriptInputs::default()
                })
            }),
        };
        value.as_object().cloned().unwrap_or_default()
    }

    /// Human-readable `description` required by the official `use_figma`
    /// schema. Only meaningful for the `use_figma`-routed variants; other
    /// variants never reach this (their `arguments()` don't call it).
    fn description(&self) -> String {
        let node_id = self.node_id_for_description();
        match self {
            Self::Snapshot { script, .. } => match script {
                BuiltinScript::FastSnapshotEnvelope | BuiltinScript::MultiRootSnapshotEnvelope => {
                    format!("devup-mcp fast node snapshot for node {node_id} (read-only)")
                }
                BuiltinScript::NodeSnapshot => {
                    format!("devup-mcp paginated node snapshot for node {node_id} (read-only)")
                }
                BuiltinScript::SectionIndex => {
                    format!("devup-mcp Section screen index for node {node_id} (read-only)")
                }
                BuiltinScript::VariableCatalog => {
                    format!("devup-mcp local variable/style catalog for node {node_id} (read-only)")
                }
                BuiltinScript::LocalVariables | BuiltinScript::UsedResources => {
                    format!(
                        "devup-mcp variable/style resource batch for node {node_id} (read-only)"
                    )
                }
                _ => format!("devup-mcp Figma read for node {node_id} (read-only)"),
            },
            Self::SearchSnapshot { .. } => {
                format!("devup-mcp page-scoped name search for node {node_id} (read-only)")
            }
            Self::PageCatalog { .. } => "devup-mcp file page catalog (read-only)".to_owned(),
            Self::ExploreSnapshot { .. } => {
                format!("devup-mcp screen candidate exploration near node {node_id} (read-only)")
            }
            Self::FastTheme { .. } => {
                "devup-mcp fast file-wide theme snapshot (read-only)".to_owned()
            }
            Self::LargeValue { .. } => {
                format!("devup-mcp large field value fragment for node {node_id} (read-only)")
            }
            Self::AssetExport { .. } => {
                format!("devup-mcp asset export for node {node_id} (read-only)")
            }
            _ => "devup-mcp Figma read (read-only)".to_owned(),
        }
    }

    fn node_id_for_description(&self) -> &str {
        match self {
            Self::Snapshot { node_id, .. }
            | Self::SearchSnapshot { node_id, .. }
            | Self::ExploreSnapshot { node_id, .. } => node_id,
            Self::LargeValue { options, .. } => &options.node_id,
            Self::AssetExport { request, .. } => &request.node_id,
            _ => "",
        }
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
    sessions: RemoteSessionCache,
    call_tool_timeout: Duration,
}

impl<S: CredentialStore> RemoteFigmaClient<S> {
    pub fn new(oauth: OAuthManager<S>) -> Self {
        Self {
            endpoint: DEFAULT_FIGMA_MCP_ENDPOINT.to_owned(),
            oauth,
            sessions: RemoteSessionCache::new(REMOTE_SESSION_TTL),
            call_tool_timeout: REMOTE_CALL_TOOL_TIMEOUT,
        }
    }

    pub fn with_endpoint(endpoint: impl Into<String>, oauth: OAuthManager<S>) -> Self {
        Self {
            endpoint: endpoint.into(),
            oauth,
            sessions: RemoteSessionCache::new(REMOTE_SESSION_TTL),
            call_tool_timeout: REMOTE_CALL_TOOL_TIMEOUT,
        }
    }

    async fn connect(&self) -> Result<RunningFigmaClient, DevupError> {
        let token = self.oauth.access_token().await?;
        let config = StreamableHttpClientTransportConfig::with_uri(self.endpoint.clone())
            .auth_header(token.expose().to_owned())
            .max_sse_event_size(MAX_SSE_EVENT_SIZE)
            .reinit_on_expired_session(true);
        ().serve(StreamableHttpClientTransport::from_config(config))
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::Connect, error))
    }

    async fn session(&self) -> Result<RemoteSession, DevupError> {
        self.sessions.get_or_connect(|| self.connect()).await
    }
}

#[async_trait]
impl<S: CredentialStore> FigmaUpstream for RemoteFigmaClient<S> {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        let session = self.session().await?;
        Ok(session.capabilities.iter().cloned().collect())
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        if !is_read_only_tool_name(call.tool_name()) {
            return Err(UpstreamFailureKind::CapabilityUnavailable.into_devup_error(None));
        }
        let session = self.session().await?;
        if !session.capabilities.contains(call.tool_name()) {
            return Err(UpstreamFailureKind::CapabilityUnavailable.into_devup_error(None));
        }
        let result = self
            .sessions
            .call_tool(&session, &call, self.call_tool_timeout)
            .await?;
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

fn upstream_timeout_error(context: UpstreamFailureContext) -> DevupError {
    let phase = match context {
        UpstreamFailureContext::Connect => "connect",
        UpstreamFailureContext::ListTools => "listTools",
        UpstreamFailureContext::CallTool => "callTool",
        UpstreamFailureContext::RegisterClient => "registerClient",
        UpstreamFailureContext::Decode => "decode",
    };
    let mut error = UpstreamFailureKind::Transport.into_devup_error(None);
    error.details = json!({
        "source": "direct",
        "status": null,
        "reason": "timeout",
        "phase": phase,
    });
    error
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rmcp::{
        ErrorData, RoleServer, ServerHandler, ServiceExt,
        model::{
            CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
            PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
        },
        service::RequestContext,
    };

    use crate::ErrorCode;

    use super::*;

    #[derive(Clone)]
    struct CountingToolServer {
        list_calls: Arc<AtomicUsize>,
        tool_calls: Arc<AtomicUsize>,
    }

    #[derive(Clone)]
    struct StalledToolServer {
        stall_list: bool,
        stall_call: bool,
        fail_call: bool,
    }

    impl ServerHandler for StalledToolServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, ErrorData> {
            if self.stall_list {
                std::future::pending().await
            }
            Ok(ListToolsResult {
                tools: vec![Tool::new("get_metadata", "read metadata", Map::new())],
                ..ListToolsResult::default()
            })
        }

        async fn call_tool(
            &self,
            _request: CallToolRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> Result<CallToolResponse, ErrorData> {
            if self.stall_call {
                std::future::pending().await
            }
            if self.fail_call {
                return Err(ErrorData::new(
                    rmcp::model::ErrorCode::INTERNAL_ERROR,
                    "test failure",
                    None,
                ));
            }
            Ok(CallToolResult::success(vec![ContentBlock::text("ok")]).into())
        }
    }

    impl ServerHandler for CountingToolServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, ErrorData> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            Ok(ListToolsResult {
                tools: vec![
                    Tool::new("get_metadata", "read metadata", Map::new()),
                    Tool::new("delete_node", "write-like server tool", Map::new()),
                ],
                ..ListToolsResult::default()
            })
        }

        async fn call_tool(
            &self,
            _request: CallToolRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> Result<CallToolResponse, ErrorData> {
            self.tool_calls.fetch_add(1, Ordering::SeqCst);
            Ok(CallToolResult::success(vec![ContentBlock::text("ok")]).into())
        }
    }

    async fn connect_counting_server(
        connections: Arc<AtomicUsize>,
        list_calls: Arc<AtomicUsize>,
        tool_calls: Arc<AtomicUsize>,
    ) -> Result<RunningFigmaClient, DevupError> {
        connections.fetch_add(1, Ordering::SeqCst);
        let (server_transport, client_transport) = tokio::io::duplex(4_096);
        tokio::spawn(async move {
            let server = CountingToolServer {
                list_calls,
                tool_calls,
            }
            .serve(server_transport)
            .await
            .expect("test MCP server starts");
            let _ = server.waiting().await;
        });
        ().serve(client_transport)
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::Connect, error))
    }

    async fn connect_stalled_server(
        connections: Arc<AtomicUsize>,
        stall_list: bool,
        stall_call: bool,
        fail_call: bool,
    ) -> Result<RunningFigmaClient, DevupError> {
        connections.fetch_add(1, Ordering::SeqCst);
        let (server_transport, client_transport) = tokio::io::duplex(4_096);
        tokio::spawn(async move {
            let server = StalledToolServer {
                stall_list,
                stall_call,
                fail_call,
            }
            .serve(server_transport)
            .await
            .expect("test MCP server starts");
            let _ = server.waiting().await;
        });
        ().serve(client_transport)
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::Connect, error))
    }

    #[tokio::test]
    async fn bounded_session_reuses_one_connection_and_one_filtered_capability_catalog() {
        let connections = Arc::new(AtomicUsize::new(0));
        let list_calls = Arc::new(AtomicUsize::new(0));
        let tool_calls = Arc::new(AtomicUsize::new(0));
        let cache = RemoteSessionCache::new(Duration::from_secs(30));

        let connect = || {
            let connections = connections.clone();
            let list_calls = list_calls.clone();
            let tool_calls = tool_calls.clone();
            connect_counting_server(connections, list_calls, tool_calls)
        };

        let first = cache.get_or_connect(connect).await.unwrap();
        let second = cache.get_or_connect(connect).await.unwrap();
        assert!(Arc::ptr_eq(&first.client, &second.client));
        assert_eq!(
            first
                .capabilities
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["get_metadata"]
        );
        assert!(!first.capabilities.contains("delete_node"));
        for session in [&first, &second] {
            session
                .client
                .call_tool(CallToolRequestParams::new("get_metadata").with_arguments(Map::new()))
                .await
                .unwrap();
        }

        assert_eq!(connections.load(Ordering::SeqCst), 1);
        assert_eq!(list_calls.load(Ordering::SeqCst), 1);
        assert_eq!(tool_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn expired_session_reconnects_and_reverifies_capabilities() {
        let connections = Arc::new(AtomicUsize::new(0));
        let list_calls = Arc::new(AtomicUsize::new(0));
        let tool_calls = Arc::new(AtomicUsize::new(0));
        let cache = RemoteSessionCache::new(Duration::ZERO);

        let connect =
            || connect_counting_server(connections.clone(), list_calls.clone(), tool_calls.clone());
        let first = cache.get_or_connect(connect).await.unwrap();
        let second = cache.get_or_connect(connect).await.unwrap();

        assert!(!Arc::ptr_eq(&first.client, &second.client));
        assert_eq!(connections.load(Ordering::SeqCst), 2);
        assert_eq!(list_calls.load(Ordering::SeqCst), 2);
        assert!(!second.capabilities.contains("delete_node"));
    }

    #[tokio::test]
    async fn stalled_capability_catalog_returns_a_bounded_safe_timeout() {
        let connections = Arc::new(AtomicUsize::new(0));
        let cache = RemoteSessionCache::with_timeouts(
            Duration::from_secs(30),
            Duration::from_millis(100),
            Duration::from_millis(10),
        );

        let result = tokio::time::timeout(
            Duration::from_millis(250),
            cache
                .get_or_connect(|| connect_stalled_server(connections.clone(), true, false, false)),
        )
        .await
        .expect("product deadline must complete before the test guard");
        let Err(result) = result else {
            panic!("stalled capability catalog must time out")
        };

        assert_eq!(result.code, ErrorCode::DevupFigmaDirectUnavailable);
        assert!(result.retryable);
        assert_eq!(result.details["reason"], "timeout");
        assert_eq!(result.details["phase"], "listTools");
    }

    #[tokio::test]
    async fn stalled_connection_returns_a_bounded_safe_timeout() {
        let cache = RemoteSessionCache::with_timeouts(
            Duration::from_secs(30),
            Duration::from_millis(10),
            Duration::from_millis(100),
        );

        let result = tokio::time::timeout(
            Duration::from_millis(250),
            cache.get_or_connect(|| {
                std::future::pending::<Result<RunningFigmaClient, DevupError>>()
            }),
        )
        .await
        .expect("product deadline must complete before the test guard");
        let Err(result) = result else {
            panic!("stalled connection must time out")
        };

        assert_eq!(result.code, ErrorCode::DevupFigmaDirectUnavailable);
        assert!(result.retryable);
        assert_eq!(result.details["reason"], "timeout");
        assert_eq!(result.details["phase"], "connect");
    }

    #[tokio::test]
    async fn stalled_tool_call_times_out_and_evicts_its_remote_session() {
        let connections = Arc::new(AtomicUsize::new(0));
        let cache = RemoteSessionCache::with_timeouts(
            Duration::from_secs(30),
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
        let session = cache
            .get_or_connect(|| connect_stalled_server(connections.clone(), false, true, false))
            .await
            .expect("initial session");
        let call = ReadToolCall::metadata("FileKey123", Some("1:2"));

        let error = tokio::time::timeout(
            Duration::from_millis(250),
            cache.call_tool(&session, &call, Duration::from_millis(10)),
        )
        .await
        .expect("product deadline must complete before the test guard")
        .expect_err("stalled tool call must time out");
        assert_eq!(error.code, ErrorCode::DevupFigmaDirectUnavailable);
        assert!(error.retryable);
        assert_eq!(error.details["reason"], "timeout");
        assert_eq!(error.details["phase"], "callTool");

        let replacement = cache
            .get_or_connect(|| connect_stalled_server(connections.clone(), false, false, false))
            .await
            .expect("replacement session");
        assert!(!Arc::ptr_eq(&session.client, &replacement.client));
        assert_eq!(connections.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn ordinary_tool_error_evicts_its_remote_session() {
        let connections = Arc::new(AtomicUsize::new(0));
        let cache = RemoteSessionCache::with_timeouts(
            Duration::from_secs(30),
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
        let session = cache
            .get_or_connect(|| connect_stalled_server(connections.clone(), false, false, true))
            .await
            .expect("initial session");
        let call = ReadToolCall::metadata("FileKey123", Some("1:2"));

        let error = cache
            .call_tool(&session, &call, Duration::from_millis(100))
            .await
            .expect_err("ordinary remote tool error must be classified");
        assert_eq!(error.code, ErrorCode::DevupFigmaDirectUnavailable);
        assert!(error.retryable);

        let replacement = cache
            .get_or_connect(|| connect_stalled_server(connections.clone(), false, false, false))
            .await
            .expect("replacement session");
        assert!(!Arc::ptr_eq(&session.client, &replacement.client));
        assert_eq!(connections.load(Ordering::SeqCst), 2);
    }
}
