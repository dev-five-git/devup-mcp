use super::*;
fn snapshot(metric: bool) -> devup_mcp_figma::Snapshot {
    serde_json::from_value(json!({"fileKey":"completion-fixture","roots":["root"],"nodes":if metric{json!({"root":{"id":"root","type":"FRAME","fields":{"name":"Statistics","childrenIds":["metric"]}},"metric":{"id":"metric","type":"TEXT","fields":{"name":"Metric","characters":"3 patents"}}})}else{json!({"root":{"id":"root","type":"FRAME","fields":{"name":"Home","childrenIds":[]}}})},"diagnostics":[]})).unwrap()
}

#[tokio::test]
async fn completion_recomputes_pixels_and_rejects_local_baselines_and_missing_environment() {
    // Synthetic PNGs exercise the evidence gate, not real design fidelity.
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let (root, policy, _) = fixture();
    let store = ArtifactStore::default();
    let white = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4////fwAJ+wP9KobjigAAAABJRU5ErkJggg==").unwrap();
    let black = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGD4DwABBAEAX+XDSwAAAABJRU5ErkJggg==").unwrap();
    let mut payload = super::super::artifacts::w3_tests::payload();
    payload.reference_png = Some(devup_mcp_figma::ReferencePng {
        mime_type:"image/png".into(),data_base64:STANDARD.encode(&white),
        byte_length:white.len(),sha256:hash(&white),note:None,
    });
    let artifact = store.insert(super::super::artifacts::ArtifactRequestKey::from_collection(&super::super::artifacts::w3_tests::request()),payload).await.unwrap();
    let actual = root.join(".devup/actual.png");
    std::fs::write(&actual,&white).unwrap();
    let observed = json!({"schemaVersion":1,"actualPng":actual,"viewport":{"width":1,"height":1,"deviceScaleFactor":1},"captureContext":{"innerWidth":1,"innerHeight":1,"devicePixelRatio":1,"visualViewportScale":1,"rootCssZoom":1,"bodyCssZoom":1},"environmentStatus":"valid","renderer":{"name":"playwright-chromium","version":"test"},"os":"test","locale":"en-US","timezone":"UTC"});
    let env = serde_json::from_value(json!({"tsxResource":{"uri":"devup://artifact/test/tsx","sha256":"a".repeat(64)},"themePath":"devup.json","assetDirectory":"public/images","devupUiVersion":"test","fontManifest":[{"family":"Test","sha256":"b".repeat(64)}]})).unwrap();
    let mut comparison = json!({"id":"home-page-1","nodeId":"1:1","screenshot":{"sha256":hash(&white)},"class":"page-visual","routePath":"/","viewport":{"width":1},"blockers":[],"call":{"arguments":{"actual":actual,"reference":{"artifactId":artifact.artifact_id},"threshold":0,"environment":observed}}});
    assert_eq!(verify_visual(&comparison,Some(&env),&policy,&store).await["verdict"],"pass");
    assert_eq!(verify_visual(&comparison,None,&policy,&store).await["verdict"],"inconclusive");
    std::fs::write(&actual,&black).unwrap();
    assert_eq!(verify_visual(&comparison,Some(&env),&policy,&store).await["verdict"],"inconclusive");
    comparison["screenshot"]["sha256"] = json!(hash(&black));
    assert_eq!(verify_visual(&comparison,Some(&env),&policy,&store).await["verdict"],"fail");
    comparison["call"]["arguments"]["reference"] = json!({"path":actual});
    assert_eq!(verify_visual(&comparison,Some(&env),&policy,&store).await["verdict"],"inconclusive");
    drop(artifact);drop(store);drop(policy);
    std::fs::remove_dir_all(root).unwrap();
}
fn signed_plan() -> Value {
    let mut result = json!({"artifactId":"fixture-artifact","nodeId":"root"});
    super::super::behavior_contracts::attach(&mut result, &snapshot(false));
    assert!(
        super::super::behavior_contracts::verified_coverage_inventory(&result["behaviorPlan"])
            .is_some()
    );
    result["behaviorPlan"].clone()
}
fn fixture() -> (std::path::PathBuf, OutputPolicy, CompletionInput) {
    let root = std::env::temp_dir().join(format!(
        "devup-completion-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join(".devup/exports")).unwrap();
    std::fs::write(root.join("package.json"), "{}").unwrap();
    std::fs::write(
        root.join("src/page.tsx"),
        "export function Home(){return <main/>}",
    )
    .unwrap();
    std::fs::write(root.join(".devup/exports/home.json"),serde_json::to_vec(&json!({"kind":"devup-export-record","artifactId":"fixture-artifact","rootNodeId":"root","generatedSource":"export function Home(){}","behaviorPlan":signed_plan()})).unwrap()).unwrap();
    let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
    let input = CompletionInput {
        behavior_plan_paths: vec![],
        project_root: root.to_string_lossy().into_owned(),
        audits: vec![],
        check_receipt_path: None,
        visual_environments: BTreeMap::new(),
        data_plan_path: None,
        data_receipt_path: None,
        static_bindings: BTreeMap::new(),
        routes: vec!["/".into()],
        widths: vec![360],
    };
    (root, policy, input)
}
#[tokio::test]
async fn generated_source_never_implies_application_completion() {
    let (root, policy, input) = fixture();
    let report = evaluate(&input, &policy, &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(report["stage"], "generated");
    assert_eq!(report["done"], false);
    assert_eq!(
        report["blockers"]["missingChecks"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    drop(policy);
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn build_receipts_bind_current_source_and_cannot_replace_browser_or_data_proof() {
    let (root, policy, mut input) = fixture();
    let (digest, _) = source_digest(&root, &policy).unwrap();
    let commands = ["build", "lint", "typecheck", "test", "e2e"]
        .map(|name| json!({"name":name,"command":["bun","run",name],"exitCode":0}));
    let checks = json!({"kind":"devup-project-check-receipt","schemaVersion":1,"sourceDigest":digest,"sourceChanged":false,"checks":commands});
    let path = root.join(".devup/checks.json");
    std::fs::write(&path, serde_json::to_vec(&checks).unwrap()).unwrap();
    input.check_receipt_path = Some(path.to_string_lossy().into());
    let store = ArtifactStore::default();
    let report = evaluate(&input, &policy, &store).await.unwrap();
    assert_eq!(report["stage"], "implemented");
    assert_eq!(report["done"], false);
    std::fs::write(
        root.join("src/page.tsx"),
        "export function Changed(){return <main/>}",
    )
    .unwrap();
    let changed = evaluate(&input, &policy, &store).await.unwrap();
    assert_eq!(changed["stage"], "generated");
    assert_ne!(changed["sourceDigest"], digest);
    drop(policy);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn source_digest_is_shared_with_node_and_excludes_receipts_and_dependencies() {
    let (root, policy, _) = fixture();
    std::fs::write(root.join("schema.sql"), "CREATE TABLE facts(id INTEGER);").unwrap();
    std::fs::write(root.join("README.md"), "Source documentation").unwrap();
    std::fs::create_dir_all(root.join("public")).unwrap();
    std::fs::write(root.join("public/media.custom"), b"served media").unwrap();
    let (before, files) = source_digest(&root, &policy).unwrap();
    std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    std::fs::write(root.join("node_modules/pkg/index.js"), "ignored").unwrap();
    std::fs::write(root.join(".devup/receipt.json"), "{}").unwrap();
    assert_eq!(before, source_digest(&root, &policy).unwrap().0);
    assert_eq!(files.len(), 5);
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../harness/runtime-audit/source-digest.mjs");
    let url = url::Url::from_file_path(dunce::canonicalize(script).unwrap()).unwrap();
    let code = format!(
        "const {{sourceDigest}}=await import({});console.log(JSON.stringify(await sourceDigest(process.argv[1])));",
        serde_json::to_string(url.as_str()).unwrap()
    );
    let node = std::process::Command::new("node")
        .args(["--input-type=module", "-e", &code, &root.to_string_lossy()])
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "{}",
        String::from_utf8_lossy(&node.stderr)
    );
    let js: Value = serde_json::from_slice(&node.stdout).unwrap();
    assert_eq!(js["sourceDigest"], before);
    assert_eq!(js["files"], json!(files));
    drop(policy);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_digest_public_scope_is_relative_to_project_not_an_ancestor_directory() {
    let container = std::env::temp_dir().join(format!(
        "devup-public-parent-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let root = container.join("public/project");
    std::fs::create_dir_all(root.join("public")).unwrap();
    std::fs::write(
        root.join("source.tsx"),
        "export function Main(){return <main/>}",
    )
    .unwrap();
    std::fs::write(
        root.join("unrelated.custom"),
        "outside the served public directory",
    )
    .unwrap();
    std::fs::write(root.join("public/served.custom"), "served public bytes").unwrap();
    let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
    let (digest, files) = source_digest(&root, &policy).unwrap();
    assert_eq!(files.len(), 2);
    assert!(!files.contains_key("unrelated.custom"));
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../harness/runtime-audit/source-digest.mjs");
    let url = url::Url::from_file_path(dunce::canonicalize(script).unwrap()).unwrap();
    let code = format!(
        "const {{sourceDigest}}=await import({});console.log(JSON.stringify(await sourceDigest(process.argv[1])));",
        serde_json::to_string(url.as_str()).unwrap()
    );
    let node = std::process::Command::new("node")
        .args(["--input-type=module", "-e", &code, &root.to_string_lossy()])
        .output()
        .unwrap();
    assert!(node.status.success());
    let js: Value = serde_json::from_slice(&node.stdout).unwrap();
    assert_eq!(js["sourceDigest"], digest);
    assert_eq!(js["files"], json!(files));
    drop(policy);
    std::fs::remove_dir_all(container).unwrap();
}

#[test]
fn motion_coverage_joins_exact_planned_viewport_screen_index_and_candidate() {
    let mut plan = json!({"plan":{"viewports":[{"width":360,"height":100,"deviceScaleFactor":1}],"screens":[{"id":"home","checks":[{"kind":"page-health"},{"kind":"motion","motion":{"candidateId":"count-up:metric"}}]}]}});
    let mut report = json!({"runs":[{"viewport":{"width":360,"height":100,"deviceScaleFactor":1},"screens":[{"id":"home","status":"completed","checks":[{"index":99,"kind":"motion","status":"pass","measured":{"candidateId":"count-up:metric"}}]}]},{"viewport":{"width":1440,"height":100,"deviceScaleFactor":1},"screens":[{"id":"home","status":"completed","checks":[{"index":1,"kind":"motion","status":"pass","measured":{"candidateId":"count-up:metric"}}]}]}]});
    assert!(planned_motion_coverage(&plan, &report).is_empty());
    report["runs"][0]["screens"][0]["checks"][0]["index"] = json!(1);
    plan["plan"]["screens"][0]["checks"][1]["widths"] = json!([1440]);
    assert!(planned_motion_coverage(&plan, &report).is_empty());
    plan["plan"]["screens"][0]["checks"][1]["widths"] = json!([360]);
    report["runs"][0]["screens"][0]["checks"][0]["measured"]["candidateId"] =
        json!("count-up:other");
    assert!(planned_motion_coverage(&plan, &report).is_empty());
    report["runs"][0]["screens"][0]["checks"][0]["measured"]["candidateId"] =
        json!("count-up:metric");
    assert_eq!(
        planned_motion_coverage(&plan, &report),
        BTreeSet::from(["count-up:metric".to_owned()])
    );
    let duplicate = report["runs"][0]["screens"][0]["checks"][0].clone();
    report["runs"][0]["screens"][0]["checks"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(planned_motion_coverage(&plan, &report).is_empty());
}

/// An entrance is covered by a passing reveal check at a mobile and at a
/// desktop width; one width, a tablet width or an inconclusive check is not.
#[test]
fn a_reveal_candidate_needs_passing_checks_at_mobile_and_desktop() {
    let viewport = |width: u32| json!({"width":width,"height":800,"deviceScaleFactor":1});
    let run = |width: u32, status: &str| json!({"viewport":viewport(width),"screens":[{"id":"home","status":"completed","checks":[{"index":0,"kind":"motion","status":status,"measured":{"candidateId":"section-entrance:news"}}]}]});
    let plan = |widths: &[u32]| json!({"plan":{"viewports":widths.iter().map(|width| viewport(*width)).collect::<Vec<_>>(),"screens":[{"id":"home","checks":[{"kind":"motion","motion":{"kind":"reveal","candidateId":"section-entrance:news"}}]}]}});
    let covered = BTreeSet::from(["section-entrance:news".to_owned()]);
    assert_eq!(planned_motion_coverage(&plan(&[390, 1440]), &json!({"runs":[run(390, "pass"), run(1440, "pass")]})), covered);
    for runs in [
        json!([run(1440, "pass")]),
        json!([run(390, "pass")]),
        json!([run(390, "pass"), run(858, "pass")]),
        // What could not be observed is unverified, not covered.
        json!([run(390, "pass"), run(1440, "inconclusive")]),
        json!([run(390, "fail"), run(1440, "pass")]),
    ] {
        assert!(planned_motion_coverage(&plan(&[390, 858, 1440]), &json!({"runs":runs})).is_empty(), "{runs}");
    }
}

#[tokio::test]
async fn unsigned_inventory_is_rebuilt_only_from_a_retained_capture() {
    use devup_mcp_figma::{
        CollectedPayload, CollectionRequest, CollectionScope, CollectionStats, FigmaTarget,
        PayloadCompleteness,
    };
    let (root, policy, input) = fixture();
    let store = ArtifactStore::default();
    let payload = CollectedPayload {
        target: FigmaTarget {
            file_key: "completion-fixture".into(),
            node_id: Some("root".into()),
            branch_key: None,
        },
        scope: CollectionScope::Node,
        metadata: Value::Null,
        snapshot: snapshot(true),
        variables: None,
        styles: None,
        completeness: PayloadCompleteness::UsedTokens,
        source_version: None,
        stats: CollectionStats::default(),
        assets: vec![],
        reference_png: None,
        failures: vec![],
    };
    let key = super::super::artifacts::ArtifactRequestKey::from_collection(
        &CollectionRequest::new(payload.target.clone(), CollectionScope::Node),
    );
    let artifact = store.insert(key, payload).await.unwrap();
    let record_path = root.join(".devup/exports/home.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["artifactId"] = json!(artifact.artifact_id);
    record["behaviorPlan"] =
        json!({"candidates":[],"dataBindings":[],"evidenceLimits":{"truncated":false}});
    std::fs::write(&record_path, record.to_string()).unwrap();
    let result = evaluate(&input, &policy, &store).await.unwrap();
    assert_eq!(
        result["blockers"]["incompleteBehaviorCoverage"], false,
        "{result}"
    );
    assert!(
        !result["requiredMotionIds"].as_array().unwrap().is_empty(),
        "{result}"
    );
    assert!(
        !result["requiredBindingIds"].as_array().unwrap().is_empty(),
        "{result}"
    );
    let unavailable = evaluate(&input, &policy, &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(unavailable["stage"], "incomplete");
    assert_eq!(unavailable["done"], false);
    assert_eq!(unavailable["blockers"]["incompleteBehaviorCoverage"], true);
    drop(artifact);
    drop(store);
    drop(policy);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn source_bound_static_checks_cannot_replace_figma_visuals() {
    let (root, policy, mut input) = fixture();
    let store = ArtifactStore::default();
    let (digest, _) = source_digest(&root, &policy).unwrap();
    let commands = ["build", "lint", "typecheck", "test", "e2e"]
        .map(|name| json!({"name":name,"command":["bun","run",name],"exitCode":0}));
    let checks = root.join(".devup/checks.json");
    std::fs::write(&checks,serde_json::to_vec(&json!({"kind":"devup-project-check-receipt","schemaVersion":1,"sourceDigest":digest,"sourceChanged":false,"checks":commands})).unwrap()).unwrap();
    input.check_receipt_path = Some(checks.to_string_lossy().into());
    let kinds = [
        "page-health",
        "no-horizontal-overflow",
        "no-clipped-content",
        "markup",
        "rendered-completeness",
    ];
    let plan:super::super::runtime_audit::PlanInput=serde_json::from_value(json!({"baseUrl":"http://127.0.0.1:4173","allowedOrigins":["http://127.0.0.1:4173"],"projectRoot":root,"sourceDigest":digest,"viewports":[{"width":360,"height":100}],"screens":[{"id":"home","routePath":"/","checks":kinds.map(|kind|if kind=="rendered-completeness"{json!({"kind":kind,"widths":[360],"completeness":{"frameHeight":100,"sections":[],"images":[]}})}else{json!({"kind":kind})})}]})).unwrap();
    let directory = root.join(".devup/audit");
    std::fs::create_dir_all(&directory).unwrap();
    let plan_path = directory.join("plan.json");
    let written = super::super::runtime_audit::create_plan(
        plan,
        &policy,
        Some(&plan_path.to_string_lossy()),
        None,
        &store,
    )
    .await
    .unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(360, 100)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let png = png.into_inner();
    std::fs::write(directory.join("home.png"), &png).unwrap();
    let report = json!({"schemaVersion":1,"kind":"devup-runtime-audit-report","planId":written["planId"],"planSha256":hash(&std::fs::read(&plan_path).unwrap()),"sourceDigest":digest,"sourceVerification":{"projectRoot":root,"beforeDigest":digest,"afterDigest":digest,"unchanged":true,"planMatched":true},"status":"completed","runs":[{"viewport":{"width":360,"height":100,"deviceScaleFactor":1},"environmentErrors":[],"screens":[{"id":"home","status":"completed","url":"http://127.0.0.1:4173/","screenshot":{"path":"home.png","sha256":hash(&png),"bytes":png.len()},"checks":kinds.iter().enumerate().map(|(index,kind)|json!({"index":index,"kind":kind,"status":"pass","measured":{}})).collect::<Vec<_>>()}]}]});
    let report_path = directory.join("report.json");
    std::fs::write(&report_path, serde_json::to_vec(&report).unwrap()).unwrap();
    input.audits.push(AuditPair {
        plan_path: plan_path.to_string_lossy().into(),
        report_path: report_path.to_string_lossy().into(),
    });
    let result = write(&input, &policy, &store, None).await.unwrap();
    assert_eq!(result["stage"], "implemented", "{result}");
    assert_eq!(result["blockers"]["missingVisualComparisons"], json!([["/",360]]));
    // Once both the observed E2E command and a native Figma comparison exist,
    // the same source-bound evidence can advance all completion stages.
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let mut payload = super::super::artifacts::w3_tests::payload();
    payload.reference_png = Some(devup_mcp_figma::ReferencePng {
        mime_type: "image/png".into(), data_base64: STANDARD.encode(&png),
        byte_length: png.len(), sha256: hash(&png), note: None,
    });
    let artifact = store.insert(super::super::artifacts::ArtifactRequestKey::from_collection(&super::super::artifacts::w3_tests::request()), payload).await.unwrap();
    let mut planned: Value = serde_json::from_slice(&std::fs::read(&plan_path).unwrap()).unwrap();
    planned["plan"]["screens"][0]["figma"] = json!({"nodeId":"root","drawnWidths":[360],"frames":[{"width":360,"height":100,"nodeId":"1:1","artifactId":artifact.artifact_id}]});
    let rewritten = super::super::runtime_audit::create_plan(serde_json::from_value(planned["plan"].clone()).unwrap(), &policy, Some(&plan_path.to_string_lossy()), None, &store).await.unwrap();
    let mut report = report;
    report["planId"] = rewritten["planId"].clone();
    report["planSha256"] = json!(hash(&std::fs::read(&plan_path).unwrap()));
    report["runs"][0]["screens"][0]["captureContext"] = json!({"innerWidth":360,"innerHeight":100,"devicePixelRatio":1,"visualViewportScale":1,"rootCssZoom":1,"bodyCssZoom":1});
    report["executor"] = json!({"browser":{"name":"chromium","version":"test"}});
    report["environment"] = json!({"os":"test","locale":"en-US","timezone":"UTC"});
    std::fs::write(&report_path, report.to_string()).unwrap();
    let ingested = super::super::runtime_audit::ingest_with_artifacts(&policy, &plan_path.to_string_lossy(), &report_path.to_string_lossy(), &store).await.unwrap();
    let comparison_id = ingested["visualComparisons"][0]["id"].as_str().unwrap().to_owned();
    input.visual_environments.insert(comparison_id, serde_json::from_value(json!({"tsxResource":{"uri":"devup://artifact/test/tsx","sha256":"a".repeat(64)},"themePath":"devup.json","assetDirectory":"public/images","devupUiVersion":"test","fontManifest":[{"family":"Test","sha256":"b".repeat(64)}]})).unwrap());
    let result = write(&input, &policy, &store, None).await.unwrap();
    assert_eq!(result["stage"], "data-verified", "{result}");
    // The one receipt that may be reported is the one that carries the offer.
    assert!(super::super::skills::marks_completion("devup_runtime_audit", "completion", &result));
    assert!(!super::super::skills::marks_completion("devup_runtime_audit", "ingest", &result));
    assert_eq!(refresh(&input.project_root, &policy, &store).await.unwrap()["done"], true);
    assert_eq!(result["dataVerificationScope"], "not-required");
    // A syntactically present but unsigned empty inventory cannot remove all
    // data/motion obligations and turn a generated fixture into a done project.
    let record_path = root.join(".devup/exports/home.json");
    let signed_record = std::fs::read(&record_path).unwrap();
    let mut unsigned: Value = serde_json::from_slice(&signed_record).unwrap();
    unsigned["behaviorPlan"] =
        json!({"candidates":[],"dataBindings":[],"evidenceLimits":{"truncated":false}});
    std::fs::write(&record_path, unsigned.to_string()).unwrap();
    let unverified = refresh(&input.project_root, &policy, &store).await.unwrap();
    assert_eq!(unverified["done"], false);
    assert_eq!(unverified["blockers"]["incompleteBehaviorCoverage"], true);
    let mut unidentified: Value = serde_json::from_slice(&signed_record).unwrap();
    unidentified["artifactId"] = Value::Null;
    std::fs::write(&record_path, unidentified.to_string()).unwrap();
    assert_eq!(
        refresh(&input.project_root, &policy, &store).await.unwrap()["done"],
        false
    );
    std::fs::write(&record_path, &signed_record).unwrap();
    // A copied plan digest does not prove browser execution observed sources.
    let mut unbound = report.clone();
    unbound["sourceVerification"] = Value::Null;
    std::fs::write(&report_path, unbound.to_string()).unwrap();
    assert_eq!(
        refresh(&input.project_root, &policy, &store).await.unwrap()["stages"]["browserVerified"],
        false
    );
    std::fs::write(&report_path, report.to_string()).unwrap();
    std::fs::write(
        root.join("src/page.tsx"),
        "export function New(){return <main/>}",
    )
    .unwrap();
    assert_eq!(
        refresh(&input.project_root, &policy, &store).await.unwrap()["done"],
        false
    );
    drop(policy);
    std::fs::remove_dir_all(root).unwrap();
}
