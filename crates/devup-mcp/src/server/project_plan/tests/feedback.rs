use super::*;

#[test]
fn api_contract_report_requires_bindings_and_real_http_operations() {
    let dir = TestDir::new();
    std::fs::write(dir.path().join("openapi.json"), json!({"paths":{
        "/posts":{"get":{"operationId":"listPosts"},"parameters":{"operationId":"notAnOperation"}}
    }}).to_string()).unwrap();
    let mut plan = json!({"features":[{"id":"news","entities":["post"]}],"apiWiring":[]});
    let root = dir.path().to_string_lossy();
    assert_eq!(wiring::report(&plan, &root)["missing"][0]["feature"], "news");
    plan["apiWiring"] = json!([{"feature":"news","operationId":"notAnOperation"}]);
    assert_eq!(array(&wiring::report(&plan, &root)["missing"]).len(), 1);
    plan["apiWiring"][0]["operationId"] = json!("listPosts");
    assert!(array(&wiring::report(&plan, &root)["missing"]).is_empty());
    assert_eq!(array(&wiring::report(&plan, &root)["wired"]).len(), 1);
}

#[tokio::test]
async fn split_adds_supervisor_integration_after_all_workers() {
    let dir = TestDir::new();
    let (request, policy, _) = drafted(dir.path(), false).await;
    run(&request, &policy).await.unwrap();
    let mut plan = read(&request.project_root).unwrap();
    let mut tx = OutputTransaction::new();
    units::split(&request, &mut plan, &mut tx, &policy).unwrap();
    let integration = array(&plan["units"]).iter().find(|u| u["id"] == "integration").unwrap();
    assert_eq!(integration["stage"], 2);
    assert_eq!(integration["owner"], "supervisor");
    assert_eq!(integration["dependencies"], json!(["posts-front"]));
}

#[test]
fn backend_checks_require_runtime_dependencies_and_real_module() {
    let dir = TestDir::new();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[dev-dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n").unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "// mod models;\nfn main() {}\n").unwrap();
    let plan = json!({"state":"confirmed","modelsWorkingDirectory":"."});
    assert_eq!(wiring::backend_checks(&plan, &dir.path().to_string_lossy()).len(), 3);
    std::fs::write(dir.path().join("Cargo.toml"), "[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n").unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "mod models;\nfn main() {}\n").unwrap();
    assert!(wiring::backend_checks(&plan, &dir.path().to_string_lossy()).is_empty());
}

#[tokio::test]
async fn split_scope_prioritizes_confirmed_feature_answers() {
    let dir = TestDir::new();
    let (request, policy, _) = drafted(dir.path(), false).await;
    run(&request, &policy).await.unwrap();
    let mut plan = read(&request.project_root).unwrap();
    plan["features"][0]["scope"] = json!("Posts with /ko prefix");
    plan["answers"]["posts.locale"] = json!("No locale prefix");
    let mut tx = OutputTransaction::new();
    units::split(&request, &mut plan, &mut tx, &policy).unwrap();
    let front = array(&plan["units"]).iter().find(|u| u["id"] == "posts-front").unwrap();
    assert_eq!(front["scopeOverrides"]["posts.locale"], "No locale prefix");
    assert_eq!(front["scopePrecedence"], json!(["answers", "decisions", "scope"]));
}

#[tokio::test]
async fn split_briefs_contain_only_owned_route_and_worker_files() {
    let dir = TestDir::new();
    let (request, policy, _) = drafted(dir.path(), false).await;
    run(&request, &policy).await.unwrap();
    let mut plan = read(&request.project_root).unwrap();
    plan["features"][0]["screens"] = json!(["/posts"]);
    plan["facts"]["posts.exports"] = json!(["exports/posts.tsx"]);
    plan["facts"]["posts.assets"] = json!([{"path":"/posts.webp","url":"figma-bridge://current?node-id=1-2"}]);
    plan["facts"]["posts.imports"] = json!(["@/components/common/PageHero"]);
    plan["facts"]["protectedFiles"] = json!(["src/components/layout/Header.tsx"]);
    plan["screenPlan"] = json!({"state":"confirmed","routes":[
        {"url":"/posts","nodeIds":["1:2"],"file":"src/app/posts/page.tsx"},
        {"url":"/unrelated","nodeIds":["3:4"],"file":"src/app/unrelated/page.tsx"}
    ]});
    let mut tx = OutputTransaction::new();
    let mut split = request.clone();
    split.action = "split".into();
    units::split(&split, &mut plan, &mut tx, &policy).unwrap();
    tx.commit().unwrap();
    let unit = array(&plan["units"]).iter().find(|u| u["id"] == "posts-front").unwrap();
    assert_eq!(unit["contract"]["routes"].as_array().unwrap().len(), 1);
    assert_eq!(unit["contract"]["exports"], json!(["exports/posts.tsx"]));
    assert_eq!(unit["contract"]["imports"], json!(["@/components/common/PageHero"]));
    assert_eq!(unit["contract"]["assets"][0]["path"], "/posts.webp");
    assert_eq!(unit["contract"]["protectedFiles"], json!(["src/components/layout/Header.tsx"]));
    let brief = std::fs::read_to_string(dir.path().join(".devup/units/posts-front.md")).unwrap();
    assert!(brief.contains("exports/posts.tsx"));
    assert!(!brief.contains("/unrelated"));
}

#[tokio::test]
async fn project_facts_auto_fill_screen_answers_with_explicit_answer_precedence() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), false).await;
    std::fs::write(dir.path().join(".devup/screen-plan.json"), json!({
        "kind":"devup-screen-plan","schemaVersion":1,"planId":"screen","state":"draft",
        "questions":[{"id":"siteName"},{"id":"siteUrl"}]
    }).to_string()).unwrap();
    request.action = "draft".into();
    request.answers.clear();
    request.answers.insert("siteUrl".into(), json!("https://explicit.example"));
    request.evidence = vec![serde_json::from_value(json!({"source":"spec","facts":{
        "siteName":"Chondrogen","siteUrl":"https://source.example"
    }})).unwrap()];
    let result = run(&request, &policy).await.unwrap();
    assert_eq!(result["screenPlanForward"]["arguments"]["answers"], json!({"siteName":"Chondrogen","siteUrl":"https://explicit.example"}));
    assert!(read(&request.project_root).unwrap()["answers"]["siteName"].is_null());
}

#[tokio::test]
async fn redraft_reports_added_changed_and_kept_records() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), false).await;
    request.action = "draft".into();
    request.answers.clear();
    request.decisions = vec![decision("keep"), decision("change")];
    run(&request, &policy).await.unwrap();
    request.decisions = vec![decision("new"), Decision {recommended:"Different".into(), ..decision("change")}];
    request.answers.insert("posts.states".into(), json!("Empty list"));
    let result = run(&request, &policy).await.unwrap();
    assert_eq!(result["changeSummary"]["decisions"], json!({"added":["new"],"changed":["change"],"kept":["keep"],"removed":[]}));
    assert_eq!(result["changeSummary"]["features"]["kept"], json!(["posts"]));
    assert_eq!(result["changeSummary"]["answers"]["added"], json!(["posts.states"]));
}

#[test]
fn populated_erd_choices_bind_entity_columns_and_lifecycle() {
    let plan = json!({"facts":{"parts":"API"},"answers":{},"issues":[],
        "features":[{"id":"news","scope":"News","acceptance":[]}],
        "entities":[{"name":"post","columns":[{"name":"published_at","type":"timestamptz","nullable":true,"index":true}]}]});
    let questions = rounds::detail(&plan, &[]).unwrap();
    let lifecycle = questions.iter().find(|q| q["id"] == "lifecycle").unwrap();
    let indexes = questions.iter().find(|q| q["id"] == "indexes").unwrap();
    assert_eq!(lifecycle["entities"], json!(["post"]));
    assert_ne!(lifecycle["options"], indexes["options"]);
    assert!(text(&indexes["resolve"][0]["action"]).contains("post.published_at"));
    assert!(text(&lifecycle["resolve"][1]["action"]).contains("post"));
}

#[test]
fn shared_feature_topics_are_one_question_with_concrete_choices() {
    let plan = json!({"facts":{},"answers":{},"issues":[],"entities":[],"features":[
        {"id":"contact","scope":"Contact form with SMTP email","acceptance":[]},
        {"id":"signup","scope":"Signup form with SMTP email","acceptance":[]}
    ]});
    let questions = rounds::detail(&plan, &[]).unwrap();
    let forms = questions.iter().filter(|q| text(&q["id"]).ends_with(".forms")).collect::<Vec<_>>();
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0]["features"], json!(["contact", "signup"]));
    let notifications = questions.iter().find(|q| text(&q["id"]).ends_with(".notifications")).unwrap();
    assert_eq!(array(&notifications["options"]).len(), 3);
    assert!(array(&notifications["resolve"]).iter().any(|option| option["action"] == "SMTP email to a staff address"));
}

#[tokio::test]
async fn confirm_missing_answers_returns_executable_recovery() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), false).await;
    request.answers.clear();
    let error = run(&request, &policy).await.unwrap_err();
    let details = error.details;
    assert_eq!(details["nextAction"]["tool"], "devup_project_plan");
    assert!(!array(&details["missingQuestionIds"]).is_empty());
    let recovery: ProjectPlanInput = serde_json::from_value(details["nextAction"]["arguments"].clone()).unwrap();
    assert_eq!(run(&recovery, &policy).await.unwrap()["state"], "confirmed");
}

#[tokio::test]
async fn confirm_summary_counts_all_persisted_decisions() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), false).await;
    request.decisions = vec![decision("session-expiry"), decision("no-locale-prefix")];
    request.detail = None;
    let result = run(&request, &policy).await.unwrap();
    assert_eq!(result["decisionCount"], 2);
    let stored = read(&request.project_root).unwrap();
    assert!(kept(&stored["decisions"], "session-expiry"));
    assert!(kept(&stored["decisions"], "no-locale-prefix"));
}

#[tokio::test]
async fn api_without_entities_reports_erd_draft_guidance() {
    let dir = TestDir::new();
    let policy = OutputPolicy::from_roots(vec![dir.path().to_path_buf()]).unwrap();
    let mut request = input(dir.path(), "intake");
    request.evidence = vec![serde_json::from_value(json!({
        "source":"scope", "facts":{"parts":"front + API"}
    })).unwrap()];
    let result = run(&request, &policy).await.unwrap();
    assert_eq!(result["erdDraftRequired"]["nextAction"]["arguments"]["action"], "draft");
    assert_eq!(result["erdDraftRequired"]["nextAction"]["arguments"]["planId"], result["planId"]);
    assert!(text(&result["erdDraftRequired"]["reason"]).contains("API"));
    request.action = "status".into();
    let result = run(&request, &policy).await.unwrap();
    assert!(result["erdDraftRequired"].is_object());
}

#[tokio::test]
async fn split_uses_one_shared_table_owner_and_skips_static_backend_layers() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), true).await;
    request.action = "draft".into();
    request.features = vec![
        Feature {id:"layout".into(),scope:"Header and footer".into(),screens:vec!["Layout".into()],acceptance:vec![],entities:vec![]},
        Feature {id:"news".into(),scope:"Public news".into(),screens:vec!["News".into()],acceptance:vec![],entities:vec!["post".into()]},
        Feature {id:"admin-news".into(),scope:"Manage news".into(),screens:vec!["Editor".into()],acceptance:vec![],entities:vec!["post".into()]},
    ];
    run(&request, &policy).await.unwrap();
    request.action = "confirm".into();
    request.answers.clear();
    request.accept_recommended = true;
    run(&request, &policy).await.unwrap();
    request.action = "split".into();
    let result = run(&request, &policy).await.unwrap();
    let units = array(&result["plan"]["units"]);
    assert_eq!(units.iter().filter(|unit| unit["id"] == "model-post").count(), 1);
    assert!(!units.iter().any(|unit| ["layout-api","layout-client","layout-models"].contains(&text(&unit["id"]))));
    for id in ["news-api", "admin-news-api"] {
        assert_eq!(units.iter().find(|unit| unit["id"] == id).unwrap()["dependencies"], json!(["model-post"]));
    }
}

#[test]
fn static_features_only_ask_relevant_topics_and_sourced_answers_suppress_them() {
    let mut plan = json!({"facts":{},"answers":{},"features":[
        {"id":"products","scope":"Product introduction","screens":["Products"],"acceptance":["Display descriptions"]}
    ],"issues":[],"entities":[]});
    let questions = rounds::detail(&plan, &[]).unwrap();
    assert!(!questions.iter().any(|question| ["products.forms","products.permissions","products.notifications"].contains(&text(&question["id"]))));
    plan["features"][0] = json!({"id":"contact","scope":"Contact form with SMTP staff email","screens":["Contact"],"acceptance":["Validate email"]});
    let questions = rounds::detail(&plan, &[]).unwrap();
    assert!(questions.iter().any(|question| question["id"] == "contact.forms"));
    assert!(questions.iter().any(|question| question["id"] == "contact.notifications"));
    plan["facts"]["contact.forms"] = json!("Required email");
    plan["facts"]["contact.notifications"] = json!("SMTP staff email");
    let questions = rounds::detail(&plan, &[]).unwrap();
    assert!(!questions.iter().any(|question| ["contact.forms","contact.notifications"].contains(&text(&question["id"]))));
}

#[tokio::test]
async fn backend_checks_report_missing_serde_and_model_module() {
    let dir = TestDir::new();
    let (request, policy, _) = drafted(dir.path(), true).await;
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[dependencies]\nsea-orm = \"2\"\n").unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let result = run(&request, &policy).await.unwrap();
    assert_eq!(array(&result["backendChecks"]).len(), 3);
    std::fs::write(dir.path().join("Cargo.toml"), "[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n").unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "mod models;\nfn main() {}\n").unwrap();
    assert!(wiring::backend_checks(&read(&request.project_root).unwrap(), &request.project_root).is_empty());
}

#[test]
fn merged_screen_answers_are_forwarded_without_becoming_project_answers() {
    let dir = TestDir::new();
    std::fs::create_dir_all(dir.path().join(".devup")).unwrap();
    std::fs::write(dir.path().join(".devup/screen-plan.json"), json!({
        "kind":"devup-screen-plan", "schemaVersion":1,"planId":"screen-id","state":"draft",
        "questions":[{"id":"route:1:2"}]
    }).to_string()).unwrap();
    let answers = BTreeMap::from([
        ("route:1:2".into(), json!("/contact")),
        ("parts".into(), json!("front only"))
    ]);
    let (own, forwarded) = forward_screen_answers(&dir.path().to_string_lossy(), &answers, false);
    assert_eq!(own, BTreeMap::from([("parts".into(), json!("front only"))]));
    assert_eq!(forwarded["arguments"]["answers"], json!({"route:1:2":"/contact"}));
    assert_eq!(forwarded["arguments"]["planId"], "screen-id");
    assert_eq!(forwarded["arguments"]["action"], "confirm");
}

#[tokio::test]
async fn figma_prerequisite_is_reported_at_intake_and_draft() {
    let dir = TestDir::new();
    let policy = OutputPolicy::from_roots(vec![dir.path().to_path_buf()]).unwrap();
    let mut request = input(dir.path(), "intake");
    let url = "https://www.figma.com/design/fixture/site?node-id=1-2";
    request.evidence = vec![serde_json::from_value(json!({"source":"Figma", "facts":{"figma":[url],"goal":"Company site"}})).unwrap()];
    let intake = run(&request, &policy).await.unwrap();
    assert_eq!(intake["nextAction"]["tool"], "devup_screen_plan");
    assert_eq!(intake["nextAction"]["arguments"]["url"], url);
    request.plan_id = intake["planId"].as_str().map(str::to_owned);
    request.action = "draft".into();
    request.features = vec![Feature {id:"home".into(),scope:"Company".into(),screens:vec!["Home".into()],acceptance:vec![],entities:vec![]}];
    let draft = run(&request, &policy).await.unwrap();
    assert!(array(&draft["blockers"]).contains(&json!("screenPlan: not confirmed")));
    assert_eq!(draft["nextAction"]["arguments"]["projectRoot"], request.project_root);
}

#[tokio::test]
async fn confirm_reports_written_models_and_backend_commands_in_summary() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), true).await;
    request.detail = None;
    let confirmed = run(&request, &policy).await.unwrap();
    assert_eq!(confirmed["nextAction"]["arguments"]["action"], "split");
    assert_eq!(confirmed["writtenTo"], json!(["models/user.json", "models/post.json"]));
    assert_eq!(confirmed["backendCommands"][0], "vespertide diff");
    assert_eq!(confirmed["modelsWorkingDirectory"], ".");
    assert_eq!(confirmed["unitState"], "not-split");
    for path in array(&confirmed["writtenTo"]) {
        assert!(dir.path().join(text(path)).is_file());
    }
}

#[tokio::test]
async fn confirm_rejects_jsonb_before_writing_any_models() {
    let dir = TestDir::new();
    let (mut request, policy, draft) = drafted(dir.path(), true).await;
    request.entities = array(&draft["plan"]["entities"]).to_vec();
    request.entities[1]["columns"][1]["type"] = json!("jsonb");
    let error = run(&request, &policy).await.unwrap_err();
    assert!(error.message.contains("post.title"));
    assert!(error.message.contains("jsonb"));
    assert!(!dir.path().join("models").exists());
    assert_eq!(read(&request.project_root).unwrap()["state"], "draft");
}

#[tokio::test]
async fn draft_decisions_survive_an_answers_only_redraft() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), false).await;
    request.action = "draft".into();
    request.decisions = vec![decision("session-expiry"), decision("upload-limit")];
    run(&request, &policy).await.unwrap();
    request.decisions.clear();
    request.answers.clear();
    request.answers.insert("posts.states".into(), json!("Show an empty list"));
    let draft = run(&request, &policy).await.unwrap();
    assert!(kept(&draft["plan"]["decisions"], "session-expiry"));
    assert!(kept(&draft["plan"]["decisions"], "upload-limit"));
}


#[tokio::test]
async fn intake_discovers_korean_spec_documents_without_extracting_contents() {
    let dir = TestDir::new();
    let spec = "동국대학교 콘드로젠 요구사항 정의서 ver 0.5.pdf";
    std::fs::write(dir.path().join(spec), b"pdf contents are not evidence").unwrap();
    std::fs::write(dir.path().join("unrelated.pdf"), b"not a spec").unwrap();
    let policy = OutputPolicy::from_roots(vec![dir.path().to_path_buf()]).unwrap();
    let request = input(dir.path(), "intake");
    run(&request, &policy).await.unwrap();
    let saved = read(&request.project_root).unwrap();
    assert_eq!(saved["facts"]["specCandidates"], json!([spec]));
    assert!(array(&saved["questions"]).iter().any(|question| question["id"] == "sources" && text(&question["question"]).contains(spec)));
    assert!(array(&saved["evidence"]).is_empty());
}

#[tokio::test]
async fn new_feature_acceptance_and_answers_are_applied_in_one_draft() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), false).await;
    request.action = "draft".into();
    request.answers.insert("inquiry.forms".into(), json!("Require staff email"));
    request.features = vec![Feature {
        id:"inquiry".into(), scope:"Contact form".into(), screens:vec![],
        acceptance:vec!["Validate email before submission".into()], entities:vec![],
    }];
    let draft = run(&request, &policy).await.unwrap();
    assert_eq!(draft["plan"]["answers"]["inquiry.forms"], "Require staff email");
    assert!(!array(&draft["plan"]["questions"]).iter().any(|question| question["id"] == "inquiry.screens"));
    assert_eq!(draft["featuresWithoutScreens"]["features"], json!(["inquiry"]));
    let form = array(&draft["plan"]["questions"]).iter().find(|question| question["id"] == "inquiry.forms").unwrap();
    assert!(text(&form["question"]).contains("Validate email before submission"));
}

#[tokio::test]
async fn status_summary_does_not_duplicate_questions_or_source_facts() {
    let dir = TestDir::new();
    let (mut request, policy, _) = drafted(dir.path(), false).await;
    request.action = "status".into();
    request.detail = None;
    let summary = run(&request, &policy).await.unwrap();
    assert!(summary.get("plan").is_none());
    assert!(summary.get("questionRounds").is_none());
    assert!(summary.get("proposalMarkdown").is_none());
    assert!(summary["openQuestionCount"].as_u64().unwrap() > 0);
    assert!(summary["nextAction"].is_object());
    assert!(serde_json::to_vec(&summary).unwrap().len() < 8192);
}

#[tokio::test]
async fn draft_accepts_new_evidence_before_intake_answers_without_losing_them() {
    let dir = TestDir::new();
    let policy = OutputPolicy::from_roots(vec![dir.path().to_path_buf()]).unwrap();
    let mut request = input(dir.path(), "intake");
    let intake = run(&request, &policy).await.unwrap();
    request.plan_id = intake["planId"].as_str().map(str::to_owned);
    request.action = "draft".into();
    request.features = vec![Feature {
        id: "home".into(), scope: "Company introduction".into(),
        screens: vec!["Home".into()], acceptance: vec![], entities: vec![],
    }];
    request.evidence = vec![serde_json::from_value(json!({
        "source":"requirements.pdf", "facts":{"goal":"Publish the company site"}
    })).unwrap()];
    let draft = run(&request, &policy).await.unwrap();
    assert_eq!(draft["state"], "draft");
    assert!(array(&draft["openQuestionIds"]).contains(&json!("parts")));
    assert_eq!(read(&request.project_root).unwrap()["facts"]["goal"], "Publish the company site");
    request.features.clear();
    request.evidence.clear();
    request.answers.insert("parts".into(), json!("front only"));
    run(&request, &policy).await.unwrap();
    assert_eq!(read(&request.project_root).unwrap()["answers"]["parts"], "front only");
}
