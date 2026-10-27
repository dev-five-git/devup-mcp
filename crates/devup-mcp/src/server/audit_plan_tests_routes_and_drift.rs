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
    record_round(
        &mut plan,
        &audit,
        "run",
        &[VisualResultInput {
            id: "home|asset|icon|1440".into(),
            verdict: "pass".into(),
            changed_ratio: Some(0.0),
            diff_path: None,
            reason: None,
        }],
        &BTreeMap::new(),
        3,
    );
    assert!(
        plan["audit"]["visualPending"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
