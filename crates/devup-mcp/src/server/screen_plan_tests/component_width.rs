fn width_fixture() -> devup_mcp_figma::Snapshot {
    let mut nodes = serde_json::Map::new();
    for (screen, width, master) in [
        ("mobile-a", 360, "mobile"),
        ("mobile-b", 360, "mobile"),
        ("mobile-c", 360, "mobile"),
        ("home", 360, "desktop"),
        ("wide", 1440, "desktop"),
    ] {
        let footer = format!("{screen}-footer");
        nodes.insert(screen.into(), json!({"id":screen,"type":"FRAME","fields":{
            "name":screen,"width":width,"childrenIds":[footer]}}));
        nodes.insert(footer.clone(), json!({"id":footer,"type":"INSTANCE","fields":{
            "name":"Footer","mainComponentName":"Footer","mainComponentId":master}}));
    }
    serde_json::from_value(json!({"fileKey":"fixture","roots":["mobile-a","mobile-b","mobile-c","home","wide"],"nodes":nodes,"diagnostics":[]})).unwrap()
}

#[test]
fn width_mismatch_selects_majority_sibling_and_keeps_singletons() {
    let snapshot = width_fixture();
    let mismatches = component_width_mismatches(&snapshot, &snapshot.roots);
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0]["nodeId"], "home-footer");
    assert_eq!(mismatches[0]["usedMaster"], "desktop");
    assert_eq!(mismatches[0]["expectedMaster"], "mobile");
    assert_eq!(mismatches[0]["expectedCount"], 3);
    assert_eq!(mismatches[0]["totalCount"], 4);
    assert_eq!(mismatches[0]["correctNodeId"], "mobile-a-footer");
}

#[test]
fn width_mismatch_does_not_resolve_ties_or_unrelated_families() {
    let mut snapshot = width_fixture();
    snapshot.nodes.remove("mobile-c");
    snapshot.nodes.remove("mobile-c-footer");
    snapshot.nodes.get_mut("mobile-b-footer").unwrap().fields.insert("mainComponentId".into(), json!("desktop"));
    assert!(component_width_mismatches(&snapshot, &snapshot.roots).is_empty());
    snapshot.nodes.get_mut("mobile-b-footer").unwrap().fields.insert("mainComponentName".into(), json!("OtherFooter"));
    snapshot.nodes.get_mut("home-footer").unwrap().fields.insert("mainComponentName".into(), json!("OtherFooter"));
    assert!(component_width_mismatches(&snapshot, &snapshot.roots).is_empty());
}

#[test]
fn width_mismatch_records_idempotent_assumption_and_status() {
    let snapshot = width_fixture();
    let mut plan = json!({"assumptions":[],"routes":[],"screens":[]});
    let mismatches = component_width_mismatches(&snapshot, &snapshot.roots);
    apply_component_width_mismatches(&mut plan, &mismatches);
    apply_component_width_mismatches(&mut plan, &mismatches);
    assert_eq!(plan["assumptions"].as_array().unwrap().len(), 1);
    assert_eq!(plan["assumptions"][0]["about"], "home-footer");
    assert_eq!(plan["assumptions"][0]["decision"], "use mobile (mobile-a-footer)");
    assert_eq!(plan["assumptions"][0]["evidence"]["render"], "unavailable");
    let report = status_report(&plan, &evidence(), None);
    assert_eq!(report["componentWidthMismatches"][0]["code"], "DEVUP_DESIGN_COMPONENT_WIDTH_MISMATCH");
}

#[test]
fn width_mismatch_draft_persists_source_decision() {
    let snapshot = width_fixture();
    let mismatches = component_width_mismatches(&snapshot,&snapshot.roots);
    let plan = draft_plan(&[screen("home","Home",360.0)],&BTreeMap::from([("home".into(),"/".into())]),&evidence(),json!({"componentWidthMismatches":mismatches}),None).unwrap();
    assert_eq!(plan["componentWidthMismatches"][0]["expectedMaster"],"mobile");
    assert_eq!(plan["assumptions"][0]["decision"],"use mobile (mobile-a-footer)");
}

#[test]
fn width_mismatch_export_reads_persisted_assumptions() {
    let fixture = Fixture::new();
    let snapshot = width_fixture();
    let mut plan = json!({"kind":PLAN_KIND,"schemaVersion":1,"screens":[],"assumptions":[]});
    apply_component_width_mismatches(&mut plan,&component_width_mismatches(&snapshot,&snapshot.roots));
    fixture.write(PLAN_FILE,&plan.to_string());
    let mut exported = json!({"source":{"nodeId":"home"}});
    attach_component_width_plan(&mut exported,fixture.root()).unwrap();
    assert_eq!(exported["componentWidthMismatches"][0]["nodeId"],"home-footer");
    assert_eq!(exported["componentWidthAssumptions"][0]["decision"],"use mobile (mobile-a-footer)");
}
