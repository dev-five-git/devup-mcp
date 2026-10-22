use super::*;

#[test]
fn image_modes_have_distinct_jobs_and_same_mode_recovers_its_job() {
    let selection = devup_mcp_figma::AssetSelection {
        usage_aspect_ratio: None,
        max_width: None,
        srcset: false,
        visible_scope_node_id: None,
        asset_id: "sample:fills:0".into(),
        format: devup_mcp_figma::AssetFormat::Png,
        scale: 1.0,
        render_mode: devup_mcp_figma::AssetRenderMode::Node,
        quality: None,
        lossless: None,
    };
    let mut request = CollectionRequest::new(
        devup_mcp_figma::FigmaTarget::parse(
            "https://www.figma.com/design/Fixture/Example?node-id=1-1",
        )
        .unwrap(),
        devup_mcp_figma::CollectionScope::Node,
    );
    request.asset_selections = vec![selection];
    let operation = PendingOperation::Export {
        outputs: vec!["assetManifest".into()],
        previous_design_fingerprints: None,
        component_name: None,
        include_diagnostics: false,
        root_layout: devup_mcp_devup_ui::codegen::RootLayout::default(),
        asset_names_per_node: true,
        scope: "node".into(),
        theme_options: Default::default(),
        strict: false,
        output_paths: Default::default(),
        page_scaffold: None,
        frame_ids: vec![],
        all_screens: false,
        asset_captures: request.asset_selections.clone(),
        web_ready_images: false,
        asset_output_paths: Default::default(),
        asset_public_root: None,
        fields: vec![],
        response_selection: Default::default(),
        delivery: super::super::delivery::DeliveryMode::Auto,
        width_policy: None,
    };
    let jobs = AssetJobs::default();
    let (node, created) = jobs.acquire(&operation, &request, false).unwrap();
    assert!(created);
    let (same, created) = jobs.acquire(&operation, &request, false).unwrap();
    assert!(!created);
    assert_eq!(node.id, same.id);
    request.asset_selections[0].render_mode = devup_mcp_figma::AssetRenderMode::Fill;
    let (fill, created) = jobs.acquire(&operation, &request, false).unwrap();
    assert!(created);
    assert_ne!(node.id, fill.id);
    request.asset_selections[0].render_mode = devup_mcp_figma::AssetRenderMode::Auto;
    let (automatic, created) = jobs.acquire(&operation, &request, false).unwrap();
    assert!(created);
    assert_ne!(automatic.id, node.id);
    assert_ne!(automatic.id, fill.id);
    request.asset_selections[0].format = devup_mcp_figma::AssetFormat::Webp;
    request.asset_selections[0].quality = Some(20);
    request.asset_selections[0].lossless = Some(false);
    let (quality20, _) = jobs.acquire(&operation, &request, false).unwrap();
    request.asset_selections[0].quality = Some(21);
    let (quality21, _) = jobs.acquire(&operation, &request, false).unwrap();
    assert_ne!(quality20.id, quality21.id);
    request.asset_selections[0].lossless = Some(true);
    let (lossless, _) = jobs.acquire(&operation, &request, false).unwrap();
    assert_ne!(lossless.id, quality21.id);
    let variant_jobs = AssetJobs::default();
    let mut ids = std::collections::BTreeSet::new();
    for variant in 0..6 {
        let mut changed = request.clone();
        match variant {
            0 => changed.asset_selections[0].max_width = Some(240),
            1 => changed.asset_selections[0].srcset = true,
            2 => changed.asset_selections[0].scale = 1.5,
            3 => changed.asset_selections[0].visible_scope_node_id = Some("parent".into()),
            4 => changed.asset_selections[0].usage_aspect_ratio = Some(1.25),
            _ => {
                changed.asset_selections[0].render_mode =
                    devup_mcp_figma::AssetRenderMode::OriginalSource
            }
        }
        let (job, created) = variant_jobs.acquire(&operation, &changed, false).unwrap();
        assert!(created);
        assert!(ids.insert(job.id.clone()));
        let (same, created) = variant_jobs.acquire(&operation, &changed, false).unwrap();
        assert!(!created);
        assert_eq!(same.id, job.id);
    }
    request.asset_selections.clear();
    request.web_ready_images = true;
    let (cached_preset, _) = jobs.acquire(&operation, &request, false).unwrap();
    cached_preset.captured(&[serde_json::from_value(json!({"assetId":"cached:fills:0","nodeId":"cached","field":"fills/0","sourceKind":"image-fill",
        "format":"webp","scale":1,"quality":80,"lossless":false,"status":"failed","errorCode":"DEVUP_ASSET_NODE_HIDDEN"})).unwrap()]);
    let cached = cached_preset.response_with(true).unwrap();
    assert_eq!(cached["exportJob"]["assets"].as_array().unwrap().len(), 1);
    assert_eq!(cached["exportJob"]["assets"][0]["quality"], 80);
    cached_preset.state.lock().unwrap().state = "paused";
    cached_preset.resume();
    assert_eq!(cached_preset.state.lock().unwrap().state, "running");
    cached_preset.cancel();
    cached_preset.finish(Err(DevupError::new(
        ErrorCode::DevupInvalidInput,
        "cancelled synthetic pending preset",
        false,
    )));
    let cancelled = cached_preset.response().unwrap_err();
    assert_eq!(cancelled.details["exportJob"]["state"], "cancelled");
    assert_eq!(cancelled.details["assetJob"]["state"], "cancelled");
    assert!(cancelled.details["exportJob"]["nextAction"].is_null());
    assert_eq!(
        automatic.state.lock().unwrap().assets[0]["renderMode"],
        "auto"
    );
    assert_eq!(fill.state.lock().unwrap().assets[0]["renderMode"], "fill");
    fill.finish(Ok(
        json!({"resources":[{"assetMetadata":{"assets":[{"assetId":"sample:fills:0",
        "renderMode":"fill","renderInfo":{"outputSize":{"width":32,"height":16}},
        "suggestedFilename":"gallery-picture-fill.png"}]}}]}),
    ));
    let progress = fill.response_with(true).unwrap();
    assert_eq!(
        progress["exportJob"]["assets"][0]["renderInfo"]["outputSize"]["width"],
        32
    );
    assert_eq!(
        progress["exportJob"]["assets"][0]["suggestedFilename"],
        "gallery-picture-fill.png"
    );
}

#[test]
fn r10_terminal_jobs_never_offer_repeated_status_as_next_action() {
    for correction in [false, true] {
        let job = AssetJob {
            id: "r10".into(),
            key: "r10".into(),
            budget: json!({}),
            started: Instant::now(),
            resume: Notify::new(),
            cancellation: Notify::new(),
            cancelled: std::sync::atomic::AtomicBool::new(false),
        delivery: Default::default(),
            paths: BTreeMap::new(),
            state: Mutex::new(JobState {
                state: "running",
                stage: "snapshot",
                calls: vec![],
                completed_calls: 0,
                error: None,
                result: None,
                finished: None,
                assets: vec![json!({"assetId":"pending:fills:0","status":"pending"})],
            }),
        };
        assert_eq!(
            job.response().unwrap()["exportJob"]["nextAction"]["arguments"]["jobAction"],
            "status"
        );
        job.state
            .lock()
            .unwrap()
            .calls
            .push(json!({"stage":"asset-export","state":"running","startedMs":0}));
        let progress = job.response().unwrap();
        assert!(progress["exportJob"]["calls"][0]["elapsedMs"].is_u64());
        assert!(progress["exportJob"]["assetTimeoutMs"].is_u64());
        let next = if correction {
            json!({"tool":"devup_figma_export","arguments":{"url":"https://www.figma.com/design/test?node-id=1-1","frameIds":["2:1"]}})
        } else {
            Value::Null
        };
        job.finish(Err(DevupError::with_details(
            ErrorCode::DevupFigmaNodeNotFound,
            "test",
            false,
            json!({"nextAction":next}),
        )));
        let error = job.response().unwrap_err();
        assert_eq!(error.details["exportJob"]["nextAction"], next);
        assert_eq!(error.details["assetJob"]["aliasOf"], "exportJob");
        assert_eq!(error.details["exportJob"]["assets"][0]["status"], "failed");
        assert_eq!(
            error.details["exportJob"]["assets"][0]["acquisitionState"],
            "not-attempted"
        );
        assert_eq!(error.details["exportJob"]["assetProgress"]["pending"], 0);
        assert_eq!(error.details["exportJob"]["assetProgress"]["failed"], 1);
        assert_eq!(
            error.details["exportJob"]["assetProgress"]["maxRetainedAssetBytes"],
            devup_mcp_figma::MAX_COLLECTED_ASSET_BYTES
        );
        if !correction {
            assert!(error.details["exportJob"]["nextActionReason"].is_string());
        }
        job.finish(Ok(json!({"status":"complete"})));
        assert!(job.response().unwrap()["exportJob"]["nextAction"].is_null());
    }
}

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
        budget: json!({}),
        started: Instant::now(),
        resume: Notify::new(),
        cancellation: Notify::new(),
        cancelled: std::sync::atomic::AtomicBool::new(false),
        delivery: Default::default(),
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
    assert_eq!(response["exportJob"]["assets"][0]["fileState"], "written");
    assert_eq!(
        response["exportJob"]["assets"][1]["fileState"],
        "not-written"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn polls_answer_compact_rows_and_reference_the_alias() {
    let job = AssetJob {
        id: "compact".into(),
        key: "compact".into(),
        budget: json!({}),
        started: Instant::now(),
        resume: Notify::new(),
        cancellation: Notify::new(),
        cancelled: std::sync::atomic::AtomicBool::new(false),
        delivery: Default::default(),
        paths: BTreeMap::from([
            ("a".into(), "/out/a.svg".into()),
            ("b".into(), "/out/b.svg".into()),
        ]),
        state: Mutex::new(JobState {
            state: "running",
            stage: "asset-export",
            calls: vec![],
            completed_calls: 1,
            error: None,
            result: None,
            finished: None,
            assets: vec![
                json!({"assetId":"a","status":"exported","byteLength":10,"sha256":"0".repeat(64),
                    "exportDiagnostics":{"large":"x".repeat(4096)},"renderInfo":{"large":"y".repeat(4096)}}),
                json!({"assetId":"b","status":"pending"}),
            ],
        }),
    };

    let compact = job.response().unwrap();
    let rows = compact["exportJob"]["assets"].as_array().unwrap();
    assert_eq!(compact["exportJob"]["assetRows"], "compact");
    assert!(rows[0].get("exportDiagnostics").is_none(), "{compact}");
    assert!(rows[0].get("renderInfo").is_none(), "{compact}");
    assert_eq!(rows[0]["sha256"], "0".repeat(64));
    assert_eq!(rows[0]["requestedOutputPath"], "/out/a.svg");
    // Exported but not yet written: the state says why.
    assert_eq!(rows[0]["fileState"], "pending");
    assert!(
        rows[0]["fileStateReason"]
            .as_str()
            .unwrap()
            .contains("projection-and-write")
    );
    assert_eq!(rows[1]["fileStateReason"], "Not acquired yet.");
    assert_eq!(
        compact["exportJob"]["assetDetail"]["arguments"]["responseFields"],
        json!(["exportJob.assets"])
    );
    // The alias names the job; it no longer repeats it.
    assert_eq!(
        compact["assetJob"],
        json!({"aliasOf":"exportJob","jobId":"compact","state":"running"})
    );
    assert!(
        serde_json::to_vec(&compact).unwrap().len() < 4096,
        "{compact}"
    );

    let full = job.response_with(true).unwrap();
    assert_eq!(full["exportJob"]["assetRows"], "full");
    assert!(full["exportJob"]["assets"][0]["renderInfo"].is_object());
    assert!(full["exportJob"].get("assetDetail").is_none());
}

#[test]
fn r26_polls_keep_the_delivery_of_the_first_call() {
    let job = |delivery| AssetJob {
        id: "p26".into(),
        key: "p26".into(),
        budget: json!({}),
        started: Instant::now(),
        resume: Notify::new(),
        cancellation: Notify::new(),
        cancelled: std::sync::atomic::AtomicBool::new(false),
        delivery,
        paths: BTreeMap::new(),
        state: Mutex::new(JobState {
            state: "running",
            stage: "acquisition",
            calls: vec![],
            completed_calls: 0,
            error: None,
            result: None,
            finished: None,
            assets: vec![],
        }),
    };
    let resource = job(super::super::delivery::DeliveryMode::Resource);
    let next = resource.response().unwrap()["exportJob"]["nextAction"].clone();
    assert_eq!(next["arguments"]["delivery"], "resource");
    assert_eq!(next["arguments"]["jobAction"], "status");
    let auto = job(super::super::delivery::DeliveryMode::Auto);
    let next = auto.response().unwrap()["exportJob"]["nextAction"].clone();
    assert!(next["arguments"].get("delivery").is_none());
}

#[test]
fn r39_unattempted_assets_make_a_partial_job_with_resume_and_one_count_per_request() {
    let job = AssetJob {
        id: "p39".into(),
        key: "p39".into(),
        budget: json!({}),
        started: Instant::now(),
        resume: Notify::new(),
        cancellation: Notify::new(),
        cancelled: std::sync::atomic::AtomicBool::new(false),
        delivery: Default::default(),
        paths: BTreeMap::new(),
        state: Mutex::new(JobState {
            state: "running",
            stage: "projection-and-write",
            calls: vec![],
            completed_calls: 0,
            error: None,
            result: None,
            finished: None,
            assets: vec![
                json!({"assetId":"1:1:node","status":"exported"}),
                json!({"assetId":"1:2:node","status":"pending"}),
                json!({"assetId":"1:2:fills:0","status":"pending"}),
            ],
        }),
    };
    job.finish(Ok(json!({"status":"complete"})));
    let job_json = job.response().unwrap()["exportJob"].clone();
    assert_eq!(job_json["state"], "partial");
    assert_eq!(job_json["nextAction"]["arguments"]["jobAction"], "resume");
    assert_eq!(job_json["assetProgress"]["total"], 2);
    assert_eq!(job_json["assetProgress"]["exported"], 1);
    assert_eq!(job_json["assetProgress"]["failed"], 1);
    assert_eq!(job_json["assetProgress"]["substeps"], 3);
}

#[test]
fn r39_identical_bytes_are_marked_duplicate_and_substeps_count_once() {
    let job = AssetJob {
        id: "dup".into(),
        key: "dup".into(),
        budget: json!({}),
        started: Instant::now(),
        resume: Notify::new(),
        cancellation: Notify::new(),
        cancelled: std::sync::atomic::AtomicBool::new(false),
        delivery: Default::default(),
        paths: BTreeMap::new(),
        state: Mutex::new(JobState {
            state: "running",
            stage: "asset-export",
            calls: vec![],
            completed_calls: 1,
            error: None,
            result: None,
            finished: None,
            assets: vec![
                json!({"assetId":"1:2:node","status":"exported","sha256":"aa"}),
                json!({"assetId":"1:2:fills:0","status":"exported","sha256":"aa"}),
                json!({"assetId":"1:3","status":"exported","sha256":"bb"}),
                json!({"assetId":"1:4","status":"exported","sha256":"aa"}),
            ],
        }),
    };
    let response = job.response().unwrap();
    let rows = response["exportJob"]["assets"].as_array().unwrap();
    assert!(rows[0].get("duplicateOf").is_none());
    assert_eq!(rows[1]["duplicateOf"], "1:2:node");
    assert!(rows[2].get("duplicateOf").is_none());
    assert_eq!(rows[3]["duplicateOf"], "1:2:node");
    assert_eq!(response["exportJob"]["assetProgress"]["total"], 3);
    assert_eq!(response["exportJob"]["assetProgress"]["substeps"], 4);
}
