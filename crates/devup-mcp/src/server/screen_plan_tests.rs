use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "screen-plan-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fixture = Self(dunce::canonicalize(&root).unwrap());
        fixture.write("package.json", "{}");
        fixture.write(
            "src/app/layout.tsx",
            "import { Header } from '../components/Header';\nexport default function RootLayout({ children }) { return <html><body><Header />{children}</body></html>; }",
        );
        fixture.write(
            "src/components/Header.tsx",
            "import Link from 'next/link';\nexport function Header() { return <nav><Link href=\"/company\">회사소개</Link><Link href=\"/\">Home</Link></nav>; }",
        );
        fixture.write(
            "src/app/page.tsx",
            "export default function Page() { return <main />; }",
        );
        fixture.write(
            "src/app/company/page.tsx",
            "export default function Page() { return <main />; }",
        );
        fixture
    }

    fn write(&self, path: &str, text: &str) {
        let target = self.0.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, text).unwrap();
    }

    fn root(&self) -> &str {
        self.0.to_str().unwrap()
    }

    fn policy(&self) -> OutputPolicy {
        OutputPolicy::from_roots(vec![self.0.clone()]).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn screen(node_id: &str, name: &str, width: f64) -> PlanScreenInput {
    PlanScreenInput {
        node_id: node_id.into(),
        name: name.into(),
        width,
        height: Some(900.0),
        ..PlanScreenInput::default()
    }
}

fn evidence() -> ProjectEvidence {
    ProjectEvidence {
        routes: vec![
            ("/".into(), "src/app/page.tsx".into()),
            ("/company".into(), "src/app/company/page.tsx".into()),
            ("/news/[id]".into(), "src/app/news/[id]/page.tsx".into()),
        ],
        nav_links: vec![(
            "회사소개".into(),
            "/company".into(),
            "src/components/Header.tsx:2".into(),
        )],
        layout_parts: vec!["Header".into()],
        app_dir: "src/app".into(),
        inventory_note: None,
    }
}

fn question_ids(plan: &Value) -> Vec<String> {
    array(&plan["questions"])
        .iter()
        .map(|question| text(&question["id"]).to_owned())
        .collect()
}

fn input(fixture: &Fixture, action: &str) -> ScreenPlanInput {
    ScreenPlanInput {
        action: action.into(),
        project_root: Some(fixture.root().into()),
        ..ScreenPlanInput::default()
    }
}

/// The route the user never saw being chosen is the failure this tool exists
/// for, so a screen nothing names gets no route and a question instead.
#[test]
fn an_unverified_route_is_null_and_becomes_a_question() {
    let plan = draft_plan(
        &[screen("1:2", "Pricing", 1440.0)],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    let row = &plan["screens"][0];
    assert_eq!(row["proposedRoute"], Value::Null);
    assert_eq!(row["routeEvidence"], "UNVERIFIED");
    let question = array(&plan["questions"])
        .iter()
        .find(|question| question["id"] == "route:1:2")
        .expect("an unverified route is asked");
    let labels = array(&question["options"])
        .iter()
        .map(|option| text(&option["label"]))
        .collect::<Vec<_>>();
    assert_eq!(labels[0], "/pricing (Recommended)");
    assert!(labels.contains(&SKIP));
}

/// Each kind of evidence proposes the route on its own, and a screen it
/// proposes is not asked about its route.
#[test]
fn an_evidence_backed_route_is_proposed_without_a_route_question() {
    let targets = BTreeMap::from([("2:1".to_owned(), "/contact?from=nav".to_owned())]);
    let plan = draft_plan(
        &[
            screen("2:1", "Contact", 1440.0),
            screen("2:2", "회사소개", 1440.0),
            screen("2:3", "Home", 1440.0),
        ],
        &targets,
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    let rows = array(&plan["screens"]);
    assert_eq!(rows[0]["proposedRoute"], "/contact");
    assert_eq!(rows[0]["routeEvidence"], "navigationTargets[2:1]");
    assert_eq!(rows[1]["proposedRoute"], "/company");
    assert!(text(&rows[1]["routeEvidence"]).starts_with("nav-label"));
    assert_eq!(rows[2]["proposedRoute"], "/");
    let ids = question_ids(&plan);
    assert!(
        !ids.iter()
            .any(|id| id == "route:2:1" || id.ends_with(":2:1")),
        "{ids:?}"
    );
    assert!(!ids.iter().any(|id| id.starts_with("route:")), "{ids:?}");
    // An existing page is a different question: what building over it does.
    assert!(ids.iter().any(|id| id == "collision:2:2"));
    assert_eq!(rows[1]["existingFile"], "src/app/company/page.tsx");
    assert_eq!(rows[0]["sharedParts"], json!(["Header"]));
}

/// Two screens the evidence puts on one URL leave neither decided.
#[test]
fn two_screens_on_one_evidence_route_are_both_asked() {
    let targets = BTreeMap::from([
        ("3:1".to_owned(), "/about".to_owned()),
        ("3:2".to_owned(), "/about".to_owned()),
    ]);
    let plan = draft_plan(
        &[
            screen("3:1", "About A", 1440.0),
            screen("3:2", "About B", 1440.0),
        ],
        &targets,
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    let ids = question_ids(&plan);
    assert!(ids.contains(&"route:3:1".to_owned()) && ids.contains(&"route:3:2".to_owned()));
}

#[test]
fn widths_of_one_screen_are_grouped_and_modals_asked_as_state_or_route() {
    let plan = draft_plan(
        &[
            screen("4:1", "Main - 1440", 1440.0),
            screen("4:2", "Main - 390", 390.0),
            screen("4:3", "Login Modal", 480.0),
        ],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    let rows = array(&plan["screens"]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["drawnWidths"], json!([1440, 390]));
    assert_eq!(rows[0]["proposedRoute"], "/");
    let ids = question_ids(&plan);
    assert!(ids.contains(&"responsive".to_owned()));
    let modal = array(&plan["questions"])
        .iter()
        .find(|question| question["id"] == "modal:4:3")
        .expect("a modal is asked");
    assert!(text(&modal["options"][0]["label"]).starts_with("State on Main"));
}

/// Every question must be accepted as-is by a structured question tool.
#[test]
fn every_question_has_the_question_tool_shape() {
    let mut linked = screen("5:1", "Gallery", 1440.0);
    linked.links = vec![PlanLinkInput {
        label: "More".into(),
        destination_node_id: Some("9:9".into()),
        url: None,
    }];
    linked.parts = vec!["Footer".into()];
    let mut other = screen("5:2", "News", 1440.0);
    other.parts = vec!["Footer".into()];
    let targets = BTreeMap::from([("5:3".to_owned(), "/news/[id]".to_owned())]);
    let plan = draft_plan(
        &[
            linked,
            other,
            screen("5:3", "News detail", 1440.0),
            screen("5:4", "공지 팝업", 400.0),
            screen("5:5", "Company - 1440", 1440.0),
            screen("5:6", "Company - 390", 390.0),
        ],
        &targets,
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    let questions = ask_shape(array(&plan["questions"]));
    let ids = questions
        .iter()
        .map(|question| text(&question["id"]))
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), questions.len(), "question ids are unique");
    for kind in [
        "route:",
        "link:",
        "layout:",
        "dynamic:",
        "modal:",
        "responsive",
        "collision:",
    ] {
        assert!(
            ids.iter().any(|id| id.starts_with(kind)),
            "{kind} missing from {ids:?}"
        );
    }
    for question in &questions {
        let header = text(&question["header"]);
        assert!(
            !header.is_empty() && header.chars().count() <= HEADER_LIMIT,
            "{header}"
        );
        assert!(!text(&question["question"]).is_empty());
        assert_eq!(question["multiple"], false);
        assert!(
            question.get("resolve").is_none(),
            "resolution stays in the plan file"
        );
        let options = array(&question["options"]);
        assert!((2..=MAX_OPTIONS).contains(&options.len()), "{question}");
        assert!(
            text(&options[0]["label"]).ends_with(RECOMMENDED),
            "{question}"
        );
        for option in &options[1..] {
            assert!(
                !text(&option["label"]).contains("(Recommended)"),
                "{question}"
            );
        }
        assert!(
            options
                .iter()
                .all(|option| !text(&option["description"]).is_empty())
        );
    }
}

#[tokio::test]
async fn confirm_rejects_missing_answers_by_id_and_accepts_a_complete_set() {
    let fixture = Fixture::new();
    let mut draft_input = input(&fixture, "draft");
    draft_input.navigation_targets = BTreeMap::from([("6:3".to_owned(), "/contact".to_owned())]);
    let drafted = draft(
        &draft_input,
        &[
            screen("6:1", "Pricing", 1440.0),
            screen("6:2", "Careers", 1440.0),
            screen("6:3", "Contact", 1440.0),
        ],
        json!({"from": "caller"}),
        &fixture.policy(),
    )
    .await
    .unwrap();
    assert_eq!(drafted["askInstruction"], ASK_INSTRUCTION);
    assert!(fixture.0.join(".devup/screen-plan.json").is_file());
    assert!(fixture.0.join(".devup/screen-plan.md").is_file());
    let plan_id = text(&drafted["planId"]).to_owned();

    let mut confirm_input = input(&fixture, "confirm");
    confirm_input.plan_id = Some(plan_id.clone());
    confirm_input.answers = BTreeMap::from([(
        "route:6:1".to_owned(),
        AnswerInput::One("/pricing (Recommended)".into()),
    )]);
    let error = confirm(&confirm_input, &fixture.policy()).unwrap_err();
    assert_eq!(error.details["missing"], json!(["route:6:2"]));
    assert!(error.message.contains("route:6:2"));
    assert_eq!(
        read_plan(fixture.root()).unwrap().unwrap()["status"],
        "draft"
    );

    confirm_input
        .answers
        .insert("route:6:2".into(), AnswerInput::One("/jobs".into()));
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    let urls = array(&confirmed["routes"])
        .iter()
        .map(|row| row["url"].clone())
        .collect::<Vec<_>>();
    assert_eq!(urls, [json!("/pricing"), json!("/jobs"), json!("/contact")]);
    assert_eq!(confirmed["customAnswers"][0]["answer"], "/jobs");
    let saved = read_plan(fixture.root()).unwrap().unwrap();
    assert_eq!(saved["status"], "confirmed");
    let markdown = std::fs::read_to_string(fixture.0.join(".devup/screen-plan.md")).unwrap();
    assert!(markdown.contains("| /jobs | Careers |"), "{markdown}");
}

#[tokio::test]
async fn a_custom_route_answer_must_be_a_route() {
    let fixture = Fixture::new();
    let drafted = draft(
        &input(&fixture, "draft"),
        &[screen("7:1", "Pricing", 1440.0)],
        json!({}),
        &fixture.policy(),
    )
    .await
    .unwrap();
    let mut confirm_input = input(&fixture, "confirm");
    confirm_input.plan_id = drafted["planId"].as_str().map(str::to_owned);
    confirm_input.answers = BTreeMap::from([(
        "route:7:1".to_owned(),
        AnswerInput::One("the pricing page".into()),
    )]);
    let error = confirm(&confirm_input, &fixture.policy()).unwrap_err();
    assert!(error.message.contains("custom answer must be an app route"));
}

fn confirmed_plan() -> Value {
    json!({
        "kind": PLAN_KIND, "schemaVersion": SCHEMA_VERSION, "planId": "sp-test", "status": "confirmed",
        "source": {"nodeId": "10:0"},
        "routes": [
            {"url": "/", "screen": "10:1", "name": "Main", "nodeIds": ["10:1", "10:2"], "drawnWidths": [1440, 390], "implementation": "page", "file": "src/app/page.tsx"},
            {"url": "/about", "screen": "10:3", "name": "About", "nodeIds": ["10:3"], "drawnWidths": [1440], "implementation": "page", "file": "src/app/about/page.tsx"},
            {"url": null, "screen": "10:4", "name": "Login modal", "nodeIds": ["10:4"], "drawnWidths": [480], "implementation": "state", "hostUrl": "/"},
        ],
    })
}

/// A width the audit has no item for is not-run, and a URL with a failing
/// width is never counted as verified.
#[test]
fn status_marks_widths_without_an_audit_item_not_run() {
    let audit = json!({
        "verdict": "fail",
        "widthTable": [{"width": 1440}, {"width": 390}],
        "screenTable": [
            {"screen": "main", "routePath": "/", "width": 1440, "status": "pass", "reasons": [], "screenshot": "1440x900@1-main.png"},
            {"screen": "about", "routePath": "/about", "width": 1440, "status": "pass", "reasons": []},
            {"screen": "about", "routePath": "/about/", "width": 390, "status": "fail", "reasons": ["horizontal overflow: 42px past the viewport"]},
        ],
    });
    let report = status_report(&confirmed_plan(), &evidence(), Some(&audit));
    let rows = array(&report["rows"]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["implemented"], "yes");
    assert_eq!(rows[1]["implemented"], "no");
    assert_eq!(rows[0]["widths"][0]["result"], "pass");
    assert_eq!(rows[0]["widths"][1]["result"], "not-run");
    assert_eq!(rows[1]["widths"][1]["result"], "fail");
    assert_eq!(
        report["summary"],
        "2 URLs: 1 implemented, 0 verified at all widths, /about overflows at 390"
    );
    assert_eq!(report["states"][0]["hostUrl"], "/");
    assert!(text(&report["markdownTable"]).contains("1440x900@1-main.png"));

    let without = status_report(&confirmed_plan(), &evidence(), None);
    assert!(
        array(&without["rows"])
            .iter()
            .flat_map(|row| array(&row["widths"]))
            .all(|width| width["result"] == "not-run")
    );
    assert!(text(&without["summary"]).contains("every width is not-run"));
}

fn write_plan_file(fixture: &Fixture, plan: &Value) {
    fixture.write(PLAN_FILE, &serde_json::to_string(plan).unwrap());
}

fn export<'a>(fixture: &'a Fixture, frames: &[&str]) -> ExportRequest<'a> {
    ExportRequest {
        project_root: fixture.root(),
        url: None,
        frames: frames.iter().map(|frame| (*frame).to_owned()).collect(),
        all_screens: false,
        scaffold_write: false,
        scaffold_route: None,
        output_paths: Vec::new(),
    }
}

#[test]
fn the_export_gate_blocks_page_writes_without_a_confirmed_plan() {
    let fixture = Fixture::new();
    let mut scaffold = export(&fixture, &["10:3"]);
    scaffold.scaffold_write = true;
    scaffold.scaffold_route = Some("/about");
    let Gate::Blocked(response) = export_gate(&scaffold).unwrap() else {
        panic!("a pageScaffold write without a plan is blocked");
    };
    assert_eq!(response["status"], "plan_required");
    assert_eq!(response["nextAction"]["tool"], "devup_screen_plan");
    assert_eq!(response["nextAction"]["arguments"]["action"], "draft");

    let mut app_path = export(&fixture, &["10:3"]);
    app_path.output_paths = vec!["src/app/about/page.tsx"];
    assert!(matches!(export_gate(&app_path).unwrap(), Gate::Blocked(_)));

    let mut several = export(&fixture, &["10:1", "10:3"]);
    several.output_paths = vec!["src/components/Screens.tsx"];
    assert!(matches!(export_gate(&several).unwrap(), Gate::Blocked(_)));

    let mut draft_only = confirmed_plan();
    draft_only["status"] = json!("draft");
    write_plan_file(&fixture, &draft_only);
    assert!(matches!(export_gate(&scaffold).unwrap(), Gate::Blocked(_)));
}

#[test]
fn the_export_gate_never_blocks_a_component_or_a_preview() {
    let fixture = Fixture::new();
    let mut component = export(&fixture, &["10:3"]);
    component.output_paths = vec!["src/components/Card.tsx"];
    assert!(matches!(export_gate(&component).unwrap(), Gate::Open));
    let preview = export(&fixture, &["10:1", "10:3"]);
    assert!(matches!(export_gate(&preview).unwrap(), Gate::Open));
    let mut scaffold_preview = export(&fixture, &["10:3"]);
    scaffold_preview.scaffold_route = Some("/about");
    assert!(matches!(
        export_gate(&scaffold_preview).unwrap(),
        Gate::Open
    ));
}

#[test]
fn a_confirmed_plan_supplies_the_scaffold_route_and_refuses_another() {
    let fixture = Fixture::new();
    write_plan_file(&fixture, &confirmed_plan());
    let mut scaffold = export(&fixture, &["10:2"]);
    scaffold.scaffold_write = true;
    scaffold.scaffold_route = Some("");
    let Gate::Route(route) = export_gate(&scaffold).unwrap() else {
        panic!("the plan covers 10:2");
    };
    assert_eq!(route, "/");
    scaffold.scaffold_route = Some("/home");
    let error = export_gate(&scaffold)
        .err()
        .expect("a different route is a mismatch");
    assert_eq!(error.details["routeMismatch"]["planned"], "/");

    let mut uncovered = export(&fixture, &["99:1"]);
    uncovered.scaffold_write = true;
    uncovered.scaffold_route = Some("/x");
    let Gate::Blocked(response) = export_gate(&uncovered).unwrap() else {
        panic!("a frame the plan does not place is blocked");
    };
    assert_eq!(
        response["planRequired"]["details"]["uncoveredFrames"],
        json!(["99:1"])
    );

    let mut several = export(&fixture, &["10:1", "10:3"]);
    several.output_paths = vec!["src/components/Screens.tsx"];
    assert!(matches!(export_gate(&several).unwrap(), Gate::Open));
}

#[tokio::test]
async fn project_evidence_reads_routes_layout_parts_and_nav_links() {
    let fixture = Fixture::new();
    let evidence = project_evidence(fixture.root()).await.unwrap();
    let routes = evidence
        .routes
        .iter()
        .map(|(route, _)| route.as_str())
        .collect::<Vec<_>>();
    assert!(
        routes.contains(&"/") && routes.contains(&"/company"),
        "{routes:?}"
    );
    assert_eq!(evidence.app_dir, "src/app");
    assert!(evidence.layout_parts.contains(&"Header".to_owned()));
    assert!(
        evidence
            .nav_links
            .iter()
            .any(|(label, href, _)| label == "회사소개" && href == "/company"),
        "{:?}",
        evidence.nav_links
    );
}
