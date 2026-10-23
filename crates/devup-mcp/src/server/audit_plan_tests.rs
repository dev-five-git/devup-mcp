use super::*;

#[test]
fn absent_source_anchors_leave_design_checks_not_run() {
    let project = Project::new("missing-anchors");
    project.write("src/app/page.tsx", "export default function Page() { return <main /> }");
    let mut built = url_plan(&home(), "http://localhost:3000", &sources(), &VisualThresholds::default()).unwrap();
    gate_source_anchors(&mut built, Path::new(project.root()));
    let checks = &built.plan.screens[0].checks;
    assert!(checks.iter().any(|check| check.kind == "bounds" && check.unavailable_reason.as_deref().is_some_and(|reason| reason.contains("anchors absent"))));
    assert!(checks.iter().filter(|check| check.unavailable_reason.is_some()).all(|check| check.subject.is_none() && check.container.is_none()));
    assert!(checks.iter().any(|check| check.kind == "page-health" && check.unavailable_reason.is_none()));
}

#[test]
fn partial_source_anchors_do_not_enable_unmatched_nodes() {
    let project = Project::new("partial-anchors");
    project.write("src/app/page.tsx", "<main data-figma-node-id='10:1'><section data-figma-node-id='10:12' /></main>");
    let mut built = url_plan(&home(), "http://localhost:3000", &sources(), &VisualThresholds::default()).unwrap();
    gate_source_anchors(&mut built, Path::new(project.root()));
    assert!(built.plan.screens[0].checks.iter().any(|check| check.kind == "bounds" && check.node_id.as_deref() == Some("10:12") && check.unavailable_reason.is_none()));
    assert!(built.plan.screens[0].checks.iter().filter(|check| check.node_id.as_deref() == Some("10:131")).all(|check| check.unavailable_reason.is_some()));
}

fn payload(target: &str, nodes: Value, assets: Value, reference: bool) -> CollectedPayload {
    let mut value = json!({
        "target": {"fileKey": "audit", "nodeId": target},
        "scope": "node", "metadata": {},
        "snapshot": {"fileKey": "audit", "version": "1", "roots": [target], "nodes": nodes, "diagnostics": []},
        "variables": null, "styles": null, "completeness": "resolved-values-only", "sourceVersion": "1",
        "assets": assets,
    });
    if reference {
        value["referencePng"] =
            json!({"mimeType": "image/png", "dataBase64": "", "byteLength": 0, "sha256": "0"});
    }
    serde_json::from_value(value).unwrap()
}

fn frame(id: &str, name: &str, at: [f64; 4], children: &[&str], extra: Value) -> (String, Value) {
    let mut fields = json!({"name": name, "childrenIds": children,
        "absoluteBoundingBox": {"x": at[0], "y": at[1], "width": at[2], "height": at[3]}});
    for (key, value) in extra.as_object().into_iter().flatten() {
        fields[key] = value.clone();
    }
    (
        id.to_owned(),
        json!({"id": id, "type": "FRAME", "fields": fields}),
    )
}

fn native(node: &str, asset: &str) -> Value {
    json!({"assetId": asset, "nodeId": node, "field": "node", "sourceKind": "node", "status": "exported",
        "format": "png", "scale": 1.0, "mimeType": "image/png", "renderMode": "node",
        "renderInfo": {"representation": "native-node"}})
}

/// `/` drawn at 1440 (with its referencePng and the renders of the hero and
/// its icon) and at 390 (without a referencePng); `/about` at 1440.
fn sources() -> Sources {
    let main = [
        frame(
            "10:1",
            "Main",
            [0.0, 0.0, 1440.0, 1200.0],
            &["10:11", "10:12", "10:13"],
            json!({}),
        ),
        frame("10:11", "Header", [0.0, 0.0, 1440.0, 80.0], &[], json!({})),
        frame(
            "10:12",
            "Hero",
            [0.0, 80.0, 1440.0, 600.0],
            &["10:121"],
            json!({}),
        ),
        frame(
            "10:121",
            "Icon",
            [100.0, 180.0, 24.0, 24.0],
            &[],
            json!({"isAsset": true}),
        ),
        frame(
            "10:13",
            "Cards",
            [0.0, 680.0, 1440.0, 520.0],
            &["10:131", "10:132"],
            json!({}),
        ),
        frame(
            "10:131",
            "Card 1",
            [120.0, 720.0, 400.0, 300.0],
            &[],
            json!({}),
        ),
        frame(
            "10:132",
            "Card 2",
            [560.0, 720.0, 400.0, 300.0],
            &[],
            json!({}),
        ),
    ]
    .into_iter()
    .collect::<serde_json::Map<_, _>>();
    let mobile = [
        frame(
            "10:2",
            "Main",
            [0.0, 0.0, 390.0, 2400.0],
            &["10:21"],
            json!({}),
        ),
        frame("10:21", "Hero", [0.0, 0.0, 390.0, 800.0], &[], json!({})),
    ]
    .into_iter()
    .collect::<serde_json::Map<_, _>>();
    let about = [
        frame(
            "10:3",
            "About",
            [0.0, 0.0, 1440.0, 900.0],
            &["10:31"],
            json!({}),
        ),
        frame("10:31", "Story", [0.0, 0.0, 1440.0, 900.0], &[], json!({})),
    ]
    .into_iter()
    .collect::<serde_json::Map<_, _>>();
    Sources::from_payloads(vec![
        (
            "art-main".into(),
            payload(
                "10:1",
                Value::Object(main),
                json!([
                    native("10:12", "hero-render"),
                    native("10:121", "icon-render")
                ]),
                true,
            ),
        ),
        (
            "art-mobile".into(),
            payload("10:2", Value::Object(mobile), json!([]), false),
        ),
        (
            "art-about".into(),
            payload("10:3", Value::Object(about), json!([]), true),
        ),
    ])
}

fn home() -> Target<'static> {
    Target {
        url: "/",
        screen: "home".into(),
        file: Some("src/app/page.tsx"),
        frames: vec![
            (1440, "10:1".into(), Some(1200.0)),
            (390, "10:2".into(), Some(2400.0)),
        ],
        next_url: Some("/about"),
        hidden: Vec::new(),
    }
}

fn kinds(plan: &PlanInput) -> BTreeSet<&str> {
    plan.screens[0]
        .checks
        .iter()
        .map(|check| check.kind.as_str())
        .collect()
}

#[test]
fn a_two_url_audit_plan_checks_the_page_its_sections_assets_geometry_overflow_and_transitions() {
    let sources = sources();
    let thresholds = VisualThresholds::default();
    let home = url_plan(&home(), "http://localhost:3000", &sources, &thresholds).unwrap();
    let about = url_plan(
        &Target {
            url: "/about",
            screen: "about".into(),
            file: Some("src/app/about/page.tsx"),
            frames: vec![(1440, "10:3".into(), Some(900.0))],
            next_url: Some("/"),
            hidden: Vec::new(),
        },
        "http://localhost:3000",
        &sources,
        &thresholds,
    )
    .unwrap();
    for plan in [&home.plan, &about.plan] {
        for kind in [
            "links-resolve",
            "page-health",
            "styles-applied",
            "sequence-order",
            "section-layout",
            "rendered-completeness",
            "no-horizontal-overflow",
            "no-clipped-text",
            "bounds",
            "interaction",
            "view-transition",
        ] {
            assert!(kinds(plan).contains(kind), "{kind} in {:?}", kinds(plan));
        }
    }
    let checks = &home.plan.screens[0].checks;
    for kind in ["no-low-contrast-text", "image-loaded"] {
        assert!(checks.iter().any(|check| check.kind == kind && check.widths.is_empty()));
    }
    let completeness = checks.iter().filter(|check| check.kind == "rendered-completeness").collect::<Vec<_>>();
    assert_eq!(completeness.len(), 2);
    for check in completeness {
        assert_eq!(check.widths.len(), 1);
        let frame = check.completeness.as_ref().unwrap();
        assert!(frame.frame_height > 0.0);
        assert!(!frame.sections.is_empty());
    }

    let captures = checks
        .iter()
        .filter_map(|check| {
            check
                .reference
                .as_ref()
                .map(|reference| (check.node_id.clone().unwrap(), reference))
        })
        .collect::<Vec<_>>();
    assert_eq!(captures.len(), 2, "the hero and its icon have renders");
    assert_eq!(captures[0].0, "10:12");
    assert_eq!(captures[0].1.scope, "section");
    assert_eq!(captures[0].1.threshold, Some(SECTION_THRESHOLD));
    assert_eq!(captures[1].0, "10:121");
    assert_eq!(captures[1].1.scope, "asset");
    assert_eq!(captures[1].1.asset_id, "icon-render");

    // Geometry: the second card sits 440px in and 40px down its section, at
    // the drawn width only, so a card out of order fails.
    let card = checks
        .iter()
        .find(|check| check.kind == "bounds" && check.node_id.as_deref() == Some("10:132"))
        .unwrap();
    let expected = card.expected.as_ref().unwrap();
    assert_eq!(
        (expected.x, expected.y, expected.width),
        (560.0, 40.0, 400.0)
    );
    assert_eq!(card.widths, [1440]);
    assert!(
        card.container
            .as_ref()
            .unwrap()
            .css
            .as_deref()
            .unwrap()
            .starts_with("[data-figma-node-id=\"10:13\"]")
    );
    let icon = checks
        .iter()
        .find(|check| check.kind == "bounds" && check.node_id.as_deref() == Some("10:121"))
        .unwrap();
    assert_eq!(icon.tolerance_px, None, "geometry is exact");
    assert!(
        checks.iter().any(|check| check.kind == "contained"
            && check.label.as_deref() == Some("navigation not cut off")),
        "the Header section asks for the nav containment check"
    );

    // Every width, the drawn ones as tall as their frames.
    let viewports = home
        .plan
        .viewports
        .iter()
        .map(|viewport| (viewport.width, viewport.height))
        .collect::<Vec<_>>();
    assert_eq!(
        viewports,
        [(1440, 1200), (1100, 900), (858, 900), (390, 2400)]
    );
    let figma = home.plan.screens[0].figma.as_ref().unwrap();
    assert_eq!(figma.drawn_widths, [1440, 390]);
    assert_eq!(figma.frames[0].artifact_id.as_deref(), Some("art-main"));
    assert_eq!(figma.frames[1].artifact_id, None);
}

#[test]
fn the_view_transition_check_is_last_and_comes_back_to_its_screen() {
    let plan = url_plan(
        &home(),
        "http://localhost:3000",
        &sources(),
        &VisualThresholds::default(),
    )
    .unwrap()
    .plan;
    let checks = &plan.screens[0].checks;
    let last = checks.last().unwrap();
    assert_eq!(last.kind, "view-transition");
    assert_eq!(
        last.subject.as_ref().unwrap().css.as_deref(),
        Some("a[href=\"/about\"] >> nth=0")
    );
    assert_eq!(
        checks
            .iter()
            .filter(|check| check.kind == "view-transition")
            .count(),
        1
    );
    // Every screen starts from its own URL, so nothing after the transition
    // depends on where it left the page.
    assert!(
        plan.screens
            .iter()
            .all(|screen| screen.route_path.is_some())
    );
    assert!(runtime_audit::validate_plan(&plan).is_ok());
}

#[test]
fn a_drawn_width_without_its_reference_png_is_reported() {
    let built = url_plan(
        &home(),
        "http://localhost:3000",
        &sources(),
        &VisualThresholds::default(),
    )
    .unwrap();
    let missing = built
        .missing
        .iter()
        .filter(|item| item["kind"] == "referencePng")
        .collect::<Vec<_>>();
    assert_eq!(missing.len(), 1, "{:?}", built.missing);
    assert_eq!(missing[0]["nodeId"], "10:2");
    assert_eq!(missing[0]["width"], 390);
    assert_eq!(
        missing[0]["call"]["arguments"]["outputs"],
        json!(["referencePng"])
    );
    let renders = built
        .missing
        .iter()
        .filter(|item| item["kind"] == "nativeRender")
        .flat_map(|item| item["nodeIds"].as_array().unwrap().clone())
        .collect::<Vec<_>>();
    assert_eq!(renders, [json!("10:11"), json!("10:13"), json!("10:21")]);

    let without_frame = url_plan(
        &Target {
            frames: vec![(1440, "99:1".into(), None)],
            ..home()
        },
        "http://localhost:3000",
        &sources(),
        &VisualThresholds::default(),
    )
    .unwrap();
    assert_eq!(without_frame.missing[0]["kind"], "frame");
}

#[test]
fn per_node_thresholds_tune_one_asset() {
    let thresholds = VisualThresholds {
        asset: Some(0.08),
        nodes: BTreeMap::from([("10:121".to_owned(), 0.12)]),
        ..VisualThresholds::default()
    };
    let plan = url_plan(&home(), "http://localhost:3000", &sources(), &thresholds)
        .unwrap()
        .plan;
    let icon = plan.screens[0]
        .checks
        .iter()
        .find(|check| check.kind == "capture" && check.node_id.as_deref() == Some("10:121"))
        .unwrap();
    assert_eq!(icon.reference.as_ref().unwrap().threshold, Some(0.12));
}

fn audit_with(findings: Value, comparisons: Value) -> Value {
    json!({
        "screenTable": [{"screen": "home", "routePath": "/", "width": 1440, "status": "fail", "reasons": []}],
        "findings": findings,
        "visualComparisons": comparisons,
    })
}

fn position_finding() -> Value {
    json!({"id": "home|bounds|10:132|1440", "class": "layout-position", "screen": "home", "routePath": "/",
        "width": 1440, "kind": "bounds", "nodeId": "10:132", "codeFile": "src/app/page.tsx",
        "reason": "position: x off by 440px", "fixHint": "order", "evidence": {}})
}

#[test]
fn a_finding_is_unresolved_after_the_fix_attempt_budget_with_its_evidence() {
    let mut plan = json!({"status": "confirmed"});
    let audit = audit_with(json!([position_finding()]), json!([]));
    let files = BTreeMap::from([(
        "10:132".to_owned(),
        ("/".to_owned(), "src/components/Cards.tsx".to_owned()),
    )]);
    record_round(&mut plan, &audit, "report-1", &[], &files, 3);
    let record = &plan["audit"]["findings"]["home|bounds|10:132|1440"];
    assert_eq!(record["status"], "open");
    assert_eq!(record["fixAttempts"], 0);
    assert_eq!(
        record["codeFile"], "src/components/Cards.tsx",
        "the node's own file"
    );
    // The same report again is not another attempt.
    record_round(&mut plan, &audit, "report-1", &[], &files, 3);
    assert_eq!(
        plan["audit"]["findings"]["home|bounds|10:132|1440"]["fixAttempts"],
        0
    );
    for (round, report) in ["report-2", "report-3"].iter().enumerate() {
        record_round(&mut plan, &audit, report, &[], &files, 3);
        let record = &plan["audit"]["findings"]["home|bounds|10:132|1440"];
        assert_eq!(record["fixAttempts"], round + 1);
        assert_eq!(record["status"], "open");
    }
    record_round(&mut plan, &audit, "report-4", &[], &files, 3);
    let unresolved = findings_by_status(&plan, "unresolved");
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0]["fixAttempts"], 3);
    let rows = [
        json!({"url": "/", "screen": "home", "implemented": "yes", "widths": [{"width": 1440, "result": "fail"}]}),
    ];
    let queue = work_queue(&plan, &rows, &[]);
    assert!(
        queue[0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["do"] != "fix"),
        "an unresolved finding leaves the queue: {queue:?}"
    );
    assert!(
        final_report(&plan, &rows, "").contains("layout-position 10:132: position: x off by 440px")
    );

    // A later run that measures it and does not see it fail resolves it.
    record_round(
        &mut plan,
        &audit_with(json!([]), json!([])),
        "report-5",
        &[],
        &files,
        3,
    );
    assert_eq!(
        plan["audit"]["findings"]["home|bounds|10:132|1440"]["status"],
        "resolved"
    );
}

#[test]
fn a_failing_comparison_becomes_a_finding_and_a_passing_one_resolves_it() {
    let mut plan = json!({"status": "confirmed"});
    let comparison = json!({"id": "home|asset|10:121|1440", "class": "asset-mismatch", "screen": "home",
        "routePath": "/", "nodeId": "10:121", "codeFile": "src/app/page.tsx", "viewport": {"width": 1440},
        "screenshot": {"path": "report/c7.png", "sha256": "shot-1"},
        "call": {"arguments": {"threshold": 0.05, "reference": {"artifactId": "art-main", "assetId": "icon-render"}}},
        "ifItFails": {"fixHint": "mask on a multi-colour SVG"}, "blockers": []});
    let audit = audit_with(json!([]), json!([comparison]));
    let pending = record_round(&mut plan, &audit, "r1", &[], &BTreeMap::new(), 3);
    assert_eq!(
        pending["pendingComparisons"],
        json!(["home|asset|10:121|1440"])
    );
    let failed = VisualResultInput {
        id: "home|asset|10:121|1440".into(),
        verdict: "fail".into(),
        changed_ratio: Some(0.4),
        diff_path: Some("report/diff/icon.png".into()),
        reason: None,
    };
    record_round(
        &mut plan,
        &audit,
        "r1",
        std::slice::from_ref(&failed),
        &BTreeMap::new(),
        3,
    );
    let open = findings_by_status(&plan, "open");
    assert_eq!(open[0]["class"], "asset-mismatch");
    assert_eq!(
        open[0]["reason"],
        "40.0% of pixels differ from the Figma render (allowed 5.0%)"
    );
    assert_eq!(open[0]["evidence"]["diff"], "report/diff/icon.png");
    let passed = VisualResultInput {
        verdict: "pass".into(),
        ..failed
    };
    record_round(&mut plan, &audit, "r2", &[passed], &BTreeMap::new(), 3);
    assert!(findings_by_status(&plan, "open").is_empty());
}

#[test]
fn blocked_work_goes_to_the_back_of_its_url() {
    let plan = json!({
        "audit": {"plans": [{"url": "/", "screen": "home"}], "findings": {
            "home|bounds|10:132|1440": {"status": "open", "class": "layout-position", "routePath": "/", "width": 1440, "fixAttempts": 0}}},
        "pendingDecisions": [{"id": "contact-target", "about": "/ contact form", "question": "Where does the form post?", "recommended": "mailto", "status": "open"}],
    });
    let rows = [
        json!({"url": "/", "screen": "home", "implemented": "yes", "widths": [{"width": 1440, "result": "fail"}]}),
    ];
    let queue = work_queue(&plan, &rows, &[json!("home|section|10:12|1440")]);
    let items = queue[0]["items"].as_array().unwrap();
    let order = items
        .iter()
        .map(|item| item["do"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(order, ["fix", "compare", "blocked"]);
    assert_eq!(queue[0]["done"], false);
}

#[test]
fn geometry_is_exact_and_merged_pages_are_found_per_width() {
    let plan = url_plan(
        &home(),
        "http://localhost:3000",
        &sources(),
        &VisualThresholds::default(),
    )
    .unwrap()
    .plan;
    let bounds = plan.screens[0]
        .checks
        .iter()
        .filter(|check| check.kind == "bounds")
        .collect::<Vec<_>>();
    assert!(
        bounds.iter().all(|check| check.tolerance_px.is_none()),
        "no tolerance"
    );
    let card = bounds
        .iter()
        .find(|check| check.node_id.as_deref() == Some("10:132"))
        .unwrap();
    assert_eq!(
        card.container.as_ref().unwrap().css.as_deref(),
        Some("[data-figma-node-id=\"10:13\"], [data-figma-node-ids~=\"1440:10:13\"]")
    );
    // The 390 frame's own sections are checked at 390, found through the
    // merged page's per-width map.
    let mobile = bounds
        .iter()
        .find(|check| check.node_id.as_deref() == Some("10:21"))
        .unwrap();
    assert_eq!(mobile.widths, [390]);
    assert!(
        mobile
            .subject
            .as_ref()
            .unwrap()
            .css
            .as_deref()
            .unwrap()
            .contains("390:10:21")
    );
}

#[test]
fn a_merged_element_a_width_does_not_draw_is_checked_hidden_there() {
    let records = vec![json!({"responsive": [
        {"ids": [{"width": 1440, "nodeId": "10:1"}, {"width": 390, "nodeId": "10:2"}], "hiddenAt": []},
        {"ids": [{"width": 1440, "nodeId": "10:13"}], "hiddenAt": [390]},
    ]})];
    let frames = home().frames;
    let hidden = hidden_elements(&records, &frames);
    assert_eq!(hidden, [(390, "10:13".to_owned())]);
    let plan = url_plan(
        &Target { hidden, ..home() },
        "http://localhost:3000",
        &sources(),
        &VisualThresholds::default(),
    )
    .unwrap()
    .plan;
    let check = plan.screens[0]
        .checks
        .iter()
        .find(|check| check.kind == "hidden")
        .unwrap();
    assert_eq!(check.widths, [390]);
    assert_eq!(check.node_id.as_deref(), Some("10:13"));
}

struct Project(std::path::PathBuf);
impl Project {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("audit-plan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, text: &str) {
        let target = self.0.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, text).unwrap();
    }
    fn root(&self) -> &str {
        self.0.to_str().unwrap()
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn blog_route() -> Value {
    json!({"url": "/blog/[slug]", "file": "src/app/blog/[slug]/page.tsx", "app": ".", "nodeIds": ["20:1"], "drawnWidths": [1440]})
}

#[test]
fn a_dynamic_route_takes_its_literal_static_params() {
    let project = Project::new("literal");
    project.write(
        "src/app/blog/[slug]/page.tsx",
        "export function generateStaticParams() {\n  return [{ slug: 'hello' }, { slug: \"second-post\" }]\n}\nexport default function Page() { return null }\n",
    );
    let found = dynamic_urls(
        project.root(),
        &blog_route(),
        &Value::Null,
        &Sources::from_payloads(Vec::new()),
    );
    assert_eq!(found.source, "generateStaticParams");
    assert_eq!(found.urls, ["/blog/hello", "/blog/second-post"]);
}

#[test]
fn a_dynamic_route_reads_the_local_data_its_static_params_map() {
    let project = Project::new("data");
    project.write(
        "src/app/blog/[slug]/page.tsx",
        "import { posts } from '@/data/posts'\nexport async function generateStaticParams() {\n  return posts.map((post) => ({ slug: post.slug }))\n}\nexport default function Page() { return null }\n",
    );
    project.write(
        "src/data/posts.ts",
        "export const posts = [\n  { slug: 'launch', title: 'Launch' },\n  { slug: 'roadmap', title: 'Roadmap' },\n]\n",
    );
    let found = dynamic_urls(
        project.root(),
        &blog_route(),
        &Value::Null,
        &Sources::from_payloads(Vec::new()),
    );
    assert_eq!(found.urls, ["/blog/launch", "/blog/roadmap"]);
}

#[test]
fn a_dynamic_route_without_params_takes_the_links_audited_pages_hold() {
    let project = Project::new("links");
    let links =
        json!({"/": ["/blog/from-home", "/about", "/blog/other"], "/about": ["/blog/from-home"]});
    let found = dynamic_urls(
        project.root(),
        &blog_route(),
        &links,
        &Sources::from_payloads(Vec::new()),
    );
    assert_eq!(found.source, "links on audited pages");
    assert_eq!(found.urls, ["/blog/from-home", "/blog/other"]);
    assert!(found.assumption.is_none());
}

#[test]
fn a_dynamic_route_falls_back_to_its_detail_frame_as_an_assumption() {
    let project = Project::new("frame");
    let detail = [
        frame("20:1", "Blog detail", [0.0, 0.0, 1440.0, 900.0], &["20:2"], json!({})),
        ("20:2".to_owned(), json!({"id": "20:2", "type": "TEXT", "fields": {"characters": "Our First Release!", "parentId": "20:1"}})),
    ]
    .into_iter()
    .collect::<serde_json::Map<_, _>>();
    let sources = Sources::from_payloads(vec![(
        "art-blog".into(),
        payload("20:1", Value::Object(detail), json!([]), true),
    )]);
    let found = dynamic_urls(project.root(), &blog_route(), &Value::Null, &sources);
    assert_eq!(found.source, "Figma detail frame");
    assert_eq!(found.urls, ["/blog/our-first-release"]);
    assert!(found.assumption.unwrap().contains("Our First Release!"));

    let nothing = dynamic_urls(
        project.root(),
        &blog_route(),
        &Value::Null,
        &Sources::from_payloads(Vec::new()),
    );
    assert!(nothing.urls.is_empty());
    assert!(nothing.why.contains("generateStaticParams"));
}

#[test]
fn a_catch_all_route_joins_its_segments() {
    assert_eq!(
        values_of("{ slug: ['docs', 'intro'] }", "slug"),
        ["docs/intro"]
    );
    assert_eq!(params("/docs/[...slug]"), [("slug".to_owned(), true)]);
    assert_eq!(
        fill(
            "/docs/[...slug]",
            &BTreeMap::from([("slug".to_owned(), "docs/intro".to_owned())])
        )
        .as_deref(),
        Some("/docs/docs/intro")
    );
}

const EXPORTED: &str = "export default function Page() {\n  return (\n    <Flex data-figma-node-id=\"10:13\" gap=\"40px\" p=\"40px 120px\">\n      <Box data-figma-node-id=\"10:132\" h=\"300px\" w=\"400px\" />\n    </Flex>\n  )\n}\n";

fn geometry_audit() -> Value {
    json!({"findings": [{"id": "home|bounds|10:132|1440", "class": "layout-position", "kind": "bounds",
        "nodeId": "10:132", "routePath": "/", "width": 1440, "reason": "position: x off by 1px",
        "evidence": {"screenshot": "report/home.png", "measured": {
            "expected": {"x": 560, "y": 40, "width": 400, "height": 300},
            "actual": {"x": 561, "y": 40, "width": 400, "height": 300}}}}]})
}

fn exported_project(name: &str) -> (Project, OutputPolicy) {
    let project = Project::new(name);
    let policy = OutputPolicy::from_roots(vec![project.0.clone()]).unwrap();
    let response = json!({"tsx": EXPORTED, "figmaNodeIds": {"root": "10:1"},
        "outputResults": {"tsx": {"_sourceMap": [
            {"nodeId": "10:132", "property": "width", "generatedProperty": "w=\"400px\"", "resolution": "exact"}]}}});
    let written =
        super::super::export_record::write(&policy, project.root(), "FILEKEY", "10:1", &response)
            .unwrap()
            .unwrap();
    assert!(written.ends_with("10-1.json"), "{written}");
    (project, policy)
}

#[test]
fn a_mismatch_with_the_code_still_equal_to_the_export_is_a_converter_bug_with_a_repro() {
    let (project, policy) = exported_project("converter-bug");
    // The agent added behaviour, not layout: still the export's element.
    project.write(
        "src/app/page.tsx",
        &EXPORTED.replace(
            "<Box data-figma-node-id=\"10:132\"",
            "<Box data-figma-node-id=\"10:132\" onClick={open} _hover={{ bg: '$primary' }}",
        ),
    );
    let mut audit = geometry_audit();
    classify_geometry(
        &mut audit,
        project.root(),
        &["src/app/page.tsx".to_owned()],
        &policy,
    );
    let finding = &audit["findings"][0];
    assert_eq!(finding["class"], "converter-bug", "{finding}");
    let repro_path = finding["evidence"]["repro"].as_str().unwrap();
    assert!(
        repro_path
            .replace('\\', "/")
            .ends_with(".devup/converter-bugs/10-132.json"),
        "{repro_path}"
    );
    let repro: Value = serde_json::from_slice(
        &std::fs::read(project.0.join(".devup/converter-bugs/10-132.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(repro["fileKey"], "FILEKEY");
    assert_eq!(repro["nodeId"], "10:132");
    assert_eq!(repro["figmaBox"]["x"], 560);
    assert_eq!(repro["domBox"]["x"], 561);
    assert_eq!(
        repro["generatedProps"],
        json!(["h=\"300px\"", "w=\"400px\""])
    );
    assert_eq!(repro["sourceMap"][0]["generatedProperty"], "w=\"400px\"");

    // It leaves the fix loop at once and stays reported.
    let mut plan =
        json!({"status": "confirmed", "audit": {"plans": [{"url": "/", "screen": "home"}]}});
    record_round(&mut plan, &audit, "r1", &[], &BTreeMap::new(), 3);
    assert!(findings_by_status(&plan, "open").is_empty());
    assert_eq!(findings_by_status(&plan, "converter-bug").len(), 1);
    assert_eq!(
        plan["audit"]["converterBugs"]["10:132"]["repro"],
        finding["evidence"]["repro"]
    );
    let rows = [
        json!({"url": "/", "screen": "home", "implemented": "yes", "widths": [{"width": 1440, "result": "fail"}]}),
    ];
    let queue = work_queue(&plan, &rows, &[]);
    assert_eq!(
        queue[0]["done"], false,
        "a filed converter bug remains reported, but a failing cell cannot be called done: {queue:?}"
    );
    assert_eq!(queue[0]["reported"][0]["converterBug"], "10:132");
    let report = final_report(&plan, &rows, "");
    assert!(
        report.contains("Converter bugs (filed, not retried):\n- / at 1440px, 10:132"),
        "{report}"
    );
}

#[test]
fn a_mismatch_after_a_hand_edit_is_code_drift() {
    let (project, policy) = exported_project("drift");
    project.write(
        "src/app/page.tsx",
        &EXPORTED.replace("w=\"400px\" />", "w=\"401px\" ml=\"2px\" />"),
    );
    let mut audit = geometry_audit();
    classify_geometry(
        &mut audit,
        project.root(),
        &["src/app/page.tsx".to_owned()],
        &policy,
    );
    let finding = &audit["findings"][0];
    assert_eq!(finding["class"], "code-drift", "{finding}");
    assert_eq!(
        finding["evidence"]["drift"]["missing"],
        json!(["w=\"400px\""])
    );
    assert_eq!(finding["evidence"]["drift"]["added"], json!(["ml=\"2px\""]));
    assert!(
        finding["fixHint"]
            .as_str()
            .unwrap()
            .contains("Restore the values the export wrote")
    );
    assert!(!project.0.join(".devup/converter-bugs").exists());
}

#[test]
fn a_mismatch_without_an_export_record_keeps_its_class_and_says_why() {
    let project = Project::new("no-record");
    let policy = OutputPolicy::from_roots(vec![project.0.clone()]).unwrap();
    let mut audit = geometry_audit();
    classify_geometry(&mut audit, project.root(), &[], &policy);
    assert_eq!(audit["findings"][0]["class"], "layout-position");
    assert!(
        audit["findings"][0]["evidence"]["cause"]
            .as_str()
            .unwrap()
            .contains("No export record")
    );
}

#[test]
fn an_asset_capture_waiting_for_comparison_blocks_completion() {
    let mut plan = json!({"audit":{}});
    let audit = json!({"screenTable":[{"routePath":"/", "width":1440}], "visualComparisons":[
        {"id":"home|asset|icon|1440", "routePath":"/", "viewport":{"width":1440}, "blockers":[]}
    ]});
    record_round(&mut plan, &audit, "run", &[], &BTreeMap::new(), 3);
    assert_eq!(plan["audit"]["visualPending"].as_array().unwrap().len(), 1);
    record_round(&mut plan, &audit, "run", &[VisualResultInput { id:"home|asset|icon|1440".into(), verdict:"pass".into(), changed_ratio:Some(0.0), diff_path:None, reason:None }], &BTreeMap::new(), 3);
    assert!(plan["audit"]["visualPending"].as_array().unwrap().is_empty());
}
