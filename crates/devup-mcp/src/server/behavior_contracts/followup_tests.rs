use super::*;

fn fixture(count: usize) -> Snapshot {
    let mut nodes = vec![
        json!({"id":"root","type":"FRAME","fields":{"name":"Statistics","childrenIds":(0..count).map(|i|format!("n{i}")).collect::<Vec<_>>()}}),
    ];
    nodes.extend((0..count).map(|i| json!({"id":format!("n{i}"),"type":"TEXT","fields":{"name":"Metric","characters":"2000건"}})));
    super::tests::snapshot(json!(nodes), &["root"])
}

#[test]
fn range_2000_statistic_and_negative_decimal_are_not_calendar_dates() {
    let mut snapshot = fixture(2);
    snapshot
        .nodes
        .get_mut("n1")
        .unwrap()
        .fields
        .insert("characters".into(), json!("-8.52%"));
    let mut result = json!({"tsx":"<Box/>","artifactId":"one"});
    attach(&mut result, &snapshot);
    let values = result["behaviorPlan"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["evidence"]["target"].as_f64().unwrap())
        .collect::<Vec<_>>();
    assert!(values.contains(&2000.0));
    assert!(values.contains(&-8.52));
    for text in ["2026년", "2026-11-04", "1,2건", "001건"] {
        snapshot
            .nodes
            .get_mut("n0")
            .unwrap()
            .fields
            .insert("characters".into(), json!(text));
        let mut result = json!({"tsx":"<Box/>"});
        attach(&mut result, &snapshot);
        assert!(
            !result["behaviorPlan"]["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["nodeId"] == "n0"),
            "{text}"
        );
    }
}

#[test]
fn money_prefix_precision_and_korean_section_exclusions_are_preserved() {
    let design = super::tests::snapshot(
        json!([
            {"id":"root","type":"FRAME","fields":{"name":"Home","childrenIds":["hero","menu","form","metric"]}},
            {"id":"hero","type":"FRAME","fields":{"name":"Hero","height":500}},
            {"id":"menu","type":"FRAME","fields":{"name":"메뉴","height":500}},
            {"id":"form","type":"FRAME","fields":{"name":"문의 폼","height":500}},
            {"id":"metric","type":"TEXT","fields":{"name":"Metric","characters":"$2,000.50"}}
        ]),
        &["root"],
    );
    let mut result = json!({"tsx":"<Box/>"});
    attach(&mut result, &design);
    assert_eq!(result["behaviorPlan"]["candidateCount"], 1);
    let metric = &result["behaviorPlan"]["candidates"][0];
    assert_eq!(metric["evidence"]["target"], 2000.5);
    assert_eq!(metric["evidence"]["precision"], 2);
    assert_eq!(metric["evidence"]["prefix"], "$");
}

#[test]
fn continuation_keeps_all_candidates_bindings_scope_and_deterministic_replays() {
    let snapshot = fixture(MAX_CANDIDATES * 3 + 7);
    let mut first = json!({"tsx":"<Box/>","artifactId":"same"});
    attach(&mut first, &snapshot);
    let mut page = first;
    let mut candidates = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    let mut pages = 0;
    loop {
        pages += 1;
        for candidate in page["behaviorPlan"]["candidates"].as_array().unwrap() {
            assert!(candidates.insert(candidate["id"].as_str().unwrap().to_owned()));
        }
        for binding in page["behaviorPlan"]["dataBindings"].as_array().unwrap() {
            assert!(bindings.insert(binding["nodeId"].as_str().unwrap().to_owned()));
        }
        assert_eq!(page["behaviorPlan"]["scopeNodeIds"], json!(["root"]));
        if page["behaviorPlan"]["pagination"]["complete"] == true {
            let (inventory, keys) = verified_coverage_inventory(&page["behaviorPlan"]).unwrap();
            assert_eq!(inventory.len(), MAX_CANDIDATES * 3 + 7);
            assert_eq!(keys.len(), inventory.len());
            let mut tampered = page["behaviorPlan"].clone();
            tampered["scopeNodeIds"] = json!(["other"]);
            assert!(verified_coverage_inventory(&tampered).is_none());
            break;
        }
        let action = &page["behaviorPlan"]["pagination"]["nextAction"];
        assert_eq!(action["tool"], "devup_figma_export");
        let token = action["arguments"]["behaviorCursor"].as_str().unwrap();
        let next = resume("same", &snapshot, token).unwrap();
        assert_eq!(next, resume("same", &snapshot, token).unwrap());
        assert!(resume("other", &snapshot, token).is_err());
        let mut changed = snapshot.clone();
        changed.version = Some("changed".into());
        assert!(resume("same", &changed, token).is_err());
        page = next;
        assert!(pages < 10);
    }
    assert_eq!(candidates.len(), MAX_CANDIDATES * 3 + 7);
    assert_eq!(bindings.len(), candidates.len());
    assert!(pages > 1);
}

#[test]
fn node_scan_limit_is_resumable_even_without_any_candidate() {
    let mut snapshot = fixture(MAX_NODES + 12);
    for (id, node) in &mut snapshot.nodes {
        if id != "root" {
            node.fields
                .insert("characters".into(), json!("ordinary copy"));
        }
    }
    let mut result = json!({"tsx":"<Box/>","artifactId":"scan"});
    attach(&mut result, &snapshot);
    assert_eq!(
        result["behaviorPlan"]["evidenceLimits"]["nodesScanned"],
        MAX_NODES
    );
    let token = result["behaviorPlan"]["pagination"]["nextAction"]["arguments"]["behaviorCursor"]
        .as_str()
        .unwrap();
    let next = resume("scan", &snapshot, token).unwrap();
    assert_eq!(next["behaviorPlan"]["pagination"]["complete"], true);
    assert_eq!(
        next["behaviorPlan"]["pagination"]["accumulatedNodes"],
        MAX_NODES + 13
    );
}

#[test]
fn generated_motion_files_are_real_code_resource_outputs_with_explicit_bindings() {
    let mut result = json!({"tsx":"<Text>unchanged</Text>","artifactId":"files"});
    attach(&mut result, &fixture(1));
    assert!(
        result["motionControllerTsx"]
            .as_str()
            .unwrap()
            .contains("'use client'")
    );
    let runtime = result["motionRuntime"].as_str().unwrap();
    for name in [
        "function countUp",
        "function marquee",
        "function stepCycle",
        "function slidingHighlight",
        "export function mountMotion",
    ] {
        assert!(runtime.contains(name));
    }
    assert!(!runtime.contains("data-figma"));
    assert!(!runtime.contains("@devup-"));
    assert_eq!(result["tsx"], "<Text>unchanged</Text>");
    assert!(result["motionBindings"]["bindings"][0]["binding"]["target"].is_null());
    let outputs =
        super::super::projection::projected_outputs_from_result(result.as_object().unwrap())
            .unwrap();
    assert!(outputs.len() >= 4);
    let controller = outputs
        .iter()
        .find(|output| output.name == "MotionController.tsx")
        .unwrap();
    assert!(controller.bytes.starts_with(b"'use client'"));
    assert!(
        outputs
            .iter()
            .any(|output| output.name == "motion-runtime.ts")
    );
    assert!(
        outputs
            .iter()
            .any(|output| output.name == "motion-bindings.json")
    );
}

#[test]
fn section_candidates_are_budgeted_before_their_overflow_can_bloat_a_cursor() {
    let count = MAX_CANDIDATES * 3 + 5;
    let mut snapshot = fixture(count);
    for (id, node) in &mut snapshot.nodes {
        if id != "root" {
            node.node_type = "FRAME".into();
            node.fields.insert("height".into(), json!(200));
        }
    }
    let mut result = json!({"tsx":"<Box/>","artifactId":"sections"});
    attach(&mut result, &snapshot);
    let mut found = BTreeSet::new();
    loop {
        for candidate in result["behaviorPlan"]["candidates"].as_array().unwrap() {
            assert!(found.insert(candidate["id"].as_str().unwrap().to_owned()));
        }
        if result["behaviorPlan"]["pagination"]["complete"] == true {
            break;
        }
        let token =
            result["behaviorPlan"]["pagination"]["nextAction"]["arguments"]["behaviorCursor"]
                .as_str()
                .unwrap();
        assert!(token.len() < 128 * 1024, "{}", token.len());
        result = resume("sections", &snapshot, token).unwrap();
    }
    assert_eq!(found.len(), count - 1);
    assert_eq!(
        verified_coverage_inventory(&result["behaviorPlan"])
            .unwrap()
            .0
            .len(),
        found.len()
    );
}
