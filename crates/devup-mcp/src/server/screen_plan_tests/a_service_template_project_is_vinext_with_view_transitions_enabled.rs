

/// service-template: bun workspaces, vinext + React 19.3, no next
/// dependency. Its front app is an App Router app with view transitions
/// enabled, and the repository root, holding two apps, asks which one.
#[tokio::test]
async fn a_service_template_project_is_vinext_with_view_transitions_enabled() {
    let fixture = template_fixture();
    let front = fixture.0.join("apps").join("front");
    let (evidence, plan) = plan_for(front.to_str().unwrap()).await;
    assert_eq!(evidence.framework(), "vinext");
    let chosen = default_item(&plan, "view-transitions");
    assert_eq!(chosen["mechanism"], "react-view-transition", "{chosen}");
    assert!(text(&chosen["evidence"]).contains("vinext"), "{chosen}");
    assert!(
        array(&chosen["docs"])
            .iter()
            .any(|url| text(url).contains("vinext"))
    );
    assert!(!question_ids(&plan).contains(&"site:framework".to_owned()));

    let (evidence, plan) = plan_for(fixture.root()).await;
    assert_eq!(evidence.apps.len(), 2);
    assert!(evidence.target_app.is_none());
    // "Home" is a public-site screen: apps/front, without asking.
    assert_eq!(default_item(&plan, "target-app")["decision"], "apps/front");
    assert!(!question_ids(&plan).contains(&"site:app".to_owned()));

    let routes = super::super::project_context::run("routes", Some(fixture.root()), None)
        .await
        .unwrap();
    let apps = array(&routes["frameworks"]["apps"])
        .iter()
        .map(|app| {
            (
                text(&app["dir"]).to_owned(),
                text(&app["framework"]).to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        apps,
        [
            ("apps/admin".to_owned(), "next".to_owned()),
            ("apps/front".to_owned(), "vinext".to_owned())
        ]
    );
}

/// An existing project on the next CLI is asked once whether to migrate,
/// never migrated silently; the config can answer for it.
#[tokio::test]
async fn a_next_cli_project_is_asked_once_about_vinext() {
    let fixture = with_package(
        r#"{"dependencies": {"next": "16.1.0", "react": "19.3.0"}}"#,
        None,
    );
    let (_, plan) = plan_for(fixture.root()).await;
    let asked = find(&plan, "site:framework");
    assert_eq!(first_label(asked), "Migrate to vinext (Recommended)");
    assert!(text(&asked["question"]).contains("next 16.1.0"));
    assert_eq!(
        default_item(&plan, "view-transitions")["mechanism"],
        "react-view-transition"
    );

    fixture.write(
        CONFIG_FILE,
        r#"{"framework": "next", "favicon": "app/icon.svg", "ogImage": "public/og.png", "siteName": "Prime", "siteUrl": "https://example.com", "motion": "none"}"#,
    );
    let (_, plan) = plan_for(fixture.root()).await;
    assert!(!question_ids(&plan).contains(&"site:framework".to_owned()));
}

/// No app yet: start from the template without asking, ask only its name
/// and parts.
#[tokio::test]
async fn a_new_project_starts_from_the_template() {
    let root = std::env::temp_dir().join(format!("screen-plan-new-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let (_, plan) = plan_for(root.to_str().unwrap()).await;
    assert!(
        text(&default_item(&plan, "project-start")["decision"])
            .contains("dev-five-git/service-template")
    );
    assert_eq!(
        default_item(&plan, "view-transitions")["mechanism"],
        "react-view-transition"
    );
    let ids = question_ids(&plan);
    assert!(
        ids.contains(&"site:app-name".to_owned()) && ids.contains(&"site:parts".to_owned()),
        "{ids:?}"
    );
    assert_eq!(
        first_label(find(&plan, "site:parts")),
        "Front only (Recommended)"
    );
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn the_dependency_step_is_a_default_and_its_outcome_is_reported() {
    let fixture = Fixture::new();
    let (_, plan) = plan_for(fixture.root()).await;
    let step = default_item(&plan, "dependencies");
    assert!(text(&step["decision"]).contains("bunx @dependency-check-updates/cli -d -u --rm"));
    assert!(text(&step["decision"]).contains("Cargo.lock and target/"));

    let mut confirm_input = drafted(&fixture, &[screen("51:1", "Pricing", 1440.0)]).await;
    confirm_input.answers = answers(&[("route:51:1", "/pricing (Recommended)")]);
    confirm(&confirm_input, &fixture.policy()).unwrap();
    let report = status_report(
        &read_plan(fixture.root()).unwrap().unwrap(),
        &evidence(),
        None,
    );
    let line = array(&report["siteCompleteness"])
        .iter()
        .find(|item| item["item"] == "dependencies")
        .cloned()
        .unwrap();
    assert_eq!(line["state"], "missing");

    let mut record = input(&fixture, "assume");
    record.dependency_update = Some(DependencyUpdateInput {
        bumped: 14,
        pins: vec![PinInput {
            package: "oxlint".into(),
            version: "1.86.0".into(),
            reason: "eslint-plugin-devup does not load on 1.87 yet".into(),
        }],
    });
    let recorded = assume(&record, &fixture.policy()).unwrap();
    assert_eq!(recorded["assumptions"][0]["about"], "dependency oxlint");
    let report = status_report(
        &read_plan(fixture.root()).unwrap().unwrap(),
        &evidence(),
        None,
    );
    let line = array(&report["siteCompleteness"])
        .iter()
        .find(|item| item["item"] == "dependencies")
        .cloned()
        .unwrap();
    assert_eq!(line["state"], "filled");
    assert!(
        text(&line["detail"]).contains("14 packages bumped, pinned oxlint@1.86.0"),
        "{line}"
    );

    fixture.write(CONFIG_FILE, r#"{"updateDependencies": false, "favicon": "app/icon.svg", "ogImage": "public/og.png", "siteName": "Prime", "siteUrl": "https://example.com", "motion": "none"}"#);
    let (_, plan) = plan_for(fixture.root()).await;
    assert_eq!(default_item(&plan, "dependencies")["mechanism"], "skipped");
}

#[test]
fn bun_lock_versions_are_read() {
    let lock = r#"{
  "packages": {
    "react": ["react@19.3.1", "", {}, "sha512-x"],
    "vinext": ["vinext@1.0.2", "", {}, "sha512-y"],
  }
}"#;
    assert_eq!(bun_locked(lock, "react").as_deref(), Some("19.3.1"));
    assert_eq!(bun_locked(lock, "vinext").as_deref(), Some("1.0.2"));
    assert_eq!(bun_locked(lock, "next"), None);
}

async fn plan_screens_at(root: &str, screens: &[PlanScreenInput]) -> Value {
    let evidence = project_evidence(root).await.unwrap();
    draft_plan(screens, &BTreeMap::new(), &evidence, json!({}), None).unwrap()
}

fn rows_app(plan: &Value) -> Vec<(String, String)> {
    array(&plan["screens"])
        .iter()
        .map(|row| (text(&row["id"]).to_owned(), text(&row["app"]).to_owned()))
        .collect()
}

/// apps/front and apps/admin: the app is picked by what the frames are, not
/// by name order. Marketing frames build the public site without a question.
#[tokio::test]
async fn marketing_frames_go_to_the_public_app_without_a_question() {
    let config: PlanConfig = serde_json::from_value(json!({"appSelectionKeywords": {
        "publicApps": ["storefront"], "admin": ["controlroom"],
        "publicScreens": ["catalogue"], "adminRoutes": ["control"]
    }}))
    .unwrap();
    assert_eq!(
        selection_words(&config, "publicApps", &PUBLIC_APP_WORDS),
        vec!["storefront"]
    );
    assert_eq!(
        words_side(&["controlroom"], &["storefront"], &config),
        (0, 1)
    );
    assert_eq!(
        screen_side(
            &PlanScreenInput {
                name: "catalogue".into(),
                ..Default::default()
            },
            &config
        ),
        Side::Public
    );
    assert_eq!(
        selection_words(&PlanConfig::default(), "admin", &ADMIN_WORDS),
        *ADMIN_WORDS
    );
    let fixture = template_fixture();
    let plan = plan_screens_at(
        fixture.root(),
        &[
            screen("60:1", "Main", 1440.0),
            screen("60:2", "회사소개", 1440.0),
            screen("60:3", "Contact", 1440.0),
        ],
    )
    .await;
    assert!(
        !question_ids(&plan)
            .iter()
            .any(|id| id.starts_with("site:app") || id.starts_with("app:")),
        "{:?}",
        question_ids(&plan)
    );
    let chosen = default_item(&plan, "target-app");
    assert_eq!(chosen["decision"], "apps/front");
    assert_eq!(chosen["mechanism"], "decided");
    assert!(
        chosen["evidence"]
            .to_string()
            .contains("named like the public site"),
        "{chosen}"
    );
    assert!(rows_app(&plan).iter().all(|(_, app)| app == "apps/front"));
    assert_eq!(plan["appDir"], "apps/front/src/app");
}

#[tokio::test]
async fn admin_frames_go_to_the_admin_app() {
    let fixture = template_fixture();
    let plan = plan_screens_at(
        fixture.root(),
        &[
            screen("61:1", "Admin - 회원관리", 1440.0),
            screen("61:2", "Admin - 상품 목록", 1440.0),
        ],
    )
    .await;
    assert_eq!(default_item(&plan, "target-app")["decision"], "apps/admin");
    assert!(rows_app(&plan).iter().all(|(_, app)| app == "apps/admin"));
    assert!(
        !question_ids(&plan)
            .iter()
            .any(|id| id.starts_with("site:app"))
    );
}

/// A file with both maps each screen to its side; a screen with no admin
/// evidence goes to the public app without a question, and each screen's
/// route options point into its own app.
#[tokio::test]
async fn a_mixed_file_maps_each_screen_and_sends_the_unclear_to_the_public_app() {
    let fixture = template_fixture();
    let screens = [
        screen("62:1", "Home", 1440.0),
        screen("62:2", "관리자 - 대시보드", 1440.0),
        screen("62:3", "Settings", 1440.0),
    ];
    let plan = plan_screens_at(fixture.root(), &screens).await;
    let chosen = default_item(&plan, "target-app");
    assert_eq!(chosen["mechanism"], "per-screen");
    assert!(
        text(&chosen["decision"]).contains("no admin evidence go to apps/front"),
        "{chosen}"
    );
    let apps = rows_app(&plan).into_iter().collect::<BTreeMap<_, _>>();
    assert_eq!(apps["62:1"], "apps/front");
    assert_eq!(apps["62:2"], "apps/admin");
    assert_eq!(apps["62:3"], "apps/front");
    assert!(
        !question_ids(&plan)
            .iter()
            .any(|id| id.starts_with("app:") || id == "site:app"),
        "{:?}",
        question_ids(&plan)
    );
    let settings = find(&plan, "route:62:3");
    assert!(
        text(&settings["options"][0]["description"])
            .contains("apps/front/src/app/settings/page.tsx"),
        "{settings}"
    );
    // Home is offered nothing from the admin app; the dashboard is offered
    // the admin app's own pages.
    for question in array(&plan["questions"]) {
        let about_home = text(&question["id"]).ends_with("62:1");
        let mentions_admin = question["options"].to_string().contains("apps/admin/");
        assert!(!(about_home && mentions_admin), "{question}");
    }
    assert!(
        find(&plan, "route:62:2")["options"]
            .to_string()
            .contains("apps/admin/src/app/page.tsx")
    );
}

/// Frames that say nothing either way go to the public app: with no admin
/// evidence that is the default, recorded rather than asked.
#[tokio::test]
async fn frames_that_say_nothing_go_to_the_public_app_without_a_question() {
    let fixture = template_fixture();
    let screens = [
        screen("63:1", "Frame 12", 1440.0),
        screen("63:2", "Frame 13", 1440.0),
    ];
    let plan = plan_screens_at(fixture.root(), &screens).await;
    assert!(!question_ids(&plan).iter().any(|id| id == "site:app"));
    let chosen = default_item(&plan, "target-app");
    assert_eq!(chosen["mechanism"], "decided");
    assert_eq!(chosen["decision"], "apps/front");
    assert!(
        chosen["evidence"].to_string().contains("no admin evidence"),
        "{chosen}"
    );
    assert!(rows_app(&plan).iter().all(|(_, app)| app == "apps/front"));
}

/// Only when no app looks like the public site is the app asked - and the
/// answer still decides where each route is written.
#[tokio::test]
async fn the_app_is_asked_only_when_no_app_is_the_public_site() {
    let fixture = template_fixture();
    for (from, to) in [("apps/front", "apps/alpha"), ("apps/admin", "apps/beta")] {
        std::fs::rename(fixture.0.join(from), fixture.0.join(to)).unwrap();
    }
    fixture.write(
        "apps/alpha/package.json",
        r#"{"name": "alpha", "dependencies": {"next": "16.1.0"}}"#,
    );
    fixture.write(
        "apps/beta/package.json",
        r#"{"name": "beta", "dependencies": {"next": "16.1.0"}}"#,
    );
    let screens = [
        screen("63:1", "Frame 12", 1440.0),
        screen("63:2", "Frame 13", 1440.0),
    ];
    let plan = plan_screens_at(fixture.root(), &screens).await;
    let which = find(&plan, "site:app");
    assert_eq!(labels(which).len(), 2, "{which}");

    let draft_input = ScreenPlanInput {
        action: "draft".into(),
        project_root: Some(fixture.root().into()),
        ..ScreenPlanInput::default()
    };
    let drafted = draft(&draft_input, &screens, json!({}), &fixture.policy())
        .await
        .unwrap();
    let mut confirm_input = input(&fixture, "confirm");
    confirm_input.plan_id = drafted["planId"].as_str().map(str::to_owned);
    let mut pairs = vec![("site:app", "apps/beta")];
    for question in array(&drafted["questions"]) {
        let id = text(&question["id"]);
        if id.starts_with("route:") {
            pairs.push((
                id,
                if id.ends_with("63:1") {
                    "/members"
                } else {
                    "/orders"
                },
            ));
        }
    }
    let owned = pairs
        .iter()
        .map(|(id, answer)| (id.to_string(), answer.to_string()))
        .collect::<Vec<_>>();
    confirm_input.answers = answers(
        &owned
            .iter()
            .map(|(id, answer)| (id.as_str(), answer.as_str()))
            .collect::<Vec<_>>(),
    );
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    let files = array(&confirmed["routes"])
        .iter()
        .map(|row| (text(&row["app"]).to_owned(), text(&row["file"]).to_owned()))
        .collect::<Vec<_>>();
    assert_eq!(
        files[0],
        (
            "apps/beta".to_owned(),
            "apps/beta/src/app/members/page.tsx".to_owned()
        ),
        "{files:?}"
    );
}

/// auditPlan writes one runtime-audit plan per confirmed URL and the runner
/// once, records them, and reports the frames it has no export for instead
/// of skipping them.
#[tokio::test]
async fn audit_plan_writes_a_plan_per_url_and_reports_what_it_cannot_compare() {
    let fixture = Fixture::new();
    write_plan_file(&fixture, &confirmed_plan());
    let store = ArtifactStore::default();
    let mut request = input(&fixture, "auditPlan");
    request.artifacts = vec!["expired-artifact".into()];
    let result = audit_plan(&request, &fixture.policy(), &store)
        .await
        .unwrap();
    let plans = array(&result["plans"]);
    assert_eq!(
        plans
            .iter()
            .map(|plan| text(&plan["url"]))
            .collect::<Vec<_>>(),
        ["/", "/about"]
    );
    assert!(
        fixture
            .0
            .join(".devup/runtime-audit/home/plan.json")
            .exists()
    );
    assert!(
        fixture
            .0
            .join(".devup/runtime-audit/about/plan.json")
            .exists()
    );
    assert!(
        fixture
            .0
            .join(".devup/runtime-audit/runner/run.mjs")
            .exists()
    );
    assert_eq!(result["runner"]["state"], "written");
    // The plugin that strips the audit ids from production builds sits next
    // to the app's vite.config, and the config is not wired to it yet.
    let strip = &result["productionStrip"][0];
    assert!(
        fixture
            .0
            .join("vite-plugin-strip-figma-node-ids.mjs")
            .exists()
    );
    assert_eq!(strip["written"], true);
    assert_eq!(strip["wired"], false);
    assert!(text(&strip["verify"]).contains("grep -rl data-figma-node-id"));
    assert_eq!(
        result["expiredArtifacts"][0]["artifactId"],
        "expired-artifact"
    );
    assert_eq!(result["maxFixAttempts"], 3);
    let frames = array(&result["missingReferences"])
        .iter()
        .filter(|item| item["kind"] == "frame")
        .map(|item| {
            (
                text(&item["nodeId"]).to_owned(),
                item["width"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        frames,
        [
            ("10:1".to_owned(), 1440),
            ("10:2".to_owned(), 390),
            ("10:3".to_owned(), 1440)
        ]
    );
    let home = &plans[0];
    assert_eq!(home["checks"]["view-transition"], 1);
    assert_eq!(home["checks"]["page-health"], 1);
    let written = read_plan(fixture.root()).unwrap().unwrap();
    assert_eq!(array(&written["audit"]["plans"]).len(), 2);
    assert!(text(&written["audit"]["plans"][0]["reportPath"]).ends_with("home/report/report.json"));

    // The runner is written once; a second run leaves it alone.
    let again = audit_plan(&request, &fixture.policy(), &store)
        .await
        .unwrap();
    assert_eq!(again["runner"]["state"], "current");
}

/// A decision mid-run never stops the work: it is pending, its URL's other
/// work comes first, and assume takes it at the end - except a destructive
/// one, which stays unbuilt and is reported.
#[tokio::test]
async fn a_pending_decision_is_taken_at_the_end_and_a_destructive_one_is_left_unbuilt() {
    let fixture = Fixture::new();
    write_plan_file(&fixture, &confirmed_plan());
    let mut request = input(&fixture, "defer");
    request.decision = Some(PendingDecisionInput {
        id: "about-form".into(),
        about: "/about contact form".into(),
        question: "Where does the contact form post?".into(),
        options: vec!["mailto".into(), "an API route".into()],
        recommended: "mailto".into(),
        destructive: false,
    });
    let deferred = defer(&request, &fixture.policy()).unwrap();
    assert_eq!(deferred["pendingDecisions"][0]["status"], "open");
    request.decision = Some(PendingDecisionInput {
        id: "drop-old-page".into(),
        about: "src/app/old/page.tsx".into(),
        question: "Delete the old page?".into(),
        options: Vec::new(),
        recommended: "keep it".into(),
        destructive: true,
    });
    defer(&request, &fixture.policy()).unwrap();

    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let report = status_report(&plan, &evidence(), None);
    let about = array(&report["workQueue"])
        .iter()
        .find(|row| row["url"] == "/about")
        .unwrap()
        .clone();
    let order = array(&about["items"])
        .iter()
        .map(|item| text(&item["do"]).to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        order.last().map(String::as_str),
        Some("blocked"),
        "{order:?}"
    );

    let mut take = input(&fixture, "assume");
    take.assumptions = vec![
        AssumptionInput {
            about: "about-form".into(),
            decision: "mailto".into(),
            reason: Some("recommended".into()),
        },
        AssumptionInput {
            about: "drop-old-page".into(),
            decision: "delete".into(),
            reason: None,
        },
    ];
    let taken = assume(&take, &fixture.policy()).unwrap();
    let decisions = array(&taken["pendingDecisions"]);
    assert_eq!(decisions[0]["status"], "taken");
    assert_eq!(decisions[0]["taken"], "mailto");
    assert_eq!(decisions[1]["status"], "left-unbuilt");
    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let report = status_report(&plan, &evidence(), None);
    let text_report = text(&report["finalReport"]);
    assert!(
        text_report
            .contains("about-form: Where does the contact form post? -> mailto (taken at the end)"),
        "{text_report}"
    );
    assert!(
        text_report
            .contains("drop-old-page: Delete the old page? -> keep it (destructive: left unbuilt)")
    );
    assert!(
        text_report
            .contains("| URL | width | result | completeness | health | geometry | drift / converter bugs | page | section | asset | interaction |")
    );
    assert!(text_report.contains("Dependencies: the update step has not been recorded."));
}
