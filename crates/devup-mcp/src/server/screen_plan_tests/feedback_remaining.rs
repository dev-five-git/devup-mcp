#[test]
fn r53_proposal_folds_inactive_routes_and_groups_existing_pages() {
    let screens = [
        sectioned("153:1", "Desktop", 1440.0, "Main"),
        sectioned("153:2", "Mobile", 360.0, "Main"),
        sectioned("153:3", "Desktop", 1440.0, "Company"),
        sectioned("153:4", "Mobile", 360.0, "Company"),
    ];
    let targets = BTreeMap::from([
        ("153:1".into(), "/".into()), ("153:2".into(), "/".into()),
        ("153:3".into(), "/company".into()), ("153:4".into(), "/company".into()),
    ]);
    let plan = draft_plan(&screens, &targets, &evidence(), json!({}), None).unwrap();
    let rows = proposal(array(&plan["questions"]));
    assert!(!rows.iter().any(|row| text(&row["id"]).starts_with("split-route:")), "{rows:?}");
    let collisions = rows.iter().filter(|row| text(&row["id"]).starts_with("collision:")).collect::<Vec<_>>();
    assert_eq!(collisions.len(), 1, "{rows:?}");
    assert_eq!(array(&collisions[0]["questionIds"]).len(), 2);
    let expanded = proposal_answers(array(&plan["questions"]), &answers(&[("collision:153:1", "Overwrite the existing page")]));
    assert!(matches!(expanded.get("collision:153:3"), Some(AnswerInput::One(label)) if label == "Overwrite the existing page"));
    assert!(expanded.contains_key("collision:153:3"));
}

#[test]
fn r53_api_scoped_dynamic_screen_recommends_real_data() {
    let fixture = Fixture::new();
    fixture.write(".devup/project-plan.json", &json!({"features":[{
        "id":"news", "scope":"API news records", "screens":["News detail"]
    }]}).to_string());
    let mut project = evidence();
    project.root = fixture.0.clone();
    let plan = draft_plan(&[screen("153:9", "News detail", 1440.0)],
        &BTreeMap::from([("153:9".into(), "/news/[id]".into())]), &project, json!({}), None).unwrap();
    assert_eq!(strip_recommended(first_label(find(&plan, "dynamic:153:9"))), "Wire to the API now");
}

#[test]
fn r56_metadata_uses_the_planned_app_not_the_first_root_route() {
    let fixture = Fixture::new();
    fixture.write("apps/admin/src/app/page.tsx", "export default function AdminPage() { return <main />; }");
    fixture.write("apps/front/src/app/page.tsx", "export const metadata = { title: 'Front' }; export default function HomePage() { return <main />; }");
    let mut project = evidence();
    project.root = fixture.0.clone();
    project.routes = vec![("/".into(), "apps/admin/src/app/page.tsx".into()), ("/".into(), "apps/front/src/app/page.tsx".into())];
    let plan = json!({"routes":[{"url":"/","implementation":"page","app":"apps/front","file":"apps/front/src/app/page.tsx"}]});
    let items = site_completeness(&plan, &project);
    let metadata = items.iter().find(|item| item["item"] == "metadata /").unwrap();
    assert_eq!(metadata["state"], "filled");
    assert_eq!(metadata["detail"], "apps/front/src/app/page.tsx");
}

#[tokio::test]
async fn r58_audit_plan_finds_the_confirmed_monorepo_plan_from_front() {
    let fixture = Fixture::new();
    let mut plan = confirmed_plan();
    plan["routes"][0]["app"] = json!("apps/front");
    write_plan_file(&fixture, &plan);
    fixture.write("apps/front/package.json", "{}");
    let mut request = input(&fixture, "auditPlan");
    request.project_root = Some(fixture.0.join("apps/front").to_string_lossy().into_owned());
    request.plan_id = Some(text(&plan["planId"]).to_owned());
    let result = audit_plan(&request, &fixture.policy(), &ArtifactStore::default()).await.unwrap();
    assert!(!array(&result["plans"]).is_empty());
    assert!(!fixture.0.join("apps/front/.devup/screen-plan.json").exists());
    assert!(text(&result["plans"][0]["planPath"]).starts_with(fixture.root()));
}

#[tokio::test]
async fn r59_audit_plan_refreshes_its_output_when_the_runner_needs_recreation() {
    let fixture = Fixture::new();
    write_plan_file(&fixture, &confirmed_plan());
    let request = input(&fixture, "auditPlan");
    let store = ArtifactStore::default();
    let first = audit_plan(&request, &fixture.policy(), &store).await.unwrap();
    let path = text(&first["plans"][0]["planPath"]);
    let report = Path::new(path).parent().unwrap().join("report/run.log");
    std::fs::create_dir_all(report.parent().unwrap()).unwrap();
    std::fs::write(&report, "keep this report").unwrap();
    std::fs::remove_dir_all(fixture.0.join(".devup/runtime-audit/runner")).unwrap();
    let refreshed = audit_plan(&request, &fixture.policy(), &store).await.unwrap();
    assert_eq!(refreshed["plans"][0]["planPath"], first["plans"][0]["planPath"]);
    assert_eq!(std::fs::read_to_string(report).unwrap(), "keep this report");
    assert!(fixture.0.join(".devup/runtime-audit/runner/run.mjs").is_file());
}
