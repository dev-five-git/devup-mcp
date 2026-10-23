use super::*;

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
