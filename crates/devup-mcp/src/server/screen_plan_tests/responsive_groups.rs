fn ambiguous_contact_plan() -> Value {
    let mut plan = confirmed_plan();
    plan["routes"][0]["nodeIds"] = json!(["d:1", "m:1", "d:2", "m:2", "d:3", "m:3", "d:4", "m:4"]);
    plan["routes"][0]["drawnWidths"] = json!([1440, 360, 1440, 360, 1440, 360, 1440, 360]);
    plan
}

fn contact_groups() -> Vec<ResponsiveGroupInput> {
    ["normal", "dropdown", "error", "success"]
        .iter()
        .enumerate()
        .map(|(index, name)| ResponsiveGroupInput {
            url: "/".into(),
            id: (*name).into(),
            kind: if index == 0 { "baseline" } else { "state" }.into(),
            frame_ids: vec![format!("d:{}", index + 1), format!("m:{}", index + 1)],
            reason: format!("Captured labels identify {name} on both frames."),
        })
        .collect()
}

#[test]
fn duplicate_width_recovery_never_asks_for_an_impossible_responsive_export() {
    let plan = ambiguous_contact_plan();
    let action = responsive_export_action(&plan, &plan["routes"][0], "root");
    assert_eq!(action["status"], "responsive_grouping_required");
    assert_eq!(action["arguments"]["outputs"], json!(["outline"]));
    assert!(array(&action["arguments"]["frameIds"]).len() <= 3);
    assert_eq!(array(&action["remainingReads"]).len(), 2);
    assert_eq!(array(&action["candidates"]).len(), 8);
    assert_eq!(action["resolution"]["field"], "responsiveGroups");
    let fixture = Fixture::new();
    write_plan_file(&fixture, &plan);
    let response = responsive_required(
        fixture.root(),
        &["d:1".into(), "m:1".into()],
        &["responsiveTsx".into()],
    )
    .unwrap()
    .unwrap();
    assert_eq!(response["status"], "responsive_grouping_required");
}

#[test]
fn distinct_widths_in_one_responsive_slot_are_not_an_executable_group() {
    let mut route = json!({"url":"/","nodeIds":["mobile:1","mobile:2"],"drawnWidths":[360,390],"implementation":"page"});
    assert!(
        responsive_groups(&route)
            .unwrap_err()
            .contains("same responsive slot")
    );
    route["responsiveGroups"] = json!([{"id":"normal","kind":"baseline","frameIds":["mobile:1","mobile:2"],"reason":"Captured labels."}]);
    assert!(
        responsive_groups(&route)
            .unwrap_err()
            .contains("repeats responsive slot")
    );
}

#[test]
fn assume_records_all_state_pairings_and_exports_each_group_independently() {
    let fixture = Fixture::new();
    write_plan_file(&fixture, &ambiguous_contact_plan());
    let mut request = input(&fixture, "assume");
    request.responsive_groups = contact_groups();
    assume(&request, &fixture.policy()).unwrap();
    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let groups = responsive_groups(&plan["routes"][0]).unwrap();
    assert_eq!(groups.len(), 4);
    assert_eq!(groups[0]["frameIds"], json!(["m:1", "d:1"]));
    assert_eq!(
        array(&plan["assumptions"]).last().unwrap()["decision"],
        "recorded explicit baseline/state responsive groups"
    );
    for index in 1..=4 {
        assert!(
            responsive_required(
                fixture.root(),
                &[format!("d:{index}"), format!("m:{index}")],
                &["responsiveTsx".into()]
            )
            .unwrap()
            .is_none()
        );
    }
    let mixed = responsive_required(
        fixture.root(),
        &["d:1".into(), "m:2".into()],
        &["responsiveTsx".into()],
    )
    .unwrap()
    .unwrap();
    assert_eq!(mixed["status"], "responsive_required");
    assert_eq!(array(&mixed["groupActions"]).len(), 4);
    let targets = audit_routes(&plan);
    assert_eq!(array(&targets[0]["nodeIds"]), &[json!("m:1"), json!("d:1")]);
    assert_eq!(
        targets
            .iter()
            .filter(|target| target["state"].is_object())
            .count(),
        3
    );
    assert!(
        targets
            .iter()
            .filter(|target| target["url"] == "/")
            .all(|target| array(&target["nodeIds"]).len() == 2)
    );
}

#[test]
fn responsive_group_validation_refuses_omissions_duplicate_widths_and_unsupported_frames() {
    let baseline = ambiguous_contact_plan();
    let mut invalids = Vec::new();
    let mut omitted = contact_groups();
    omitted.pop();
    invalids.push(omitted);
    let mut repeated = contact_groups();
    repeated[0].frame_ids = vec!["d:1".into(), "d:2".into()];
    invalids.push(repeated);
    let mut unknown = contact_groups();
    unknown[0].frame_ids[0] = "outside".into();
    invalids.push(unknown);
    let mut baselines = contact_groups();
    baselines[1].kind = "baseline".into();
    invalids.push(baselines);
    for groups in invalids {
        let mut plan = baseline.clone();
        assert!(apply_responsive_groups(&mut plan, &groups).is_err());
    }
}

#[test]
fn group_status_requires_every_baseline_and_state_export_record() {
    let fixture = Fixture::new();
    let mut plan = ambiguous_contact_plan();
    apply_responsive_groups(&mut plan, &contact_groups()).unwrap();
    let evidence = ProjectEvidence {
        root: fixture.0.clone(),
        ..evidence()
    };
    for index in 1..=4 {
        super::super::export_record::write(&fixture.policy(),fixture.root(),"file",&format!("group:{index}"),&json!({
            "responsiveTsx":"<Box />", "responsiveEvidence":{"frames":[{"nodeId":format!("d:{index}")},{"nodeId":format!("m:{index}")}]}
        })).unwrap();
        let records = super::super::export_record::records(&fixture.0);
        let status = responsive_status(&plan, &plan["routes"][0], &evidence, &records);
        assert_eq!(status["responsive"], index == 4);
        assert_eq!(
            array(&status["groups"])
                .iter()
                .filter(|group| group["responsive"] == true)
                .count(),
            index
        );
    }
}

#[test]
fn page_source_recovery_anchors_the_requested_frame_and_preserves_file_identity() {
    for source in [
        "figma-bridge://current?node-id=0%3A1&mode=dev",
        "https://www.figma.com/branch/ActualFile/ActualBranch/Design?node-id=0-1&mode=dev",
    ] {
        let mut plan = confirmed_plan();
        plan["source"]["url"] = json!(source);
        let action = responsive_export_action(&plan, &plan["routes"][0], "root");
        let link = text(&action["arguments"]["url"]);
        let parsed = FigmaTarget::parse(link).unwrap();
        assert_eq!(parsed.node_id.as_deref(), Some("10:2"));
        let original = FigmaTarget::parse(source).unwrap();
        assert_eq!(parsed.file_key, original.file_key);
        assert_eq!(parsed.branch_key, original.branch_key);
        let url = url::Url::parse(link).unwrap();
        assert!(
            url.query_pairs()
                .any(|(key, value)| key == "mode" && value == "dev")
        );
        assert_eq!(
            url.query_pairs()
                .filter(|(key, _)| key == "node-id")
                .count(),
            1
        );
        plan["routes"][0]["states"] = json!([{"nodeId":"10:7","name":"known state","width":390}]);
        let states = state_exports(&plan, &plan["routes"][0]);
        assert_eq!(
            FigmaTarget::parse(text(&states[0]["exportCall"]["arguments"]["url"]))
                .unwrap()
                .node_id
                .as_deref(),
            Some("10:7")
        );
        plan["routes"][0]["nodeIds"] = json!(["10:1", "10:2", "10:5", "10:6"]);
        plan["routes"][0]["drawnWidths"] = json!([1440, 390, 1440, 390]);
        let recovery = responsive_export_action(&plan, &plan["routes"][0], "root");
        for call in std::iter::once(&recovery).chain(array(&recovery["remainingReads"])) {
            let parsed = FigmaTarget::parse(text(&call["arguments"]["url"])).unwrap();
            assert_eq!(
                parsed.node_id.as_deref(),
                array(&call["arguments"]["frameIds"])
                    .first()
                    .and_then(Value::as_str)
            );
        }
    }
}

#[test]
fn projection_confirmation_finds_explicit_group_subsets_from_app_roots() {
    let fixture = Fixture::new();
    fixture.write("apps/front/package.json", "{}");
    let mut plan = ambiguous_contact_plan();
    apply_responsive_groups(&mut plan, &contact_groups()).unwrap();
    write_plan_file(&fixture, &plan);
    let app = fixture.0.join("apps/front");
    let app = app.to_str().unwrap();
    assert!(
        super::super::projection::responsive_groups::confirmed(
            Some(app),
            &["d:1".into(), "m:1".into()]
        )
        .unwrap()
    );
    assert!(
        super::super::projection::responsive_groups::confirmed(
            Some(app),
            &["m:2".into(), "d:2".into()]
        )
        .unwrap()
    );
    assert!(
        !super::super::projection::responsive_groups::confirmed(
            Some(app),
            &["d:1".into(), "m:2".into()]
        )
        .unwrap()
    );
    assert!(
        !super::super::projection::responsive_groups::confirmed(Some(app), &["d:1".into()])
            .unwrap()
    );
}
