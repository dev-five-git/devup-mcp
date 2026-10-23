//! Server-local acquisition checkpoints. The worker owns its collector even
//! when the caller drops its MCP future; pausing never rejects the pending read.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use devup_mcp_figma::{
    CollectionRequest, CollectorSession, DevupError, ErrorCode, ReadToolCall, UpstreamResult,
};
use serde_json::{Value, json};

use super::asset_job_progress::{request_key, request_progress};
use sha2::{Digest, Sha256};
use tokio::{sync::Notify, time::Instant};

use super::{DevupServer, operation::PendingOperation};

const CALL_TIMEOUT: Duration = devup_mcp_figma::READ_TIME_LIMIT;
const JOB_LIFETIME: Duration = Duration::from_secs(30 * 60);
const RESULT_RETENTION: Duration = Duration::from_secs(5 * 60);
const MAX_JOBS: usize = 8;
const MAX_COMPLETED_JOBS: usize = 16;

/// What a compact asset row keeps: enough to see progress, verify bytes and
/// find the file, without the per-asset diagnostics.
const COMPACT_ASSET_FIELDS: &[&str] = &[
    "assetId",
    "status",
    "byteLength",
    "sha256",
    "errorCode",
    "errorDetail",
    "acquisitionState",
    "outputPath",
    "requestedOutputPath",
    "fileState",
    "fileStateReason",
    "suggestedFilename",
    "duplicateOf",
];

#[derive(Clone, Default)]
pub(super) struct AssetJobs(Arc<Mutex<BTreeMap<String, Arc<AssetJob>>>>);

pub(super) struct AssetJob {
    id: String,
    key: String,
    started: Instant,
    paths: BTreeMap<String, String>,
    budget: Value,
    state: Mutex<JobState>,
    resume: Notify,
    cancellation: Notify,
    cancelled: std::sync::atomic::AtomicBool,
    /// The delivery the first call asked for; every poll keeps it.
    delivery: super::delivery::DeliveryMode,
}

struct JobState {
    state: &'static str,
    stage: &'static str,
    assets: Vec<Value>,
    calls: Vec<Value>,
    completed_calls: usize,
    error: Option<DevupError>,
    result: Option<Result<Value, DevupError>>,
    finished: Option<Instant>,
}

impl AssetJobs {
    fn acquire(
        &self,
        operation: &PendingOperation,
        request: &CollectionRequest,
        refresh: bool,
    ) -> Result<(Arc<AssetJob>, bool), DevupError> {
        // These structures contain targets and output settings, never credentials.
        let key = Sha256::digest(format!("{operation:?}:{request:?}:{refresh}"))
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let mut jobs = self.0.lock().unwrap();
        jobs.retain(|_, job| {
            let state = job.state.lock().unwrap();
            state
                .finished
                .is_none_or(|at| at.elapsed() < RESULT_RETENTION)
                && job.started.elapsed() < JOB_LIFETIME
        });
        if let Some(job) = jobs
            .values()
            .find(|job| job.key == key && (!refresh || job.state.lock().unwrap().result.is_none()))
        {
            return Ok((job.clone(), false));
        }
        let active = jobs
            .values()
            .filter(|job| job.state.lock().unwrap().result.is_none())
            .count();
        if active >= MAX_JOBS {
            return Err(DevupError::with_details(
                ErrorCode::DevupInvalidInput,
                "Export job capacity reached. Poll/resume existing active jobs before starting more work.",
                true,
                json!({"maxAssetJobs":MAX_JOBS,"jobIds":jobs.keys().collect::<Vec<_>>()}),
            ));
        }
        // Completed replies are a separate bounded cache, not active capacity.
        while jobs.len().saturating_sub(active) >= MAX_COMPLETED_JOBS {
            let oldest = jobs
                .iter()
                .filter_map(|(id, job)| {
                    job.state
                        .lock()
                        .unwrap()
                        .finished
                        .map(|at| (id.clone(), at))
                })
                .min_by_key(|(_, at)| *at)
                .map(|(id, _)| id);
            if let Some(id) = oldest {
                jobs.remove(&id);
            } else {
                break;
            }
        }
        let PendingOperation::Export {
            asset_output_paths,
            delivery,
            ..
        } = operation
        else {
            unreachable!()
        };
        let id = format!("asset-{:016x}", rand::random::<u64>());
        let job = Arc::new(AssetJob {
            id: id.clone(),
            key,
            started: Instant::now(),
            paths: asset_output_paths.clone(),
            budget: match operation {
                PendingOperation::Export {
                    frame_ids,
                    outputs,
                    all_screens,
                    ..
                } => {
                    let frames = (!all_screens).then_some(frame_ids.len().max(1));
                    let units = frames.map(|frames| frames * outputs.len());
                    json!({"frameIds":frame_ids,"allScreens":all_screens,"frameCount":frames,"frameOutputUnits":units,
                        "planningSecondsPerUnit":[5,20],"planningSeconds":units.map(|units|[units*5,units*20]),
                        "isGuarantee":false,"initialReplyBudgetSeconds":1,
                        "note":include_str!("text/asset_jobs-acquire_AssetJob-1e1eb717.md")})
                }
                _ => unreachable!(),
            },
            resume: Notify::new(),
            cancellation: Notify::new(),
            cancelled: std::sync::atomic::AtomicBool::new(false),
            delivery: *delivery,
            state: Mutex::new(JobState {
                state: "running",
                stage: "acquisition",
                assets: request
                    .asset_selections
                    .iter()
                    .map(|a| json!({"assetId":a.asset_id,"renderMode":a.render_mode,"format":a.format,"scale":a.scale,"quality":a.quality,"lossless":a.lossless,"usageAspectRatio":a.usage_aspect_ratio,"maxWidth":a.max_width,"srcset":a.srcset,"visibleScopeNodeId":a.visible_scope_node_id,"status":"pending"}))
                    .collect(),
                calls: Vec::new(),
                completed_calls: 0,
                error: None,
                result: None,
                finished: None,
            }),
        });
        jobs.insert(id, job.clone());
        Ok((job, true))
    }

    pub(super) fn get(&self, id: &str) -> Result<Arc<AssetJob>, DevupError> {
        self.0.lock().unwrap().get(id).filter(|job| {
            job.started.elapsed() < JOB_LIFETIME && job.state.lock().unwrap().finished.is_none_or(|at| at.elapsed() < RESULT_RETENTION)
        }).cloned().ok_or_else(|| DevupError::new(ErrorCode::DevupFigmaHandoffExpired,
            "Export job is missing or expired. Jobs survive client timeouts, not server restarts; pending jobs retain checkpoints for 30 minutes and completed results for 5 minutes.", false))
    }
}

impl AssetJob {
    pub(super) fn cancel(&self) {
        if self.state.lock().unwrap().finished.is_none() {
            self.cancelled
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.cancellation.notify_one();
        }
    }
    pub(super) fn resume(&self) {
        let mut state = self.state.lock().unwrap();
        if state.state == "paused" {
            state.state = "running";
            state.error = None;
            self.resume.notify_one();
        }
    }

    pub(super) fn captured(&self, assets: &[devup_mcp_figma::AssetManifestEntry]) {
        let mut state = self.state.lock().unwrap();
        for asset in assets {
            if !state.assets.iter().any(|a| a["assetId"] == asset.asset_id) {
                state.assets.push(json!({"assetId":asset.asset_id}));
            }
        }
        for progress in &mut state.assets {
            if let Some(asset) = assets.iter().find(|a| progress["assetId"] == a.asset_id) {
                *progress = json!({"assetId":asset.asset_id,"status":asset.status,"byteLength":asset.byte_length,
                    "sha256":asset.sha256,"errorCode":asset.error_code,"errorDetail":asset.error_detail,"exportDiagnostics":asset.export_diagnostics,"renderMode":asset.render_mode,"format":asset.format,"scale":asset.scale,"quality":asset.quality,"lossless":asset.lossless,"usageAspectRatio":asset.usage_aspect_ratio,"maxWidth":asset.max_width,"srcset":asset.srcset,"visibleScopeNodeId":asset.visible_scope_node_id,"renderInfo":asset.render_info});
            }
        }
        state.stage = "projection-and-write";
        state.calls.push(json!({"stage":"projection-and-write","state":"running","startedMs":self.started.elapsed().as_millis() as u64}));
    }

    pub(super) fn progress(&self, collector: &CollectorSession) {
        self.state.lock().unwrap().assets = collector.asset_progress();
    }

    pub(super) async fn call(
        &self,
        server: &DevupServer,
        call: ReadToolCall,
    ) -> Result<UpstreamResult, DevupError> {
        let (stage, detail) = match &call {
            ReadToolCall::AssetExport { request, .. } => (
                "asset-export",
                json!({"assetId":request.asset_id,"nodeId":request.node_id}),
            ),
            ReadToolCall::LargeValue { options, .. } => (
                "chunk-read",
                json!({"nodeId":options.node_id,"field":options.field,"offset":options.offset,"byteLength":options.byte_length}),
            ),
            ReadToolCall::Snapshot {
                script,
                node_id,
                root_ids,
                snapshot,
                ..
            } => (
                "snapshot",
                json!({"script":format!("{script:?}"),"nodeId":node_id,"rootIds":root_ids,"pagination":snapshot}),
            ),
            _ => ("acquisition", json!({"kind":"resource-or-metadata"})),
        };
        let call_timeout = if call.is_asset_read() {
            devup_mcp_figma::asset_timeout() + Duration::from_secs(10)
        } else {
            CALL_TIMEOUT
        };
        loop {
            let began = Instant::now();
            {
                let mut state = self.state.lock().unwrap();
                state.stage = stage;
                if state.calls.len() == 128 {
                    state.calls.remove(0);
                }
                state.calls.push(json!({"stage":stage,"detail":detail,"state":"running","startedMs":self.started.elapsed().as_millis() as u64,"timeoutMs":call_timeout.as_millis() as u64}));
            }
            let result = match server
                .call_waiting_out_a_spent_allowance(call.clone(), Some(call_timeout))
                .await
            {
                Ok(result) => match super::operation::upstream_error(&result.raw) {
                    Some(error) if error.retryable => Err(error),
                    _ => Ok(result),
                },
                Err(error) => Err(error),
            };
            let paused = result.as_ref().err().is_some_and(|e| e.retryable);
            {
                let mut state = self.state.lock().unwrap();
                let current = state.calls.last_mut().unwrap();
                current["elapsedMs"] = json!(began.elapsed().as_millis() as u64);
                current["state"] = json!(if paused {
                    "paused"
                } else if result.is_ok() {
                    "received"
                } else {
                    "failed"
                });
                if paused {
                    state.state = "paused";
                    state.error = result.as_ref().err().cloned();
                } else {
                    state.completed_calls += 1;
                }
            }
            if !paused {
                return result;
            }
            // notify_one retains a permit if resume arrives before this await.
            self.resume.notified().await;
        }
    }

    fn finish(&self, result: Result<Value, DevupError>) {
        let mut state = self.state.lock().unwrap();
        if let Ok(value) = &result {
            let assets = value["assetManifest"]["assets"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(
                    value["resources"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .flat_map(|resource| {
                            resource["assetMetadata"]["assets"]
                                .as_array()
                                .into_iter()
                                .flatten()
                        }),
                )
                .collect::<Vec<_>>();
            for progress in &mut state.assets {
                if let Some(asset) = assets.iter().find(|a| a["assetId"] == progress["assetId"])
                    .or_else(|| assets.iter().find(|a| {
                        progress["assetId"].as_str().is_some_and(|id| id.ends_with(":node"))
                            && a["assetId"].as_str().zip(progress["assetId"].as_str())
                                .is_some_and(|(actual, requested)| request_key(actual) == request_key(requested))
                    })) {
                    for field in [
                        "status",
                        "byteLength",
                        "sha256",
                        "errorCode",
                        "errorDetail",
                        "exportDiagnostics",
                        "renderMode",
                        "format",
                        "scale",
                        "quality",
                        "lossless",
                        "renderInfo",
                        "suggestedFilename",
                        "outputPath",
                        "path",
                    ] {
                        if let Some(v) = asset.get(field) {
                            progress[field] = v.clone();
                        }
                    }
                    if progress["exportDiagnostics"].is_object() {
                        progress["exportDiagnostics"]["renderInfo"] =
                            progress["renderInfo"].clone();
                    }
                }
            }
        }
        for asset in &mut state.assets {
            if let Some(path) = result.as_ref().ok().and_then(|v| {
                v["outputPaths"][format!("asset:{}", asset["assetId"].as_str().unwrap_or(""))]
                    .as_str()
            }) {
                asset["outputPath"] = json!(path);
                // A path can be shared by conflicting assets. The export hash,
                // not the manifest's path alone, proves which bytes were written.
                let written = std::fs::read(path).ok().is_some_and(|bytes| {
                    let hash = Sha256::digest(bytes)
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>();
                    asset["sha256"].as_str() == Some(&hash)
                });
                asset["fileState"] = json!(if written { "written" } else { "not-written" });
            }
        }
        if let Some(call) = state
            .calls
            .last_mut()
            .filter(|c| c["stage"] == "projection-and-write")
        {
            call["elapsedMs"] = json!(
                (self.started.elapsed().as_millis() as u64)
                    .saturating_sub(call["startedMs"].as_u64().unwrap_or(0))
            );
            call["state"] = json!(if result.is_ok() { "complete" } else { "failed" });
        }
        state.state = if self.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            "cancelled"
        } else if result.is_ok() {
            "complete"
        } else {
            "failed"
        };
        // A terminal job cannot leave acquisition rows promising future work.
        // Preserve successfully acquired rows if a later projection/write failed.
        let mut left_unattempted = false;
        for asset in &mut state.assets {
            if asset["status"] == "pending" && asset["fileState"] == "written" {
                asset["status"] = json!("exported");
            } else if asset["status"] == "pending" {
                left_unattempted = result.is_ok();
                asset["status"] = json!("failed");
                asset["acquisitionState"] = json!("not-attempted");
                asset["errorCode"] = result
                    .as_ref()
                    .err()
                    .map(|e| json!(e.code))
                    .unwrap_or_else(|| json!("DEVUP_ASSET_NOT_ATTEMPTED"));
                asset["errorDetail"] = result
                    .as_ref()
                    .err()
                    .map(|e| json!(e.message))
                    .unwrap_or_else(|| json!("The job finished before this asset was acquired."));
            }
        }
        if left_unattempted && state.state == "complete" {
            state.state = "partial";
        }
        state.stage = "finished";
        state.error = result.as_ref().err().cloned();
        state.result = Some(result);
        state.finished = Some(Instant::now());
    }

    /// A poll keeps the delivery the first call asked for.
    fn poll_arguments(&self, mut arguments: Value) -> Value {
        if self.delivery != super::delivery::DeliveryMode::Auto {
            arguments["delivery"] = json!(self.delivery);
        }
        arguments
    }

    #[cfg(test)]
    fn response(&self) -> Result<Value, DevupError> {
        self.response_with(false)
    }

    /// The job as a poll reports it. Without `full`, one compact row per
    /// asset: the full rows (`exportDiagnostics`, `renderInfo`, ...) repeated
    /// tens of kilobytes on every poll of a running job. A poll that selects
    /// with responseFields reads them, and a completed job's asset manifest
    /// carries them too.
    fn response_with(&self, full: bool) -> Result<Value, DevupError> {
        let state = self.state.lock().unwrap();
        let mut calls = state.calls.clone();
        for call in &mut calls {
            if call["state"] == "running" {
                call["elapsedMs"] = json!(
                    (self.started.elapsed().as_millis() as u64)
                        .saturating_sub(call["startedMs"].as_u64().unwrap_or(0))
                );
            }
        }
        let mut first_by_hash: BTreeMap<String, String> = BTreeMap::new();
        let mut duplicates: BTreeMap<String, String> = BTreeMap::new();
        for asset in &state.assets {
            let (Some(id), Some(hash)) = (asset["assetId"].as_str(), asset["sha256"].as_str())
            else {
                continue;
            };
            match first_by_hash.get(hash) {
                Some(first) => {
                    duplicates.insert(id.to_owned(), first.clone());
                }
                None => {
                    first_by_hash.insert(hash.to_owned(), id.to_owned());
                }
            }
        }
        let assets: Vec<_> = state
            .assets
            .iter()
            .map(|asset| {
                let mut asset = asset.clone();
                if let Some(first) = asset["assetId"].as_str().and_then(|id| duplicates.get(id))
                    .filter(|first| asset["assetId"].as_str().is_some_and(|id| request_key(id) != request_key(first))) {
                    asset["duplicateOf"] = json!(first);
                }
                let path = asset["assetId"].as_str().and_then(|id| self.paths.get(id));
                asset["requestedOutputPath"] = json!(path);
                if asset.get("fileState").is_none() {
                    asset["fileState"] = json!(if path.is_none() {
                        "not-requested"
                    } else if state.result.is_some() {
                        "not-written"
                    } else {
                        "pending"
                    });
                    // Bytes are held until the whole job is projected, so an
                    // exported asset reads `pending` until then; say so.
                    if asset["fileState"] == "pending" {
                        asset["fileStateReason"] = json!(if asset["status"] == "exported" {
                            "Acquired and held in memory. Files are written together in the projection-and-write stage, after the last asset of this job is acquired."
                        } else {
                            "Not acquired yet."
                        });
                    }
                }
                if !full {
                    return COMPACT_ASSET_FIELDS
                        .iter()
                        .filter_map(|field| {
                            asset
                                .get(*field)
                                .filter(|value| !value.is_null())
                                .map(|value| ((*field).to_owned(), value.clone()))
                        })
                        .collect::<serde_json::Map<_, _>>()
                        .into();
                }
                if state.assets.len() > 32 {
                    asset["renderInfoSummary"] = json!({"representation":asset["renderInfo"]["representation"],"outputSize":asset["renderInfo"]["outputSize"],"fidelity":asset["renderInfo"]["fidelity"],"compression":asset["renderInfo"]["compression"]});
                    asset.as_object_mut().unwrap().remove("renderInfo");
                    asset.as_object_mut().unwrap().remove("exportDiagnostics");
                }
                asset
            })
            .collect();
        let terminal = state.finished.is_some()
            || state.stage == "finished"
            || matches!(state.state, "failed" | "complete" | "finished");
        let remaining: Vec<&str> = state
            .assets
            .iter()
            .filter(|a| a["acquisitionState"] == "not-attempted")
            .filter_map(|a| a["assetId"].as_str())
            .collect();
        let next_action = if state.state == "partial" {
            json!({"tool":"devup_figma_export","arguments":self.poll_arguments(json!({"jobId":self.id,"jobAction":"resume","remainingAssetIds":remaining}))})
        } else if terminal {
            state
                .error
                .as_ref()
                .and_then(|e| e.details.get("nextAction"))
                .cloned()
                .or_else(|| {
                    state
                        .result
                        .as_ref()
                        .and_then(|r| r.as_ref().ok())
                        .and_then(|v| v.get("nextAction"))
                        .cloned()
                })
                .filter(|v| v["arguments"]["jobId"].is_null())
                .unwrap_or(Value::Null)
        } else {
            json!({"tool":"devup_figma_export","arguments":self.poll_arguments(json!({"jobId":self.id,"jobAction":if state.state=="paused" {"resume"} else {"status"}}))})
        };
        let mut job = json!({"jobId":self.id,"budget":self.budget,"state":state.state,"stage":state.stage,"assets":assets,
            "assetProgress":{"total":request_progress(&state.assets).0,"exported":request_progress(&state.assets).1,"failed":request_progress(&state.assets).2,
                "pending":request_progress(&state.assets).3,"substeps":state.assets.len(),"maxAssetsPerBatch":6,"maxRetainedAssetBytes":devup_mcp_figma::MAX_COLLECTED_ASSET_BYTES,
                "observations":if state.assets.len()>32 {"summarized; full observations in completed manifest/audit resources"}else{"full"}},
            "completedCalls":state.completed_calls,"calls":calls,"elapsedMs":self.started.elapsed().as_millis() as u64,
            "lastError":state.error,"checkpointScope":"server-process","retentionSeconds":JOB_LIFETIME.as_secs(),
            "resultRetentionSeconds":RESULT_RETENTION.as_secs(),"callTimeoutSeconds":CALL_TIMEOUT.as_secs(),
            "assetTimeoutMs":devup_mcp_figma::asset_timeout().as_millis() as u64,
            "nextActionReason":if terminal && next_action.is_null() { Some("This job has finished; status polling repeats the retained result. No corrective action is available.") } else { None },
            "nextAction":next_action});
        if full {
            job["assetRows"] = json!("full");
        } else {
            job["assetRows"] = json!("compact");
            job["assetDetail"] = json!({
                "how": include_str!("text/asset_jobs-response_with-526ef5eb.md"),
                "arguments": {"jobId": self.id, "jobAction": "status", "responseFields": ["exportJob.assets"]},
            });
        }
        // `assetJob` is a compatibility name for this same object. Repeating
        // the body doubled every poll, so the alias now points at it.
        let alias = json!({"aliasOf": "exportJob", "jobId": self.id, "state": state.state});
        // Preserve the existing error contract for code-only upstream refusals,
        // while returning the retained job ID needed to resume the pending read.
        if state.state == "paused"
            && state.assets.is_empty()
            && let Some(error) = &state.error
        {
            let mut error = error.clone();
            if !error.details.is_object() {
                error.details = json!({});
            }
            error.details["exportJob"] = job;
            return Err(error);
        }
        let mut value = match &state.result {
            Some(Ok(result)) => result.clone(),
            Some(Err(error)) => {
                let mut error = error.clone();
                if !error.details.is_object() {
                    error.details = json!({});
                }
                error.details["exportJob"] = job;
                error.details["assetJob"] = alias;
                return Err(error);
            }
            None => json!({"status":if state.state=="paused" {"paused"} else {"in_progress"}}),
        };
        value["exportJob"] = job;
        value["assetJob"] = alias;
        Ok(value)
    }

    /// Waits up to a second for a running job, then answers it; `full` keeps
    /// every per-asset field, for a poll that selects from them.
    pub(super) async fn wait_briefly(&self, full: bool) -> Result<Value, DevupError> {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if self.state.lock().unwrap().state != "running" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.response_with(full)
    }
}

impl DevupServer {
    pub(super) async fn start_asset_job(
        &self,
        operation: PendingOperation,
        request: CollectionRequest,
        refresh: bool,
    ) -> Result<Value, DevupError> {
        let (job, created) = self.asset_jobs.acquire(&operation, &request, refresh)?;
        if created {
            let server = self.clone();
            let worker = job.clone();
            tokio::spawn(async move {
                let acquisition = tokio::time::timeout(
                    JOB_LIFETIME,
                    server.start_operation_scoped_tracked(
                        operation,
                        request,
                        refresh,
                        None,
                        Some(&worker),
                    ),
                );
                let result = tokio::select! {
                    result = acquisition => result.unwrap_or_else(|_| {
                    Err(DevupError::new(
                        ErrorCode::DevupFigmaHandoffExpired,
                        "Export job checkpoint expired after 30 minutes.",
                        false,
                    ))
                    }),
                    _ = worker.cancellation.notified() => Err(DevupError::new(ErrorCode::DevupInvalidInput,
                        "Export job cancelled. No later batches will run; already accepted upstream reads may finish remotely. Start a new request to collect remaining assets.", false)),
                };
                worker.finish(result);
            });
        }
        job.wait_briefly(false).await
    }
}

#[cfg(test)]
#[path = "asset_jobs_tests.rs"]
mod tests;
