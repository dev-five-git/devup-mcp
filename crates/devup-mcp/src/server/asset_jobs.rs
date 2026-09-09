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
use sha2::{Digest, Sha256};
use tokio::{sync::Notify, time::Instant};

use super::{DevupServer, operation::PendingOperation};

const CALL_TIMEOUT: Duration = Duration::from_secs(90);
const JOB_LIFETIME: Duration = Duration::from_secs(30 * 60);
const RESULT_RETENTION: Duration = Duration::from_secs(5 * 60);
const MAX_JOBS: usize = 8;

#[derive(Clone, Default)]
pub(super) struct AssetJobs(Arc<Mutex<BTreeMap<String, Arc<AssetJob>>>>);

pub(super) struct AssetJob {
    id: String,
    key: String,
    started: Instant,
    paths: BTreeMap<String, String>,
    state: Mutex<JobState>,
    resume: Notify,
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
        if jobs.len() >= MAX_JOBS {
            return Err(DevupError::with_details(
                ErrorCode::DevupInvalidInput,
                "Asset job capacity reached. Poll/resume existing jobs or wait for completed results to expire.",
                true,
                json!({"maxAssetJobs":MAX_JOBS,"jobIds":jobs.keys().collect::<Vec<_>>()}),
            ));
        }
        let PendingOperation::Export {
            asset_output_paths, ..
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
            resume: Notify::new(),
            state: Mutex::new(JobState {
                state: "running",
                stage: "acquisition",
                assets: request
                    .asset_selections
                    .iter()
                    .map(|a| json!({"assetId":a.asset_id,"status":"pending"}))
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
            "Asset job is missing or expired. Jobs survive client timeouts, not server restarts; pending jobs retain checkpoints for 30 minutes and completed results for 5 minutes.", false))
    }
}

impl AssetJob {
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
        for progress in &mut state.assets {
            if let Some(asset) = assets.iter().find(|a| progress["assetId"] == a.asset_id) {
                *progress = json!({"assetId":asset.asset_id,"status":asset.status,"byteLength":asset.byte_length,
                    "sha256":asset.sha256,"errorCode":asset.error_code});
            }
        }
        state.stage = "projection-and-write";
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
            ReadToolCall::Snapshot { script, .. } => {
                ("snapshot", json!({"script":format!("{script:?}")}))
            }
            _ => ("acquisition", json!({"kind":"resource-or-metadata"})),
        };
        loop {
            let began = Instant::now();
            {
                let mut state = self.state.lock().unwrap();
                state.stage = stage;
                if state.calls.len() == 128 {
                    state.calls.remove(0);
                }
                state.calls.push(json!({"stage":stage,"detail":detail,"state":"running","startedMs":self.started.elapsed().as_millis() as u64}));
            }
            let result = match tokio::time::timeout(
                CALL_TIMEOUT,
                server.call_waiting_out_a_spent_allowance(call.clone()),
            )
            .await
            {
                Ok(Ok(result)) => match super::operation::upstream_error(&result.raw) {
                    Some(error) if error.retryable => Err(error),
                    _ => Ok(result),
                },
                Ok(Err(error)) => Err(error),
                Err(_) => Err(DevupError::with_details(
                    ErrorCode::DevupFigmaDirectUnavailable,
                    "Asset job read timed out; accepted reads and asset bytes are retained. Resume this job to retry only the pending read.",
                    true,
                    json!({"timeoutSeconds":CALL_TIMEOUT.as_secs(),"stage":stage,"call":detail}),
                )),
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
        if let Ok(value) = &result
            && let Some(assets) = value["assetManifest"]["assets"].as_array()
        {
            for progress in &mut state.assets {
                if let Some(asset) = assets.iter().find(|a| a["assetId"] == progress["assetId"]) {
                    for field in [
                        "status",
                        "byteLength",
                        "sha256",
                        "errorCode",
                        "outputPath",
                        "path",
                    ] {
                        if let Some(v) = asset.get(field) {
                            progress[field] = v.clone();
                        }
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
        state.state = if result.is_ok() { "complete" } else { "failed" };
        state.stage = "finished";
        state.error = result.as_ref().err().cloned();
        state.result = Some(result);
        state.finished = Some(Instant::now());
    }

    fn response(&self) -> Result<Value, DevupError> {
        let state = self.state.lock().unwrap();
        let assets: Vec<_> = state
            .assets
            .iter()
            .map(|asset| {
                let mut asset = asset.clone();
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
                }
                asset
            })
            .collect();
        let job = json!({"jobId":self.id,"state":state.state,"stage":state.stage,"assets":assets,
            "completedCalls":state.completed_calls,"calls":state.calls,"elapsedMs":self.started.elapsed().as_millis() as u64,
            "lastError":state.error,"checkpointScope":"server-process","retentionSeconds":JOB_LIFETIME.as_secs(),
            "resultRetentionSeconds":RESULT_RETENTION.as_secs(),"callTimeoutSeconds":CALL_TIMEOUT.as_secs(),
            "nextAction":{"tool":"devup_figma_export","arguments":{"jobId":self.id,"jobAction":if state.state=="paused" {"resume"} else {"status"}}}});
        let mut value = match &state.result {
            Some(Ok(result)) => result.clone(),
            Some(Err(error)) => {
                let mut error = error.clone();
                if !error.details.is_object() {
                    error.details = json!({});
                }
                error.details["assetJob"] = job;
                return Err(error);
            }
            None => json!({"status":if state.state=="paused" {"paused"} else {"in_progress"}}),
        };
        value["assetJob"] = job;
        Ok(value)
    }

    pub(super) async fn wait_briefly(&self) -> Result<Value, DevupError> {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if self.state.lock().unwrap().state != "running" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.response()
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
                let result = tokio::time::timeout(
                    JOB_LIFETIME,
                    server.start_operation_scoped_tracked(
                        operation,
                        request,
                        refresh,
                        None,
                        Some(&worker),
                    ),
                )
                .await
                .unwrap_or_else(|_| {
                    Err(DevupError::new(
                        ErrorCode::DevupFigmaHandoffExpired,
                        "Asset job checkpoint expired after 30 minutes.",
                        false,
                    ))
                });
                worker.finish(result);
            });
        }
        job.wait_briefly().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r3_shared_path_reports_only_matching_bytes_as_written() {
        let path =
            std::env::temp_dir().join(format!("devup-r3-collision-{}.bin", rand::random::<u64>()));
        std::fs::write(&path, b"first").unwrap();
        let hash = |bytes: &[u8]| {
            Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let path_text = path.to_string_lossy().to_string();
        let job = AssetJob {
            id: "test".into(),
            key: "test".into(),
            started: Instant::now(),
            resume: Notify::new(),
            paths: BTreeMap::from([
                ("a".into(), path_text.clone()),
                ("b".into(), path_text.clone()),
            ]),
            state: Mutex::new(JobState {
                state: "running",
                stage: "projection-and-write",
                calls: vec![],
                completed_calls: 0,
                error: None,
                result: None,
                finished: None,
                assets: vec![
                    json!({"assetId":"a","status":"exported","sha256":hash(b"first")}),
                    json!({"assetId":"b","status":"exported","sha256":hash(b"second")}),
                ],
            }),
        };
        job.finish(Ok(
            json!({"outputPaths":{"asset:a":path_text,"asset:b":path_text}}),
        ));
        let response = job.response().unwrap();
        assert_eq!(response["assetJob"]["assets"][0]["fileState"], "written");
        assert_eq!(
            response["assetJob"]["assets"][1]["fileState"],
            "not-written"
        );
        std::fs::remove_file(path).unwrap();
    }
}
