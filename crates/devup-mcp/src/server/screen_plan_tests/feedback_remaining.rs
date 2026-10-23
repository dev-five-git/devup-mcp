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
