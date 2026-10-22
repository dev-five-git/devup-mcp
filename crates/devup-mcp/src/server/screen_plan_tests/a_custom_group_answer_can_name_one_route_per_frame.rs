

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
    // Always planned; missing until the sources carry it.
    assert_eq!(item("view-transitions")["state"], "missing");
    assert_eq!(
        item("view-transitions")["detail"],
        "planned (react-view-transition) but not in the sources yet"
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

/// View transitions are always on: the dependency step brings every project
/// to the latest Next and React, so no version is checked. Only an explicit
/// opt-out, or a project that forbids the update while on an old React,
/// skips them; an existing page-motion library makes them a question.
#[tokio::test]
async fn view_transitions_are_always_on_unless_the_project_opts_out() {
    for package in [
        r#"{"dependencies": {"next": "^16.1.0", "react": "19.3.0"}}"#,
        r#"{"dependencies": {"next": "15.3.0", "react": "19.1.0"}}"#,
        r#"{"dependencies": {"next": "^14.2.0", "react": "^18.3.0"}}"#,
        r#"{"dependencies": {"next": "latest"}}"#,
    ] {
        let fixture = with_package(package, None);
        let chosen = transition_default(&fixture).await;
        assert_eq!(
            chosen["mechanism"], "react-view-transition",
            "{package}: {chosen}"
        );
        assert!(text(&chosen["decision"]).contains("import { ViewTransition } from 'react'"));
        assert!(text(&chosen["decision"]).contains("prefers-reduced-motion"));
        assert!(text(&chosen["decision"]).contains("view-transition check"));
        assert!(!array(&chosen["docs"]).is_empty());
    }

    let opted_out = with_package(
        r#"{"dependencies": {"next": "16.1.0", "react": "19.3.0"}}"#,
        None,
    );
    opted_out.write(
        CONFIG_FILE,
        &SITE_CONFIG.replace('}', r#", "viewTransitions": false}"#),
    );
    let chosen = transition_default(&opted_out).await;
    assert_eq!(chosen["mechanism"], "skipped");
    assert!(text(&chosen["evidence"]).contains("viewTransitions: false"));

    let frozen_old = with_package(
        r#"{"dependencies": {"next": "14.2.0", "react": "18.3.1"}}"#,
        None,
    );
    frozen_old.write(
        CONFIG_FILE,
        &SITE_CONFIG.replace('}', r#", "updateDependencies": false}"#),
    );
    assert_eq!(
        transition_default(&frozen_old).await["mechanism"],
        "skipped"
    );
    let frozen_new = with_package(
        r#"{"dependencies": {"next": "16.1.0", "react": "19.3.0"}}"#,
        None,
    );
    frozen_new.write(
        CONFIG_FILE,
        &SITE_CONFIG.replace('}', r#", "updateDependencies": false}"#),
    );
    assert_eq!(
        transition_default(&frozen_new).await["mechanism"],
        "react-view-transition"
    );

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

fn template_fixture() -> Fixture {
    let fixture = Fixture::new();
    for stale in ["package.json", "src"] {
        let path = fixture.0.join(stale);
        if path.is_dir() {
            std::fs::remove_dir_all(&path).unwrap();
        } else {
            std::fs::remove_file(&path).ok();
        }
    }
    crate::server::framework::tests::service_template(&fixture.0);
    fixture
}

fn default_item(plan: &Value, item: &str) -> Value {
    array(&plan["defaults"])
        .iter()
        .find(|default| default["item"] == item)
        .cloned()
        .unwrap_or_else(|| panic!("no {item} default: {}", plan["defaults"]))
}

async fn plan_for(root: &str) -> (ProjectEvidence, Value) {
    let evidence = project_evidence(root).await.unwrap();
    let plan = draft_plan(
        &[screen("50:1", "Home", 1440.0)],
        &BTreeMap::new(),
        &evidence,
        json!({}),
        None,
    )
    .unwrap();
    (evidence, plan)
}
#[tokio::test]
async fn assume_can_remap_a_planned_frame_to_another_url() {
    let fixture = Fixture::new();
    let mut confirm_input = drafted(
        &fixture,
        &[
            screen("13:1", "Pricing", 1440.0),
            screen("13:2", "Careers", 1440.0),
        ],
    )
    .await;
    confirm_input.answers = answers(&[
        ("route:13:1", "/pricing (Recommended)"),
        ("route:13:2", "/careers (Recommended)"),
    ]);
    confirm(&confirm_input, &fixture.policy()).unwrap();
    let mut remap = input(&fixture, "assume");
    remap.navigation_targets = BTreeMap::from([("13:2".to_owned(), "/pricing".to_owned())]);
    let recorded = assume(&remap, &fixture.policy()).unwrap();
    assert_eq!(recorded["remapped"][0]["to"], "/pricing");
    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let routes = array(&plan["routes"]);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0]["url"], "/pricing");
    assert_eq!(array(&routes[0]["nodeIds"]).len(), 2);
}