#[test]
fn r34_the_gate_finds_a_plan_confirmed_at_the_monorepo_root_from_an_app_directory() {
    let fixture = Fixture::new();
    write_plan_file(&fixture, &confirmed_plan());
    fixture.write("apps/front/devup.json", "{}");
    let app = fixture.0.join("apps").join("front");
    let app = app.to_str().unwrap();
    let mut scaffold = ExportRequest {
        project_root: app,
        url: None,
        frames: vec!["10:2".to_owned()],
        all_screens: false,
        scaffold_write: true,
        scaffold_route: Some(""),
        output_paths: Vec::new(),
    };
    let Gate::Route(route) = export_gate(&scaffold).unwrap() else {
        panic!("the root plan covers 10:2 and must be found from apps/front");
    };
    assert_eq!(route, "/");
    scaffold.frames = vec!["99:1".to_owned()];
    scaffold.scaffold_route = Some("/x");
    let Gate::Blocked(response) = export_gate(&scaffold).unwrap() else {
        panic!("an uncovered frame is judged against the found plan, not 'no plan'");
    };
    assert_eq!(
        response["planRequired"]["details"]["uncoveredFrames"],
        json!(["99:1"])
    );
}

#[test]
fn an_app_directory_records_assumptions_on_the_plan_its_repository_root_holds() {
    let fixture = Fixture::new();
    let mut plan = confirmed_plan();
    plan["routes"][0]["app"] = json!("apps/front");
    write_plan_file(&fixture, &plan);
    fixture.write("apps/front/devup.json", "{}");
    let app = fixture.0.join("apps").join("front");
    let assume_input = ScreenPlanInput {
        action: "assume".into(),
        project_root: Some(app.to_str().unwrap().into()),
        assumptions: vec![AssumptionInput {
            about: "scope".into(),
            decision: "recorded from the app directory".into(),
            reason: None,
        }],
        ..ScreenPlanInput::default()
    };
    let recorded = assume(&assume_input, &fixture.policy()).unwrap();
    assert_eq!(recorded["assumptions"][0]["about"], "scope");
    let saved = read_plan(fixture.root()).unwrap().unwrap();
    assert_eq!(saved["assumptions"][0]["decision"], "recorded from the app directory");
}

#[test]
fn status_does_not_take_another_apps_page_for_this_route() {
    let mut plan = confirmed_plan();
    plan["routes"][0]["app"] = json!("apps/front");
    plan["routes"][0]["file"] = json!(null);
    let mut evidence = evidence();
    evidence.routes = vec![("/".to_owned(), "apps/admin/src/app/page.tsx".to_owned())];
    let report = status_report(&plan, &evidence, None);
    assert_ne!(report["rows"][0]["file"], "apps/admin/src/app/page.tsx", "{report}");
}

#[test]
fn r34_ancestor_plan_must_match_the_requested_app() {
    let fixture = Fixture::new();
    let mut plan = confirmed_plan();
    plan["routes"][0]["app"] = json!("apps/front");
    write_plan_file(&fixture, &plan);
    fixture.write("apps/front/devup.json", "{}");
    fixture.write("apps/admin/devup.json", "{}");
    for (app, covered) in [("front", true), ("admin", false)] {
        let root = fixture.0.join("apps").join(app);
        let request = ExportRequest {
            project_root: root.to_str().unwrap(), url: None,
            frames: vec!["10:2".into()], all_screens: false,
            scaffold_write: true, scaffold_route: Some(""), output_paths: Vec::new(),
        };
        assert_eq!(matches!(export_gate(&request).unwrap(), Gate::Route(_)), covered);
    }
}
