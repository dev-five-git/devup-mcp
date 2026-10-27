#[test]
fn multi_width_export_redirects_single_width_code_before_collection() {
    let fixture = Fixture::new();
    write_plan_file(&fixture, &confirmed_plan());
    for output in ["tsx", "componentTsx", "pageScaffold"] {
        let response = responsive_required(fixture.root(), &["10:1".into()], &[output.into()]).unwrap().unwrap();
        assert_eq!(response["status"], "responsive_required");
        assert_eq!(response["written"], false);
        assert_eq!(response["nextAction"]["arguments"]["outputs"], json!(["responsiveTsx"]));
        assert_eq!(response["nextAction"]["arguments"]["frameIds"], json!(["10:2", "10:1"]));
    }
    assert!(responsive_required(fixture.root(), &["10:1".into(), "10:2".into()], &["responsiveTsx".into()]).unwrap().is_none());
    assert!(responsive_required(fixture.root(), &["10:3".into()], &["tsx".into()]).unwrap().is_none());
    assert!(responsive_required(fixture.root(), &["10:1".into()], &["outline".into()]).unwrap().is_none());
    assert!(responsive_required(fixture.root(), &["10:1".into()], &["responsiveTsx".into()]).unwrap().is_some());
    assert!(responsive_required(fixture.root(), &["10:1".into(), "10:2".into()], &["responsiveTsx".into(), "tsx".into()]).unwrap().is_some());
}

#[test]
fn responsive_scaffold_uses_the_merged_module_instead_of_the_first_frame() {
    let fixture = Fixture::new();
    let code = "export function Main() { return <Box w={['360px', null, '1440px']} />; }";
    let mut result = json!({"responsiveTsx": code, "frames": [{"nodeId": "10:1", "tsx": "desktop"}, {"nodeId": "10:2", "tsx": "mobile"}]}).as_object().unwrap().clone();
    let paths = BTreeMap::from([("pageScaffold".into(), fixture.root().into())]);
    super::super::projection::page_scaffold::attach(&mut result,
        &super::super::projection::page_scaffold::PageScaffoldOptions { route: "/".into(), write: false, overwrite: false },
        &paths, None, &BTreeMap::new(), &fixture.policy(), false).unwrap();
    assert!(array(&result["pageScaffold"]["files"]).iter().any(|file| file["content"] == code));
}

#[test]
fn status_reports_single_width_export_and_responsive_recovery() {
    let fixture = Fixture::new();
    super::super::export_record::write(&fixture.policy(), fixture.root(), "file", "10:1", &json!({"tsx": "<Box h=\"900px\" />"})).unwrap();
    let evidence = ProjectEvidence { root: fixture.0.clone(), ..evidence() };
    let report = status_report(&confirmed_plan(), &evidence, None);
    assert_eq!(report["rows"][0]["generatedExport"]["responsive"], false);
    assert!(report["rows"][0]["generatedExport"]["issue"].is_string());
    let item = array(&report["workQueue"]).iter().find(|item| item["url"] == "/").unwrap();
    assert_eq!(item["nextExport"]["arguments"]["frameIds"], json!(["10:2", "10:1"]));
}

#[test]
fn status_detects_collapsed_removed_and_added_styles_without_identity_attributes() {
    let fixture = Fixture::new();
    super::super::export_record::write(&fixture.policy(), fixture.root(), "file", "10:0", &json!({
        "responsiveTsx": "<Box w={['360px', null, '1440px']} gap=\"20px\" />",
        "responsiveEvidence": {"frames": [{"nodeId": "10:1"}, {"nodeId": "10:2"}]}
    })).unwrap();
    fixture.write("src/components/Main.tsx", "<Link href=\"/about\"><Box w=\"1440px\" h=\"640px\" onClick={go} aria-label=\"Next\" /></Link>");
    let evidence = ProjectEvidence { root: fixture.0.clone(), route_sources: BTreeMap::from([("/".into(), vec!["src/components/Main.tsx".into()])]), ..evidence() };
    let report = status_report(&confirmed_plan(), &evidence, None);
    let export = &report["rows"][0]["generatedExport"];
    assert_eq!(export["responsive"], true);
    let findings = array(&export["generatedDrift"]["findings"]);
    assert!(findings.iter().any(|finding| finding["kind"] == "responsive-array-collapsed" && finding["prop"] == "w"));
    assert!(findings.iter().any(|finding| finding["kind"] == "prop-removed" && finding["prop"] == "gap"));
    assert!(findings.iter().any(|finding| finding["kind"] == "style-added" && finding["prop"] == "h"));
    assert!(findings.iter().all(|finding| finding["file"] == "src/components/Main.tsx"));
}

#[test]
fn status_allows_link_and_event_wiring_without_layout_drift() {
    let fixture = Fixture::new();
    super::super::export_record::write(&fixture.policy(), fixture.root(), "file", "10:0", &json!({
        "responsiveTsx": "<Box w={['360px', null, '1440px']} />",
        "responsiveEvidence": {"frames": [{"nodeId": "10:1"}, {"nodeId": "10:2"}]}
    })).unwrap();
    fixture.write("src/components/Main.tsx", "<Link href=\"/about\"><Box w={['360px', null, '1440px']} onClick={go} aria-label=\"Next\" key=\"next\" /></Link>");
    let evidence = ProjectEvidence { root: fixture.0.clone(), route_sources: BTreeMap::from([("/".into(), vec!["src/components/Main.tsx".into()])]), ..evidence() };
    let report = status_report(&confirmed_plan(), &evidence, None);
    assert_eq!(report["rows"][0]["generatedExport"]["generatedDrift"]["findings"], json!([]));
}
