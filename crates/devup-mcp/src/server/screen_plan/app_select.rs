

/// The defaults that come before any page: start from the template when there
/// is no app, which framework runs the app, and the dependency update.
fn stack_defaults(evidence: &ProjectEvidence) -> Vec<Value> {
    let mut defaults = Vec::new();
    if is_new_project(evidence) {
        defaults.push(json!({"item": "project-start",
            "decision": format!(include_str!("../text/screen_plan-stack_defaults-d534196c.md"), UPDATE_STEP = UPDATE_STEP),
            "evidence": "no app, route or framework in the project"}));
    } else if let Some(app) = &evidence.target_app {
        defaults.push(
            json!({"item": "framework", "decision": format!("{} runs {}", app.dir, app.framework),
            "evidence": app.evidence}),
        );
    }
    defaults.push(if evidence.config.update_dependencies == Some(false) {
        json!({"item": "dependencies", "mechanism": "skipped",
            "decision": "Dependencies are not updated: the project config forbids it.",
            "evidence": format!("updateDependencies: false in {CONFIG_FILE}")})
    } else {
        json!({"item": "dependencies", "mechanism": "update",
            "decision": format!(include_str!("../text/screen_plan-stack_defaults-c2140f37.md"), UPDATE_STEP = UPDATE_STEP),
            "evidence": "DevFive works on the latest versions; API knowledge from memory is assumed stale"})
    });
    defaults
}

/// Words that mark an app, or a screen, as the public site or the admin
/// console. An app named `admin` is the admin console; a Figma design for a
/// public site is almost never for it.
static PUBLIC_APP_WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/screen-plan-public-app-words.json"))
        .expect("vendored keyword JSON")
});
static ADMIN_WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/screen-plan-admin-words.json"))
        .expect("vendored keyword JSON")
});
/// Screen names a public site has and an admin console does not.
static PUBLIC_SCREEN_WORDS: std::sync::LazyLock<Vec<&'static str>> =
    std::sync::LazyLock::new(|| {
        serde_json::from_str(include_str!("../text/screen-plan-public-screen-words.json"))
            .expect("vendored keyword JSON")
    });
/// Routes that say an app is a console: a sign-in gate and a dashboard.
static ADMIN_ROUTE_SEGMENTS: std::sync::LazyLock<Vec<&'static str>> =
    std::sync::LazyLock::new(|| {
        serde_json::from_str(include_str!("../text/screen-plan-admin-route-segments.json"))
            .expect("vendored keyword JSON")
    });

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Public,
    Admin,
    Unclear,
}

fn selection_words<'a>(config: &'a PlanConfig, key: &str, defaults: &[&'a str]) -> Vec<&'a str> {
    config
        .app_selection_keywords
        .get(key)
        .map(|words| words.iter().map(String::as_str).collect())
        .unwrap_or_else(|| defaults.to_vec())
}
fn words_side(names: &[&str], public: &[&str], config: &PlanConfig) -> (usize, usize) {
    let admin_words = selection_words(config, "admin", &ADMIN_WORDS);
    let admin = names
        .iter()
        .filter(|name| admin_words.iter().any(|word| keyword_in(name, word)))
        .count();
    let public = names
        .iter()
        .filter(|name| public.iter().any(|word| keyword_in(name, word)))
        .count();
    (public, admin)
}

/// Which side an app serves, and the evidence: its directory and package
/// name, then its routes.
fn app_side(app: &super::framework::App, evidence: &ProjectEvidence) -> (Side, Vec<String>) {
    let base = app.dir.rsplit('/').next().unwrap_or(&app.dir).to_owned();
    let mut names = vec![base.as_str()];
    if let Some(name) = &app.name {
        names.push(name.as_str());
    }
    let (mut public, mut admin) = words_side(
        &names,
        &selection_words(&evidence.config, "publicApps", &PUBLIC_APP_WORDS),
        &evidence.config,
    );
    let mut why = Vec::new();
    if public > 0 {
        why.push(format!("{} is named like the public site", app.dir));
    }
    if admin > 0 {
        why.push(format!("{} is named like an admin console", app.dir));
    }
    let console_routes = evidence
        .routes
        .iter()
        .filter(|(_, page)| page.starts_with(&format!("{}/", app.dir)))
        .filter(|(route, _)| {
            route.trim_matches('/').split('/').any(|segment| {
                selection_words(&evidence.config, "adminRoutes", &ADMIN_ROUTE_SEGMENTS)
                    .contains(&segment)
            })
        })
        .map(|(route, _)| route.clone())
        .collect::<Vec<_>>();
    if !console_routes.is_empty() {
        admin += 1;
        why.push(format!(
            "{} has console routes ({})",
            app.dir,
            console_routes.join(", ")
        ));
    }
    if public == admin {
        public = 0;
        admin = 0;
    }
    let side = match public.cmp(&admin) {
        std::cmp::Ordering::Greater => Side::Public,
        std::cmp::Ordering::Less => Side::Admin,
        std::cmp::Ordering::Equal => Side::Unclear,
    };
    (side, why)
}

/// Which side one Figma frame belongs to, from its own name and its
/// section's. Admin markers win: a "관리자 - 메인" frame is the console's.
fn screen_side(screen: &PlanScreenInput, config: &PlanConfig) -> Side {
    let mut names = vec![screen.name.as_str()];
    names.extend(screen.section_name.as_deref());
    names.extend(screen.breadcrumb.iter().map(String::as_str));
    match words_side(
        &names,
        &selection_words(config, "publicScreens", &PUBLIC_SCREEN_WORDS),
        config,
    ) {
        (_, admin) if admin > 0 => Side::Admin,
        (public, _) if public > 0 => Side::Public,
        _ => Side::Unclear,
    }
}

fn app_action(app: &super::framework::App) -> Value {
    json!({"app": {"dir": app.dir, "appDir": app.path(app.app_router.as_deref().unwrap_or("src/app"))}})
}

/// Which app each screen is built in, by meaning rather than by name order.
///
/// With one app, or a projectRoot that is an app, there is nothing to pick.
/// Otherwise the screens' own names decide: all public screens go to the
/// public app and all admin screens to the admin app without a question; a
/// file with both maps each screen to its side and asks only about screens
/// whose side is unclear; a file whose screens say nothing either way is the
/// one genuinely ambiguous case, asked once - recommending the public app,
/// never whichever comes first alphabetically.
///
/// Returns the app per screen (by node id), the questions, and the default
/// that records the evidence.
fn assign_apps(
    evidence: &ProjectEvidence,
    screens: &[PlanScreenInput],
) -> (
    BTreeMap<String, super::framework::App>,
    Vec<Value>,
    Option<Value>,
) {
    let mut assigned = BTreeMap::new();
    if evidence.target_app.is_some() || evidence.apps.len() < 2 {
        return (assigned, Vec::new(), None);
    }
    let sides = evidence
        .apps
        .iter()
        .map(|app| (app, app_side(app, evidence)))
        .collect::<Vec<_>>();
    let public_app = sides
        .iter()
        .find(|(_, (side, _))| *side == Side::Public)
        .or_else(|| sides.iter().find(|(_, (side, _))| *side != Side::Admin))
        .map(|(app, _)| (*app).clone())
        .unwrap_or_else(|| evidence.apps[0].clone());
    let admin_app = sides
        .iter()
        .find(|(_, (side, _))| *side == Side::Admin)
        .map(|(app, _)| (*app).clone());
    let app_evidence = sides
        .iter()
        .flat_map(|(_, (_, why))| why.clone())
        .collect::<Vec<_>>();
    let screen_sides = screens
        .iter()
        .map(|screen| (screen, screen_side(screen, &evidence.config)))
        .collect::<Vec<_>>();
    let admin_screens = screen_sides
        .iter()
        .filter(|(_, side)| *side == Side::Admin)
        .count();
    let public_screens = screen_sides
        .iter()
        .filter(|(_, side)| *side == Side::Public)
        .count();
    let mut questions = Vec::new();
    let options = || {
        let mut ordered = vec![public_app.clone()];
        ordered.extend(
            evidence
                .apps
                .iter()
                .filter(|app| app.dir != public_app.dir)
                .cloned(),
        );
        ordered
            .iter()
            .take(MAX_OPTIONS)
            .map(|app| {
                let side = sides
                    .iter()
                    .find(|(candidate, _)| candidate.dir == app.dir)
                    .map(|(_, (side, _))| *side);
                choice(
                    app.dir.as_str(),
                    format!(
                        "{} - {} app{}",
                        match side {
                            Some(Side::Public) => "the public site",
                            Some(Side::Admin) => "the admin console",
                            _ => "an app",
                        },
                        app.framework,
                        app.name
                            .as_deref()
                            .map(|name| format!(" ({name})"))
                            .unwrap_or_default()
                    ),
                    app_action(app),
                )
            })
            .collect::<Vec<_>>()
    };
    let has_public = sides.iter().any(|(_, (side, _))| *side == Side::Public);
    let decision = match (admin_screens, public_screens, &admin_app) {
        // Nothing says admin, so the public site: the bias the user chose.
        (0, 0, _) if has_public => {
            for (screen, _) in &screen_sides {
                assigned.insert(screen.node_id.clone(), public_app.clone());
            }
            json!({"item": "target-app", "mechanism": "decided", "decision": public_app.dir,
                "evidence": ([app_evidence, vec!["the frames name neither side; with no admin evidence the public app is taken".to_owned()]].concat())})
        }
        (0, 0, _) => {
            let mut asked = question(
                "site:app".into(),
                "Which app".into(),
                format!(
                    "This repository has {} apps ({}), and the frames' names say nothing about which one they are for. Which app does this design build? A custom answer is its directory.",
                    evidence.apps.len(),
                    evidence
                        .apps
                        .iter()
                        .map(|app| app.dir.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                options(),
                "site",
            );
            asked["field"] = json!("app");
            questions.push(asked);
            for (screen, _) in &screen_sides {
                assigned.insert(screen.node_id.clone(), public_app.clone());
            }
            json!({"item": "target-app", "mechanism": "asked", "question": "site:app",
                "decision": format!("Asked; {} is recommended as the public app.", public_app.dir), "evidence": app_evidence})
        }
        (admin, public, Some(admin_app)) if admin > 0 && public > 0 => {
            for (screen, side) in &screen_sides {
                let app = if *side == Side::Admin {
                    admin_app.clone()
                } else {
                    public_app.clone()
                };
                assigned.insert(screen.node_id.clone(), app);
            }
            json!({"item": "target-app", "mechanism": "per-screen",
                "decision": format!("Admin screens go to {}; public screens and screens with no admin evidence go to {}.", admin_app.dir, public_app.dir),
                "evidence": app_evidence})
        }
        (admin, 0, Some(admin_app)) if admin > 0 => {
            for (screen, _) in &screen_sides {
                assigned.insert(screen.node_id.clone(), admin_app.clone());
            }
            json!({"item": "target-app", "mechanism": "decided", "decision": admin_app.dir,
                "evidence": ([app_evidence, vec![format!("{admin} of {} frames are named for the admin console", screens.len())]].concat())})
        }
        _ => {
            for (screen, _) in &screen_sides {
                assigned.insert(screen.node_id.clone(), public_app.clone());
            }
            json!({"item": "target-app", "mechanism": "decided", "decision": public_app.dir,
                "evidence": ([app_evidence, vec![format!("{public_screens} of {} frames are public-site screens and none is named for an admin console", screens.len())]].concat())})
        }
    };
    (assigned, questions, Some(decision))
}

/// The questions about the stack itself: migrating off the `next` CLI, and
/// for a new project its name and parts. Which app is [`assign_apps`]'s.
fn stack_questions(evidence: &ProjectEvidence) -> Vec<Value> {
    let mut questions = Vec::new();
    if evidence.framework() == "next" && evidence.config.framework.is_none() {
        let app = evidence.target_app.as_ref();
        let mut asked = question(
            "site:framework".into(),
            "Framework".into(),
            format!(
                "{} runs on the next CLI ({}). DevFive's default is vinext, which runs the same App Router on Vite. Migrate it, or keep next?",
                app.map_or(".", |app| app.dir.as_str()),
                app.map(|app| app.evidence.join("; ")).unwrap_or_default()
            ),
            vec![
                choice(
                    "Migrate to vinext",
                    include_str!("../text/screen_plan-stack_questions-eb6d40b2.md"),
                    json!({"framework": {"source": "vinext"}}),
                ),
                choice(
                    "Keep next",
                    "Stay on the next CLI; view transitions follow its version.",
                    json!({"framework": {"source": "next"}}),
                ),
            ],
            "site",
        );
        asked["field"] = json!("framework");
        questions.push(asked);
    }
    if is_new_project(evidence) {
        let mut name = question(
            "site:app-name".into(),
            "App name".into(),
            "The project starts from dev-five-git/service-template. What is the app called? A custom answer is the name.".into(),
            vec![
                choice("front", "The template's own app name (apps/front).", json!({"appName": {"source": "template", "value": "front"}})),
                choice("Use the site name", "Named after the site name asked with the other site facts.", json!({"appName": {"source": "site-name"}})),
            ],
            "site",
        );
        name["field"] = json!("appName");
        questions.push(name);
        let mut parts = question(
            "site:parts".into(),
            "Template parts".into(),
            "Which parts of the template does this project keep?".into(),
            vec![
                choice(
                    "Front only",
                    "apps/front alone; admin and the Rust API are removed.",
                    json!({"parts": {"source": "front"}}),
                ),
                choice(
                    "Front + admin + API",
                    "apps/front, apps/admin and apis/api, as the template ships them.",
                    json!({"parts": {"source": "all"}}),
                ),
            ],
            "site",
        );
        parts["field"] = json!("parts");
        questions.push(parts);
    }
    questions
}

/// What the agent fills in by itself, without a question. Recorded in the
/// plan's `defaults` and the final report, so nothing it added is a surprise.
fn site_defaults(evidence: &ProjectEvidence, screens: &[PlanScreenInput]) -> Vec<Value> {
    let lang = evidence.lang.clone().unwrap_or_else(|| {
        let korean = screens
            .iter()
            .any(|screen| screen.name.chars().any(|c| ('가'..='힣').contains(&c)));
        if korean {
            (
                "ko".into(),
                "Korean text in the design's layer names".into(),
            )
        } else {
            (
                "en".into(),
                "no Korean text in the design's layer names".into(),
            )
        }
    });
    let mut defaults = vec![
        json!({"item": "interaction-recipes",
            "decision": "Each interactive element gets its role's recipe; status counts every role against it.",
            "recipes": INTERACTION_RECIPES.iter().map(|(role, recipe)| json!({"role": role, "recipe": recipe})).collect::<Vec<_>>(),
            "restraint": RESTRAINT_RULES,
            "evidence": "devfive-frontend critical-rules 4 (interactive states); devup-ui pseudo-selectors; the devup-implementation-plan recipe table"}),
        json!({"item": "transition",
            "decision": "One shared transition, 150-250ms, ease or ease-out, on the properties that change (at most two per state), defined once as a token or shared constant and not per element.",
            "evidence": "devfive-frontend critical-rules 4 (transition 0.15-0.3s ease)"}),
        json!({"item": "reduced-motion",
            "decision": "_motionReduce removes transforms and transitions (or one globalCss rule does), so prefers-reduced-motion gets no movement.",
            "evidence": "devup-ui _motionReduce"}),
        json!({"item": "metadata",
            "decision": include_str!("../text/screen_plan-site_defaults-a7c1a186.md"),
            "evidence": "devfive-frontend critical-rules 2 (SEO metadata with openGraph)"}),
        json!({"item": "lang", "decision": format!("<html lang=\"{}\">", lang.0), "evidence": lang.1}),
        json!({"item": "alt-text", "decision": "alt text from the Figma layer name or its text content on every image.",
            "evidence": "devfive-frontend critical-rules 3 (image alt required)"}),
        json!({"item": "image-dimensions", "decision": "width and height on every image, so nothing shifts as it loads.",
            "evidence": "no layout shift"}),
    ];
    let transitions = view_transition_default(evidence);
    defaults.push(if evidence.motion_libraries.is_empty() {
        transitions
    } else {
        json!({"item": "view-transitions", "mechanism": "asked", "question": "site:view-transitions",
            "decision": "Asked: the project already animates with its own library.", "ifAdded": transitions,
            "evidence": evidence.motion_libraries.join(", ")})
    });
    defaults.push(match &evidence.not_found {
        Some(path) => {
            json!({"item": "not-found", "decision": format!("Keep {path}."), "evidence": path})
        }
        None => json!({"item": "not-found",
            "decision": format!("Add {}/not-found.tsx in the site's own style.", evidence.app_dir),
            "evidence": "no not-found page in the project"}),
    });
    defaults
}

/// One site fact that needs a file or a fact only the user has.
fn site_question(
    field: &str,
    id: &str,
    header: &str,
    prompt: String,
    choices: Vec<Choice>,
) -> Value {
    let mut asked = question(id.into(), header.into(), prompt, choices, "site");
    asked["field"] = json!(field);
    asked
}

/// The site-level questions and the state of each fact, asked in the same
/// up-front rounds as everything else. Never asked when the project already
/// says it, or the project config pre-answers it.
fn site_questions(
    evidence: &ProjectEvidence,
    assets: &[(String, String, &'static str)],
    cover: Option<(&str, &str)>,
) -> (Value, Vec<Value>) {
    let config = &evidence.config;
    let mut site = serde_json::Map::new();
    let mut questions = Vec::new();
    let mut file_question = |field: &str,
                             id: &str,
                             header: &str,
                             what: &str,
                             files: &[String],
                             kind: &'static str| {
        let mut choices = Vec::new();
        for file in files.iter().take(2) {
            choices.push(choice(
                format!("Use {file}"),
                "Already in the project.",
                json!({field: {"source": "file", "path": file}}),
            ));
        }
        let (format, target) = if kind == "logo" {
            ("svg", format!("{}/icon.svg", evidence.app_dir))
        } else {
            ("png", format!("{}/opengraph-image.png", evidence.app_dir))
        };
        for (node, name, _) in assets.iter().filter(|(_, _, found)| *found == kind).take(2) {
            choices.push(choice(
                format!("Export \"{}\" ({node}) as {}", truncate(name, 30), format.to_uppercase()),
                format!("A Figma node in the linked scope; exported with devup_figma_export assetRequests (format {format}) to {target}."),
                json!({field: {"source": "figma", "nodeId": node, "format": format, "path": target}}),
            ));
        }
        if kind == "og"
            && let Some((node, name)) = cover
        {
            choices.push(choice(
                format!("Use the top of \"{}\" ({node})", truncate(name, 24)),
                format!("Export the first screen as PNG and crop it to 1200x630 for {target}."),
                json!({field: {"source": "figma-cover", "nodeId": node, "format": "png", "path": target}}),
            ));
        }
        choices.truncate(MAX_OPTIONS - 2);
        choices.push(choice(
            "I will provide a file",
            "Answer with its path, e.g. public/favicon.ico. Nothing is generated or drawn.",
            json!({field: {"source": "provide"}}),
        ));
        choices.push(choice(
            "Leave it for later",
            "Not added now; status reports it missing.",
            json!({field: {"source": "later"}}),
        ));
        let evidence_text =
            if files.is_empty() && !assets.iter().any(|(_, _, found)| *found == kind) {
                "Nothing in the project or the linked Figma scope looks like one.".to_owned()
            } else {
                "The options come from the project's files and the linked Figma scope.".to_owned()
            };
        questions.push(site_question(
            field,
            id,
            header,
            format!("Which file should be the site's {what}? {evidence_text}"),
            choices,
        ));
    };
    match &config.favicon {
        Some(value) => {
            site.insert(
                "favicon".into(),
                json!({"state": "config", "value": value, "evidence": CONFIG_FILE}),
            );
        }
        None => {
            site.insert(
                "favicon".into(),
                json!({"state": "asked", "question": "site:favicon", "found": evidence.icon_files}),
            );
            file_question(
                "favicon",
                "site:favicon",
                "Favicon / app icon",
                "favicon and app icon",
                &evidence.icon_files,
                "logo",
            );
        }
    }
    match &config.og_image {
        Some(value) => {
            site.insert(
                "ogImage".into(),
                json!({"state": "config", "value": value, "evidence": CONFIG_FILE}),
            );
        }
        None => {
            site.insert(
                "ogImage".into(),
                json!({"state": "asked", "question": "site:og", "found": evidence.og_files}),
            );
            file_question(
                "ogImage",
                "site:og",
                "Share (OG) image",
                "share image (Open Graph)",
                &evidence.og_files,
                "og",
            );
        }
    }
    match (&config.site_name, &evidence.site_name) {
        (Some(value), _) => {
            site.insert(
                "siteName".into(),
                json!({"state": "config", "value": value, "evidence": CONFIG_FILE}),
            );
        }
        (None, Some((value, from))) => {
            site.insert(
                "siteName".into(),
                json!({"state": "found", "value": value, "evidence": from}),
            );
        }
        (None, None) => {
            site.insert(
                "siteName".into(),
                json!({"state": "asked", "question": "site:name"}),
            );
            let mut choices = Vec::new();
            if let Some(package) = &evidence.package_name {
                choices.push(choice(
                    package.as_str(),
                    "The package.json name; it may not be the name people see.",
                    json!({"siteName": {"source": "package", "value": package}}),
                ));
            }
            choices.push(choice(
                "Use the main page's heading",
                "Taken from the first page's heading text in Figma while building.",
                json!({"siteName": {"source": "heading"}}),
            ));
            choices.push(choice(
                "Leave it for later",
                "Titles go without a site suffix until it is known.",
                json!({"siteName": {"source": "later"}}),
            ));
            questions.push(site_question(
                "siteName",
                "site:name",
                "Site name",
                "What is the site's name, for the title template (\"Page | Name\")? It is not in the project's metadata or environment. A custom answer is the name itself.".into(),
                choices,
            ));
        }
    }
    match (&config.site_url, &evidence.site_url) {
        (Some(value), _) => {
            site.insert(
                "siteUrl".into(),
                json!({"state": "config", "value": value, "evidence": CONFIG_FILE}),
            );
        }
        (None, Some((value, from))) => {
            site.insert(
                "siteUrl".into(),
                json!({"state": "found", "value": value, "evidence": from}),
            );
        }
        (None, None) => {
            site.insert(
                "siteUrl".into(),
                json!({"state": "asked", "question": "site:url"}),
            );
            questions.push(site_question(
                "siteUrl",
                "site:url",
                "Production URL",
                "What is the production URL, for metadataBase and absolute share links? It is not in the project's metadata or .env files. A custom answer is the URL itself.".into(),
                vec![
                    choice(
                        "No production URL yet",
                        "metadataBase is left out; share images use relative URLs until it is set.",
                        json!({"siteUrl": {"source": "none"}}),
                    ),
                    choice(
                        "Read it from NEXT_PUBLIC_SITE_URL",
                        "metadataBase comes from that environment variable, set per deployment.",
                        json!({"siteUrl": {"source": "env", "name": "NEXT_PUBLIC_SITE_URL"}}),
                    ),
                ],
            ));
        }
    }
    if !evidence.analytics.is_empty() {
        site.insert(
            "analytics".into(),
            json!({"state": "asked", "question": "site:analytics", "found": evidence.analytics}),
        );
        questions.push(site_question(
            "analytics",
            "site:analytics",
            "Analytics / verification",
            format!(
                "The project shows analytics or verification setup: {}. Wire it into the new root layout?",
                evidence.analytics.join(", ")
            ),
            vec![
                choice("Keep the existing setup", "Carried into the new layout as it is configured.", json!({"analytics": {"source": "keep"}})),
                choice("Leave it out", "The new layout has no analytics or verification tags.", json!({"analytics": {"source": "none"}})),
            ],
        ));
    } else {
        site.insert("analytics".into(), json!({"state": "not-needed", "evidence": "no analytics or verification signs in the project"}));
    }
    if !evidence.motion_libraries.is_empty() {
        let libraries = evidence.motion_libraries.join(", ");
        site.insert("viewTransitions".into(), json!({"state": "asked", "question": "site:view-transitions", "found": evidence.motion_libraries}));
        questions.push(site_question(
            "viewTransitions",
            "site:view-transitions",
            "View transitions",
            format!("The project already animates with {libraries}. Add view transitions between pages as well, or keep page motion to {libraries}?"),
            vec![
                choice(format!("Keep {libraries} only"), "No view transitions; the existing motion stays the only page motion.", json!({"viewTransitions": {"source": "keep-library"}})),
                choice("Add view transitions too", "A short cross-fade between pages, beside the existing library.", json!({"viewTransitions": {"source": "add"}})),
            ],
        ));
    }
    match &config.motion {
        Some(value) => {
            site.insert(
                "motion".into(),
                json!({"state": "config", "value": value, "evidence": CONFIG_FILE}),
            );
        }
        None => {
            site.insert(
                "motion".into(),
                json!({"state": "asked", "question": "site:motion"}),
            );
            questions.push(site_question(
                "motion",
                "site:motion",
                "Motion beyond hover",
                "Beyond hover, focus and page transitions, should sections reveal on scroll? Nothing in the plan shows designed motion.".into(),
                vec![
                    choice("None", "Recommended unless the design shows motion; hover and focus transitions are still added.", json!({"motion": {"source": "none"}})),
                    choice("Subtle reveal on scroll", "Sections fade and slide in once as they enter; off under prefers-reduced-motion.", json!({"motion": {"source": "reveal"}})),
                ],
            ));
        }
    }
    (Value::Object(site), questions)
}

/// The site completeness block of `status`: each item filled, asked and
/// answered, or missing, read from the files as they are now.
fn site_completeness(plan: &Value, evidence: &ProjectEvidence) -> Vec<Value> {
    let site = &plan["site"];
    let decided = |field: &str| {
        let entry = &site[field];
        let source = entry["answer"][field]["source"]
            .as_str()
            .or_else(|| entry["answer"]["source"].as_str());
        entry["state"] == "config" || (entry["state"] == "answered" && source != Some("later"))
    };
    let file_item = |item: &str, field: &str, files: &[String]| {
        if !files.is_empty() {
            json!({"item": item, "state": "filled", "detail": files.join(", ")})
        } else if decided(field) {
            json!({"item": item, "state": "asked-and-answered", "detail": "decided, but no file is in place yet"})
        } else {
            json!({"item": item, "state": "missing", "detail": "no file, and no answer that provides one"})
        }
    };
    let mut items = vec![
        file_item("favicon", "favicon", &evidence.icon_files),
        file_item("og-image", "ogImage", &evidence.og_files),
    ];
    for row in array(&plan["routes"])
        .iter()
        .filter(|row| row["implementation"] == "page")
    {
        let url = text(&row["url"]);
        let page = evidence
            .routes
            .iter()
            .find(|(pattern, _)| pattern == url)
            .map(|(_, page)| page.clone());
        let state = match &page {
            Some(page) => match read_small(&join_relative(&evidence.root, page)) {
                Some(source)
                    if devup_mcp_devup_ui::ui_validate::site::exports_metadata(&source) =>
                {
                    "filled"
                }
                _ => "missing",
            },
            None => "missing",
        };
        items.push(json!({"item": format!("metadata {url}"), "state": state,
            "detail": page.unwrap_or_else(|| "page not implemented".into())}));
    }
    let mut roles = BTreeMap::<devup_mcp_devup_ui::ui_validate::site::Role, (usize, usize)>::new();
    let mut motion = false;
    let mut reduced = false;
    let mut transitions = false;
    for relative in &evidence.sources {
        let Some(source) = read_small(&join_relative(&evidence.root, relative)) else {
            continue;
        };
        for row in devup_mcp_devup_ui::ui_validate::site::role_coverage(relative, &source) {
            let entry = roles.entry(row.role).or_default();
            entry.0 += row.total;
            entry.1 += row.complete;
        }
        motion |= source.contains("transition")
            || source.contains("transform")
            || source.contains("animation");
        reduced |= source.contains("_motionReduce") || source.contains("prefers-reduced-motion");
        transitions |= source.contains("ViewTransition")
            || source.contains("viewTransitionName")
            || source.contains("@view-transition");
    }
    let complete = roles.values().all(|(total, done)| total == done);
    items.push(json!({"item": "interaction-roles",
        "state": if roles.is_empty() { "not-applicable" } else if complete { "filled" } else { "missing" },
        "detail": if roles.is_empty() {
            "no interactive elements in the sources".to_owned()
        } else {
            roles.iter().map(|(role, (total, done))| format!("{} {done}/{total} {}", role.as_str(), role.requirement())).collect::<Vec<_>>().join(", ")
        }}));
    items.push(json!({"item": "reduced-motion",
        "state": if reduced { "filled" } else if motion { "missing" } else { "not-applicable" },
        "detail": if reduced { "prefers-reduced-motion is handled" } else if motion { "transitions or transforms with no prefers-reduced-motion handling" } else { "no motion in the sources" }}));
    // Decided again from what is installed now: the plan was drafted before
    // the dependency update, and the update is what makes it possible.
    let planned = view_transition_default(evidence);
    let mechanism = text(&planned["mechanism"]).to_owned();
    let declined = plan["site"]["viewTransitions"]["answer"]["source"] == "keep-library";
    items.push(match mechanism.as_str() {
        "skipped" | "" => json!({"item": "view-transitions", "state": "not-applicable",
            "detail": format!("skipped: {}", text(&planned["evidence"]))}),
        _ if declined => json!({"item": "view-transitions", "state": "not-applicable",
            "detail": "skipped: the user kept the project's own page motion"}),
        _ if transitions => json!({"item": "view-transitions", "state": "filled",
            "detail": format!("enabled ({mechanism}); the runtime audit's view-transition check proves it runs")}),
        _ => json!({"item": "view-transitions", "state": "missing",
            "detail": format!("planned ({mechanism}) but not in the sources yet")}),
    });
    let skipped_update = array(&plan["defaults"])
        .iter()
        .any(|default| default["item"] == "dependencies" && default["mechanism"] == "skipped");
    let record = &plan["dependencies"];
    items.push(if skipped_update {
        json!({"item": "dependencies", "state": "not-applicable", "detail": format!("not updated: updateDependencies is false in {CONFIG_FILE}")})
    } else if record.is_object() {
        let pins = array(&record["pins"])
            .iter()
            .map(|pin| format!("{}@{} ({})", text(&pin["package"]), text(&pin["version"]), text(&pin["reason"])))
            .collect::<Vec<_>>();
        json!({"item": "dependencies", "state": "filled",
            "detail": format!("updated {}: {} packages bumped, {}", text(&record["updatedAt"]), record["bumped"],
                if pins.is_empty() { "no pins".to_owned() } else { format!("pinned {}", pins.join(", ")) })})
    } else {
        json!({"item": "dependencies", "state": "missing",
            "detail": format!("not updated yet: run {UPDATE_STEP}, then record it with assume (dependencyUpdate)")})
    });
    for (item, field) in [("site-name", "siteName"), ("site-url", "siteUrl")] {
        let entry = &site[field];
        let state = match entry["state"].as_str() {
            Some("found" | "config") => "filled",
            Some("answered") if decided(field) => "asked-and-answered",
            _ => "missing",
        };
        items.push(json!({"item": item, "state": state, "detail": entry.get("value").or(entry.get("answer")).cloned()}));
    }
    items
}

/// Names that mark a page, section or frame as not for implementation. Words
/// in ASCII match whole words; the rest match anywhere in the name.
static EXCLUDE_WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/screen-plan-exclude-words.json"))
        .expect("vendored keyword JSON")
});

fn keyword_in(name: &str, keyword: &str) -> bool {
    let name = name.to_lowercase();
    let keyword = keyword.trim().to_lowercase();
    if keyword.is_empty() {
        return false;
    }
    if keyword.chars().all(|c| c.is_ascii_alphanumeric()) {
        name.split(|c: char| !c.is_alphanumeric())
            .any(|word| word == keyword)
    } else {
        name.contains(&keyword)
    }
}