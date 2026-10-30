#[path = "../artifacts.rs"]
pub mod artifacts;
#[path = "../asset_guard.rs"]
mod asset_guard;
#[path = "../asset_job_progress.rs"]
mod asset_job_progress;
#[path = "../asset_jobs.rs"]
mod asset_jobs;
#[path = "../audit_plan.rs"]
mod audit_plan;
#[path = "../call_cache.rs"]
mod call_cache;
#[path = "../compact.rs"]
mod compact;
#[path = "../delivery.rs"]
pub mod delivery;
#[path = "../design_drift.rs"]
mod design_drift;
#[path = "../diagnostics.rs"]
mod diagnostics;
#[path = "../export_record.rs"]
mod export_record;
#[path = "../feature_trace.rs"]
mod feature_trace;
#[cfg(test)]
#[path = "../feedback_interactions_tests.rs"]
mod feedback_interactions_tests;
#[path = "../field_select.rs"]
mod field_select;
#[path = "../framework.rs"]
mod framework;
#[path = "../guide.rs"]
mod guide;
#[path = "../interactions.rs"]
mod interactions;
#[path = "../mask_guard.rs"]
mod mask_guard;
#[path = "../measurement.rs"]
mod measurement;
#[path = "../navigation.rs"]
mod navigation;
#[path = "../operation.rs"]
pub mod operation;
#[path = "../output.rs"]
pub mod output;
#[path = "../pacing.rs"]
mod pacing;
#[path = "../project_context.rs"]
mod project_context;
#[path = "../project_plan/mod.rs"]
mod project_plan;
#[path = "../project_root.rs"]
mod project_root;
#[path = "../projection.rs"]
mod projection;
#[path = "../quality.rs"]
mod quality;
#[path = "../release_check.rs"]
mod release_check;
#[path = "../resources.rs"]
pub mod resources;
#[path = "../result_contract.rs"]
mod result_contract;
#[path = "../review.rs"]
mod review;
#[path = "../runtime_audit.rs"]
mod runtime_audit;
#[path = "../screen_plan.rs"]
mod screen_plan;
#[path = "../screenshot_observations.rs"]
mod screenshot_observations;
#[path = "../section_checklist.rs"]
mod section_checklist;
#[path = "../self_update.rs"]
pub mod self_update;
#[path = "../skills.rs"]
mod skills;
#[cfg(test)]
#[path = "../source_limits.rs"]
mod source_limits;
#[path = "../stack_diff.rs"]
mod stack_diff;
#[path = "../tools.rs"]
mod tools;
#[path = "../validation.rs"]
mod validation;
#[path = "../validation_guidance.rs"]
mod validation_guidance;
#[path = "../verdict_scope.rs"]
mod verdict_scope;
#[path = "../visual_compare.rs"]
mod visual_compare;

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
    AuthStatus, BRIDGE_CURRENT_KEY, BridgeFigmaClient, BridgeOptions, BridgeRole, BridgeServer,
    ClientCredentialSource, ClientCredentials, CollectedParts, CollectedPayload, CollectionRequest,
    CollectionScope, CollectorSession, CollectorStep, CredentialStore, DEFAULT_CLIENT_NAME,
    DevupError, DirectPathSnapshot, ErrorCode, ExploreCandidate, ExploreKind, ExploreNode,
    ExploreReadOptions, FallbackUpstream, FigmaTarget, FigmaUpstream, KeyringClientCredentialStore,
    KeyringCredentialStore, OAuthManager, ReadToolCall, RemoteFigmaClient, ResourceScope,
    SearchReadOptions, SecretString, SectionCandidate, SectionIndex, SectionReadOptions, Snapshot,
    SystemBrowser, TokenState, UpstreamResult,
};

use artifacts::{ArtifactKind, ArtifactRequestKey, ArtifactStore};
use call_cache::CallCache;
use delivery::{DeliveryMode, tool_result};
use operation::PendingOperation;
use output::OutputPolicy;
use pacing::CallPacer;
use projection::complete_operation;
use validation::{
    parse_asset_requests, parse_collection_scope, parse_root_layout, validate_artifact_projection,
    validate_outputs,
};

pub use tools::{
    AuthInput, FigmaAssetRequestInput, FigmaExploreInput, FigmaExportInput, FigmaSearchInput,
    ProjectContextInput, SkillsInput, StackDiffInput, UiValidateInput,
};

/// Additive workflow options; existing input defaults remain in tools.rs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaExportWorkflowInput {
    #[serde(flatten)]
    pub input: FigmaExportInput,
    /// Opt in to rewriting exported asset references. Existing absolute local
    /// directory served at URL /. Only exported files below it are mapped;
    /// URL path segments are percent-encoded. Omit to keep placeholder paths.
    #[serde(default)]
    pub asset_public_root: Option<String>,

    /// Poll a server-local asset job. Omit url/artifactId/assetRequests when polling.
    #[serde(default)]
    pub job_id: Option<String>,
    /// status (default) or resume a paused asset job without repeating accepted reads.
    #[serde(default)]
    #[schemars(extend("enum" = ["status", "resume", "cancel"]))]
    pub job_action: Option<String>,
}

const FIGMA_ENDPOINT: &str = "https://mcp.figma.com/mcp";

/// The most matches `devup_figma_search` and screen candidates
/// `devup_figma_explore` rank before the caller's `limit` cuts the answer.
///
/// Both tools rank first and cut afterwards, so the ranked list has to be
/// built past `limit` or the cut decides the answer. In search it decided it
/// wrongly: five frames share the name `Loading` across the file, ranking put
/// the two outside the linked Section first, and a `limit` of five removed
/// both of the frames the caller had actually pointed at. This is also the
/// ceiling both tools already validate `limit` against, so nothing above it
/// could have been asked for.
const MATCH_CEILING: usize = 100;

/// The byte-and-node budget every `devup_figma_explore` collection is given,
/// which is the highest the collection script accepts. It is a constant on
/// purpose - see the comment where it is used.
const EXPLORE_PROJECTION_LIMIT: usize = 400;

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
            client_name: DEFAULT_CLIENT_NAME.to_owned(),
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
            "This auth backend does not support configuring client credentials.",
            false,
        ))
    }
}

/// The system browser, with the URL said aloud first.
///
/// Two logins in a row timed out waiting for a callback that never came, and
/// nothing said whether the browser had opened at all. `webbrowser::open`
/// reports failure to launch but not a launch into a window nobody is
/// looking at, and the URL it was handed was kept nowhere. So it is logged
/// before the browser is asked: if the tab does not appear, the URL is in
/// stderr to be opened by hand. Nothing in it is a secret — the verifier
/// stays in the process, and the challenge, state and client_id are what
/// the address bar shows anyway.
struct SpokenBrowser;

impl devup_mcp_figma::BrowserOpener for SpokenBrowser {
    fn open(&self, authorization_url: &str) -> Result<(), DevupError> {
        // stderr is where this binary's traces go; stdout is MCP frames only.
        eprintln!(
            "devup-mcp: opening the browser for Figma authorization. \
             If no tab appears, open this URL by hand:\n{authorization_url}"
        );
        SystemBrowser.open(authorization_url)
    }
}

#[async_trait]
impl<S: CredentialStore> DevupAuth for OAuthManager<S> {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        OAuthManager::status(self).await
    }

    async fn login(&self) -> Result<AuthStatus, DevupError> {
        OAuthManager::login(self, &SpokenBrowser).await?;
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
    /// Shared, so that concurrent collections meter against one ceiling rather
    /// than one each and together exceed it.
    pacer: Arc<CallPacer>,
    /// Off unless a directory was named, in which case each read that succeeds
    /// is kept so a later attempt need not pay for it again.
    call_cache: Arc<CallCache>,
}

impl Services {
    pub fn new(auth: Arc<dyn DevupAuth>, upstream: Arc<dyn FigmaUpstream>) -> Self {
        Self {
            auth,
            upstream,
            pacer: Arc::new(CallPacer::from_env()),
            call_cache: Arc::new(CallCache::from_env()),
        }
    }

    /// Names the directory to bank calls in, rather than reading it from the
    /// environment. A caller running two collections against one bank needs to
    /// say which bank without setting a variable the whole process shares.
    pub fn with_call_cache_dir(
        auth: Arc<dyn DevupAuth>,
        upstream: Arc<dyn FigmaUpstream>,
        directory: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            auth,
            upstream,
            pacer: Arc::new(CallPacer::from_env()),
            call_cache: Arc::new(CallCache::new(
                directory,
                std::time::Duration::from_secs(48 * 60 * 60),
            )),
        }
    }

    fn production(
        figma_direct: crate::FigmaDirectConfig,
        bridge: Bridge,
    ) -> Result<Self, DevupError> {
        // To check what happens without a Figma login while the person stays
        // logged in: the login and the client credential both kept in memory,
        // empty at start, so the operating system's store is neither read nor
        // written.
        if std::env::var("DEVUP_FIGMA_CREDENTIAL_STORE").as_deref() == Ok("memory") {
            return Self::assemble(
                devup_mcp_figma::MemoryCredentialStore::default(),
                Arc::new(devup_mcp_figma::MemoryClientCredentialStore::default()),
                figma_direct,
                bridge,
            );
        }
        Self::assemble(
            KeyringCredentialStore,
            Arc::new(KeyringClientCredentialStore),
            figma_direct,
            bridge,
        )
    }

    fn assemble<S: devup_mcp_figma::CredentialStore>(
        store: S,
        client_store: Arc<dyn devup_mcp_figma::ClientCredentialStore>,
        figma_direct: crate::FigmaDirectConfig,
        bridge: Bridge,
    ) -> Result<Self, DevupError> {
        let mut oauth = OAuthManager::with_endpoint(FIGMA_ENDPOINT, store)?
            .with_client_credential_store(client_store);
        if figma_direct.callback_port.is_some() {
            oauth = oauth.with_callback_port(figma_direct.callback_port);
        }
        if let Some(client_name) = figma_direct.client_name {
            oauth = oauth.with_client_name(client_name);
        }
        // Three minutes is enough when the browser opens itself and the person
        // is already looking at it. It is not enough when the URL has to be
        // carried to them by hand — read from a log, pasted into a chat, opened
        // a few minutes later — which is how a login came back "timed out"
        // after being approved: the approval arrived at a listener that had
        // already closed. Whoever is carrying the URL sets this.
        if let Some(seconds) = std::env::var("DEVUP_FIGMA_CALLBACK_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|seconds| *seconds > 0)
        {
            oauth = oauth.with_callback_timeout(std::time::Duration::from_secs(seconds));
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
        let remote = RemoteFigmaClient::new(oauth.clone());
        // A script read goes to the bridge when a plugin is reachable - through
        // this process's own port, or through the devup-mcp that holds it. With
        // no plugin attached every call falls straight through to the remote
        // path, so opening the door costs nothing when nobody walks through.
        let bridge = match bridge {
            Bridge::FromEnvironment => BridgeServer::from_env_with(BridgeOptions {
                build_id: Some(crate::build_id().to_owned()),
                secret_path: None,
            }),
            Bridge::Off => None,
        };
        let pacer = Arc::new(CallPacer::from_env());
        let upstream: Arc<dyn FigmaUpstream> = match bridge {
            Some(bridge) => Arc::new(
                FallbackUpstream::new(
                    BridgeFigmaClient::new(bridge.state()).with_port(bridge.port()),
                    remote,
                )
                .with_meter(pacer.clone()),
            ),
            None => Arc::new(remote),
        };
        let mut services = Self::new(Arc::new(oauth), upstream);
        services.pacer = pacer;
        Ok(services)
    }
}

#[derive(Clone)]
pub struct DevupServer {
    tool_router: ToolRouter<Self>,
    services: Services,
    artifacts: ArtifactStore,
    output_policy: OutputPolicy,
    asset_jobs: asset_jobs::AssetJobs,
    /// The MCP client's own name, learned at `initialize` and kept because
    /// the tool methods never see the request context.
    ///
    /// `Arc` so every clone of the server reads the one that was filled in:
    /// a per-clone copy would be empty in whichever clone the handshake did
    /// not run through, and the skill roots would silently go back to
    /// guessing.
    client_name: std::sync::Arc<std::sync::OnceLock<String>>,
}

impl DevupServer {
    /// Which agent runtime is on the other end, for choosing skill roots.
    ///
    /// `Unknown` before the handshake or from a client that sends no name,
    /// and that case installs every convention rather than picking one.
    fn client_name(&self) -> Option<&str> {
        self.client_name.get().map(String::as_str)
    }

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
            asset_jobs: asset_jobs::AssetJobs::default(),
            services,
            artifacts: ArtifactStore::default(),
            output_policy: OutputPolicy::from_roots(roots)?,
            client_name: std::sync::Arc::new(std::sync::OnceLock::new()),
        })
    }

    pub fn production_with_config(
        roots: Vec<std::path::PathBuf>,
        figma_direct: crate::FigmaDirectConfig,
    ) -> Result<Self, DevupError> {
        Self::production_with_bridge(roots, figma_direct, Bridge::FromEnvironment)
    }

    /// The production stack, with the Devup Bridge opened as `bridge` says.
    pub fn production_with_bridge(
        roots: Vec<std::path::PathBuf>,
        figma_direct: crate::FigmaDirectConfig,
        bridge: Bridge,
    ) -> Result<Self, DevupError> {
        Self::with_output_roots(Services::production(figma_direct, bridge)?, roots)
    }

    /// The production stack for use inside another process - a test, an
    /// embedder - so the Devup Bridge stays shut: the plugin's port belongs
    /// to the devup-mcp sessions the machine is running.
    pub fn production_with_output_roots(
        roots: Vec<std::path::PathBuf>,
    ) -> Result<Self, DevupError> {
        Self::production_with_bridge(roots, crate::FigmaDirectConfig::default(), Bridge::Off)
    }
}

/// Whether a production server opens the Devup Bridge.
///
/// Opening it binds the plugin's port, or reads through the devup-mcp that
/// holds it - which is what a serving process should do, and exactly what a
/// process that only checks its own configuration (`--self-check`) or a test
/// building the stack in-process should not: port 1993 belongs to whatever
/// sessions the machine runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bridge {
    /// `DEVUP_FIGMA_BRIDGE_PORT` decides, as in a serving process.
    FromEnvironment,
    Off,
}
