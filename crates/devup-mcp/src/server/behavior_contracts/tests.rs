use super::*;

fn snapshot(nodes: Value, roots: &[&str]) -> Snapshot {
    let nodes = nodes
        .as_array()
        .unwrap()
        .iter()
        .map(|value| serde_json::from_value::<RawNode>(value.clone()).unwrap())
        .map(|node| (node.id.clone(), node))
        .collect();
    Snapshot {
        file_key: "behavior-fixture".into(),
        version: None,
        roots: roots.iter().map(|id| (*id).into()).collect(),
        nodes,
        diagnostics: Vec::new(),
    }
}
fn kinds(result: &Value) -> Vec<&str> {
    result["behaviorPlan"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|candidate| candidate["kind"].as_str())
        .collect()
}
fn kind<'a>(result: &'a Value, name: &str) -> &'a Value {
    result["behaviorPlan"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["kind"] == name)
        .unwrap()
}

#[test]
fn presentation_decimals_keep_units_and_require_real_backend_sources() {
    let design = snapshot(
        json!([
            {"id":"root","type":"FRAME","fields":{"childrenIds":["metric","year","page"]}},
            {"id":"metric","type":"INSTANCE","fields":{"name":"StatItem","childrenIds":["value","caption"]}},
            {"id":"value","type":"TEXT","fields":{"name":"Statistic value","characters":"20.3%","fontSize":48}},
            {"id":"caption","type":"TEXT","fields":{"characters":"고령인구 비율"}},
            {"id":"year","type":"TEXT","fields":{"name":"Year","characters":"2026","fontSize":56}},
            {"id":"page","type":"TEXT","fields":{"name":"pagination","characters":"3","fontSize":48}}
        ]),
        &["root"],
    );
    let mut result = json!({"tsx":"export const C=()=> <Text>20.3%</Text>;"});
    attach(&mut result, &design);
    assert_eq!(kinds(&result), ["count-up"]);
    let statistic = kind(&result, "count-up");
    assert_eq!(statistic["evidence"]["target"], 20.3);
    assert_eq!(statistic["evidence"]["precision"], 1);
    assert_eq!(statistic["evidence"]["unit"], "%");
    assert_eq!(statistic["classification"], "implementation-recommendation");
    assert_eq!(statistic["figmaAnimationVerified"], false);
    let binding = &result["behaviorPlan"]["dataBindings"][0];
    assert_eq!(binding["backendMatched"], false);
    assert!(binding["apiAnchor"].is_null());
    assert!(
        binding["nextAction"]
            .as_str()
            .unwrap()
            .contains("devup_project_context")
    );
    assert!(
        binding["labelPolicy"]
            .as_str()
            .unwrap()
            .contains("fixed captions")
    );
    assert_eq!(result["tsx"], "export const C=()=> <Text>20.3%</Text>;");
    assert!(
        binding["orderingPolicy"]
            .as_str()
            .unwrap()
            .contains("3/2/1")
    );
}

#[test]
fn statistic_names_do_not_confuse_candidates_or_research_years_with_calendar_dates() {
    let design = snapshot(
        json!([
            {"id":"root","type":"FRAME","fields":{"name":"Statistics","childrenIds":["candidate","research","identifier","malformed"]}},
            {"id":"candidate","type":"TEXT","fields":{"name":"Candidate count","characters":"4","fontSize":48}},
            {"id":"research","type":"TEXT","fields":{"name":"Research year experience","characters":"30년","fontSize":48}},
            {"id":"identifier","type":"TEXT","fields":{"name":"Patent number","characters":"11045498","fontSize":48}},
            {"id":"malformed","type":"TEXT","fields":{"name":"Statistic count","characters":"1,2%","fontSize":48}}
        ]),
        &["root"],
    );
    let mut result = json!({"tsx":"<Text>Statistics</Text>"});
    attach(&mut result, &design);
    let mut ids = result["behaviorPlan"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|candidate| candidate["nodeId"].as_str())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(ids, ["candidate", "research"]);
}

#[test]
fn a_single_responsive_delivery_does_not_acquire_a_synthetic_frames_field() {
    let design = snapshot(json!([{"id":"root","type":"FRAME","fields":{}}]), &["root"]);
    let mut result = json!({"responsiveTsx":"<Box/>"});
    attach(&mut result, &design);
    assert!(result.get("frames").is_none());
    assert!(result["behaviorPlan"].is_object());
}

#[test]
fn numeric_labels_are_reviewed_and_identifiers_or_hidden_values_are_not_animated() {
    let design = snapshot(
        json!([
            {"id":"root","type":"FRAME","fields":{"childrenIds":["card","hidden","outside"]}},
            {"id":"card","type":"INSTANCE","fields":{"name":"Stat summary","childrenIds":["number","identifier"]}},
            {"id":"number","type":"TEXT","fields":{"name":"Label","characters":"8.52","fontSize":48}},
            {"id":"identifier","type":"TEXT","fields":{"name":"등록번호","characters":"001","fontSize":48}},
            {"id":"hidden","type":"FRAME","fields":{"visible":false,"childrenIds":["secret"]}},
            {"id":"secret","type":"TEXT","fields":{"name":"Statistic","characters":"99","fontSize":48}},
            {"id":"outside","type":"TEXT","fields":{"name":"Paragraph","characters":"One item"}}
        ]),
        &["root"],
    );
    let mut result = json!({"componentTsx":"const C=()=> <Box/>"});
    attach(&mut result, &design);
    assert_eq!(kinds(&result), ["count-up"]);
    let number = kind(&result, "count-up");
    assert_eq!(number["evidence"]["precision"], 2);
    assert_eq!(number["evidence"]["numericLabelRequiresReview"], true);
}

#[test]
fn single_line_overflow_suggests_marquee_without_treating_wrapped_text_as_motion() {
    let design = snapshot(
        json!([
            {"id":"root","type":"FRAME","fields":{"width":400,"clipsContent":true,"childrenIds":["strip","paragraph"]}},
            {"id":"strip","type":"TEXT","fields":{"width":1800,"maxLines":1,"characters":"Regenerating Joints ✦ Restoring Lives"}},
            {"id":"paragraph","type":"TEXT","fields":{"width":1800,"characters":"A heading\nA paragraph","maxLines":1}}
        ]),
        &["root"],
    );
    let mut result = json!({"tsx":"const C=()=> <Box/>"});
    attach(&mut result, &design);
    assert_eq!(kinds(&result), ["marquee"]);
    assert_eq!(
        kind(&result, "marquee")["evidence"]["clippingAncestorId"],
        "root"
    );
    assert!(
        kind(&result, "marquee")["implementation"]["pauseControls"]
            .as_str()
            .unwrap()
            .contains("only when required")
    );
}

#[test]
fn step_cycle_requires_multiple_steps_and_progress_evidence() {
    let mut design = snapshot(
        json!([
            {"id":"root","type":"FRAME","fields":{"name":"Technology","childrenIds":["one","two"],"reactions":[{"trigger":{"type":"AFTER_TIMEOUT"},"actions":[{"type":"NODE","destinationId":"next"}]}]}},
            {"id":"one","type":"INSTANCE","fields":{"name":"TechStep","variantProperties":{"Active":"true"},"childrenIds":["bar"]}},
            {"id":"two","type":"INSTANCE","fields":{"name":"TechStep","variantProperties":{"Active":"false"}}},
            {"id":"bar","type":"RECTANGLE","fields":{"name":"Progress"}}
        ]),
        &["root"],
    );
    let mut result = json!({"tsx":"const C=()=> <Box/>"});
    attach(&mut result, &design);
    let cycle = kind(&result, "progress-step-cycle");
    assert_eq!(
        cycle["evidence"]["capturedStepVariants"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        cycle["intentBasis"],
        "captured-prototype-plus-visual-inference"
    );
    assert_eq!(
        cycle["capturedPrototypeActions"][0]["trigger"]["type"],
        "AFTER_TIMEOUT"
    );
    assert_eq!(cycle["figmaAnimationVerified"], false);
    design
        .nodes
        .get_mut("bar")
        .unwrap()
        .fields
        .insert("name".into(), json!("Divider"));
    attach(&mut result, &design);
    assert!(!kinds(&result).contains(&"progress-step-cycle"));
}

#[test]
fn route_tabs_and_view_controls_have_one_state_and_persistent_layout_contracts() {
    let design = snapshot(
        json!([
            {"id":"root","type":"FRAME","fields":{"childrenIds":["tabs","toggle"]}},
            {"id":"tabs","type":"FRAME","fields":{"name":"Tabs menu","childrenIds":["a","b"]}},
            {"id":"a","type":"TEXT","fields":{"characters":"기업 뉴스"}},
            {"id":"b","type":"TEXT","fields":{"characters":"공지사항"}},
            {"id":"toggle","type":"FRAME","fields":{"name":"ViewToggle","childrenIds":["thumbnail","list"]}},
            {"id":"thumbnail","type":"TEXT","fields":{"characters":"썸네일형"}},
            {"id":"list","type":"TEXT","fields":{"characters":"목록형"}}
        ]),
        &["root"],
    );
    let mut result = json!({"tsx":"const C=()=> <Box/>"});
    attach(&mut result, &design);
    assert!(
        kind(&result, "sliding-tab-indicator")["implementation"]["ownership"]
            .as_str()
            .unwrap()
            .contains("shared server layout")
    );
    assert!(
        kind(&result, "view-mode-state")["implementation"]["stateSource"]
            .as_str()
            .unwrap()
            .contains("same normalized URL")
    );
    assert!(
        kind(&result, "view-mode-state")["implementation"]["synchronize"]
            .as_array()
            .unwrap()
            .contains(&json!("icon"))
    );
}

#[test]
fn generic_step_layout_with_partial_track_is_a_review_candidate_not_captured_motion() {
    let design = snapshot(
        json!([
            {"id":"root","type":"FRAME","fields":{"name":"Frame 45","childrenIds":["one","two"]}},
            {"id":"one","type":"INSTANCE","fields":{"name":"TechStep","childrenIds":["track"]}},
            {"id":"two","type":"INSTANCE","fields":{"name":"TechStep"}},
            {"id":"track","type":"FRAME","fields":{"name":"Rectangle","width":330,"height":3,"childrenIds":["fill"]}},
            {"id":"fill","type":"RECTANGLE","fields":{"name":"Rectangle","width":110,"height":3}}
        ]),
        &["root"],
    );
    let mut result = json!({"tsx":"const C=()=> <Box/>"});
    attach(&mut result, &design);
    let cycle = kind(&result, "progress-step-cycle");
    assert_eq!(cycle["intentBasis"], "visual-inference");
    assert_eq!(cycle["evidence"]["progressNodeIds"], json!(["track"]));
    assert_eq!(cycle["figmaAnimationVerified"], false);
}

#[test]
fn page_footer_and_reveal_recommendations_preserve_readable_server_content() {
    let design = snapshot(
        json!([
            {"id":"root","type":"FRAME","fields":{"name":"Home","childrenIds":["header","hero","content","form","footer"]}},
            {"id":"header","type":"FRAME","fields":{"name":"Header","height":200}},
            {"id":"hero","type":"FRAME","fields":{"name":"Hero","height":600}},
            {"id":"content","type":"FRAME","fields":{"name":"Research section","height":500}},
            {"id":"form","type":"FRAME","fields":{"name":"Contact form","height":500}},
            {"id":"footer","type":"FRAME","fields":{"name":"Footer","height":100}}
        ]),
        &["root"],
    );
    let mut result = json!({"tsx":"const C=()=> <Box/>"});
    attach(&mut result, &design);
    assert_eq!(kinds(&result), ["section-entrance"]);
    assert_eq!(kind(&result, "section-entrance")["nodeId"], "content");
    assert!(
        kind(&result, "section-entrance")["implementation"]["serverMarkup"]
            .as_str()
            .unwrap()
            .contains("never prehide SSR")
    );
    assert_eq!(
        result["behaviorPlan"]["pageContracts"][0]["implementation"]["minHeight"],
        "100dvh"
    );
    assert_eq!(result["behaviorPlan"]["implemented"], false);
    assert_eq!(result["behaviorPlan"]["runtimeVerified"], false);
}

#[test]
fn paint_overrides_request_complete_state_review_without_inventing_tokens() {
    let design = snapshot(
        json!([
            {"id":"root","type":"INSTANCE","fields":{"name":"Secondary CTA"}}
        ]),
        &["root"],
    );
    let mut result = json!({"tsx":"const C=()=> <Box/>","instanceOverrides":[{"instanceNodeId":"root","mainComponentId":"primary","overriddenFields":["fills"]}]});
    attach(&mut result, &design);
    let review = kind(&result, "palette-state-review");
    assert!(
        review["implementation"]["review"]
            .as_array()
            .unwrap()
            .contains(&json!("active"))
    );
    assert!(
        review["implementation"]["derivation"]
            .as_str()
            .unwrap()
            .contains("do not invent")
    );
    assert_eq!(result["tsx"], "const C=()=> <Box/>");
}

#[test]
fn each_frame_gets_a_local_plan_and_other_snapshot_roots_do_not_leak() {
    let design = snapshot(
        json!([
            {"id":"first","type":"FRAME","fields":{"name":"Stats","childrenIds":["a"]}},
            {"id":"a","type":"TEXT","fields":{"name":"Metric","characters":"30"}},
            {"id":"second","type":"FRAME","fields":{"name":"Stats","childrenIds":["b"]}},
            {"id":"b","type":"TEXT","fields":{"name":"Metric","characters":"4"}},
            {"id":"unselected","type":"TEXT","fields":{"name":"Metric","characters":"900"}}
        ]),
        &["first", "second", "unselected"],
    );
    let mut result = json!({"frames":[{"nodeId":"first","tsx":"const A=()=> <Box/>"},{"nodeId":"second","tsx":"const B=()=> <Box/>"}]});
    attach(&mut result, &design);
    assert_eq!(result["behaviorPlan"]["candidateCount"], 2);
    assert_eq!(result["frames"][0]["behaviorPlan"]["candidateCount"], 1);
    assert_eq!(result["frames"][1]["behaviorPlan"]["candidateCount"], 1);
    assert_eq!(
        result["frames"][0]["behaviorPlan"]["candidates"][0]["nodeId"],
        "a"
    );
    assert_eq!(
        result["frames"][1]["behaviorPlan"]["candidates"][0]["nodeId"],
        "b"
    );
}

#[test]
fn evidence_only_responses_do_not_claim_an_application_plan() {
    let design = snapshot(json!([]), &[]);
    let mut result = json!({"rawSnapshot":{}});
    attach(&mut result, &design);
    assert!(result.get("behaviorPlan").is_none());
}

#[test]
fn bounded_candidates_report_incomplete_coverage_without_changing_source() {
    let mut nodes = Vec::new();
    let children = (0..(MAX_CANDIDATES + 1))
        .map(|index| format!("n{index}"))
        .collect::<Vec<_>>();
    nodes.push(
        json!({"id":"root","type":"FRAME","fields":{"name":"Statistics","childrenIds":children}}),
    );
    for id in children {
        nodes.push(json!({"id":id,"type":"TEXT","fields":{"name":"Metric","characters":"30"}}));
    }
    let design = snapshot(json!(nodes), &["root"]);
    let mut result = json!({"tsx":"const C=()=> <Box/>"});
    attach(&mut result, &design);
    assert_eq!(result["behaviorPlan"]["candidateCount"], MAX_CANDIDATES);
    assert_eq!(result["behaviorPlan"]["evidenceLimits"]["truncated"], true);
    assert_eq!(result["tsx"], "const C=()=> <Box/>");
}
