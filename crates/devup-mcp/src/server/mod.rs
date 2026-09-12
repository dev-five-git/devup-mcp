pub mod artifacts;
mod asset_jobs;
mod call_cache;
pub mod delivery;
mod diagnostics;
mod feature_trace;
pub mod operation;
pub mod output;
mod pacing;
mod project_context;
mod project_root;
mod projection;
mod quality;
pub mod resources;
mod result_contract;
mod stack_diff;
mod tools;
mod validation;
mod validation_guidance;
mod verdict_scope;
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
    AuthStatus, ClientCredentialSource, ClientCredentials, CollectedParts, CollectedPayload,
    CollectionRequest, CollectionScope, CollectorSession, CollectorStep, CredentialStore,
    DEFAULT_CLIENT_NAME, DevupError, DirectPathSnapshot, ErrorCode, ExploreCandidate, ExploreKind,
    ExploreNode, ExploreReadOptions, FigmaTarget, FigmaUpstream, KeyringClientCredentialStore,
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
    ProjectContextInput, StackDiffInput, UiValidateInput,
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
    #[schemars(extend("enum" = ["status", "resume"]))]
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

    fn production(figma_direct: crate::FigmaDirectConfig) -> Result<Self, DevupError> {
        let mut oauth = OAuthManager::with_endpoint(FIGMA_ENDPOINT, KeyringCredentialStore)?
            .with_client_credential_store(Arc::new(KeyringClientCredentialStore));
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
        let upstream = RemoteFigmaClient::new(oauth.clone());
        Ok(Self::new(Arc::new(oauth), Arc::new(upstream)))
    }
}

#[derive(Clone)]
pub struct DevupServer {
    tool_router: ToolRouter<Self>,
    services: Services,
    artifacts: ArtifactStore,
    output_policy: OutputPolicy,
    asset_jobs: asset_jobs::AssetJobs,
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
            asset_jobs: asset_jobs::AssetJobs::default(),
            services,
            artifacts: ArtifactStore::default(),
            output_policy: OutputPolicy::from_roots(roots)?,
        })
    }

    pub fn production_with_config(
        roots: Vec<std::path::PathBuf>,
        figma_direct: crate::FigmaDirectConfig,
    ) -> Result<Self, DevupError> {
        Self::with_output_roots(Services::production(figma_direct)?, roots)
    }

    pub fn production_with_output_roots(
        roots: Vec<std::path::PathBuf>,
    ) -> Result<Self, DevupError> {
        Self::production_with_config(roots, crate::FigmaDirectConfig::default())
    }
}

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
        let auth_status = self.services.auth.status().await?;
        if auth_status == AuthStatus::Disconnected {
            return Err(DevupError::with_details(
                ErrorCode::DevupAuthRequired,
                "Using the Figma direct connection requires devup_figma_auth login.",
                false,
                json!({"source": "direct"}),
            ));
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

        let mut attempt = 1;
        loop {
            // Before the call, not after the refusal: a collection that paces
            // itself under the ceiling rarely has to be waited out at all.
            self.services.pacer.acquire().await;
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
        let mut collector = CollectorSession::new(request);
        loop {
            if let Some(job) = job {
                job.progress(&collector);
            }
            match collector.advance()? {
                CollectorStep::Call(planned) => {
                    let call_id = planned.id.clone();
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
                                error.details["fileKey"] = json!(target.file_key);
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
                                collector.accept(&call_id, result)?;
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

/// Cuts the ranked candidate list to `limit` and reports the two truncations
/// apart.
///
/// `limit` is applied here rather than inside the projection so that it stays
/// a property of the answer alone: the projection is always asked for the same
/// ceiling, so no choice of `limit` can change how much of the design was read.
/// `truncated` arrives meaning "the snapshot itself was incomplete" and is
/// widened back to "something was cut" only after the two are recorded apart.
fn apply_explore_limit(mut response: Value, limit: usize) -> Value {
    let Some(Value::Array(mut candidates)) = response.get_mut("candidates").map(Value::take) else {
        return response;
    };
    let projection_truncated = response["truncated"].as_bool().unwrap_or(false);
    let found = candidates.len();
    candidates.truncate(limit);
    let returned = candidates.len();
    let candidates_truncated = found > returned;
    let at_ceiling = found >= MATCH_CEILING;

    let reason = match (projection_truncated, candidates_truncated) {
        (false, false) => {
            "Nothing was cut: every screen candidate found is in this answer.".to_owned()
        }
        (false, true) => format!(
            "{found} screen candidates were found and {returned} returned, because limit is \
             {limit}. Nothing is missing from the design - raise limit to see the rest."
        ),
        (true, false) => "The Figma projection this was read from is itself incomplete - the \
                          collection script reached its byte ceiling and left nodes out - so \
                          screens may be missing that no limit would bring back. Explore a \
                          narrower anchor. Text previews use a separate budget and do not \
                          change candidate count, IDs, or projection truncation."
            .to_owned(),
        (true, true) => format!(
            "Two cuts: limit {limit} returned {returned} of {found} candidates, and the Figma \
             projection was itself incomplete, so further screens may exist that no limit \
             would bring back. Raise limit to see collected candidates, or explore a narrower \
             anchor for missing screens. Text previews use a separate budget and do not \
             change candidate count, IDs, or projection truncation."
        ),
    };
    let mut truncation = json!({
        "projection": projection_truncated,
        "candidates": candidates_truncated,
        "returned": returned,
        "found": found,
        "atCeiling": at_ceiling,
        "reason": reason,
    });
    if candidates_truncated {
        truncation["nextAction"] = json!({
            "limit": found.max(returned).min(MATCH_CEILING),
            "note": if at_ceiling {
                "This is the highest limit there is; a Section holding more screens than that \
                 has to be exported in parts."
            } else {
                "The ranked order is stable, so a larger limit extends this list rather than \
                 reshuffling it."
            },
        });
    }
    response["candidates"] = Value::Array(candidates);
    response["count"] = json!(returned);
    // There is no cursor to hand back: the projection is rebuilt per call and
    // has no position to resume from. `truncation` says which lever moves it.
    response["truncated"] = json!(projection_truncated || candidates_truncated);
    response["truncation"] = truncation;
    response
}

/// The same Figma file with no node linked.
fn file_scope_url(target: &FigmaTarget) -> String {
    match &target.branch_key {
        Some(branch_key) => format!(
            "https://www.figma.com/branch/{}/{branch_key}/devup",
            target.file_key
        ),
        None => format!("https://www.figma.com/design/{}/devup", target.file_key),
    }
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
                    "configure requires clientId.",
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
                    "action must be status, login, logout, configure, or doctor.",
                    false,
                )));
            }
        }
        .map_err(to_mcp_error)?;
        Ok(tool_result(json!({ "status": status })))
    }

    #[tool(
        description = "Search Figma pages, sections, frames, and components by name to locate the target before devup_figma_export. \
                       A node-id in the URL scopes the search to that node and everything under it; a URL without one searches the whole file. \
                       The answer says which of the two it did under `scope`, and `limit` is the number of matches returned.",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_figma_search(
        &self,
        Parameters(input): Parameters<FigmaSearchInput>,
    ) -> Result<CallToolResult, ErrorData> {
        // Refused here rather than after collecting, the way the explore tool
        // already refuses one: the projection below asks for the ceiling when
        // the search is scoped, and would otherwise swallow the rejection that
        // an out-of-range limit is owed.
        if !(1..=MATCH_CEILING).contains(&input.limit) {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "The search limit must be between 1 and 100 inclusive.",
                false,
            )));
        }
        let target = FigmaTarget::parse(&input.url).map_err(to_mcp_error)?;
        let scope = SearchScope {
            node_id: target.node_id.clone(),
            limit: input.limit,
            file_url: file_scope_url(&target),
        };
        // Scope the upstream read itself to the linked subtree. Keep the ranked
        // projection above the caller's limit so ancestry filtering happens
        // before the response is cut.
        let collection_scope = if scope.node_id.is_some() {
            CollectionScope::Node
        } else {
            CollectionScope::File
        };
        let projected_limit = if scope.node_id.is_some() {
            MATCH_CEILING
        } else {
            input.limit
        };
        let mut request = CollectionRequest::new(target, collection_scope);
        request.search = Some(SearchReadOptions {
            query: input.query.clone(),
            node_types: input.node_types.clone(),
            match_kind: input.match_kind.clone(),
            limit: projected_limit,
        });
        let result = self
            .start_operation_scoped(
                PendingOperation::Search {
                    query: input.query,
                    node_types: input.node_types,
                    match_kind: input.match_kind,
                    limit: projected_limit,
                },
                request,
                false,
                Some(&scope),
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }

    #[tool(
        description = "Explore screen candidates spatially related to a linked Figma node to locate the right screen before devup_figma_export. \
                       `limit` is the number of candidates returned and nothing else - it never changes how much of the design is read, so raising it can only lengthen the answer. \
                       `truncation` reports the two cuts apart: `candidates` means limit cut the list and raising it returns the rest, `projection` means the collected snapshot was itself incomplete and no limit will bring those screens back. \
                       `includeTextPreview` uses a separate budget; turning it on or off does not change candidate count, IDs, or `truncation.projection`.",
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
                "Exploring neighboring Figma screens requires a node-id.",
                false,
            ))
        })?;
        if !(1..=MATCH_CEILING).contains(&input.limit) {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "The explore limit must be between 1 and 100 inclusive.",
                false,
            )));
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
            text_preview_limit: if input.include_text_preview { 160 } else { 0 },
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
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(apply_explore_limit(result, input.limit)))
    }

    #[tool(
        description = "Export a small Figma selection; the Figma-to-code entry point. Asset requests: recommend 1–3 per call, maximum 6; split larger assetRequests before calling. All fresh exports return exportJob (assetJob compatibility alias) within a one-second initial wait when collection is still running, with per-call frame/root IDs, pagination and elapsed time. Slow asset calls also retain per-asset progress. Poll with jobId; use jobAction=resume when paused. Jobs retain accepted reads/bytes across client timeouts for 30 minutes in this server process, not across restart. Identical arguments recover a lost job reply. Completed results are retained for 5 minutes. Recommend 1–3 frames per call; allow at most 6 frames and 12 frame-times-output units. Budget roughly 5–20 seconds per frame-output unit (15–60 seconds per frame for three outputs) as a planning heuristic, not a guarantee: paging, complexity and throttling can exceed it and clients commonly time out at 300 seconds. Oversized selections are refused before screen collection; split frameIds into one-frame calls when isolating latency, or poll jobId. Partial per-frame projection failures retain successful frame outputs. projectionIssues always explains reported approximations, unclassified layout loss and missing generated-property provenance, even without includeDiagnostics. mappingComplete=false identifies mapping gaps; mapping-incomplete is not value loss and cannot be exact. projectionEvidence includes source fields and calculations for generated attributes. outputPathResults lists supported keys and diagnostics; frame file keys are frame:<nodeId>:<tsx|componentTsx|sourceMap>, while outputPaths reports actual committed writes. Resource responses offer nextAction.tool/arguments for same-artifact body comparison and sizeEstimate with explicit unmeasured wire overhead; coupled asset/path arguments remain together. quality.assets grades binary collection; assetSummary.description explains collection state. Merge saved batch responses offline with devup-mcp --merge-asset-batches batch1.json batch2.json for cumulative collection, unrequested, failed and conflict counts. SECTION links use two stages: receive selection_required, then run nextAction.example to export a selected screen. \
                       Ask only for what you will read: `tsx` is the deliverable, and the response always carries `status`, `quality`, `cache.artifactId`, `collection` and `source` beside it. \
                       `outputs` defaults to `[\"tsx\"]`. Add `devupJson` only when the project has no `devup.json` yet, or when you are introducing tokens it does not define - if it already has one, that file is what the code must match, and `devup_project_context` is what reads it. \
                       If you already know the node ids you want - a brief named them, or an earlier call did - pass them straight to `frameIds` and skip `devup_figma_explore`; exploring to rediscover ids you are already holding spends a Figma call for nothing. Explore is for when a Section link is all you have. \
                       Each output adds its own keys and nothing else - tsx adds `tsx`; componentTsx adds `componentTsx`; responsiveTsx adds `responsiveTsx`, `responsiveSlots` and, where a width asked for something one tree cannot say, `responsiveUnrepresented`; devupJson adds `devupJson`, `themeCounts`, `themeCompleteness`, `conflicts` and `unresolvedVariables`; sourceMap, assetManifest and referencePng each add the key they name. \
                       `rawSnapshot` and `rawPayload` are the collected design in raw form and need `debug: true`. Use them for one question only - a screen looks wrong and you must decide whether the generator is at fault or the design says so - never to implement, since the tsx already carries what they carry. \
                       `fidelity` and `completenessReport` appear only when the result is not exact or complete, or when includeDiagnostics is set. \
                       tsx expands every instance into primitives while componentTsx keeps them as <Name /> references, so requesting both gives the same screen twice and the difference between them is each component's body. responsiveTsx merges every width the capture carries into one module whose differing values are devup-ui responsive arrays, and is produced whenever there is more than one width. \
                       `assetManifest` lists the assets and their ids and writes nothing. To get files, call again with `assetRequests`, giving each entry an `outputPath` under an allowed write root - and make that call with the original `url` rather than `artifactId`, because an acquisition made without asset capture cannot serve asset requests. By default manifest `path` and TSX references remain placeholders. Opt in with `assetPublicRoot`, an existing absolute local directory served at URL /, together with assetRequests.outputPath in this call (saved output mappings are not part of artifacts): files below that root map to percent-encoded relative URLs in manifest and all TSX outputs. Identical exported bytes share one canonical path/outputPath; inspect the returned paths. assetNamesPerNode remains true by default; matching names alone never trigger deduplication. \
                       Reuse a previous acquisition with `artifactId` from `cache` to project further outputs without calling Figma again. Omitted frameIds/allScreens reuse the stored Section selection; explicit selection overrides it. Debug outputs can use resource delivery in auto mode: read the linked resources or follow nextAction to reproject inline. sourceMap is a node/field/generatedProperty/resolution map without offsets; source.generatedOutput identifies the annotated output. Even when reusing artifactId, sourceMap requires tsx, componentTsx or devupJson in outputs in the same call. Example: {\"artifactId\":\"<artifactId>\",\"outputs\":[\"tsx\",\"rawSnapshot\",\"sourceMap\"],\"debug\":true}",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_figma_export(
        &self,
        Parameters(workflow): Parameters<FigmaExportWorkflowInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = workflow.input;
        projection::page_scaffold::validate(input.page_scaffold.as_ref(), &input.outputs)
            .map_err(to_mcp_error)?;
        validation::validate_asset_budget(&input.asset_requests).map_err(to_mcp_error)?;
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
                _ => {
                    return Err(to_mcp_error(DevupError::new(
                        ErrorCode::DevupInvalidInput,
                        "jobAction must be status or resume.",
                        false,
                    )));
                }
            }
            return job
                .wait_briefly()
                .await
                .map(tool_result)
                .map_err(to_mcp_error);
        }
        if workflow.job_action.is_some() {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupInvalidInput,
                "jobAction requires jobId.",
                false,
            )));
        }
        let asset_public_root =
            validation::validate_public_root(workflow.asset_public_root.as_deref())
                .map_err(to_mcp_error)?;
        if asset_public_root.is_some()
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
                .map(|(target, id)| {
                    format!(
                        "{}?node-id={}",
                        file_scope_url(target),
                        id.replace(':', "-")
                    )
                });
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
                        "how":"Request each frame by its direct canonicalUrl with outputs including referencePng and scope=node. For multiple selected frames, repeat once per direct frame URL. If the cached file URL is unavailable, copy the frame link from Figma."}}),
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
                    "artifactId cannot be used together with url or refresh.",
                    false,
                )));
            }
            let artifact = self
                .artifacts
                .get_for_export(artifact_id)
                .await
                .map_err(to_mcp_error)?;
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
                            asset_names_per_node: input.asset_names_per_node,
                            scope: input.scope,
                            strict: input.strict,
                            output_paths: input.output_paths,
                            page_scaffold: input.page_scaffold,
                            frame_ids: input.frame_ids,
                            all_screens: input.all_screens,
                            asset_captures: asset_selections,
                            asset_output_paths,
                            asset_public_root: asset_public_root.clone(),
                            delivery,
                        },
                        request,
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
                    asset_names_per_node: input.asset_names_per_node,
                    scope: input.scope,
                    strict: input.strict,
                    output_paths: input.output_paths,
                    page_scaffold: input.page_scaffold,
                    frame_ids: input.frame_ids,
                    all_screens: input.all_screens,
                    asset_captures: asset_selections,
                    asset_output_paths,
                    asset_public_root: asset_public_root.clone(),
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
                "Either url or artifactId is required.",
                false,
            ))
        })?;
        let target = FigmaTarget::parse(url).map_err(to_mcp_error)?;
        if input.outputs.iter().any(|output| output == "tsx") && target.node_id.is_none() {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "A TSX export link requires a node-id.",
                false,
            )));
        }
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
                    asset_names_per_node: input.asset_names_per_node,
                    scope: input.scope,
                    strict: input.strict,
                    output_paths: input.output_paths,
                    page_scaffold: input.page_scaffold,
                    frame_ids: input.frame_ids,
                    all_screens: input.all_screens,
                    asset_captures: asset_selections,
                    asset_output_paths,
                    asset_public_root: asset_public_root.clone(),
                    delivery,
                },
                request,
                input.refresh,
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }

    #[tool(
        description = "Read a project's real devup.json theme tokens, openapi.json endpoints/schemas, Vespertide models/*.json tables/columns, or UI component/props/import/route reuse evidence (scope: theme | api | db | ui | all) — read-only, no session cache, never guesses. UI is opt-in and excluded from all: large monorepo inventories must not pollute token context. Identify deployments by server.commit/buildId, not version alone; server.displayVersion is a readable version+buildId.",
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
        description = "Validate DevupUI TSX against a project's real devup.json: unknown $token references, hardcoded colors/lengths, unknown props on Box/Flex/Text/Center/Grid/Image, and non-static values inside css()/globalCss()/keyframes() calls. \
                       `ok` is decided by severity, not by count: errors fail, and `strict: true` also fails warnings. Hardcoded values with exact matching tokens are warnings with token advice; unmatched values are info without token advice and never fail strict mode. `okReason` distinguishes clean, info-only, warnings-only, warnings-and-info, strict-warnings, and error-violations. `themeNotes` summarizes each encountered empty token category once. \
                       `checkedTokens` counts $token references this TSX makes and `availableTokenCount` counts tokens devup.json defines; they count different things and are not a ratio, which `tokens` restates by name.",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_ui_validate(
        &self,
        Parameters(input): Parameters<UiValidateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        use devup_mcp_devup_ui::ui_validate::{bundle_theme, check_bundle, validate_bundle};
        let invalid = |message: String| ErrorData::invalid_params(message, None);
        let theme_lookup = if let Some(files) = &input.files {
            check_bundle(files).map_err(invalid)?;
            if !input.tsx.is_empty() {
                return Err(invalid("Supply either tsx or files, not both".into()));
            }
            let theme = bundle_theme(files).map_err(invalid)?;
            let guardrail = theme.is_none().then(|| json!({
                "message": "No devup.json supplied in files; token checks skipped. Bundle validation never reads the filesystem."
            }));
            project_context::ThemeLookup { theme, guardrail }
        } else {
            project_context::theme_for_validation(input.project_root.as_deref())
                .map_err(to_mcp_error)?
        };
        let report = if let Some(files) = &input.files {
            validate_bundle(files, theme_lookup.theme.as_ref(), input.strict).map_err(invalid)?
        } else {
            devup_mcp_devup_ui::ui_validate::validate_devup_ui_tsx(
                &input.tsx,
                theme_lookup.theme.as_ref(),
                input.strict,
            )
        };
        // These keys are assembled here rather than serialized from
        // `UiValidation`, so anything the struct's own documentation
        // explains reaches nobody unless it is answered here too. Three
        // things in this response cannot be read off it:
        //
        // `ok: true` next to ten violations looks like a contradiction
        // until you know severity decides it and count does not, so the
        // severity split is reported and `okReason` names the case.
        //
        // `checkedTokens: 4` next to `availableTokenCount: 81` reads as
        // "4 of 81 checked". It is not: one counts references the code
        // makes, the other definitions the theme holds. `tokens` says
        // which is which in the key names, where a reader cannot miss it.
        //
        // And with no theme the token check is skipped rather than passed,
        // which a bare `checkedTokens` cannot distinguish.
        use devup_mcp_devup_ui::ui_validate::Severity;
        let count_severity = |severity| {
            report
                .violations
                .iter()
                .filter(|finding| finding.severity == severity)
                .count()
        };
        let errors = count_severity(Severity::Error);
        let warnings = count_severity(Severity::Warning);
        let infos = count_severity(Severity::Info);
        let ok_reason = match (errors, warnings, infos, input.strict) {
            (1.., _, _, _) => "error-violations",
            (_, 1.., _, true) => "strict-warnings",
            (_, 1.., 1.., _) => "warnings-and-info",
            (_, 1.., _, _) => "warnings-only",
            (_, _, 1.., _) => "info-only",
            _ => "clean",
        };
        let mut result = json!({
            "ok": report.ok,
            "okReason": ok_reason,
            "strict": input.strict,
            "violations": report.violations,
            "violationCounts": { "error": errors, "warning": warnings, "info": infos },
            "themeNotes": report.theme_notes,
            "checkedTokens": report.checked_tokens,
            "availableTokenCount": report.available_token_count,
            "tokens": {
                "referencedByTsx": report.checked_tokens,
                "definedByTheme": report.available_token_count,
                "unknownTokenCheckRan": theme_lookup.theme.is_some(),
            },
            "themeAvailable": theme_lookup.theme.is_some(),
            "themeGuardrail": theme_lookup.guardrail,
        });
        if let Some(name) = &input.source_name {
            for finding in result["violations"].as_array_mut().unwrap() {
                finding["sourceName"] = json!(name);
            }
        }
        if !report.ok {
            result.as_object_mut().unwrap().extend(
                validation_guidance::guidance(&input, &report, theme_lookup.theme.as_ref())
                    .as_object()
                    .unwrap()
                    .clone(),
            );
        }
        Ok(tool_result(result))
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

    #[tool(
        description = "Compare consumer-produced actual PNG with exactly one reference PNG path or cached artifactId. Paths must be allowlisted. Never renders, launches a browser, or runs commands. Default threshold is 0.005 (0.5 percent). Supply the content-free visual renderer contract environment manifest; absent, incomplete, or invalid environment yields verdict inconclusive even when visual.passed is true. Optional diff PNG uses auto|inline|resource delivery.",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_visual_compare(
        &self,
        Parameters(input): Parameters<tools::VisualCompareInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = visual_compare::compare(input, &self.output_policy, &self.artifacts)
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }

    #[tool(
        description = "Read-only cross-layer feature slice and acceptance matrix from explicit anchors: routePath, figmaNodeId/artifactId, operationId or apiPath+method, componentPath, tableName. Refuses anchorless prose. Returns evidence-backed or UNVERIFIED hops, generated-source ownership, ranked UI reuse, literal design binding versus request/response fields, required-state coverage, and named truncation caps. Optional requirement/acceptanceCriteria are echoed without semantic interpretation. componentTsx is a caller-declared Figma export; artifactId uses the cached snapshot. Static parsing never proves runtime behavior.",
        output_schema = permissive_object_output_schema()
    )]
    async fn devup_feature_trace(
        &self,
        Parameters(input): Parameters<tools::FeatureTraceInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = feature_trace::run(input, &self.artifacts)
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
            text_preview: candidate.text_preview.clone(),
            text_preview_state: candidate.text_preview_state.clone(),
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
            "scope must be node, page, or file.",
            false,
        )),
    }
}

fn with_error_identity(mut error: ErrorData) -> ErrorData {
    let mut data = error.data.take().unwrap_or_else(|| json!({}));
    if !data.is_object() {
        data = json!({"details":data});
    }
    data["server"] = delivery::server_identity();
    error.data = Some(data);
    error
}

/// Maps a [`DevupError`] onto the JSON-RPC error the caller actually sees.
///
/// The protocol code is not decoration here: the caller is usually an agent
/// choosing between "fix the arguments and call again" and "stop and report",
/// and every error used to arrive as INTERNAL_ERROR, which says the second
/// thing about both. A mistake in the call itself is INVALID_PARAMS, so that
/// decision can be made without parsing the message. `data` keeps carrying
/// the exact `code` and `retryable`, unchanged.
fn to_mcp_error(mut error: DevupError) -> ErrorData {
    if error.code == ErrorCode::DevupFigmaHandoffExpired
        && error.details.get("exportJob").is_none()
        && error.details.get("assetJob").is_none()
    {
        if !error.details.is_object() {
            error.details = json!({"upstreamDetails":error.details});
        }
        if error.details.get("stage").is_none() {
            error.details["stage"] = json!("artifact-lookup");
        }
        if error.details.get("artifactState").is_none() {
            error.details["artifactState"] = json!("unknown");
        }
        let recovery = error.details.get("recoveryArguments").cloned();
        error.details["recoveryState"] = json!(if recovery.is_some() {
            "available"
        } else {
            "unrecoverable"
        });
        if recovery.is_none() {
            error.details["recoveryReason"] = json!(
                "Original URL/selection cannot be recovered from this server. The ID may be unknown, evicted beyond metadata retention, or lost on restart; the cause cannot be determined."
            );
        }
        error.details["nextAction"] = json!({"tool":"devup_figma_export",
            "arguments":{"refresh":true},"requiredArguments":["url"],"omitArguments":["artifactId"],
            "how":"Use the original Figma SECTION URL and frameIds (or allScreens) with refresh:true to collect a new artifact. Then reuse the new cache.artifactId; do not send the expired artifactId with url or refresh."});
        if let Some(arguments) = recovery {
            error.details["nextAction"]["arguments"] = arguments;
        }
    }
    let mcp_code = if validation::is_caller_mistake(&error) {
        McpErrorCode::INVALID_PARAMS
    } else {
        McpErrorCode::INTERNAL_ERROR
    };
    ErrorData::new(
        mcp_code,
        error.message,
        Some(
            json!({ "code": error.code, "retryable": error.retryable, "details": error.details, "server":delivery::server_identity() }),
        ),
    )
}

fn structured_tool_error(error: ErrorData) -> rmcp::model::CallToolResult {
    let mut data = error.data.unwrap_or_else(|| json!({}));
    if !data.is_object() {
        data = json!({"details":data});
    }
    data.as_object_mut().unwrap().remove("server");
    if data.get("code").is_none() {
        data["code"] = json!("DEVUP_INVALID_INPUT");
    }
    if data.get("retryable").is_none() {
        data["retryable"] = json!(false);
    }
    if data.get("details").is_none() {
        data["details"] = json!({});
    }
    data["message"] = json!(error.message);
    data["rpcCode"] = json!(error.code);
    let mut result = tool_result(json!({"error":data}));
    result.is_error = Some(true);
    result
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DevupServer {
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, ErrorData> {
        let call = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        let mut response = match self.tool_router.call(call).await {
            Ok(response) => response,
            Err(error) if error.data.as_ref().and_then(|d| d.get("code")).is_some() => {
                return Ok(structured_tool_error(error).into());
            }
            Err(error) => return Err(with_error_identity(error)),
        };
        if let rmcp::model::CallToolResponse::Complete(result) = &mut response
            && result.is_error == Some(true)
        {
            // Router argument decoding failures arrive as content-only tool
            // errors. Give them the same error envelope as execution errors.
            if result
                .structured_content
                .as_ref()
                .and_then(|v| v.get("error"))
                .is_none()
            {
                *result = structured_tool_error(ErrorData::invalid_params(
                    "Invalid tool arguments.",
                    Some(json!({"details":{"content":result.content}})),
                ));
            }
        }
        Ok(response)
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::new("devup-mcp", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "Build identity: identify deployments by server.commit/buildId, not version alone; server.displayVersion combines version and buildId. Reconnect the MCP server if the expected build differs.\n\
             1. devup-mcp is the primary source for turning a Figma design into code. Do not replace it with another source.\n\
             2. When the goal is implementation, call devup_figma_export first and take tsx. That is the deliverable; a complete response marks it with deliverable.isFinal.\n\
             2a. Ask for an output only when you will read it. Measured on the Korean WQUW-120 modal, semantic sourceMap is about 5.4x TSX (48,832 versus 9,099 UTF-8 bytes; 12.6% smaller than its former offset map). Sizes vary by screen; earlier rawPayload/rawSnapshot measurements were about 7x/2x TSX, so requesting them by default spends most of the response on bytes nothing reads. sourceMap records nodeId, original property, generatedProperty and resolution, plus optional variableId (variable token), styleId (style token), assetId (asset reference), with no character/byte offsets. raw-fallback means a raw-value mapping, not necessarily an inaccurate value; verified-explicit-dimension checks emitted pixels equal the source dimension, while verified-layout-sizing checks sizing intent against emitted CSS. exact verifies that field-to-property mapping, not rendered pixel equivalence. Use generatedSource diagnostics for node code excerpts; rawSnapshot and rawPayload are for banking a capture as an offline fixture. componentTsx is the same screen with instances left as <Name /> references, and responsiveTsx appears on its own whenever the capture carries more than one width.\n\
             3. get_design_context, screenshots, and visual reasoning are verification aids only. Do not overwrite devup-mcp output.\n\
             4. Do not hand-interpret a node tree to write devup-ui code. Do not infer layout from coordinates.\n\
             5. If a devup-mcp call fails, record it explicitly. Do not silently route around it.\n\
             6. Do not guess UI values such as color, spacing, radius, or typography. If you could not obtain them, stop and report.\n\
             7. Do not implement a Section link as one whole subtree. Check the selection_required candidates and continue with bounded per-screen frameIds batches in nextAction; allScreens is valid only when the complete list fits the advertised frame/output budget.\n\
             8. The generated component name comes from the Figma layer name and is a starting point, not a contract. Rename it to fit the codebase, and rename a name that is meaningless or not a valid identifier.\n\
             10. An asset path in the output, such as a maskImage or Image src, is a placeholder built from the layer name. Rename the file to fit the project. If the asset varies per usage, lift it into a prop instead of hardcoding it.\n\
             11. A fixed asset such as an icon must actually be exported, never referenced by a path that does not exist yet. Read assetManifest for the asset IDs, then call devup_figma_export again with assetRequests, giving each entry an outputPath under an allowed write root, and make the path in the code match the path you wrote.\n\
             12. Prefer delivery: \"resource\" for assets and large outputs. devup-mcp then returns devup://artifact/... resource links to read on demand instead of inlining bytes in every response.",
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
            .map_err(|_| {
                with_error_identity(ErrorData::resource_not_found("resource not found", None))
            })
    }
}

#[cfg(test)]
mod p3_error_tests {
    use super::*;

    #[test]
    fn p3_expired_artifact_is_state_failure_with_original_message() {
        let error = to_mcp_error(DevupError::new(
            ErrorCode::DevupFigmaHandoffExpired,
            "The Figma artifact is missing or expired.",
            true,
        ));
        assert_eq!(error.code, McpErrorCode::INTERNAL_ERROR);
        assert_eq!(error.message, "The Figma artifact is missing or expired.");
    }

    #[test]
    fn p3_output_root_violation_is_invalid_params() {
        let policy = OutputPolicy::from_roots(vec![std::env::current_dir().unwrap()]).unwrap();
        let error = policy.resolve("../outside.svg").err().unwrap();
        assert_eq!(to_mcp_error(error).code, McpErrorCode::INVALID_PARAMS);
    }
}

#[cfg(test)]
mod r7_error_tests {
    use super::*;
    #[test]
    fn r7_errors_have_the_same_build_identity_as_success() {
        let success = serde_json::to_value(tool_result(json!({"status":"complete"}))).unwrap();
        let success: Value =
            serde_json::from_str(success["content"][0]["text"].as_str().unwrap()).unwrap();
        for code in [
            ErrorCode::DevupInvalidInput,
            ErrorCode::DevupSnapshotUnsupported,
            ErrorCode::DevupFigmaHandoffExpired,
        ] {
            let error = to_mcp_error(DevupError::new(code, "failure", false));
            assert_eq!(error.data.as_ref().unwrap()["server"], success["server"]);
        }
    }
    #[test]
    fn r7_plugin_expiry_has_recovery_and_keeps_its_stage() {
        let raw = json!({"isError":true,"content":[{"type":"text","text":"DEVUP_FIGMA_HANDOFF_EXPIRED"}]});
        let error = to_mcp_error(operation::upstream_error(&raw).unwrap());
        let data = error.data.unwrap();
        assert_eq!(data["code"], "DEVUP_FIGMA_HANDOFF_EXPIRED");
        assert_eq!(data["details"]["stage"], "plugin-execution");
        assert_eq!(data["details"]["nextAction"]["arguments"]["refresh"], true);
    }

    #[test]
    fn r7_expired_artifact_explains_refresh_arguments() {
        let error = to_mcp_error(DevupError::new(
            ErrorCode::DevupFigmaHandoffExpired,
            "expired",
            true,
        ));
        let d = &error.data.as_ref().unwrap()["details"];
        assert_eq!(d["stage"], "artifact-lookup");
        assert_eq!(d["nextAction"]["tool"], "devup_figma_export");
        assert_eq!(d["nextAction"]["arguments"]["refresh"], true);
        assert!(
            d["nextAction"]["omitArguments"]
                .as_array()
                .unwrap()
                .contains(&json!("artifactId"))
        );
    }
}

#[cfg(test)]
mod r8_recovery_tests {
    use super::*;
    #[test]
    fn r8_expired_artifact_returns_known_url_or_explicit_unrecoverable_state() {
        for known in [true, false] {
            let details = if known {
                json!({"artifactState":"expired","recoveryArguments":{"url":"https://www.figma.com/design/test?node-id=1-2","frameIds":["2:1"],"refresh":true}})
            } else {
                json!({})
            };
            let e = to_mcp_error(DevupError::with_details(
                ErrorCode::DevupFigmaHandoffExpired,
                "expired",
                true,
                details,
            ));
            let d = &e.data.unwrap()["details"];
            if known {
                assert_eq!(
                    d["nextAction"]["arguments"]["url"],
                    "https://www.figma.com/design/test?node-id=1-2"
                );
            } else {
                assert_eq!(d["artifactState"], "unknown");
                assert_eq!(d["recoveryState"], "unrecoverable");
            }
        }
    }
}
