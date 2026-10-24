use super::*;

#[tokio::test]
async fn scope_exclusions_survive_status_and_restart() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), false).await;
    request.action = "draft".into();
    request.answers.clear();
    request.scope_exclusions = vec![ScopeExclusion { r#ref: "posts".into(), reason: "Outside this release".into() }];
    run(&request, &policy).await.unwrap();
    request.scope_exclusions.clear();
    request.action = "status".into();
    let status = run(&request, &policy).await.unwrap();
    assert_eq!(status["plan"]["scopeExclusions"][0]["ref"], "posts");
    request.action = "intake".into();
    request.restart = true;
    run(&request, &policy).await.unwrap();
    assert_eq!(read(&request.project_root).unwrap()["scopeExclusions"][0]["ref"], "posts");
}

#[test]
fn feature_keyed_source_facts_supply_integration_recommendations() {
    let plan = json!({"facts":{},"answers":{},"entities":[],"issues":[],
        "evidence":[{"source":"spec","facts":{"contact.delivery":"SMTP email"}}],
        "features":[{"id":"contact","scope":"Contact","screens":[],"acceptance":[]}]});
    let questions = rounds::detail(&plan, &[]).unwrap();
    let question = questions.iter().find(|question| question["id"] == "contact.notifications").unwrap();
    assert!(text(&question["options"][0]["label"]).starts_with("SMTP email"));
    assert!(!question["sourcedDefaults"]["source"].is_null());
}

#[test]
fn excluded_features_do_not_supply_recommendations() {
    let plan = json!({"facts":{},"answers":{},"entities":[],"issues":[],
        "scopeExclusions":[{"ref":"contact","reason":"Not in release"}],
        "features":[{"id":"contact","scope":"Send SMTP email","screens":[],"acceptance":[]}]});
    assert!(!rounds::detail(&plan, &[]).unwrap().iter().any(|question| text(&question["id"]).starts_with("contact.")));
}

#[test]
fn smtp_evidence_recommends_email_instead_of_no_integration() {
    let plan = json!({"facts":{},"answers":{},"entities":[],"issues":[],
        "features":[{"id":"contact","scope":"Send SMTP email","screens":[],"acceptance":[]} ]});
    let questions = rounds::detail(&plan, &[]).unwrap();
    let question = questions.iter().find(|question| question["id"] == "contact.notifications").unwrap();
    assert!(text(&question["options"][0]["label"]).starts_with("SMTP email"));
}

#[tokio::test]
async fn restart_archives_plan_screen_and_units_before_creating_new_intake() {
    let dir = TestDir::new();
    let (_, policy, _) = drafted(dir.path(), false).await;
    std::fs::write(dir.path().join(".devup/screen-plan.json"), "original screen").unwrap();
    std::fs::create_dir_all(dir.path().join(".devup/units")).unwrap();
    std::fs::write(dir.path().join(".devup/units/worker.md"), "original unit").unwrap();
    let mut request = input(dir.path(), "intake");
    request.restart = true;
    request.detail = Some("full".into());
    let result = run(&request, &policy).await.unwrap();
    let archive = dir.path().join(text(&result["plan"]["archivedTo"]));
    assert!(archive.join("project-plan.json").is_file());
    assert!(archive.join("screen-plan.json").is_file());
    assert!(archive.join("units/worker.md").is_file());
    assert!(!dir.path().join(".devup/screen-plan.json").exists());
    assert_eq!(result["state"], "intake");
}

#[tokio::test]
async fn existing_intake_returns_current_plan_and_status_recovery() {
    let dir = TestDir::new();
    let (_, policy, draft) = drafted(dir.path(), false).await;
    let request = input(dir.path(), "intake");
    let error = run(&request, &policy).await.unwrap_err();
    assert_eq!(error.details["existingPlan"]["planId"], draft["planId"]);
    assert_eq!(error.details["existingPlan"]["state"], "draft");
    let recovery: ProjectPlanInput = serde_json::from_value(error.details["nextAction"]["arguments"].clone()).unwrap();
    assert_eq!(run(&recovery, &policy).await.unwrap()["planId"], draft["planId"]);
}

#[test]
fn existing_model_diagnostics_report_type_errors_and_entity_drift() {
    let dir = TestDir::new();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    std::fs::write(dir.path().join("models/post.json"), json!({"name":"post","columns":[{"name":"body","type":"jsonb","nullable":false}]}).to_string()).unwrap();
    let report = erd::disk_report(&json!({"modelsDir":"models","entities":[]}), &dir.path().to_string_lossy());
    assert_eq!(report["entityDrift"], json!(["post"]));
    assert_eq!(report["typeValidation"]["violations"][0]["got"], "jsonb");
}

#[test]
fn invalid_types_report_every_column_path() {
    let error = erd::validate(&[json!({"name":"post","columns":[
        {"name":"first","type":"invalid_one","nullable":false},
        {"name":"second","type":"invalid_two","nullable":false}
    ]})]).unwrap_err();
    assert_eq!(array(&error.details["violations"]).len(), 2);
    assert_eq!(error.details["violations"][1]["path"], "/entities/0/columns/1/type");
}

#[test]
fn list_counts_supply_pagination_and_enum_filters() {
    let mut plan = json!({"facts":{"parts":"API"},"entities":[{"name":"post","columns":[{"name":"category","type":{"kind":"enum","values":["a","b"]}}]}],
        "features":[{"id":"posts","scope":"post 목록10","screens":[],"acceptance":[],"entities":["post"]}]});
    wiring::draft(&mut plan);
    let queries = array(&plan["endpointContracts"][0]["queryParams"]);
    for name in ["page", "pageSize", "category"] {
        assert!(queries.iter().any(|query| query["name"] == name), "{name}");
    }
}

#[test]
fn negated_korean_writes_do_not_become_operations() {
    let mut plan = json!({"facts":{"parts":"API"},"entities":[{"name":"post","columns":[]}],
        "features":[{"id":"posts","scope":"post 목록","screens":[],"acceptance":["등록·수정 불가", "미등록"],"entities":["post"]}]});
    wiring::draft(&mut plan);
    assert_eq!(plan["dataMappings"][0]["requiredOperations"], json!(["list"]));
}

#[tokio::test]
async fn confirm_accepts_explicit_confirmed_decision_state() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), false).await;
    request.decisions = vec![serde_json::from_value(json!({"id":"scope","reason":"Approved scope","recommended":"Keep scope","state":"confirmed","kind":"scope-exclusion"})).unwrap()];
    let result = run(&request, &policy).await.unwrap();
    assert_eq!(result["plan"]["decisions"][0]["state"], "confirmed");
    assert_eq!(result["plan"]["decisions"][0]["kind"], "scope-exclusion");
}

#[test]
fn admin_label_does_not_imply_delete_operation() {
    let mut plan = json!({"facts":{"parts":"API"},"entities":[{"name":"post","columns":[]}],
        "features":[{"id":"posts","scope":"admin post list","screens":[],"acceptance":[],"entities":["post"]}]});
    wiring::draft(&mut plan);
    assert!(!array(&plan["dataMappings"][0]["requiredOperations"]).contains(&json!("delete")));
}

#[tokio::test]
async fn explicit_nested_models_dir_selects_api_working_directory() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), true).await;
    request.models_dir = Some("apis/api/models".into());
    let result = run(&request, &policy).await.unwrap();
    assert_eq!(result["plan"]["modelsWorkingDirectory"], "apis/api");
}

#[tokio::test]
async fn confirm_refuses_to_drop_existing_columns_without_writing_models() {
    let dir = TestDir::new();
    let (request, policy, draft) = drafted(dir.path(), true).await;
    let mut post = array(&draft["plan"]["entities"]).iter().find(|entity| entity["name"] == "post").unwrap().clone();
    post["columns"].as_array_mut().unwrap().push(json!({"name":"category","type":"text","nullable":false}));
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    let path = dir.path().join("models/post.json");
    let original = serde_json::to_vec(&post).unwrap();
    std::fs::write(&path, &original).unwrap();
    let error = run(&request, &policy).await.unwrap_err();
    assert_eq!(error.details["removedColumns"], json!([{"table":"post","column":"category"}]));
    assert_eq!(error.details["decision"]["destructive"], true);
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert!(!dir.path().join("models/user.json").exists());
    assert_eq!(read(&request.project_root).unwrap()["state"], "draft");
}

#[test]
fn empty_prefix_header_preserves_thirty_character_capacity() {
    let name = "123456789012345678901234567890";
    assert_eq!(screen_plan::header("", name), name);
    assert_eq!(screen_plan::header("Page", "Home"), "Page: Home");
}

#[tokio::test]
async fn stale_status_id_returns_current_plan_and_executable_recovery() {
    let dir = TestDir::new();
    let (mut request, policy, draft) = drafted(dir.path(), false).await;
    request.action = "status".into();
    request.plan_id = Some("stale".into());
    let error = run(&request, &policy).await.unwrap_err();
    assert_eq!(error.details["planId"], draft["planId"]);
    assert_eq!(error.details["state"], "draft");
    let recovery: ProjectPlanInput = serde_json::from_value(error.details["nextAction"]["arguments"].clone()).unwrap();
    assert_eq!(run(&recovery, &policy).await.unwrap()["planId"], draft["planId"]);
}
