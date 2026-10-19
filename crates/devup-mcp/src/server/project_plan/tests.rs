struct TestDir(std::path::PathBuf);
impl TestDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "devup-project-plan-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
use super::*;
fn input(root: &Path, action: &str) -> ProjectPlanInput {
    serde_json::from_value(json!({"action":action,"projectRoot":root})).unwrap()
}
async fn drafted(root: &Path, api: bool) -> (ProjectPlanInput, OutputPolicy, Value) {
    let policy = OutputPolicy::from_roots(vec![root.to_path_buf()]).unwrap();
    let mut i = input(root, "intake");
    i.evidence = vec![serde_json::from_str(include_str!("../../../tests/fixtures/project-plan/figma.json")).unwrap()];
    let r = run(&i, &policy).await.unwrap();
    i.plan_id = r["planId"].as_str().map(str::to_owned);
    i.action = "draft".into();
    i.answers = BTreeMap::from([
        ("workType".into(), json!("new project")),
        ("sources".into(), json!("Use recorded sources")),
        ("goal".into(), json!("Use source scope")),
        (
            "parts".into(),
            json!(if api {
                "front + admin + API"
            } else {
                "front only"
            }),
        ),
    ]);
    i.features = vec![Feature {
        id: "posts".into(),
        scope: "Post authoring".into(),
        screens: vec!["Post list".into()],
        acceptance: vec!["Authenticated authors publish posts".into()],
    }];
    i.evidence.push(serde_json::from_value(json!({"source":"repo defaults","facts":{
        "goal":"Source defaults cannot replace the selected goal",
        "parts": if api {"front only"} else {"front + admin + API"}
    }})).unwrap());
    let r = run(&i, &policy).await.unwrap();
    assert_eq!(r["plan"]["facts"]["goal"], "Build a post service");
    assert_eq!(r["plan"]["facts"]["parts"], if api {"front + admin + API"} else {"front only"});
    i.action = "confirm".into();
    i.evidence.clear();
    i.features.clear();
    i.answers = array(&r["plan"]["questions"])
        .iter()
        .map(|q| (text(&q["id"]).into(), q["options"][0]["label"].clone()))
        .collect();
    (i, policy, r)
}
#[tokio::test]
async fn figma_erd_confirm_writes_schema_valid_models() {
    let dir = TestDir::new();
    let (mut i, policy, draft) = drafted(dir.path(), true).await;
    assert!(
        draft["erdMermaid"]
            .as_str()
            .unwrap()
            .contains("user ||--o{ post")
    );
    i.entities = array(&draft["plan"]["entities"]).to_vec();
    i.entities[0]["columns"].as_array_mut().unwrap().iter_mut()
        .find(|c| c["name"] == "display_name").unwrap()["type"] = json!({"kind":"varchar", "length":100});
    let r = run(&i, &policy).await.unwrap();
    assert_eq!(r["state"], "confirmed");
    assert!(r["erdMermaid"].as_str().unwrap().contains("varchar display_name"));
    let schema: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/vespertide/model.schema.json"
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for name in ["user", "post"] {
        let model: Value = serde_json::from_slice(
            &std::fs::read(dir.path().join(format!("models/{name}.json"))).unwrap(),
        )
        .unwrap();
        if let Some(error) = validator.iter_errors(&model).next() {
            panic!("{error}");
        }
        if name == "post" {
            let sample: Value = serde_json::from_str(include_str!("../../../tests/fixtures/project-plan/post.json")).unwrap();
            assert_eq!(model, sample);
        }
        assert!(model["$schema"].is_string());
    }
    assert!(
        r["plan"]["entities"][0]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "display_name")
    );
    let source = dir.path().join("large/models");
    std::fs::create_dir_all(&source).unwrap();
    for index in 0..103 {
        let name = format!("model_{index}");
        std::fs::write(source.join(format!("{name}.json")), serde_json::to_vec(&json!({
            "name":name,"columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]
        })).unwrap()).unwrap();
    }
    let mut input = input(&dir.path().join("large"), "draft");
    input.models_dir = Some("models".into());
    let mut plan = json!({"facts":{"parts":"API"},"entities":[],"relations":[]});
    erd::draft(&input, &mut plan).unwrap();
    assert_eq!(array(&plan["entities"]).len(), 103);

}
#[tokio::test]
async fn accepting_the_proposal_confirms_without_answers() {
    let dir = TestDir::new();
    let (mut i, policy, draft) = drafted(dir.path(), true).await;
    assert_eq!(
        array(&draft["proposal"]).len(),
        array(&draft["plan"]["questions"]).len()
    );
    i.answers.clear();
    i.accept_recommended = true;
    let r = run(&i, &policy).await.unwrap();
    assert_eq!(r["state"], "confirmed");
}
#[tokio::test]
async fn many_to_many_join_table_and_gapped_roles_validate() {
    let dir = TestDir::new();
    let (mut i, policy, r) = drafted(dir.path(), true).await;
    i.entities = array(&r["plan"]["entities"]).to_vec();
    i.relations.push(Relation {
        from: "user".into(),
        to: "post".into(),
        cardinality: "manyToMany".into(),
        column: None,
        join_table: Some("user_post".into()),
        on_delete: "cascade".into(),
    });
    let r = run(&i, &policy).await.unwrap();
    let entities = array(&r["plan"]["entities"]);
    let schema: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/vespertide/model.schema.json"
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for e in entities {
        assert!(validator.is_valid(e), "{e}");
    }
    let join = entities.iter().find(|e| e["name"] == "user_post").unwrap();
    assert!(
        array(&join["columns"])
            .iter()
            .all(|c| c["primary_key"] == true && c["index"] == true)
    );
    let mut bad = entities.to_vec();
    bad[0]["columns"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["name"] == "role")
        .unwrap()["type"]["values"][1]["value"] = json!(1);
    assert!(erd::validate(&bad).is_err());
}
#[tokio::test]
async fn split_three_issues_preserves_dag_and_briefs() {
    let dir = TestDir::new();
    let (i, policy, _) = drafted(dir.path(), false).await;
    run(&i, &policy).await.unwrap();
    let mut plan = read(&i.project_root).unwrap();
    plan["screenPlan"] = json!({"status":"confirmed", "questions":[{"question":"Do not ask this?"}],
        "questionRounds":[{"question":"Do not repeat this?"}], "answers":{"route":"/posts"}});
    plan["issues"] = json!([
        {"key":"DEV-1","scope":"Models","dependencies":[],"acceptance":["schema passes"]},
        {"key":"DEV-2","scope":"API","dependencies":["DEV-1"],"acceptance":["CRUD"]},
        {"key":"DEV-3","scope":"Pages","dependencies":["DEV-2"],"acceptance":["visual audit"]}]);
    let mut tx = OutputTransaction::new();
    write(&mut tx, &policy, &i.project_root, &plan).unwrap();
    tx.commit().unwrap();
    let mut i = i;
    i.action = "split".into();
    i.answers.clear();
    let r = run(&i, &policy).await.unwrap();
    assert_eq!(array(&r["plan"]["units"]).len(), 3);
    assert_eq!(r["plan"]["units"][2]["dependencies"], json!(["DEV-2"]));
    let brief = std::fs::read_to_string(dir.path().join(".devup/units/DEV-3.md")).unwrap();
    for required in [
        "Build a post service",
        "Workers never ask",
        "ERD slice",
        "Screen-plan slice",
        "devup_stack_diff",
    ] {
        assert!(brief.contains(required));
    }
    assert!(!brief.contains('?'));
}
#[tokio::test]
async fn feature_split_and_decisions_and_state_gates() {
    let dir = TestDir::new();
    let (mut i, policy, _) = drafted(dir.path(), true).await;
    let mut incomplete = i.clone();
    incomplete.answers.clear();
    assert!(run(&incomplete, &policy).await.is_err());
    run(&i, &policy).await.unwrap();
    i.action = "split".into();
    i.answers.clear();
    let r = run(&i, &policy).await.unwrap();
    assert_eq!(array(&r["plan"]["units"]).len(), 5);
    assert_eq!(
        r["plan"]["units"][4]["dependencies"],
        json!(["posts-client"])
    );
    i.action = "status".into();
    i.unit_updates.push(UnitUpdate {
        id: "posts-api".into(),
        state: "running".into(),
        verification: vec![],
    });
    assert!(run(&i, &policy).await.is_err());
    i.unit_updates.clear();
    i.action = "defer".into();
    i.decisions.push(Decision {
        id: "delete".into(),
        reason: "Destructive cleanup".into(),
        recommended: "Keep data".into(),
        destructive: true,
    });
    run(&i, &policy).await.unwrap();
    i.action = "assume".into();
    i.decisions.clear();
    let r = run(&i, &policy).await.unwrap();
    assert_eq!(r["plan"]["decisions"][0]["state"], "unbuilt");
    i.action = "draft".into();
    assert!(run(&i, &policy).await.is_err());
    i.action = "status".into();
    i.plan_id = Some("stale".into());
    assert!(run(&i, &policy).await.is_err());
}
