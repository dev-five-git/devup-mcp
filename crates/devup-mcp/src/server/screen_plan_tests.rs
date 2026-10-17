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
        fixture.write(CONFIG_FILE, SITE_CONFIG);
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
        // The site questions are pre-answered here so each test below is
        // about its own questions; `site_*` tests start from bare evidence.
        config: answered_site(),
        ..ProjectEvidence::default()
    }
}

fn answered_site() -> PlanConfig {
    PlanConfig {
        favicon: Some("app/icon.svg".into()),
        og_image: Some("public/og.png".into()),
        site_name: Some("Prime".into()),
        site_url: Some("https://example.com".into()),
        motion: Some("none".into()),
        ..PlanConfig::default()
    }
}

/// The site config the fixtures write, matching [`answered_site`].
const SITE_CONFIG: &str = "{\"favicon\": \"app/icon.svg\", \"ogImage\": \"public/og.png\", \"siteName\": \"Prime\", \"siteUrl\": \"https://example.com\", \"motion\": \"none\"}";

fn sectioned(node_id: &str, name: &str, width: f64, section: &str) -> PlanScreenInput {
    PlanScreenInput {
        section: Some(format!("section-{section}")),
        section_name: Some(section.into()),
        ..screen(node_id, name, width)
    }
}

fn find<'a>(plan: &'a Value, id: &str) -> &'a Value {
    array(&plan["questions"])
        .iter()
        .find(|question| question["id"] == id)
        .unwrap_or_else(|| panic!("{id} is asked: {}", plan["questions"]))
}

fn first_label(question: &Value) -> &str {
    text(&question["options"][0]["label"])
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
    // Home -> / is a guess from the layer name, so it is shown and put to
    // the user in one compact question rather than decided silently.
    assert_eq!(rows[2]["routeEvidence"], NAME_MATCH);
    assert_eq!(plan["heuristics"][0]["screen"], "2:3");
    let ids = question_ids(&plan);
    assert!(ids.contains(&"heuristics".to_owned()), "{ids:?}");
    assert!(ids.contains(&"review:2:3".to_owned()), "{ids:?}");
    assert_eq!(
        first_label(find(&plan, "heuristics")),
        "Accept all (Recommended)"
    );
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
fn widths_in_one_section_recommend_one_page_and_modals_are_asked() {
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
    assert_eq!(rows[0]["grouping"]["recommended"], "single");
    let group = find(&plan, "group:4:1");
    assert_eq!(first_label(group), "One responsive page (Recommended)");
    // Separate pages need their own routes, asked in the same up-front batch
    // and only if separate is chosen. No URL is made up for the 390 frame.
    let split = find(&plan, "split-route:4:2");
    assert_eq!(split["when"][0]["question"], "group:4:1");
    assert_eq!(split["when"][0]["answer"], "Separate pages");
    assert!(!plan.to_string().contains("/390"));
    let modal = find(&plan, "modal:4:3");
    assert!(first_label(modal).starts_with("State on Main"));
    assert_eq!(rows[1]["kindEvidence"], NAME_MATCH);
    assert!(text(&modal["question"]).contains("name-match"));
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
    let targets = BTreeMap::from([
        ("5:3".to_owned(), "/news/[id]".to_owned()),
        ("5:7".to_owned(), "/".to_owned()),
    ]);
    let plan = draft_plan(
        &[
            linked,
            other,
            screen("5:3", "News detail", 1440.0),
            screen("5:4", "공지 팝업", 400.0),
            screen("5:5", "Company - 1440", 1440.0),
            screen("5:6", "Company - 390", 390.0),
            screen("5:7", "Landing", 1440.0),
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
        "group:",
        "split-route:",
        "heuristics",
        "review:",
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
        for condition in array(&question["when"]) {
            assert!(condition.get("action").is_none(), "{question}");
            assert!(ids.contains(text(&condition["question"])), "{question}");
        }
    }
    assert_rounds_cover(&plan);
}

/// Every question in exactly one round, rounds within the tool's limits, and
/// a conditional question always after the question it depends on.
fn assert_rounds_cover(plan: &Value) {
    let questions = array(&plan["questions"]);
    let rounds = question_rounds(questions, MAX_QUESTIONS_PER_ROUND);
    let mut seen = Vec::new();
    let mut round_of = BTreeMap::new();
    for (index, round) in rounds.iter().enumerate() {
        assert_eq!(round["round"], index + 1);
        let asked = array(&round["questions"]);
        assert!(
            (1..=MAX_QUESTIONS_PER_ROUND).contains(&asked.len()),
            "{round}"
        );
        for question in asked {
            assert!(array(&question["options"]).len() <= MAX_OPTIONS);
            let id = text(&question["id"]).to_owned();
            round_of.insert(id.clone(), index);
            seen.push(id);
        }
    }
    let mut expected = questions
        .iter()
        .map(|question| text(&question["id"]).to_owned())
        .collect::<Vec<_>>();
    let mut got = seen.clone();
    expected.sort();
    got.sort();
    assert_eq!(got, expected, "every question exactly once");
    for question in questions {
        for condition in array(&question["when"]) {
            assert!(
                round_of[text(&condition["question"])] < round_of[text(&question["id"])],
                "{} must come after {}",
                question["id"],
                condition["question"]
            );
        }
    }
}

#[test]
fn rounds_split_many_questions_into_fours() {
    let screens = (0..11)
        .map(|index| screen(&format!("8:{index}"), &format!("Page{index}"), 1440.0))
        .collect::<Vec<_>>();
    let plan = draft_plan(&screens, &BTreeMap::new(), &evidence(), json!({}), None).unwrap();
    let rounds = question_rounds(array(&plan["questions"]), MAX_QUESTIONS_PER_ROUND);
    assert_eq!(rounds.len(), 3);
    assert_rounds_cover(&plan);
    // A runtime with a larger cap gets fewer, larger rounds of the same
    // questions; one with no cap asks the flat list.
    let wide = question_rounds(array(&plan["questions"]), 10);
    assert_eq!(wide.len(), 2);
    assert_eq!(array(&wide[0]["questions"]).len(), 10);
    assert_eq!(question_rounds(array(&plan["questions"]), 50).len(), 1);
}

/// Frames of one name in different sections are never resolved here: the
/// question states the sections, frames, widths and ids, and offers a Figma
/// fix. Separate pages are recommended unless every section names a device.
#[test]
fn sections_are_asked_explicitly_with_a_fix_option() {
    let plan = draft_plan(
        &[
            sectioned("9:1", "About", 1440.0, "Company"),
            sectioned("9:2", "About", 390.0, "Recruit"),
            sectioned("9:3", "Pricing", 1440.0, "PC"),
            sectioned("9:4", "Pricing", 390.0, "Mobile"),
        ],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    let company = find(&plan, "sections:9:1");
    assert_eq!(
        first_label(company),
        "Separate pages: the sections are different pages (Recommended)"
    );
    let device = find(&plan, "sections:9:3");
    assert_eq!(
        first_label(device),
        "One responsive page: the sections only split by device (Recommended)"
    );
    let prompt = text(&device["question"]);
    for fact in ["PC", "Mobile", "9:3", "9:4", "1440px", "390px", "Pricing"] {
        assert!(prompt.contains(fact), "{fact} missing from {prompt}");
    }
    assert!(
        text(&device["options"][0]["description"])
            .contains("Inferred from the section names (PC, Mobile)")
    );
    let labels = array(&device["options"])
        .iter()
        .map(|option| text(&option["label"]))
        .collect::<Vec<_>>();
    assert!(labels.contains(&FIX_FIGMA), "{labels:?}");
    assert!(
        !question_ids(&plan)
            .iter()
            .any(|id| id.starts_with("group:"))
    );
    let split = find(&plan, "split-route:9:2");
    assert!(
        array(&split["options"])
            .iter()
            .any(|option| text(&option["label"]).starts_with("/recruit/about")),
        "{split}"
    );
    assert_rounds_cover(&plan);
}

#[test]
fn project_config_counts_as_evidence_and_suppresses_its_questions() {
    let mut evidence = evidence();
    evidence.config = PlanConfig {
        modal_keywords: Some(vec!["sheet".into()]),
        route_aliases: BTreeMap::from([("요금제".to_owned(), "/pricing".to_owned())]),
        width_grouping: Some("responsive".into()),
        ..answered_site()
    };
    let plan = draft_plan(
        &[
            screen("10:1", "요금제 - 1440", 1440.0),
            screen("10:2", "요금제 - 390", 390.0),
            screen("10:3", "Share sheet", 400.0),
            screen("10:4", "Login popup", 400.0),
        ],
        &BTreeMap::new(),
        &evidence,
        json!({}),
        None,
    )
    .unwrap();
    let rows = array(&plan["screens"]);
    assert_eq!(rows[0]["drawnWidths"], json!([1440, 390]));
    assert_eq!(rows[0]["grouping"]["evidence"], "config:widthGrouping");
    assert_eq!(rows[0]["proposedRoute"], "/pricing");
    assert!(text(&rows[0]["routeEvidence"]).contains("routeAliases"));
    assert_eq!(rows[1]["kind"], "modal");
    assert_eq!(rows[1]["kindEvidence"], "config");
    // The project's keywords replace the built-in ones.
    assert_eq!(rows[2]["kind"], "page");
    let ids = question_ids(&plan);
    assert!(
        !ids.iter().any(|id| id.starts_with("group:")
            || id.starts_with("route:10:1")
            || id == "heuristics"),
        "{ids:?}"
    );
}

#[test]
fn a_frame_id_already_in_a_page_is_evidence() {
    let mut evidence = evidence();
    evidence.figma_nodes.insert(
        "11:1".into(),
        ("/company".into(), "src/app/company/page.tsx".into()),
    );
    let plan = draft_plan(
        &[screen("11:1", "Intro", 1440.0)],
        &BTreeMap::new(),
        &evidence,
        json!({}),
        None,
    )
    .unwrap();
    assert_eq!(plan["screens"][0]["proposedRoute"], "/company");
    assert!(text(&plan["screens"][0]["routeEvidence"]).contains("data-figma-node-id"));
    assert_eq!(question_ids(&plan), ["collision:11:1"]);
    assert_eq!(
        figma_node_ids("<Box data-figma-node-id=\"1:2\" /><Text data-figma-node-id='3:4' />"),
        ["1:2", "3:4"]
    );
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
    assert!(
        text(&report["summary"]).starts_with(
            "2 URLs: 1 implemented, 0 verified at all widths, /about overflows at 390"
        ),
        "{}",
        report["summary"]
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

fn answers(pairs: &[(&str, &str)]) -> BTreeMap<String, AnswerInput> {
    pairs
        .iter()
        .map(|(id, answer)| ((*id).to_owned(), AnswerInput::One((*answer).to_owned())))
        .collect()
}

async fn drafted(fixture: &Fixture, screens: &[PlanScreenInput]) -> ScreenPlanInput {
    let plan = draft(
        &input(fixture, "draft"),
        screens,
        json!({}),
        &fixture.policy(),
    )
    .await
    .unwrap();
    let mut confirm_input = input(fixture, "confirm");
    confirm_input.plan_id = plan["planId"].as_str().map(str::to_owned);
    confirm_input
}

fn main_widths() -> [PlanScreenInput; 2] {
    [
        screen("12:1", "Main - 1440", 1440.0),
        screen("12:2", "Main - 390", 390.0),
    ]
}

/// Separate pages bring their own route questions; the answers decide the
/// URLs, and nothing is derived from a width.
#[tokio::test]
async fn separate_pages_need_a_route_per_frame_and_get_exactly_those() {
    let fixture = Fixture::new();
    let mut confirm_input = drafted(&fixture, &main_widths()).await;
    confirm_input.answers = answers(&[
        ("group:12:1", "Separate pages"),
        ("split-route:12:1", "/"),
        ("heuristics", "Accept all (Recommended)"),
    ]);
    let error = confirm(&confirm_input, &fixture.policy()).unwrap_err();
    assert_eq!(error.details["missing"], json!(["split-route:12:2"]));
    assert!(error.details["questionRounds"][0]["questions"][0]["id"] == "split-route:12:2");

    confirm_input
        .answers
        .insert("split-route:12:2".into(), AnswerInput::One("/m".into()));
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    let urls = array(&confirmed["routes"])
        .iter()
        .map(|row| (row["url"].clone(), row["nodeIds"].clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        urls,
        [
            (json!("/"), json!(["12:1"])),
            (json!("/m"), json!(["12:2"]))
        ]
    );
}

#[tokio::test]
async fn one_responsive_page_skips_the_split_routes_and_accepts_the_inferred_route() {
    let fixture = Fixture::new();
    let mut confirm_input = drafted(&fixture, &main_widths()).await;
    confirm_input.answers = answers(&[
        ("group:12:1", "One responsive page (Recommended)"),
        ("heuristics", "Accept all (Recommended)"),
    ]);
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    let route = &confirmed["routes"][0];
    assert_eq!(route["url"], "/");
    assert_eq!(route["nodeIds"], json!(["12:1", "12:2"]));
    assert_eq!(route["collisionPolicy"], "merge");
    assert_eq!(route["routeEvidence"], "name-match (accepted)");
}

#[tokio::test]
async fn reviewing_heuristics_asks_each_one_in_the_same_batch() {
    let fixture = Fixture::new();
    let mut confirm_input = drafted(&fixture, &main_widths()).await;
    confirm_input.answers = answers(&[
        ("group:12:1", "One responsive page (Recommended)"),
        ("heuristics", "Review individually"),
    ]);
    let error = confirm(&confirm_input, &fixture.policy()).unwrap_err();
    assert_eq!(error.details["missing"], json!(["review:12:1"]));
    confirm_input
        .answers
        .insert("review:12:1".into(), AnswerInput::One("/home".into()));
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    assert_eq!(confirmed["routes"][0]["url"], "/home");
}

#[tokio::test]
async fn a_custom_group_answer_can_name_one_route_per_frame() {
    let fixture = Fixture::new();
    let mut confirm_input = drafted(&fixture, &main_widths()).await;
    confirm_input.answers = answers(&[
        ("group:12:1", "/desktop, /mobile"),
        ("heuristics", "Accept all (Recommended)"),
    ]);
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    let urls = array(&confirmed["routes"])
        .iter()
        .map(|row| row["url"].clone())
        .collect::<Vec<_>>();
    assert_eq!(urls, [json!("/desktop"), json!("/mobile")]);
}

/// After confirm nothing is asked: what implementation decides on its own is
/// recorded, and status and the final table carry it.
#[tokio::test]
async fn assumptions_are_recorded_after_confirm_and_reported_by_status() {
    let fixture = Fixture::new();
    let mut confirm_input = drafted(&fixture, &[screen("13:1", "Pricing", 1440.0)]).await;
    let mut assume_input = input(&fixture, "assume");
    assume_input.assumptions = vec![AssumptionInput {
        about: "13:9".into(),
        decision: "Not built: outside the confirmed screen plan.".into(),
        reason: Some("found during implementation".into()),
    }];
    let early = assume(&assume_input, &fixture.policy()).unwrap_err();
    assert!(early.message.contains("after confirm"));

    confirm_input.answers = answers(&[("route:13:1", "/pricing (Recommended)")]);
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    assert!(text(&confirmed["rule"]).contains("without asking"));
    let recorded = assume(&assume_input, &fixture.policy()).unwrap();
    assert_eq!(recorded["assumptions"][0]["about"], "13:9");

    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let report = status_report(&plan, &evidence(), None);
    assert_eq!(
        report["assumptions"][0]["decision"],
        "Not built: outside the confirmed screen plan."
    );
    assert!(text(&report["summary"]).contains("; 1 assumption made during implementation"));
    assert!(text(&report["markdownTable"]).contains("- 13:9: Not built"));
    let markdown = std::fs::read_to_string(fixture.0.join(".devup/screen-plan.md")).unwrap();
    assert!(markdown.contains("Assumptions made during implementation:"));

    let mut uncovered = export(&fixture, &["13:9"]);
    uncovered.scaffold_write = true;
    uncovered.scaffold_route = Some("/x");
    let Gate::Blocked(response) = export_gate(&uncovered).unwrap() else {
        panic!("a frame outside the confirmed plan is not written");
    };
    assert_eq!(response["nextAction"]["arguments"]["action"], "assume");
    assert!(text(&response["nextAction"]["how"]).contains("Do not ask the user"));
}

#[tokio::test]
async fn an_invalid_config_is_refused_rather_than_ignored() {
    let fixture = Fixture::new();
    fixture.write(CONFIG_FILE, r#"{"widthGrouping": "sometimes"}"#);
    let error = project_evidence(fixture.root()).await.unwrap_err();
    assert!(error.message.contains("widthGrouping"));
    fixture.write(
        CONFIG_FILE,
        r#"{"routeAliases": {"메인": "/"}, "widthGrouping": "ask", "siteName": "Prime"}"#,
    );
    let evidence = project_evidence(fixture.root()).await.unwrap();
    assert_eq!(evidence.config_file.as_deref(), Some(CONFIG_FILE));
    assert_eq!(evidence.config.route_aliases["메인"], "/");
}

fn in_trash(node_id: &str, name: &str, page: &str, area: &str) -> PlanScreenInput {
    PlanScreenInput {
        section: Some(format!("section-{area}")),
        section_name: Some(area.into()),
        breadcrumb: vec![page.into(), area.into(), name.into()],
        ..screen(node_id, name, 1440.0)
    }
}

fn page_screen(node_id: &str, name: &str, page: &str, area: &str) -> PlanScreenInput {
    in_trash(node_id, name, page, area)
}

/// Picking the Figma fix marks the group: its frames are not built, the rest
/// goes on, and status reports exactly what to move or rename.
#[tokio::test]
async fn a_requested_design_fix_is_reported_not_built() {
    let fixture = Fixture::new();
    let mut confirm_input = drafted(
        &fixture,
        &[
            sectioned("14:1", "Pricing", 1440.0, "PC"),
            sectioned("14:2", "Pricing", 390.0, "Mobile"),
            sectioned("14:3", "Careers", 1440.0, "PC"),
        ],
    )
    .await;
    confirm_input.answers = answers(&[
        ("sections:14:1", FIX_FIGMA),
        ("route:14:3", "/careers (Recommended)"),
    ]);
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    let built = array(&confirmed["routes"])
        .iter()
        .flat_map(|row| array(&row["nodeIds"]).to_vec())
        .collect::<Vec<_>>();
    assert_eq!(built, [json!("14:3")]);
    let fix = &confirmed["designFixes"][0];
    assert_eq!(fix["question"], "sections:14:1");
    assert_eq!(array(&fix["frames"]).len(), 2);
    assert!(text(&fix["fix"]).contains("PC, Mobile"), "{fix}");

    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let report = status_report(&plan, &evidence(), None);
    assert!(text(&report["summary"]).contains("1 design fix requested, not built"));
    let table = text(&report["markdownTable"]);
    assert!(
        table.contains("Design fixes requested (not built):"),
        "{table}"
    );
    assert!(table.contains("14:1") && table.contains("14:2"), "{table}");
    let markdown = std::fs::read_to_string(fixture.0.join(".devup/screen-plan.md")).unwrap();
    assert!(markdown.contains("Design fixes requested"), "{markdown}");

    let mut fixed = export(&fixture, &["14:2"]);
    fixed.scaffold_write = true;
    let Gate::Blocked(response) = export_gate(&fixed).unwrap() else {
        panic!("a frame marked for a design fix is never written");
    };
    assert_eq!(response["status"], "refused");
}

/// Trash, archive and copies are never built and never silently dropped.
#[test]
fn trash_and_archive_frames_are_excluded_and_asked_about_once() {
    let plan = draft_plan(
        &[
            page_screen("15:1", "Home", "Site", "Pages"),
            in_trash("15:2", "Home", "Site", "휴지통"),
            in_trash("15:3", "About", "Site", "Archive"),
            page_screen("15:4", "Home copy", "Site", "Pages"),
        ],
        &BTreeMap::new(),
        &evidence(),
        json!({"nodeId": "page-1", "anchorName": "Site", "anchorBreadcrumb": ["Site"]}),
        None,
    )
    .unwrap();
    let excluded = array(&plan["excluded"]);
    let ids = excluded
        .iter()
        .map(|row| text(&row["nodeId"]))
        .collect::<Vec<_>>();
    assert_eq!(ids, ["15:2", "15:3", "15:4"]);
    assert!(text(&excluded[0]["reason"]).contains("휴지통"));
    assert!(text(&excluded[1]["reason"]).contains("\"archive\""));
    assert!(text(&excluded[2]["reason"]).contains("\"copy\""));
    let question = find(&plan, "exclusions");
    assert_eq!(first_label(question), "Accept the exclusions (Recommended)");
    for id in ["15:2", "15:3", "15:4"] {
        assert!(text(&question["question"]).contains(id));
    }
    assert_eq!(array(&plan["screens"]).len(), 1);
    assert!(!question_ids(&plan).iter().any(|id| id == "route:15:3"));
    assert_rounds_cover(&plan);
}

#[test]
fn the_config_can_change_and_override_the_exclusion_words() {
    let mut evidence = evidence();
    evidence.config.exclude_keywords = Some(vec!["legacy".into()]);
    evidence.config.include_keywords = None;
    evidence.config.include_keywords = Some(vec!["test drive".into()]);
    let plan = draft_plan(
        &[
            page_screen("16:1", "Archive", "Site", "Pages"),
            page_screen("16:2", "Legacy home", "Site", "Pages"),
            page_screen("16:3", "Test drive", "Site", "Legacy"),
        ],
        &BTreeMap::new(),
        &evidence,
        json!({}),
        None,
    )
    .unwrap();
    let excluded = array(&plan["excluded"])
        .iter()
        .map(|row| text(&row["nodeId"]))
        .collect::<Vec<_>>();
    assert_eq!(excluded, ["16:2"]);
}

/// A link that itself points into an archive is asked once, not excluded.
#[test]
fn a_link_into_an_archive_is_asked_once_instead_of_excluded() {
    let plan = draft_plan(
        &[in_trash("17:1", "Home", "Site", "Archive")],
        &BTreeMap::new(),
        &evidence(),
        json!({"nodeId": "section-Archive", "anchorName": "Archive", "anchorBreadcrumb": ["Site", "Archive"]}),
        None,
    )
    .unwrap();
    assert!(array(&plan["excluded"]).is_empty());
    let scope = find(&plan, "scope");
    assert!(text(&scope["question"]).contains("Archive"));
    assert_eq!(find(&plan, "heuristics")["when"][0]["question"], "scope");
}

/// Never wider than the link: a frame whose ancestry does not reach the
/// linked node is not a candidate.
#[test]
fn frames_outside_the_linked_node_are_never_candidates() {
    let plan = draft_plan(
        &[
            page_screen("18:1", "About", "Site", "Company"),
            page_screen("18:2", "Pricing", "Site", "Sales"),
        ],
        &BTreeMap::new(),
        &evidence(),
        json!({"nodeId": "section-Company", "anchorName": "Company", "anchorBreadcrumb": ["Site", "Company"]}),
        None,
    )
    .unwrap();
    let rows = array(&plan["screens"]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "18:1");
    assert_eq!(plan["outOfScope"][0]["nodeId"], "18:2");
    assert!(!plan["questions"].to_string().contains("18:2"));
    // Explore's own answer maps the same way: a frame straight on the page
    // is in no section, and ancestry keeps it in or out of scope.
    let on_page = screen_from_explore(&json!({"nodeId": "19:1", "name": "Loose", "parentId": "0:1",
        "bounds": {"width": 1440.0, "height": 900.0}, "breadcrumb": ["Site", "Loose"], "visible": true}))
    .unwrap();
    assert_eq!(on_page.section, None);
    assert!(in_scope(&on_page, Some("0:1"), Some("Site")));
    assert!(!in_scope(
        &on_page,
        Some("section-Company"),
        Some("Company")
    ));
    let hidden = screen_from_explore(&json!({"nodeId": "19:2", "name": "Old", "parentId": "s",
        "bounds": {"width": 1440.0}, "breadcrumb": ["Site", "Pages", "Old"], "visible": false}))
    .unwrap();
    assert_eq!(hidden.section.as_deref(), Some("s"));
    assert_eq!(
        exclusion_reason(&hidden, &[], &PlanConfig::default()).as_deref(),
        Some("hidden in Figma")
    );
}

#[test]
fn other_structural_oddities_are_flagged_with_the_same_fix_option() {
    let plan = draft_plan(
        &[
            sectioned("20:1", "Notice", 1440.0, "Board"),
            sectioned("20:2", "Notice", 1440.0, "Board"),
            sectioned("20:3", "Guide 1", 1440.0, "Help"),
            sectioned("20:4", "Guide 2", 1440.0, "Support"),
            screen("20:5", "Contact", 1440.0),
        ],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    for id in [
        "structure:duplicate:20:1",
        "structure:split:20:3",
        "structure:loose:20:5",
    ] {
        let question = find(&plan, id);
        let labels = array(&question["options"])
            .iter()
            .map(|option| text(&option["label"]))
            .collect::<Vec<_>>();
        assert!(labels.contains(&FIX_FIGMA), "{id}: {labels:?}");
    }
    // The duplicates' own route questions wait on the structure answer.
    let route = find(&plan, "route:20:2");
    assert!(
        array(&route["when"])
            .iter()
            .any(|condition| condition["question"] == "structure:duplicate:20:1"),
        "{route}"
    );
    assert_rounds_cover(&plan);
}

#[tokio::test]
async fn the_gate_refuses_an_excluded_frame_even_with_an_assumption() {
    let fixture = Fixture::new();
    let plan = draft(
        &input(&fixture, "draft"),
        &[
            page_screen("21:1", "Pricing", "Site", "Pages"),
            in_trash("21:2", "Pricing", "Site", "Trash"),
        ],
        json!({}),
        &fixture.policy(),
    )
    .await
    .unwrap();
    let mut confirm_input = input(&fixture, "confirm");
    confirm_input.plan_id = plan["planId"].as_str().map(str::to_owned);
    confirm_input.answers = answers(&[
        ("route:21:1", "/pricing (Recommended)"),
        ("exclusions", "Accept the exclusions (Recommended)"),
    ]);
    confirm(&confirm_input, &fixture.policy()).unwrap();
    let mut assume_input = input(&fixture, "assume");
    assume_input.assumptions = vec![AssumptionInput {
        about: "21:2".into(),
        decision: "Build it as /pricing-old".into(),
        reason: None,
    }];
    let recorded = assume(&assume_input, &fixture.policy()).unwrap();
    assert_eq!(recorded["stillNotBuilt"][0]["frame"], "21:2");
    let mut excluded = export(&fixture, &["21:2"]);
    excluded.scaffold_write = true;
    let Gate::Blocked(response) = export_gate(&excluded).unwrap() else {
        panic!("an excluded frame is never written");
    };
    assert_eq!(response["status"], "refused");
    assert!(text(&response["refused"][0]["reason"]).starts_with("excluded"));
}

fn bare_site() -> ProjectEvidence {
    ProjectEvidence {
        config: PlanConfig::default(),
        ..evidence()
    }
}

fn labels(question: &Value) -> Vec<&str> {
    array(&question["options"])
        .iter()
        .map(|option| text(&option["label"]))
        .collect()
}

/// The favicon and share image are files only the user can pick, so they
/// are asked - with the candidates the project and the linked Figma scope
/// actually hold, and never a generated one.
#[test]
fn site_files_are_asked_with_evidence_backed_candidates() {
    let mut evidence = bare_site();
    evidence.icon_files = vec!["src/app/icon.svg".into()];
    evidence.package_name = Some("prime-web".into());
    let plan = draft_plan_with(
        &[
            screen("30:3", "Home", 1440.0),
            screen("30:2", "OG image", 1200.0),
        ],
        &[PlanAssetInput {
            node_id: "30:1".into(),
            name: "Logo".into(),
            ..PlanAssetInput::default()
        }],
        &[],
        &BTreeMap::new(),
        &evidence,
        json!({}),
        None,
    )
    .unwrap();
    let favicon = find(&plan, "site:favicon");
    assert_eq!(
        labels(favicon),
        [
            "Use src/app/icon.svg (Recommended)",
            "Export \"Logo\" (30:1) as SVG",
            "I will provide a file",
            "Leave it for later"
        ]
    );
    assert!(text(&favicon["options"][2]["description"]).contains("Nothing is generated or drawn"));
    let og = find(&plan, "site:og");
    assert_eq!(
        labels(og)[..2],
        [
            "Export \"OG image\" (30:2) as PNG (Recommended)",
            "Use the top of \"Home\" (30:3)"
        ]
    );
    // A frame named like a share image is an asset, not a page.
    assert_eq!(array(&plan["screens"]).len(), 1);
    assert_eq!(
        labels(find(&plan, "site:name"))[0],
        "prime-web (Recommended)"
    );
    assert_eq!(labels(find(&plan, "site:motion"))[0], "None (Recommended)");
    assert!(!question_ids(&plan).contains(&"site:analytics".to_owned()));
    let defaults = array(&plan["defaults"])
        .iter()
        .map(|default| text(&default["item"]))
        .collect::<Vec<_>>();
    for item in [
        "interaction-recipes",
        "view-transitions",
        "transition",
        "reduced-motion",
        "metadata",
        "lang",
        "alt-text",
        "not-found",
        "image-dimensions",
    ] {
        assert!(defaults.contains(&item), "{item} missing from {defaults:?}");
    }
    assert_eq!(plan["site"]["favicon"]["state"], "asked");
    assert_rounds_cover(&plan);
}

#[test]
fn the_config_and_the_project_pre_answer_the_site_questions() {
    let plan = draft_plan(
        &[screen("31:1", "Home", 1440.0)],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    assert!(
        !question_ids(&plan).iter().any(|id| id.starts_with("site:")),
        "{:?}",
        question_ids(&plan)
    );
    assert_eq!(plan["site"]["siteName"]["state"], "config");

    let mut found = bare_site();
    found.site_name = Some((
        "Prime".into(),
        "title template in src/app/layout.tsx".into(),
    ));
    found.site_url = Some((
        "https://prime.example".into(),
        "NEXT_PUBLIC_SITE_URL in .env".into(),
    ));
    found.analytics = vec!["@vercel/analytics in package.json".into()];
    let plan = draft_plan(
        &[screen("31:1", "Home", 1440.0)],
        &BTreeMap::new(),
        &found,
        json!({}),
        None,
    )
    .unwrap();
    let ids = question_ids(&plan);
    assert!(
        !ids.contains(&"site:name".to_owned()) && !ids.contains(&"site:url".to_owned()),
        "{ids:?}"
    );
    assert_eq!(
        plan["site"]["siteUrl"]["evidence"],
        "NEXT_PUBLIC_SITE_URL in .env"
    );
    let analytics = find(&plan, "site:analytics");
    assert!(text(&analytics["question"]).contains("@vercel/analytics"));
}

#[tokio::test]
async fn project_evidence_reads_the_site_facts() {
    let fixture = Fixture::new();
    fixture.write("public/favicon.ico", "ico");
    fixture.write(
        ".env",
        "NEXT_PUBLIC_SITE_URL=https://prime.example\nNEXT_PUBLIC_GA_ID=G-1\n",
    );
    fixture.write(
        "src/app/layout.tsx",
        "export const metadata = { title: { template: '%s | Prime' } };\nexport default function RootLayout({ children }) { return <html lang=\"ko\"><body>{children}</body></html>; }",
    );
    let evidence = project_evidence(fixture.root()).await.unwrap();
    assert_eq!(evidence.icon_files, ["public/favicon.ico"]);
    assert_eq!(
        evidence.site_url.as_ref().unwrap().0,
        "https://prime.example"
    );
    assert_eq!(evidence.site_name.as_ref().unwrap().0, "Prime");
    assert_eq!(evidence.lang.as_ref().unwrap().0, "ko");
    assert!(
        evidence
            .analytics
            .iter()
            .any(|sign| sign.contains("NEXT_PUBLIC_GA_ID"))
    );
    assert!(evidence.not_found.is_none());
}

/// Status says, per item, whether it is filled, decided but not in place,
/// or missing - read from the files as they are now.
#[tokio::test]
async fn status_reports_site_completeness() {
    let fixture = Fixture::new();
    let mut confirm_input = drafted(&fixture, &[screen("32:1", "Company", 1440.0)]).await;
    confirm_input.answers = answers(&[("heuristics", "Accept all (Recommended)")]);
    confirm(&confirm_input, &fixture.policy()).unwrap();
    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let evidence = project_evidence(fixture.root()).await.unwrap();
    let report = status_report(&plan, &evidence, None);
    let item = |name: &str| {
        array(&report["siteCompleteness"])
            .iter()
            .find(|item| item["item"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} missing: {}", report["siteCompleteness"]))
    };
    assert_eq!(item("favicon")["state"], "asked-and-answered");
    assert_eq!(item("metadata /company")["state"], "missing");
    // The fixture header has two bare links in a nav.
    assert_eq!(item("interaction-roles")["state"], "missing");
    assert_eq!(
        item("interaction-roles")["detail"],
        "nav items 0/2 hover+focus+aria-current"
    );
    // App Router pages but no next in package.json: skipped, and said why.
    assert_eq!(item("view-transitions")["state"], "not-applicable");
    assert!(
        text(&item("view-transitions")["detail"]).starts_with("skipped: App Router pages exist")
    );
    assert_eq!(item("site-name")["state"], "filled");
    let table = text(&report["markdownTable"]);
    assert!(table.contains("Site completeness:"), "{table}");
    assert!(
        table.contains("Filled in by default (no question):"),
        "{table}"
    );

    fixture.write("src/app/icon.svg", "<svg/>");
    fixture.write(
        "src/app/company/page.tsx",
        "export const metadata = { title: 'Company' };\nexport default function Page() { return <main />; }",
    );
    let evidence = project_evidence(fixture.root()).await.unwrap();
    let report = status_report(&plan, &evidence, None);
    let states = array(&report["siteCompleteness"])
        .iter()
        .map(|item| {
            (
                text(&item["item"]).to_owned(),
                text(&item["state"]).to_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(states["favicon"], "filled");
    assert_eq!(states["metadata /company"], "filled");
}

#[test]
fn a_missing_app_icon_is_a_warning() {
    let fixture = Fixture::new();
    let finding = missing_app_icon(fixture.root()).expect("the fixture app has no icon");
    assert_eq!(finding.rule, "app-icon");
    fixture.write("src/app/favicon.ico", "ico");
    assert!(missing_app_icon(fixture.root()).is_none());
}

fn with_package(package: &str, lock: Option<&str>) -> Fixture {
    let fixture = Fixture::new();
    fixture.write("package.json", package);
    if let Some(lock) = lock {
        fixture.write("package-lock.json", lock);
    }
    fixture
}

async fn transition_default(fixture: &Fixture) -> Value {
    let evidence = project_evidence(fixture.root()).await.unwrap();
    let plan = draft_plan(
        &[screen("40:1", "Home", 1440.0)],
        &BTreeMap::new(),
        &evidence,
        json!({}),
        None,
    )
    .unwrap();
    array(&plan["defaults"])
        .iter()
        .find(|default| default["item"] == "view-transitions")
        .cloned()
        .expect("a view-transitions default")
}

/// The mechanism follows the versions the project declares, never a guess:
/// Next 16 with React 19.3 needs no flag, Next 15.2+ only has the
/// experimental one, older App Routers get none, and an existing page-motion
/// library turns the default into a question.
#[tokio::test]
async fn view_transitions_follow_the_project_versions() {
    let supported = with_package(
        r#"{"dependencies": {"next": "^16.1.0", "react": "19.3.0"}}"#,
        None,
    );
    let chosen = transition_default(&supported).await;
    assert_eq!(chosen["mechanism"], "react-view-transition");
    assert!(text(&chosen["decision"]).contains("import { ViewTransition } from 'react'"));
    assert!(text(&chosen["decision"]).contains("prefers-reduced-motion"));

    let experimental = with_package(
        r#"{"dependencies": {"next": "15.3.0", "react": "19.1.0"}}"#,
        None,
    );
    let chosen = transition_default(&experimental).await;
    assert_eq!(chosen["mechanism"], "react-unstable-view-transition");
    assert!(text(&chosen["decision"]).contains("experimental.viewTransition: true"));

    let old = with_package(
        r#"{"dependencies": {"next": "^14.2.0", "react": "^18.3.0"}}"#,
        None,
    );
    let chosen = transition_default(&old).await;
    assert_eq!(chosen["mechanism"], "skipped");
    assert!(text(&chosen["evidence"]).contains("one document"));

    // The lockfile's installed version wins over the declared range.
    let locked = with_package(
        r#"{"dependencies": {"next": "latest", "react": "latest"}}"#,
        Some(
            r#"{"packages": {"node_modules/next": {"version": "16.2.1"}, "node_modules/react": {"version": "19.3.2"}}}"#,
        ),
    );
    assert_eq!(
        transition_default(&locked).await["mechanism"],
        "react-view-transition"
    );

    let unknown = with_package(r#"{"dependencies": {"next": "latest"}}"#, None);
    assert_eq!(transition_default(&unknown).await["mechanism"], "skipped");

    let animated = with_package(
        r#"{"dependencies": {"next": "16.1.0", "react": "19.3.0", "framer-motion": "^11.0.0"}}"#,
        None,
    );
    let chosen = transition_default(&animated).await;
    assert_eq!(chosen["mechanism"], "asked");
    assert_eq!(chosen["ifAdded"]["mechanism"], "react-view-transition");
    let evidence = project_evidence(animated.root()).await.unwrap();
    let plan = draft_plan(
        &[screen("40:1", "Home", 1440.0)],
        &BTreeMap::new(),
        &evidence,
        json!({}),
        None,
    )
    .unwrap();
    assert_eq!(
        first_label(find(&plan, "site:view-transitions")),
        "Keep framer-motion only (Recommended)"
    );
}

#[test]
fn the_recipe_default_names_every_role_and_the_restraint_rules() {
    let plan = draft_plan(
        &[screen("41:1", "Home", 1440.0)],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    let recipes = array(&plan["defaults"])
        .iter()
        .find(|default| default["item"] == "interaction-recipes")
        .cloned()
        .unwrap();
    let roles = array(&recipes["recipes"])
        .iter()
        .map(|row| text(&row["role"]))
        .collect::<Vec<_>>();
    for role in [
        "primary button",
        "card that links",
        "nav item / tab",
        "icon button",
        "input / select / textarea",
    ] {
        assert!(roles.contains(&role), "{role} missing from {roles:?}");
    }
    let restraint = text(&recipes["restraint"]);
    for rule in [
        "at most two properties",
        "150-250ms",
        "0.97 and 1.05",
        "_motionReduce",
        "no hover",
    ] {
        assert!(restraint.contains(rule), "{rule} missing");
    }
}

/// The agent's own questions go into the same up-front rounds, and their
/// answers come back from confirm.
#[tokio::test]
async fn extra_questions_are_asked_up_front_and_answered_at_confirm() {
    let fixture = Fixture::new();
    let mut draft_input = input(&fixture, "draft");
    draft_input.extra_questions = vec![
        ExtraQuestionInput {
            id: "contact-target".into(),
            header: "Contact form target".into(),
            question: "Where should the contact form send its messages?".into(),
            options: vec![
                ExtraOptionInput {
                    label: "Email to the address in the footer (Recommended)".into(),
                    description: "A server action sending to the footer address.".into(),
                },
                ExtraOptionInput {
                    label: "An existing API endpoint".into(),
                    description: "Named in openapi.json.".into(),
                },
            ],
            multiple: false,
        },
        ExtraQuestionInput {
            id: "legal".into(),
            header: "Legal pages".into(),
            question: "Which legal pages does the site need?".into(),
            options: vec![
                ExtraOptionInput {
                    label: "Privacy policy".into(),
                    description: "/privacy".into(),
                },
                ExtraOptionInput {
                    label: "Terms of service".into(),
                    description: "/terms".into(),
                },
            ],
            multiple: true,
        },
    ];
    let drafted = draft(
        &draft_input,
        &[screen("42:1", "Pricing", 1440.0)],
        json!({}),
        &fixture.policy(),
    )
    .await
    .unwrap();
    let ids = array(&drafted["questions"])
        .iter()
        .map(|q| text(&q["id"]))
        .collect::<Vec<_>>();
    assert!(
        ids.contains(&"extra:contact-target") && ids.contains(&"extra:legal"),
        "{ids:?}"
    );
    let contact = array(&drafted["questions"])
        .iter()
        .find(|q| q["id"] == "extra:contact-target")
        .unwrap();
    assert_eq!(
        first_label(contact),
        "Email to the address in the footer (Recommended)"
    );

    let mut confirm_input = input(&fixture, "confirm");
    confirm_input.plan_id = drafted["planId"].as_str().map(str::to_owned);
    confirm_input.answers = answers(&[
        ("route:42:1", "/pricing (Recommended)"),
        ("extra:contact-target", "contact@prime.example"),
    ]);
    confirm_input.answers.insert(
        "extra:legal".into(),
        AnswerInput::Many(vec!["Privacy policy".into(), "Terms of service".into()]),
    );
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    assert_eq!(
        confirmed["extraAnswers"]["extra:contact-target"],
        "contact@prime.example"
    );
    assert_eq!(
        confirmed["extraAnswers"]["extra:legal"],
        json!(["Privacy policy", "Terms of service"])
    );

    draft_input.extra_questions[0].header = "A header far longer than thirty characters".into();
    let error = draft(
        &draft_input,
        &[screen("42:1", "Pricing", 1440.0)],
        json!({}),
        &fixture.policy(),
    )
    .await
    .unwrap_err();
    assert!(error.message.contains("header"), "{}", error.message);
}
