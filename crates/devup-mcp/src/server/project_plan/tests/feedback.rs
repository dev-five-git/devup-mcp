use super::*;

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
