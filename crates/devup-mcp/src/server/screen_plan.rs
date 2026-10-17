//! `devup_screen_plan`: which Figma screen becomes which URL, decided and
//! confirmed before any page is written.
//!
//! An agent handed "implement this design" used to export and write pages
//! straight away. Afterwards nobody could say which Figma screen had become
//! which URL, so nobody could say what had been built or whether it worked:
//! on one site the pages rendered "This page couldn't load" at some widths and
//! were cut off on the right at others, and there was no URL-by-width table on
//! which anyone could have noticed.
//!
//! So the order is fixed. `draft` maps every screen to a route from evidence
//! only - an existing page built from the frame, a nav label already bound to
//! an href, an explicit `navigationTargets` entry or the project's config -
//! and turns everything it cannot decide into questions shaped for the
//! runtime's question tool, split into rounds that are all asked up front, back
//! to back. A guess from a layer name is never silent: it is marked
//! `name-match` and put to the user in the same rounds. `confirm` takes every
//! answer that applies at once and refuses to proceed with any missing. After
//! it nothing is asked: implementation records what it decided alone with
//! `assume`. `status` reads the confirmed plan against the routes on disk and a
//! runtime audit report, one row per URL and width, where a width nobody ran is
//! `not-run` and never `pass`.
//!
//! Nothing here reaches Figma or runs a browser. The screen list comes from
//! `devup_figma_explore` (or the caller, from an earlier explore), the routes
//! from `devup_project_context`, the links from the navigation parser and the
//! widths from `devup_runtime_audit`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use devup_mcp_figma::{DevupError, ErrorCode, FigmaTarget};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::artifacts::ArtifactStore;
use super::output::{OutputPolicy, OutputTransaction};

pub(super) const PLAN_KIND: &str = "devup-screen-plan";
const SCHEMA_VERSION: u64 = 1;
/// Where the plan lives, relative to the project root. The export gate reads
/// it from the same place, so the two cannot disagree about which plan holds.
pub(super) const PLAN_FILE: &str = ".devup/screen-plan.json";
const PLAN_MARKDOWN: &str = ".devup/screen-plan.md";
pub(super) const ASK_INSTRUCTION: &str = "Ask every question now, before implementing, with your question tool and never in prose. If the tool has no per-call cap (OpenCode `question`), ask the flat `questions` list in one call; each conditional question says when it applies. If it has one (Claude Code AskUserQuestion: 4), ask the questionRounds back to back in consecutive calls with no other work in between, skipping a question whose `when` was not met. Then confirm. After confirm never ask the user anything: take the recommended or most conservative option and record it with action assume.";
/// What implementation does instead of asking. Someone may have walked away
/// from the keyboard three hours ago; a question then stops all the work.
pub(super) const IMPLEMENT_INSTRUCTION: &str = "Implement URL by URL without asking the user anything. devup_figma_export with projectRoot takes each page's route from this plan. Anything new: take the recommended or most conservative option, record it with devup_screen_plan action assume, and keep going. Then run devup_runtime_audit at every width and devup_screen_plan action status.";
/// The optional project file whose entries count as evidence.
pub(super) const CONFIG_FILE: &str = ".devup/screen-plan.config.json";
/// The question tools this is shaped for cut headers short. OpenCode's has no
/// hard limit; 30 characters stays readable in all of them.
pub(super) const HEADER_LIMIT: usize = 30;
/// Claude Code's AskUserQuestion takes two to four options and adds its own
/// "Other" for a custom answer. Four is the most every runtime accepts.
const MAX_OPTIONS: usize = 4;
/// AskUserQuestion also takes at most four questions per call.
pub(super) const MAX_QUESTIONS_PER_ROUND: usize = 4;
const RECOMMENDED: &str = " (Recommended)";
const MAX_SCREENS: usize = 100;
const MAX_PLAN_BYTES: usize = 2 * 1024 * 1024;
/// The widths `devup_runtime_audit` plans by default. Without a report they
/// are what `status` lists as not-run, because the failures this tool exists
/// to surface were at widths nobody drew.
const DEFAULT_WIDTHS: [u32; 4] = [1440, 1100, 858, 390];
const MAX_SCANNED_SOURCES: usize = 400;
const MAX_SOURCE_BYTES: u64 = 512 * 1024;
const SKIP: &str = "Skip this screen";
const NOT_A_PAGE: &str = "Not a page (part of another screen)";

const MODAL_WORDS: &[&str] = &[
    "modal",
    "popup",
    "pop-up",
    "pop up",
    "dialog",
    "overlay",
    "bottom sheet",
    "bottomsheet",
    "drawer",
    "toast",
    "팝업",
    "모달",
    "레이어",
];
const DEVICE_WORDS: &[&str] = &[
    "pc", "mobile", "mo", "mob", "m", "tablet", "tab", "desktop", "web", "dt",
];
/// Device words that name a whole section rather than a frame suffix.
const SECTION_DEVICE_WORDS: &[&str] = &["phone", "모바일", "태블릿", "데스크톱", "데스크탑", "웹"];
const ROOT_WORDS: &[&str] = &["home", "main", "index", "landing", "메인", "홈"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct ScreenPlanInput {
    /// draft: map screens to routes and return every open question.
    /// confirm: apply all answers at once. status: URL x width report.
    /// assume: after confirm, record a decision implementation made without
    /// asking.
    #[schemars(extend("enum" = ["draft", "confirm", "status", "assume"]))]
    pub action: String,
    /// The Next App Router project. The plan is written to
    /// <projectRoot>/.devup/screen-plan.json and .md, under an allowed root.
    pub project_root: Option<String>,
    /// draft: Figma Section or frame link. Omitted while one Devup Bridge
    /// plugin is attached, the Figma selection is the anchor.
    #[serde(default)]
    pub url: Option<String>,
    /// draft: screens already listed by devup_figma_explore or search. When
    /// given, Figma is not read again.
    #[serde(default)]
    pub screens: Vec<PlanScreenInput>,
    /// draft: explicit Figma node id -> route bindings. The only way to name a
    /// route that no page or nav link already shows.
    #[serde(default)]
    pub navigation_targets: BTreeMap<String, String>,
    /// draft: free text recorded in the plan, never interpreted.
    #[serde(default)]
    pub notes: Option<String>,
    /// confirm/status: the planId draft returned.
    #[serde(default)]
    pub plan_id: Option<String>,
    /// confirm: every question id -> the chosen option label (or labels when
    /// multiple), or custom text.
    #[serde(default)]
    pub answers: BTreeMap<String, AnswerInput>,
    /// status: the runtime audit plan JSON, together with auditReportPath.
    #[serde(default)]
    pub audit_plan_path: Option<String>,
    /// status: the report.json the runtime audit executor wrote.
    #[serde(default)]
    pub audit_report_path: Option<String>,
    /// draft: re-read Figma instead of reusing a cached exploration.
    #[serde(default)]
    pub refresh: bool,
    /// assume: what was decided during implementation without asking.
    #[serde(default)]
    pub assumptions: Vec<AssumptionInput>,
    /// draft: how many questions one question-tool call takes; default 4
    /// (Claude Code AskUserQuestion). A tool with no cap asks the flat
    /// `questions` list in one call instead.
    #[serde(default)]
    #[schemars(range(min = 1, max = 50))]
    pub max_questions_per_round: Option<usize>,
    /// draft: Figma nodes that may be the favicon or share image (a logo, a
    /// frame named og or share), beside the screens.
    #[serde(default)]
    pub assets: Vec<PlanAssetInput>,
    /// draft: the agent's own questions - anything it would otherwise guess
    /// during the job (form target, contact details, external links, legal
    /// pages, data source, language) - asked in the same up-front rounds.
    #[serde(default)]
    pub extra_questions: Vec<ExtraQuestionInput>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct ExtraQuestionInput {
    /// Becomes `extra:<id>`.
    pub id: String,
    /// 30 characters or fewer.
    pub header: String,
    pub question: String,
    /// Two to four, the recommended one first.
    pub options: Vec<ExtraOptionInput>,
    #[serde(default)]
    pub multiple: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct ExtraOptionInput {
    pub label: String,
    pub description: String,
}

/// The agent's own questions in the plan's shape, or why one cannot be asked
/// as given. The recommended option is the first, as everywhere else.
fn extra_questions(extra: &[ExtraQuestionInput]) -> Result<Vec<Value>, DevupError> {
    let mut seen = BTreeSet::new();
    extra
        .iter()
        .map(|asked| {
            let id = format!("extra:{}", asked.id.trim().trim_start_matches("extra:"));
            if id == "extra:" || !seen.insert(id.clone()) {
                return Err(invalid(format!(
                    "extraQuestions ids must be non-empty and unique; {:?} is not.",
                    asked.id
                )));
            }
            if asked.header.trim().is_empty() || asked.header.chars().count() > HEADER_LIMIT {
                return Err(invalid(format!(
                    "{id}: header must be 1 to {HEADER_LIMIT} characters."
                )));
            }
            if !(2..=MAX_OPTIONS).contains(&asked.options.len()) {
                return Err(invalid(format!("{id}: give 2 to {MAX_OPTIONS} options.")));
            }
            let choices = asked
                .options
                .iter()
                .map(|option| {
                    let label = strip_recommended(&option.label).to_owned();
                    choice(
                        label.clone(),
                        option.description.clone(),
                        json!({"extra": label}),
                    )
                })
                .collect::<Vec<_>>();
            let mut built = question(
                id,
                asked.header.clone(),
                asked.question.clone(),
                choices,
                "extra",
            );
            built["multiple"] = json!(asked.multiple);
            Ok(built)
        })
        .collect()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct AssumptionInput {
    /// What it is about, e.g. a frame id or a URL.
    pub about: String,
    /// The option taken: the plan's recommended one or the most conservative.
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// `.devup/screen-plan.config.json`: how this project's designers name
/// things. Each entry present counts as evidence and replaces the question
/// it would otherwise take.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PlanConfig {
    /// Replaces the built-in modal words (modal, popup, dialog, ...).
    #[serde(default)]
    pub modal_keywords: Option<Vec<String>>,
    /// Layer name -> route.
    #[serde(default)]
    pub route_aliases: BTreeMap<String, String>,
    /// responsive | separate | ask (the default). Applies to several widths
    /// in one section; frames in different sections are always asked.
    #[serde(default)]
    pub width_grouping: Option<String>,
    /// Replaces the built-in words that mark pages, sections and frames as
    /// not for implementation (trash, archive, draft, copy, ...).
    #[serde(default)]
    pub exclude_keywords: Option<Vec<String>>,
    /// Names containing one of these are never excluded by keyword.
    #[serde(default)]
    pub include_keywords: Option<Vec<String>>,
    /// Pre-answers for the site questions: the favicon and share image (a
    /// path, or figma:<nodeId>), the site name, its production URL, and
    /// motion beyond hover (none | reveal).
    #[serde(default)]
    pub favicon: Option<String>,
    #[serde(default)]
    pub og_image: Option<String>,
    #[serde(default)]
    pub site_name: Option<String>,
    #[serde(default)]
    pub site_url: Option<String>,
    #[serde(default)]
    pub motion: Option<String>,
}

fn read_config(project_root: &str) -> Result<Option<PlanConfig>, DevupError> {
    let path = Path::new(project_root).join(CONFIG_FILE);
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(None);
    };
    let mut config: PlanConfig = serde_json::from_slice(&bytes).map_err(|error| {
        invalid(format!(
            "{} is not a valid screen plan config: {error}",
            path.display()
        ))
    })?;
    if let Some(mode) = config.width_grouping.as_deref()
        && !matches!(mode, "responsive" | "separate" | "ask")
    {
        return Err(invalid(format!(
            "{}: widthGrouping must be responsive, separate or ask, not {mode:?}.",
            path.display()
        )));
    }
    for (alias, route) in &mut config.route_aliases {
        *route = normalize_route(route)
            .filter(|route| valid_route(route))
            .ok_or_else(|| {
                invalid(format!(
                    "{}: routeAliases[{alias:?}] = {route:?} is not an app route.",
                    path.display()
                ))
            })?;
    }
    Ok(Some(config))
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum AnswerInput {
    One(String),
    Many(Vec<String>),
}

impl AnswerInput {
    fn values(&self) -> Vec<&str> {
        match self {
            Self::One(value) => vec![value.as_str()],
            Self::Many(values) => values.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct PlanScreenInput {
    pub node_id: String,
    pub name: String,
    pub width: f64,
    #[serde(default)]
    pub height: Option<f64>,
    /// The Figma Section (or other parent) the frame sits in, by id or name.
    /// devup_figma_explore fills it from the frame's parent.
    #[serde(default)]
    pub section: Option<String>,
    /// The section's layer name, when `section` is an id.
    #[serde(default)]
    pub section_name: Option<String>,
    /// Layer names from the page down to this frame, as devup_figma_explore
    /// reports them. Used to keep the plan inside the linked node and out of
    /// trash, archive and draft areas.
    #[serde(default)]
    pub breadcrumb: Vec<String>,
    /// Ancestor node ids, nearest first, when known.
    #[serde(default)]
    pub ancestors: Vec<String>,
    /// false when the frame is hidden in Figma.
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub opacity: Option<f64>,
    /// Shared part names the caller saw in this screen, e.g. Header, Footer.
    #[serde(default)]
    pub parts: Vec<String>,
    /// Links the caller saw in this screen, e.g. prototype destinations.
    #[serde(default)]
    pub links: Vec<PlanLinkInput>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct PlanLinkInput {
    pub label: String,
    #[serde(default)]
    pub destination_node_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

fn invalid(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    super::format_epoch_rfc3339(seconds)
}

/// What the project already says about its routes, gathered once per call.
#[derive(Debug, Clone, Default)]
pub(super) struct ProjectEvidence {
    /// (route pattern, page file) from the App Router inventory.
    pub routes: Vec<(String, String)>,
    /// (link label, href, file:line) for every static link in scanned sources.
    pub nav_links: Vec<(String, String, String)>,
    /// Components the app's layouts import, so every page already shares them.
    pub layout_parts: Vec<String>,
    /// `src/app` or `app`, whichever the existing pages use.
    pub app_dir: String,
    pub inventory_note: Option<String>,
    /// Figma node id -> (route, file) for every `data-figma-node-id` already
    /// in one route's sources: that page was built from that frame.
    pub figma_nodes: BTreeMap<String, (String, String)>,
    pub config: PlanConfig,
    /// The config file, when one was read.
    pub config_file: Option<String>,
    /// The project root the relative paths here are under.
    pub root: PathBuf,
    /// Route pages and the components they render, relative to `root`.
    pub sources: Vec<String>,
    pub icon_files: Vec<String>,
    pub og_files: Vec<String>,
    /// (value, where it was found).
    pub site_name: Option<(String, String)>,
    pub site_url: Option<(String, String)>,
    pub package_name: Option<String>,
    pub analytics: Vec<String>,
    pub not_found: Option<String>,
    pub lang: Option<(String, String)>,
    /// (version as declared or resolved, where it was read).
    pub next_version: Option<(String, String)>,
    pub react_version: Option<(String, String)>,
    /// Page-transition and animation libraries the project depends on.
    pub motion_libraries: Vec<String>,
}

/// The `data-figma-node-id="..."` values in one source file.
fn figma_node_ids(source: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find("data-figma-node-id=") {
        rest = &rest[start + "data-figma-node-id=".len()..];
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        rest = &rest[1..];
        if let Some(end) = rest.find(quote) {
            ids.push(rest[..end].to_owned());
            rest = &rest[end..];
        }
    }
    ids
}

/// Reads routes, layout components and static links through the same
/// inventory `devup_project_context scope:"ui"` returns, then the navigation
/// parser `devup_ui_validate` uses on each scanned file.
pub(super) async fn project_evidence(project_root: &str) -> Result<ProjectEvidence, DevupError> {
    let inventory = super::project_context::run("ui", Some(project_root), None).await?;
    let config = read_config(project_root)?;
    let mut evidence = ProjectEvidence {
        app_dir: "src/app".into(),
        config_file: config.is_some().then(|| CONFIG_FILE.to_owned()),
        config: config.unwrap_or_default(),
        ..ProjectEvidence::default()
    };
    evidence.root = PathBuf::from(project_root);
    if inventory["found"] != true {
        read_site_facts(&PathBuf::from(project_root), &mut evidence);
        evidence.inventory_note = Some(
            "No App Router source was found; every route is unverified until pages exist or navigationTargets name them."
                .into(),
        );
        return Ok(evidence);
    }
    if inventory["truncation"]["truncated"] == true || inventory["unparsedFiles"] != 0 {
        evidence.inventory_note = Some(
            "The route inventory is truncated or has unparsed files; a route missing here may still exist.".into(),
        );
    }
    let root = PathBuf::from(text(&inventory["projectRoot"]));
    evidence.root = root.clone();
    let mut sources = BTreeSet::new();
    let mut owner = BTreeMap::<String, BTreeSet<String>>::new();
    for route in array(&inventory["routes"]) {
        let path = text(&route["route"]).to_owned();
        evidence
            .routes
            .push((path.clone(), text(&route["page"]).to_owned()));
        sources.insert(text(&route["page"]).to_owned());
        owner
            .entry(text(&route["page"]).to_owned())
            .or_default()
            .insert(path.clone());
        for component in array(&route["components"]) {
            sources.insert(text(component).to_owned());
            owner
                .entry(text(component).to_owned())
                .or_default()
                .insert(path.clone());
        }
    }
    if evidence
        .routes
        .iter()
        .any(|(_, page)| page.starts_with("app/"))
        && !evidence
            .routes
            .iter()
            .any(|(_, page)| page.starts_with("src/app/"))
    {
        evidence.app_dir = "app".into();
    }
    for component in array(&inventory["components"]) {
        sources.insert(text(&component["path"]).to_owned());
        let in_layout = array(&component["importedBy"]).iter().any(|site| {
            let path = text(&site["path"]);
            path.ends_with("layout.tsx") || path.ends_with("layout.jsx")
        });
        if in_layout {
            for name in array(&component["exports"]) {
                let name = text(name);
                if !name.is_empty()
                    && name != "default"
                    && !evidence.layout_parts.iter().any(|part| part == name)
                {
                    evidence.layout_parts.push(name.to_owned());
                }
            }
        }
    }
    for relative in sources
        .iter()
        .filter(|path| !path.is_empty())
        .take(MAX_SCANNED_SOURCES)
    {
        let path = root.join(relative);
        if std::fs::metadata(&path).map_or(true, |meta| meta.len() > MAX_SOURCE_BYTES) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        // A frame id in exactly one route's files says that route was built
        // from it; in several, it is a shared part and says nothing.
        if let Some(routes) = owner.get(relative).filter(|routes| routes.len() == 1) {
            for id in figma_node_ids(&source) {
                evidence.figma_nodes.entry(id).or_insert_with(|| {
                    (
                        routes.iter().next().cloned().unwrap_or_default(),
                        relative.clone(),
                    )
                });
            }
        }
        for element in array(&super::navigation::source_evidence(&source)["elements"]) {
            if element["isLink"] == true
                && let Some(href) = element["href"].as_str()
                && href.starts_with('/')
                && !text(&element["name"]).is_empty()
            {
                evidence.nav_links.push((
                    text(&element["name"]).to_owned(),
                    href.to_owned(),
                    format!("{relative}:{}", element["line"]),
                ));
            }
        }
    }
    evidence.sources = sources
        .iter()
        .filter(|path| !path.is_empty())
        .take(MAX_SCANNED_SOURCES)
        .cloned()
        .collect();
    read_site_facts(&root, &mut evidence);
    Ok(evidence)
}

/// Lowercased words of a layer name with widths and device words removed, so
/// `About - 1440`, `[MO] About_390` and `about` are one screen.
fn base_words(name: &str) -> Vec<String> {
    let lowered = name.to_lowercase();
    let words = lowered
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let kept = words
        .iter()
        .filter(|word| {
            let digits = word.trim_end_matches("px");
            let width = !digits.is_empty()
                && digits.chars().all(|c| c.is_ascii_digit())
                && digits
                    .parse::<u32>()
                    .is_ok_and(|n| (240..=4000).contains(&n));
            !width && !DEVICE_WORDS.contains(&word.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    if kept.is_empty() { words } else { kept }
}

/// The URL slug a name would have, from its ASCII words only. Empty for a
/// name with none, which is why a slug is offered as an option and never
/// proposed on its own.
fn slug(words: &[String]) -> String {
    words
        .iter()
        .filter(|word| word.chars().all(|c| c.is_ascii_alphanumeric()))
        .cloned()
        .collect::<Vec<_>>()
        .join("-")
}

fn compact(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// The same rule `pageScaffold.route` is held to, so a confirmed route is one
/// the export will accept.
pub(super) fn valid_route(route: &str) -> bool {
    let trimmed = route.trim_matches('/');
    route.starts_with('/')
        && !route.starts_with("//")
        && !route.ends_with("//")
        && trimmed.split('/').all(|segment| {
            (trimmed.is_empty() || !segment.is_empty())
                && segment != "."
                && segment != ".."
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.[]()@".contains(c))
        })
}

fn normalize_route(href: &str) -> Option<String> {
    let path = href.split(['?', '#']).next().unwrap_or_default().trim();
    if !path.starts_with('/') || path.starts_with("//") {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    Some(if trimmed.is_empty() {
        "/".to_owned()
    } else {
        trimmed.to_owned()
    })
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_owned();
    }
    let mut cut = value.chars().take(limit - 1).collect::<String>();
    cut.push('…');
    cut
}

fn header(prefix: &str, name: &str) -> String {
    truncate(&format!("{prefix}: {name}"), HEADER_LIMIT)
}

/// One option as the question tool shows it, and what choosing it does.
struct Choice {
    label: String,
    description: String,
    action: Value,
}

fn choice(label: impl Into<String>, description: impl Into<String>, action: Value) -> Choice {
    Choice {
        label: label.into(),
        description: description.into(),
        action,
    }
}

/// A question in the shape every runtime's question tool takes: the
/// recommended option first with " (Recommended)" on its label, two to four
/// options, and custom answers allowed. `resolve` stays in the plan file and is
/// stripped from what the caller is asked.
fn question(id: String, header: String, prompt: String, choices: Vec<Choice>, kind: &str) -> Value {
    let mut seen = BTreeSet::new();
    let choices = choices
        .into_iter()
        .filter(|choice| seen.insert(choice.label.clone()))
        .take(MAX_OPTIONS)
        .collect::<Vec<_>>();
    let options = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            json!({
                "label": if index == 0 { format!("{}{RECOMMENDED}", choice.label) } else { choice.label.clone() },
                "description": choice.description,
            })
        })
        .collect::<Vec<_>>();
    let resolve = choices
        .iter()
        .map(|choice| json!({"label": choice.label, "action": choice.action}))
        .collect::<Vec<_>>();
    json!({
        "id": id,
        "header": header,
        "question": prompt,
        "options": options,
        "multiple": false,
        "custom": true,
        "kind": kind,
        "resolve": resolve,
    })
}

/// The questions as they go to the question tool: no resolution table.
fn ask_shape(questions: &[Value]) -> Vec<Value> {
    ask_shape_with(questions, questions)
}

/// As [`ask_shape`], with `all` the whole plan's questions so a conditional
/// question can say in its own text when it applies - the flat list may be
/// asked in one call, where the answer it depends on is not yet known.
fn ask_shape_with(all: &[Value], questions: &[Value]) -> Vec<Value> {
    questions
        .iter()
        .map(|question| {
            let mut shaped = question.clone();
            let conditions = array(&question["when"])
                .iter()
                .map(|condition| {
                    let parent = all
                        .iter()
                        .find(|other| other["id"] == condition["question"])
                        .map_or_else(
                            || text(&condition["question"]).to_owned(),
                            |other| text(&other["header"]).to_owned(),
                        );
                    format!("\"{parent}\" is \"{}\"", text(&condition["answer"]))
                })
                .collect::<Vec<_>>();
            if !conditions.is_empty() {
                shaped["question"] = json!(format!(
                    "{} (Only if {}.)",
                    text(&question["question"]),
                    conditions.join(" and ")
                ));
            }
            if let Some(object) = shaped.as_object_mut() {
                object.remove("resolve");
                object.remove("kind");
                object.remove("fix");
                if let Some(conditions) = object.get_mut("when").and_then(Value::as_array_mut) {
                    for condition in conditions {
                        if let Some(condition) = condition.as_object_mut() {
                            condition.remove("action");
                        }
                    }
                }
            }
            shaped
        })
        .collect()
}

#[derive(Debug, Clone)]
struct Frame {
    node_id: String,
    name: String,
    width: f64,
    height: Option<f64>,
    section: Option<String>,
    section_name: Option<String>,
}

impl Frame {
    fn json(&self) -> Value {
        json!({"nodeId": self.node_id, "name": self.name, "width": self.width, "height": self.height,
            "section": self.section, "sectionName": self.section_name})
    }
}

/// How the frames of one name are turned into pages.
#[derive(Debug, Clone, PartialEq)]
enum Grouping {
    /// Decided: one page, from a single frame, project config, or evidence.
    Fixed(&'static str),
    /// Several widths in one section: asked, one responsive page recommended.
    Ask {
        recommend_single: bool,
        reason: String,
    },
    /// The same screen name in different sections. A designer usually does
    /// that for a reason, so this is never resolved here: the question names
    /// the sections and frames and offers to have the Figma fixed instead.
    Sections {
        all_device: bool,
        sections: Vec<String>,
        why: String,
    },
}

#[derive(Debug, Clone)]
struct DraftScreen {
    frames: Vec<Frame>,
    words: Vec<String>,
    parts: Vec<String>,
    links: Vec<PlanLinkInput>,
    grouping: Grouping,
    /// Conditions every question about this screen carries: a structure
    /// question about it must have been answered with "keep" for any of them
    /// to apply.
    extra_when: Vec<Value>,
    /// The structure questions this screen is part of, so confirm can tell
    /// whether the user asked for a Figma fix instead.
    structure: Vec<String>,
}

impl DraftScreen {
    fn id(&self) -> &str {
        &self.frames[0].node_id
    }
    fn name(&self) -> &str {
        &self.frames[0].name
    }
    fn widths(&self) -> Vec<u32> {
        widths_of(&self.frames)
    }
    /// The question that decides how this screen's frames become pages.
    fn grouping_question(&self) -> Option<String> {
        match self.grouping {
            Grouping::Fixed(_) => None,
            Grouping::Ask { .. } => Some(format!("group:{}", self.id())),
            Grouping::Sections { .. } => Some(format!("sections:{}", self.id())),
        }
    }
}

fn widths_of(frames: &[Frame]) -> Vec<u32> {
    let mut widths = frames
        .iter()
        .map(|frame| frame.width.round().max(0.0) as u32)
        .collect::<Vec<_>>();
    widths.sort_unstable_by(|a, b| b.cmp(a));
    widths.dedup();
    widths
}

fn width_list(widths: &[u32]) -> String {
    widths
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether a section name says only which device it is for: `PC`,
/// `Mobile`, `Tablet`, `모바일`.
fn device_name(name: &str) -> bool {
    let lowered = name.to_lowercase();
    let words = lowered
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    !words.is_empty()
        && words.iter().all(|word| {
            DEVICE_WORDS.contains(word)
                || SECTION_DEVICE_WORDS.contains(word)
                || word.chars().all(|c| c.is_ascii_digit())
        })
}

fn section_label(frame: &Frame) -> String {
    frame
        .section_name
        .clone()
        .or_else(|| frame.section.clone())
        .unwrap_or_else(|| "(no section)".to_owned())
}

fn describe_frames(frames: &[Frame]) -> String {
    frames
        .iter()
        .map(|frame| {
            format!(
                "\"{}\" ({}, {}px, section {})",
                frame.name,
                frame.node_id,
                frame.width.round(),
                section_label(frame)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Frames that share a base name are candidates for one page. Whether they
/// are one is not assumed:
///
/// - several widths in one section that can merge into one responsive tree
///   are asked, recommending one responsive page; `widthGrouping` in the
///   project config decides this case instead of asking;
/// - the same name in different sections - device-named sections included -
///   is always asked, with the facts and a "fix the Figma" option, because
///   that is a designer's structure and not this server's to correct;
/// - the same name at the same width in one section is not grouped; it is
///   flagged as a possible duplicate by [`structure_questions`].
fn group_screens(screens: &[PlanScreenInput], config: &PlanConfig) -> Vec<DraftScreen> {
    let mut order = Vec::<String>::new();
    let mut groups = BTreeMap::<String, Vec<&PlanScreenInput>>::new();
    for screen in screens {
        let key = base_words(&screen.name).join(" ");
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(screen);
    }
    let make = |members: &[&PlanScreenInput], grouping: Grouping| {
        let mut frames = members
            .iter()
            .map(|screen| Frame {
                node_id: screen.node_id.clone(),
                name: screen.name.clone(),
                width: screen.width,
                height: screen.height,
                section: screen.section.clone(),
                section_name: screen.section_name.clone(),
            })
            .collect::<Vec<_>>();
        frames.sort_by(|a, b| b.width.total_cmp(&a.width));
        let mut parts = Vec::<String>::new();
        let mut links = Vec::new();
        for screen in members {
            for part in &screen.parts {
                if !parts.contains(part) {
                    parts.push(part.clone());
                }
            }
            links.extend(screen.links.iter().cloned());
        }
        DraftScreen {
            frames,
            words: base_words(&members[0].name),
            parts,
            links,
            grouping,
            extra_when: Vec::new(),
            structure: Vec::new(),
        }
    };
    let mut result = Vec::new();
    for key in order {
        let members = &groups[&key];
        if members.len() == 1 {
            result.push(make(members, Grouping::Fixed("single-frame")));
            continue;
        }
        // Asked of the base name, not the layer name: `Main - 1440` and
        // `Main - 390` are one name across two widths once the width is gone.
        let named = members
            .iter()
            .map(|screen| (key.as_str(), screen.width))
            .collect::<Vec<_>>();
        let family = devup_mcp_devup_ui::codegen::responsive::breakpoint_family_is_possible(&named);
        let sections = members
            .iter()
            .map(|screen| screen.section.as_deref())
            .collect::<BTreeSet<_>>();
        if sections.len() > 1 {
            let probe = make(members, Grouping::Fixed(""));
            let mut names = Vec::<String>::new();
            for frame in &probe.frames {
                let label = section_label(frame);
                if !names.contains(&label) {
                    names.push(label);
                }
            }
            let all_device = probe
                .frames
                .iter()
                .all(|frame| frame.section_name.as_deref().is_some_and(device_name));
            let mut why = vec!["the same name once widths and device words are removed".to_owned()];
            if family {
                why.push(format!(
                    "their widths ({}) form a desktop-to-mobile pattern",
                    width_list(&probe.widths())
                ));
            }
            result.push(make(
                members,
                Grouping::Sections {
                    all_device,
                    sections: names,
                    why: why.join(", and "),
                },
            ));
            continue;
        }
        if !family {
            for member in members {
                result.push(make(&[member], Grouping::Fixed("single-frame")));
            }
            continue;
        }
        match config.width_grouping.as_deref() {
            Some("responsive") => {
                result.push(make(members, Grouping::Fixed("config:widthGrouping")));
            }
            Some("separate") => {
                for member in members {
                    result.push(make(&[member], Grouping::Fixed("config:widthGrouping")));
                }
            }
            _ => result.push(make(
                members,
                Grouping::Ask {
                    recommend_single: true,
                    reason: "drawn at several widths in one section with matching names".into(),
                },
            )),
        }
    }
    result
}

/// The base name without a trailing part number: `Main 1` and `Main (2)`
/// are parts of one screen if anything is.
fn part_key(words: &[String]) -> Option<String> {
    let (last, rest) = words.split_last()?;
    let numbered =
        last.chars().all(|c| c.is_ascii_digit()) && last.parse::<u32>().is_ok_and(|n| n < 100);
    let rest = rest
        .iter()
        .filter(|word| !matches!(word.as_str(), "part" | "p"))
        .cloned()
        .collect::<Vec<_>>();
    (numbered && !rest.is_empty()).then(|| rest.join(" "))
}

const KEEP_SEPARATE: &str = "They are different screens";
const KEEP_OUTSIDE: &str = "It is its own screen";
pub(super) const FIX_FIGMA: &str = "The Figma structure is wrong: I will fix it";

/// One structural oddity: the frames involved, what looks wrong, and the fix
/// the designer would make.
struct Oddity {
    kind: &'static str,
    screens: Vec<usize>,
    facts: String,
    keep: &'static str,
    keep_description: String,
    fix: String,
}

/// Figma structure that a designer may not have meant, flagged in the same
/// batch with a "fix it" option. Nothing here decides which frame is right.
fn structure_questions(drafts: &[DraftScreen]) -> Vec<Oddity> {
    let mut found = Vec::new();
    let singles = drafts
        .iter()
        .enumerate()
        .filter(|(_, screen)| screen.grouping == Grouping::Fixed("single-frame"))
        .collect::<Vec<_>>();
    // The same name and width in one section.
    let mut duplicates = BTreeMap::<(String, Option<String>, u32), Vec<usize>>::new();
    for (index, screen) in &singles {
        let frame = &screen.frames[0];
        duplicates
            .entry((
                screen.words.join(" "),
                frame.section.clone(),
                widths_of(&screen.frames)[0],
            ))
            .or_default()
            .push(*index);
    }
    for indices in duplicates.values().filter(|indices| indices.len() > 1) {
        let frames = indices
            .iter()
            .map(|index| drafts[*index].frames[0].clone())
            .collect::<Vec<_>>();
        found.push(Oddity {
            kind: "duplicate",
            screens: indices.clone(),
            facts: format!(
                "{} frames share one name and one width in one section: {}.",
                frames.len(),
                describe_frames(&frames)
            ),
            keep: KEEP_SEPARATE,
            keep_description: "Each frame is its own screen and gets its own route.".into(),
            fix: format!(
                "Delete the stale copy or rename the frames so each screen has one name: {}.",
                describe_frames(&frames)
            ),
        });
    }
    // One screen split into numbered parts across sections.
    let mut parts = BTreeMap::<String, Vec<usize>>::new();
    for (index, screen) in drafts.iter().enumerate() {
        if let Some(key) = part_key(&screen.words) {
            parts.entry(key).or_default().push(index);
        }
    }
    for indices in parts.values().filter(|indices| indices.len() > 1) {
        let frames = indices
            .iter()
            .flat_map(|index| drafts[*index].frames.clone())
            .collect::<Vec<_>>();
        let sections = frames
            .iter()
            .map(|frame| frame.section.clone())
            .collect::<BTreeSet<_>>();
        if sections.len() < 2 {
            continue;
        }
        found.push(Oddity {
            kind: "split",
            screens: indices.clone(),
            facts: format!(
                "These numbered frames look like parts of one screen spread across sections: {}.",
                describe_frames(&frames)
            ),
            keep: KEEP_SEPARATE,
            keep_description: "Each part is its own screen and gets its own route.".into(),
            fix: format!(
                "Put the parts of one screen into one frame in one section: {}.",
                describe_frames(&frames)
            ),
        });
    }
    // A frame outside any section beside frames that are in sections.
    let sectioned = drafts
        .iter()
        .flat_map(|screen| &screen.frames)
        .filter_map(|frame| frame.section.as_ref().map(|_| section_label(frame)))
        .collect::<BTreeSet<_>>();
    if !sectioned.is_empty() {
        for (index, screen) in &singles {
            let frame = &screen.frames[0];
            if frame.section.is_some() {
                continue;
            }
            found.push(Oddity {
                kind: "loose",
                screens: vec![*index],
                facts: format!(
                    "\"{}\" ({}, {}px) sits outside any section, while the other frames are in {}.",
                    frame.name,
                    frame.node_id,
                    frame.width.round(),
                    sectioned.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
                keep: KEEP_OUTSIDE,
                keep_description: "Plan it as a screen of its own, outside the sections.".into(),
                fix: format!(
                    "Move \"{}\" ({}) into the section it belongs to, or out of this area.",
                    frame.name, frame.node_id
                ),
            });
        }
    }
    found
}

/// Layer names that mark a Figma node as the site's logo or icon, and as its
/// share image. Frames named like this are site assets, not pages.
const LOGO_WORDS: &[&str] = &[
    "logo",
    "symbol",
    "favicon",
    "appicon",
    "로고",
    "심볼",
    "파비콘",
];
/// Narrow on purpose: a bare "share" or "thumbnail" names real UI - a share
/// sheet, a video thumbnail - and those are screens.
const OG_WORDS: &[&str] = &[
    "og",
    "opengraph",
    "open graph",
    "share image",
    "share card",
    "social image",
    "공유 이미지",
    "대표 이미지",
];
/// Package names that say a project already measures or verifies something.
const ANALYTICS_PACKAGES: &[&str] = &[
    "@vercel/analytics",
    "@next/third-parties",
    "react-ga4",
    "react-gtm-module",
    "@amplitude/analytics-browser",
    "mixpanel-browser",
    "posthog-js",
];
const ANALYTICS_ENV: &[&str] = &[
    "GA_",
    "GTM",
    "GOOGLE_ANALYTICS",
    "GOOGLE_SITE_VERIFICATION",
    "NAVER_SITE_VERIFICATION",
];

fn asset_kind(name: &str) -> Option<&'static str> {
    if LOGO_WORDS.iter().any(|word| keyword_in(name, word)) || keyword_in(name, "app icon") {
        Some("logo")
    } else if OG_WORDS.iter().any(|word| keyword_in(name, word)) {
        Some("og")
    } else {
        None
    }
}

/// A Figma node that may become the favicon or the share image.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct PlanAssetInput {
    pub node_id: String,
    pub name: String,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
}

/// Files under `directory` (relative to `root`) whose names start with one of
/// `prefixes`, as relative paths.
fn files_named(root: &Path, directory: &str, prefixes: &[&str]) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(join_relative(root, directory)) else {
        return Vec::new();
    };
    let mut found = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            let lowered = name.to_lowercase();
            prefixes.iter().any(|prefix| lowered.starts_with(prefix))
        })
        .map(|name| format!("{directory}/{name}"))
        .collect::<Vec<_>>();
    found.sort();
    found
}

fn join_relative(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .filter(|part| !part.is_empty())
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn read_small_up_to(path: &Path, limit: u64) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > limit {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn read_small(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_SOURCE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The quoted value after `marker` in `source`, e.g. the URL in
/// `metadataBase: new URL("...")`.
fn quoted_after(source: &str, marker: &str) -> Option<String> {
    let rest = &source[source.find(marker)? + marker.len()..];
    let quote_at = rest.find(['"', '\'', '`'])?;
    let quote = rest[quote_at..].chars().next()?;
    let value = &rest[quote_at + 1..];
    let end = value.find(quote)?;
    Some(value[..end].to_owned()).filter(|value| !value.is_empty())
}

/// What the project already says about the site as a whole: its icon, share
/// image, name, URL, analytics, language and not-found page.
fn read_site_facts(root: &Path, evidence: &mut ProjectEvidence) {
    let app = evidence.app_dir.clone();
    evidence.icon_files = [
        files_named(root, &app, &["icon.", "favicon.", "apple-icon."]),
        files_named(root, "public", &["favicon", "icon.", "apple-touch-icon"]),
    ]
    .concat();
    evidence.og_files = [
        files_named(root, &app, &["opengraph-image.", "twitter-image."]),
        files_named(root, "public", &["og", "share", "opengraph"]),
    ]
    .concat();
    evidence.not_found = ["tsx", "jsx", "js"]
        .iter()
        .map(|ext| format!("{app}/not-found.{ext}"))
        .find(|path| join_relative(root, path).is_file());
    for ext in ["tsx", "jsx", "js"] {
        let layout = format!("{app}/layout.{ext}");
        let Some(source) = read_small(&join_relative(root, &layout)) else {
            continue;
        };
        evidence.lang = quoted_after(&source, "<html lang=").map(|lang| (lang, layout.clone()));
        if let Some(url) = quoted_after(&source, "metadataBase: new URL(") {
            evidence.site_url = Some((url, format!("metadataBase in {layout}")));
        }
        if let Some(template) = quoted_after(&source, "template:")
            && let Some((_, name)) = template.split_once("%s")
        {
            let name = name.trim_matches(|c: char| c.is_whitespace() || "|-·:".contains(c));
            if !name.is_empty() {
                evidence.site_name = Some((name.to_owned(), format!("title template in {layout}")));
            }
        }
        if source.contains("googletagmanager") || source.contains("gtag(") {
            evidence.analytics.push(format!("a Google tag in {layout}"));
        }
    }
    // The lockfile says what is installed, which a range in package.json
    // does not.
    if let Some(lock) = read_small_up_to(&root.join("package-lock.json"), 8 * 1024 * 1024)
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    {
        for (name, slot) in [
            ("next", &mut evidence.next_version),
            ("react", &mut evidence.react_version),
        ] {
            if let Some(version) =
                lock["packages"][format!("node_modules/{name}")]["version"].as_str()
            {
                *slot = Some((version.to_owned(), "package-lock.json".into()));
            }
        }
    }
    for file in [".env", ".env.local", ".env.production"] {
        let Some(source) = read_small(&root.join(file)) else {
            continue;
        };
        for line in source.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches(['"', '\'']);
            if evidence.site_url.is_none()
                && matches!(
                    key,
                    "NEXT_PUBLIC_SITE_URL" | "SITE_URL" | "NEXT_PUBLIC_BASE_URL"
                )
                && (value.starts_with("http://") || value.starts_with("https://"))
            {
                evidence.site_url = Some((value.to_owned(), format!("{key} in {file}")));
            }
            if evidence.site_name.is_none()
                && matches!(key, "NEXT_PUBLIC_SITE_NAME" | "SITE_NAME")
                && !value.is_empty()
            {
                evidence.site_name = Some((value.to_owned(), format!("{key} in {file}")));
            }
            if ANALYTICS_ENV.iter().any(|prefix| key.contains(prefix)) {
                evidence.analytics.push(format!("{key} in {file}"));
            }
        }
    }
    if let Some(package) = read_small(&root.join("package.json"))
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    {
        evidence.package_name = package["name"].as_str().map(str::to_owned);
        for section in ["dependencies", "devDependencies"] {
            for (name, slot) in [
                ("next", &mut evidence.next_version),
                ("react", &mut evidence.react_version),
            ] {
                if slot.is_none()
                    && let Some(version) = package[section][name].as_str()
                {
                    *slot = Some((version.to_owned(), format!("package.json {section}")));
                }
            }
            for name in MOTION_LIBRARIES {
                if package[section].get(*name).is_some()
                    && !evidence.motion_libraries.iter().any(|known| known == name)
                {
                    evidence.motion_libraries.push((*name).to_owned());
                }
            }
        }
        for section in ["dependencies", "devDependencies"] {
            for name in ANALYTICS_PACKAGES {
                if package[section].get(*name).is_some() {
                    evidence.analytics.push(format!("{name} in package.json"));
                }
            }
        }
    }
}

/// Libraries that already animate page changes. With one present, adding
/// view transitions is a question, not a default.
const MOTION_LIBRARIES: &[&str] = &[
    "framer-motion",
    "motion",
    "next-view-transitions",
    "react-transition-group",
    "@react-spring/web",
    "gsap",
];

/// The interaction recipe per element role: what each one gets on hover,
/// press and focus. The devup-implementation-plan skill carries the same
/// table with the reasoning; status counts elements against it per role.
const INTERACTION_RECIPES: &[(&str, &str)] = &[
    (
        "primary button",
        "hover one token step darker; active scale(0.98) or one more step darker; focusVisible 2px ring with offset; disabled reduced opacity and cursor not-allowed; a loading state (disabled, spinner) while a form it submits is pending",
    ),
    (
        "secondary / ghost / outline button",
        "hover a subtle tinted background; active a slightly stronger tint; the same focus ring and disabled state",
    ),
    (
        "text link / \"더 보기 >\" link",
        "hover a colour change or underline; a trailing chevron moves 2px; focus ring",
    ),
    (
        "card that links",
        "hover a 2-4px lift and a shadow token; the image inside zooms to 1.02-1.04 with overflow hidden; active drops the lift; focus ring on the card",
    ),
    (
        "nav item / tab",
        "hover colour; the current route marked with aria-current=\"page\" and the Figma active style (e.g. the underline the header draws); focus ring",
    ),
    (
        "icon button",
        "hover a background circle or tint; active press; aria-label required",
    ),
    (
        "input / select / textarea",
        "hover border; focus border and ring; invalid state with its message; disabled",
    ),
    (
        "accordion / disclosure / dropdown",
        "aria-expanded; the chevron rotates 180deg",
    ),
    (
        "mobile menu / drawer",
        "an open and close transition, when the design has one",
    ),
];

const RESTRAINT_RULES: &str = "Figma variant states first, otherwise existing theme tokens only (no new colours; record an assumption when none fits); at most two properties change per state; 150-250ms, ease or ease-out, defined once as a shared token or constant; no bounce and no infinite or attention-seeking animation; transforms of 4px or less and scale between 0.97 and 1.05; everything that moves is off under _motionReduce; non-interactive elements get no hover.";

/// `major.minor.patch` from a version or range as package.json writes it:
/// `^16.1.0`, `~19.3.0-canary-...`, `16`. `None` for `latest`,
/// `workspace:*`, `catalog:` and the like, which say nothing about the API.
fn version_triple(value: &str) -> Option<(u64, u64, u64)> {
    let start = value.find(|c: char| c.is_ascii_digit())?;
    if !value[..start]
        .chars()
        .all(|c| matches!(c, '^' | '~' | '>' | '=' | '<' | 'v' | ' '))
    {
        return None;
    }
    let mut parts = value[start..]
        .split(|c: char| !c.is_ascii_digit())
        .take(3)
        .map(|part| part.parse::<u64>().unwrap_or(0));
    Some((
        parts.next()?,
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    ))
}

/// The view-transition default: which mechanism this project's Next.js and
/// React support, or why there is none. Decided from the versions the
/// project declares, against the current Next.js view-transitions guide
/// (App Router, `import { ViewTransition } from 'react'`, React 19.3) and the
/// Next 15 `experimental.viewTransition` flag that preceded it.
fn view_transition_default(evidence: &ProjectEvidence) -> Value {
    let item = |mechanism: &str, decision: String, why: String| json!({"item": "view-transitions", "mechanism": mechanism, "decision": decision, "evidence": why});
    let common = "a short cross-fade of about 250ms, no big slides; shared elements (page title, hero image, a card image on its detail page) named only where the same element is on both pages - a static name as the devup-ui viewTransitionName prop, a per-record name on <ViewTransition name>; `::view-transition { pointer-events: none }`; animation-duration 0s under prefers-reduced-motion; browsers without support navigate normally";
    let Some((next_text, next_from)) = &evidence.next_version else {
        if !evidence.routes.is_empty() {
            return item(
                "skipped",
                "No view transitions until the Next.js version is known.".into(),
                "App Router pages exist but package.json declares no next version".into(),
            );
        }
        return item(
            "css-cross-document",
            format!("@view-transition {{ navigation: auto; }} with a ::view-transition-old/new(root) cross-fade, for full page loads; {common}."),
            "no Next.js in package.json: navigations load whole documents, which is where the CSS rule applies".into(),
        );
    };
    let Some(next) = version_triple(next_text) else {
        return item(
            "skipped",
            "No view transitions until the Next.js version is known.".into(),
            format!(
                "next is {next_text:?} in {next_from}, which names no version; resolve it and redraft"
            ),
        );
    };
    let react = evidence
        .react_version
        .as_ref()
        .and_then(|(text, _)| version_triple(text));
    let react_text = evidence
        .react_version
        .as_ref()
        .map_or("not declared".to_owned(), |(text, _)| text.clone());
    if next.0 >= 16 && react.is_some_and(|react| react >= (19, 3, 0)) {
        return item(
            "react-view-transition",
            format!(
                "import {{ ViewTransition }} from 'react' - no next.config flag - wrapping each page's content in page.tsx (not the layout, which persists); {common}."
            ),
            format!(
                "next {next_text} and react {react_text}: the App Router supports React's ViewTransition with no configuration"
            ),
        );
    }
    if next >= (15, 2, 0) {
        return item(
            "react-unstable-view-transition",
            format!(
                "experimental.viewTransition: true in next.config and import {{ unstable_ViewTransition as ViewTransition }} from 'react', wrapping each page's content; {common}. Record it as an assumption: the Next.js docs mark this flag experimental."
            ),
            format!(
                "next {next_text} with react {react_text}: only the experimental flag is available; Next 16 with React 19.3 needs none"
            ),
        );
    }
    item(
        "skipped",
        "No view transitions.".into(),
        format!(
            "next {next_text} predates view-transition support, and its App Router navigations stay in one document, where the CSS @view-transition rule does not apply"
        ),
    )
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
            "decision": "Root layout metadata with a title template, description and metadataBase when the site URL is known; every page exports title, description and openGraph taken from its Figma heading and lead text.",
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
    let planned = array(&plan["defaults"])
        .iter()
        .find(|default| default["item"] == "view-transitions")
        .cloned()
        .unwrap_or(Value::Null);
    let mechanism = text(&planned["mechanism"]).to_owned();
    let declined = plan["site"]["viewTransitions"]["answer"]["source"] == "keep-library";
    items.push(match mechanism.as_str() {
        "skipped" | "" => json!({"item": "view-transitions", "state": "not-applicable",
            "detail": format!("skipped: {}", text(&planned["evidence"]))}),
        _ if declined => json!({"item": "view-transitions", "state": "not-applicable",
            "detail": "skipped: the user kept the project's own page motion"}),
        _ if transitions => json!({"item": "view-transitions", "state": "filled",
            "detail": format!("enabled ({mechanism})")}),
        _ => json!({"item": "view-transitions", "state": "missing",
            "detail": format!("planned ({mechanism}) but not in the sources yet")}),
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
const EXCLUDE_WORDS: &[&str] = &[
    "trash",
    "휴지통",
    "archive",
    "archived",
    "아카이브",
    "old",
    "구버전",
    "이전",
    "deprecated",
    "사용안함",
    "미사용",
    "backup",
    "백업",
    "draft",
    "wip",
    "작업중",
    "test",
    "테스트",
    "sandbox",
    "playground",
    "copy",
    "사본",
];

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

/// The first exclude keyword in any of `names`, unless an include keyword
/// forces them in.
fn excluded_by<'a>(names: &[(&'a str, &'a str)], config: &PlanConfig) -> Option<String> {
    let include = config.include_keywords.as_deref().unwrap_or_default();
    if names
        .iter()
        .any(|(_, name)| include.iter().any(|keyword| keyword_in(name, keyword)))
    {
        return None;
    }
    let defaults = EXCLUDE_WORDS
        .iter()
        .map(|word| (*word).to_owned())
        .collect::<Vec<_>>();
    let keywords = config.exclude_keywords.as_ref().unwrap_or(&defaults);
    names.iter().find_map(|(place, name)| {
        keywords
            .iter()
            .find(|keyword| keyword_in(name, keyword))
            .map(|keyword| format!("{place} \"{name}\" contains \"{keyword}\""))
    })
}

/// Why a frame is not for implementation, or `None`. The names of the
/// linked node and its ancestors are left out: the user pointed there, which
/// is asked once rather than excluded (see the `scope` question).
fn exclusion_reason(
    screen: &PlanScreenInput,
    linked: &[String],
    config: &PlanConfig,
) -> Option<String> {
    if screen.visible == Some(false) {
        return Some("hidden in Figma".into());
    }
    if screen.opacity.is_some_and(|opacity| opacity <= 0.0) {
        return Some("zero opacity in Figma".into());
    }
    let mut names = vec![("frame", screen.name.as_str())];
    if let Some(section) = screen.section_name.as_deref() {
        names.push(("section", section));
    }
    for ancestor in screen
        .breadcrumb
        .split_last()
        .map(|(_, ancestors)| ancestors)
        .unwrap_or_default()
    {
        names.push(("parent", ancestor.as_str()));
    }
    names.retain(|(place, name)| *place == "frame" || !linked.iter().any(|link| link == name));
    let mut seen = BTreeSet::new();
    names.retain(|(_, name)| seen.insert(*name));
    excluded_by(&names, config)
}

/// One devup_figma_explore candidate node as a screen to plan. Its parent
/// counts as its section only when the breadcrumb shows one between the page
/// and the frame; a frame drawn straight on the page is in no section.
pub(super) fn screen_from_explore(node: &Value) -> Option<PlanScreenInput> {
    let breadcrumb = array(&node["breadcrumb"])
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let parent = node["parentId"].as_str().map(str::to_owned);
    let in_section = breadcrumb.len() >= 3;
    Some(PlanScreenInput {
        node_id: node["nodeId"].as_str()?.to_owned(),
        name: text(&node["name"]).to_owned(),
        width: node["bounds"]["width"].as_f64()?,
        height: node["bounds"]["height"].as_f64(),
        section: parent.clone().filter(|_| in_section),
        section_name: in_section.then(|| breadcrumb[breadcrumb.len() - 2].clone()),
        ancestors: parent.into_iter().collect(),
        visible: node["visible"].as_bool(),
        opacity: node["opacity"].as_f64(),
        breadcrumb,
        ..PlanScreenInput::default()
    })
}

/// Whether a frame is under the node the user linked. A frame that carries
/// no ancestry at all came from the caller's own list and is taken as given;
/// one whose ancestry is known and does not reach the link is never a
/// candidate.
pub(super) fn in_scope(
    screen: &PlanScreenInput,
    anchor_id: Option<&str>,
    anchor_name: Option<&str>,
) -> bool {
    let Some(anchor) = anchor_id else {
        return true;
    };
    if screen.node_id == anchor
        || screen.section.as_deref() == Some(anchor)
        || screen.ancestors.iter().any(|id| id == anchor)
    {
        return true;
    }
    let ancestor_names = screen
        .breadcrumb
        .split_last()
        .map(|(_, ancestors)| ancestors)
        .unwrap_or_default();
    if anchor_name.is_some_and(|name| ancestor_names.iter().any(|ancestor| ancestor == name)) {
        return true;
    }
    screen.section.is_none() && screen.ancestors.is_empty() && screen.breadcrumb.is_empty()
}

fn with_when(mut question: Value, conditions: &[Value]) -> Value {
    if !conditions.is_empty() {
        let mut all = array(&question["when"]).to_vec();
        all.extend(conditions.iter().cloned());
        question["when"] = json!(all);
    }
    question
}

const SECTIONS_SEPARATE: &str = "Separate pages: the sections are different pages";
const SECTIONS_SINGLE: &str = "One responsive page: the sections only split by device";
const KEEP_EXCLUDED: &str = "Keep it excluded";
const BUILD_IT: &str = "Build it";
const ACCEPT_EXCLUSIONS: &str = "Accept the exclusions";
const PLAN_ANYWAY: &str = "Plan them: this is what I linked";
const NOTHING_HERE: &str = "Nothing here is for implementation";

/// One route the evidence names, what kind of evidence it is, and where.
#[derive(Debug, Clone)]
struct RouteHit {
    route: String,
    kind: &'static str,
    detail: String,
}

/// `name-match` is the only kind that is a guess: a layer name compared with
/// a route segment. Every other kind is something the project or the caller
/// already says.
const NAME_MATCH: &str = "name-match";

/// Every route the evidence supports for these frames.
fn route_hits(
    frames: &[Frame],
    words: &[String],
    targets: &BTreeMap<String, String>,
    evidence: &ProjectEvidence,
    notes: &mut Vec<String>,
) -> Vec<RouteHit> {
    let mut hits = Vec::new();
    for frame in frames {
        if let Some(target) = targets.get(&frame.node_id) {
            match normalize_route(target) {
                Some(route) => hits.push(RouteHit {
                    route,
                    kind: "navigationTargets",
                    detail: format!("navigationTargets[{}]", frame.node_id),
                }),
                None => notes.push(format!(
                    "navigationTargets[{}] = {target:?} is not an app route and was not used.",
                    frame.node_id
                )),
            }
        }
        if let Some((route, file)) = evidence.figma_nodes.get(&frame.node_id) {
            hits.push(RouteHit {
                route: route.clone(),
                kind: "figma-node",
                detail: format!("data-figma-node-id=\"{}\" in {file}", frame.node_id),
            });
        }
        for (alias, route) in &evidence.config.route_aliases {
            if alias == &frame.name || compact(alias) == compact(&words.join("")) {
                hits.push(RouteHit {
                    route: route.clone(),
                    kind: "config",
                    detail: format!("{CONFIG_FILE} routeAliases[{alias:?}]"),
                });
            }
        }
    }
    let name = compact(&words.join(""));
    for (label, href, location) in &evidence.nav_links {
        if compact(label).is_empty() || compact(label) != name {
            continue;
        }
        if let Some((route, _)) = evidence
            .routes
            .iter()
            .find(|(route, _)| super::navigation::matches_route(route, href))
        {
            hits.push(RouteHit {
                route: route.clone(),
                kind: "nav-label",
                detail: format!("nav-label: {label:?} -> {href} ({location})"),
            });
        }
    }
    if hits.is_empty() {
        let screen_slug = slug(words);
        let is_root = words.iter().any(|word| ROOT_WORDS.contains(&word.as_str()));
        for (route, page) in &evidence.routes {
            // The last segment only: `/news/[id]` is a detail page, and a
            // screen named News is the list, not that page.
            let last = route
                .trim_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default();
            if (!screen_slug.is_empty() && last == screen_slug) || (route == "/" && is_root) {
                hits.push(RouteHit {
                    route: route.clone(),
                    kind: NAME_MATCH,
                    detail: format!("layer name matches the existing route {route} ({page})"),
                });
            }
        }
    }
    hits
}

/// The route the hits agree on, if they agree.
fn agreed(hits: &[RouteHit]) -> Option<&RouteHit> {
    let distinct = hits
        .iter()
        .map(|hit| hit.route.as_str())
        .collect::<BTreeSet<_>>();
    (distinct.len() == 1).then(|| &hits[0])
}

fn existing_page<'a>(route: &str, evidence: &'a ProjectEvidence) -> Option<&'a str> {
    evidence
        .routes
        .iter()
        .find(|(candidate, _)| candidate == route)
        .map(|(_, page)| page.as_str())
}

fn planned_page(route: &str, app_dir: &str) -> String {
    let route = route.trim_matches('/');
    if route.is_empty() {
        format!("{app_dir}/page.tsx")
    } else {
        format!("{app_dir}/{route}/page.tsx")
    }
}

/// A condition on another question's answer: this question is asked only
/// when that one resolved to `action`. The caller sees the label.
fn when(question: &str, label: &str, action: Value) -> Value {
    json!({"question": question, "answer": label, "action": action})
}

const SINGLE: &str = "One responsive page";
const SEPARATE: &str = "Separate pages";
const ACCEPT_ALL: &str = "Accept all";
const REVIEW: &str = "Review individually";

/// Builds the draft plan. Pure: every input is passed in, so the rules are
/// tested without Figma or a project on disk.
#[cfg(test)]
pub(super) fn draft_plan(
    screens: &[PlanScreenInput],
    targets: &BTreeMap<String, String>,
    evidence: &ProjectEvidence,
    source: Value,
    notes: Option<&str>,
) -> Result<Value, DevupError> {
    draft_plan_with(screens, &[], &[], targets, evidence, source, notes)
}

/// [`draft_plan`] with Figma nodes that may become the favicon or share
/// image.
pub(super) fn draft_plan_with(
    screens: &[PlanScreenInput],
    assets: &[PlanAssetInput],
    extra: &[ExtraQuestionInput],
    targets: &BTreeMap<String, String>,
    evidence: &ProjectEvidence,
    source: Value,
    notes: Option<&str>,
) -> Result<Value, DevupError> {
    if screens.is_empty() {
        return Err(invalid(
            "draft found no screens. Link a Section or frame, or pass screens from devup_figma_explore.",
        ));
    }
    if screens.len() > MAX_SCREENS {
        return Err(invalid(format!(
            "draft plans at most {MAX_SCREENS} screens; link one Section at a time."
        )));
    }
    // Only what the user pointed at, and never what is marked as not for
    // implementation. Both are listed, not dropped.
    let anchor_id = source["nodeId"].as_str();
    let anchor_name = source["anchorName"].as_str();
    let linked = array(&source["anchorBreadcrumb"])
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .chain(anchor_name.map(str::to_owned))
        .collect::<Vec<_>>();
    let mut out_of_scope = Vec::new();
    let mut excluded = Vec::new();
    let mut kept = Vec::new();
    for screen in screens {
        if !in_scope(screen, anchor_id, anchor_name) {
            out_of_scope.push(json!({"nodeId": screen.node_id, "name": screen.name,
                "reason": "not under the linked node; never a candidate"}));
            continue;
        }
        match exclusion_reason(screen, &linked, &evidence.config) {
            Some(reason) => excluded.push((screen, reason)),
            None => kept.push(screen.clone()),
        }
    }
    if kept.is_empty() && excluded.is_empty() {
        return Err(invalid(
            "No screen is under the linked node. Link the Section or frame to plan; the plan never widens past it.",
        ));
    }
    let mut questions = Vec::new();
    let mut base_when = Vec::new();
    let linked_names = linked
        .iter()
        .map(|name| ("linked node", name.as_str()))
        .collect::<Vec<_>>();
    if let Some(reason) = excluded_by(&linked_names, &evidence.config) {
        questions.push(question(
            "scope".into(),
            "Linked area: build it?".into(),
            format!(
                "The link you gave points inside an area whose name marks it as not for implementation: {reason}. Plan the screens under it anyway?"
            ),
            vec![
                choice(
                    PLAN_ANYWAY,
                    "You linked this area on purpose; plan its screens.",
                    json!({"scope": "use"}),
                ),
                choice(
                    NOTHING_HERE,
                    "Build nothing from this link.",
                    json!({"scope": "none"}),
                ),
            ],
            "scope",
        ));
        base_when.push(when("scope", PLAN_ANYWAY, json!({"scope": "use"})));
    }
    if !excluded.is_empty() {
        let listed = excluded
            .iter()
            .take(8)
            .map(|(screen, reason)| format!("\"{}\" ({}): {reason}", screen.name, screen.node_id))
            .collect::<Vec<_>>()
            .join("; ");
        let more = if excluded.len() > 8 {
            format!("; and {} more listed in the plan", excluded.len() - 8)
        } else {
            String::new()
        };
        questions.push(with_when(
            question(
                "exclusions".into(),
                "Excluded areas".into(),
                format!(
                    "These {} frames were excluded as trash, archive, draft, copy or hidden, and will not be built: {listed}{more}. Accept the exclusions?",
                    excluded.len()
                ),
                vec![
                    choice(
                        ACCEPT_EXCLUSIONS,
                        "None of them is built; they stay listed in the plan.",
                        json!({"exclusions": "accept"}),
                    ),
                    choice(
                        REVIEW,
                        "Ask about each one in the next round.",
                        json!({"exclusions": "review"}),
                    ),
                ],
                "exclusions",
            ),
            &base_when,
        ));
        let review = when("exclusions", REVIEW, json!({"exclusions": "review"}));
        for (screen, reason) in &excluded {
            let id = &screen.node_id;
            let conditions = [base_when.clone(), vec![review.clone()]].concat();
            questions.push(with_when(
                question(
                    format!("exclusion:{id}"),
                    header("Excluded", &screen.name),
                    format!(
                        "\"{}\" ({id}) was excluded: {reason}. Keep it out of the build?",
                        screen.name
                    ),
                    vec![
                        choice(
                            KEEP_EXCLUDED,
                            "Not built; listed as excluded.",
                            json!({"build": false}),
                        ),
                        choice(
                            BUILD_IT,
                            "Build it; its URL is asked next.",
                            json!({"build": true}),
                        ),
                    ],
                    "exclusion",
                ),
                &conditions,
            ));
            let screen_slug = slug(&base_words(&screen.name));
            let mut choices = Vec::new();
            if !screen_slug.is_empty() {
                choices.push(choice(
                    format!("/{screen_slug}"),
                    "Named after the layer.",
                    json!({"route": format!("/{screen_slug}")}),
                ));
            }
            choices.push(choice(
                SKIP,
                "Leave it out after all.",
                json!({"skip": true}),
            ));
            if choices.len() < 2 {
                choices.push(choice(
                    NOT_A_PAGE,
                    "It is a section or state of another screen; it gets no URL.",
                    json!({"notPage": true}),
                ));
            }
            let built = [
                conditions,
                vec![when(
                    &format!("exclusion:{id}"),
                    BUILD_IT,
                    json!({"build": true}),
                )],
            ]
            .concat();
            questions.push(with_when(
                question(
                    format!("exclusion-route:{id}"),
                    header("Route", &screen.name),
                    format!("Which URL should \"{}\" ({id}) be served at?", screen.name),
                    choices,
                    "route",
                ),
                &built,
            ));
        }
    }
    let excluded_rows = excluded
        .iter()
        .map(|(screen, reason)| {
            json!({"nodeId": screen.node_id, "name": screen.name, "width": screen.width,
                "section": screen.section_name.clone().or_else(|| screen.section.clone()),
                "reason": reason})
        })
        .collect::<Vec<_>>();
    // A frame named like a logo or a share image is a site asset, not a page.
    let mut site_assets = assets
        .iter()
        .filter_map(|asset| {
            asset_kind(&asset.name).map(|kind| (asset.node_id.clone(), asset.name.clone(), kind))
        })
        .collect::<Vec<_>>();
    kept.retain(|screen| match asset_kind(&screen.name) {
        Some(kind) => {
            site_assets.push((screen.node_id.clone(), screen.name.clone(), kind));
            false
        }
        None => true,
    });
    for asked in extra_questions(extra)? {
        questions.push(with_when(asked, &base_when));
    }
    let defaults = site_defaults(evidence, &kept);
    let cover = kept
        .first()
        .map(|screen| (screen.node_id.clone(), screen.name.clone()));
    let (site, site_asked) = site_questions(
        evidence,
        &site_assets,
        cover
            .as_ref()
            .map(|(node, name)| (node.as_str(), name.as_str())),
    );
    for asked in site_asked {
        questions.push(with_when(asked, &base_when));
    }
    let mut drafts = group_screens(&kept, &evidence.config);
    for draft in &mut drafts {
        draft.extra_when.extend(base_when.iter().cloned());
    }
    for oddity in structure_questions(&drafts) {
        let first = drafts[oddity.screens[0]].id().to_owned();
        let id = format!("structure:{}:{first}", oddity.kind);
        let frames = oddity
            .screens
            .iter()
            .flat_map(|index| drafts[*index].frames.iter().map(Frame::json))
            .collect::<Vec<_>>();
        let mut asked = question(
            id.clone(),
            header("Figma structure", drafts[oddity.screens[0]].name()),
            format!(
                "{} Is that intended, or should the Figma file be fixed? This plan does not decide which frame is right.",
                oddity.facts
            ),
            vec![
                choice(
                    oddity.keep,
                    oddity.keep_description.clone(),
                    json!({"structure": "keep"}),
                ),
                choice(
                    FIX_FIGMA,
                    "These frames are not built. They are listed under Design fixes requested; everything else goes on.",
                    json!({"designFix": true}),
                ),
            ],
            "structure",
        );
        asked["fix"] = json!({"frames": frames, "text": oddity.fix});
        questions.push(with_when(asked, &base_when));
        for index in &oddity.screens {
            drafts[*index]
                .extra_when
                .push(when(&id, oddity.keep, json!({"structure": "keep"})));
            drafts[*index].structure.push(id.clone());
        }
    }
    let mut screen_notes = vec![Vec::new(); drafts.len()];
    let mut decided = drafts
        .iter()
        .enumerate()
        .map(|(index, screen)| {
            let hits = route_hits(
                &screen.frames,
                &screen.words,
                targets,
                evidence,
                &mut screen_notes[index],
            );
            let proposed = agreed(&hits).map(|hit| (hit.route.clone(), hit.kind));
            (hits, proposed)
        })
        .collect::<Vec<_>>();
    // Two screens on one URL is a decision, not a fact: neither keeps it unasked.
    let mut owners = BTreeMap::<String, Vec<usize>>::new();
    for (index, (_, proposed)) in decided.iter().enumerate() {
        if let Some((route, _)) = proposed {
            owners.entry(route.clone()).or_default().push(index);
        }
    }
    for indices in owners.values().filter(|indices| indices.len() > 1) {
        for index in indices {
            if let Some((route, _)) = decided[*index].1.take() {
                screen_notes[*index].push(format!(
                    "{route} is also the route of another screen in this plan."
                ));
            }
        }
    }
    let claimed = decided
        .iter()
        .filter_map(|(_, proposed)| proposed.as_ref().map(|(route, _)| route.clone()))
        .collect::<BTreeSet<_>>();
    let modal_words = evidence
        .config
        .modal_keywords
        .clone()
        .unwrap_or_else(|| MODAL_WORDS.iter().map(|word| (*word).to_owned()).collect());
    let kind_evidence = if evidence.config.modal_keywords.is_some() {
        "config"
    } else {
        NAME_MATCH
    };
    let is_modal = |name: &str| {
        let lowered = name.to_lowercase();
        modal_words
            .iter()
            .any(|word| lowered.contains(&word.to_lowercase()))
    };
    let page_screens = drafts
        .iter()
        .enumerate()
        .filter(|(_, screen)| !is_modal(screen.name()))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let frame_owner = drafts
        .iter()
        .enumerate()
        .flat_map(|(index, screen)| {
            screen
                .frames
                .iter()
                .map(move |frame| (frame.node_id.clone(), index))
        })
        .collect::<BTreeMap<_, _>>();
    let route_choice = |route: &str, why: &str, extra: Value| {
        let description = match existing_page(route, evidence) {
            Some(page) => format!("{why} Exists at {page}; the screen is merged into it."),
            None => format!(
                "{why} New page at {}.",
                planned_page(route, &evidence.app_dir)
            ),
        };
        let mut action = json!({"route": route});
        if let (Some(object), Some(extra)) = (action.as_object_mut(), extra.as_object()) {
            object.extend(extra.clone());
        }
        choice(route, description, action)
    };
    let unclaimed_for = |words: &[String], exclude: &BTreeSet<String>| {
        let mut unclaimed = evidence
            .routes
            .iter()
            .filter(|(route, _)| !claimed.contains(route) && !exclude.contains(route))
            .map(|(route, _)| {
                let score = route
                    .trim_matches('/')
                    .split(['/', '-'])
                    .filter(|segment| words.iter().any(|word| word == segment))
                    .count();
                (score, route.clone())
            })
            .collect::<Vec<_>>();
        unclaimed.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        unclaimed
    };
    let mut rows = Vec::new();
    let mut heuristics = Vec::new();
    let mut part_screens = BTreeMap::<String, Vec<String>>::new();
    for (index, screen) in drafts.iter().enumerate() {
        let (hits, proposed) = &decided[index];
        let modal = is_modal(screen.name());
        let id = screen.id().to_owned();
        let name = screen.name().to_owned();
        let widths = screen.widths();
        let mut decisions = Vec::new();
        let candidates = hits
            .iter()
            .map(|hit| hit.route.clone())
            .collect::<BTreeSet<_>>();
        let screen_slug = slug(&screen.words);
        let new_route = (!screen_slug.is_empty()).then(|| format!("/{screen_slug}"));
        // Questions about this screen as one page apply only if the group is
        // one page; when that is still being asked, they carry the condition.
        let group_id = screen.grouping_question().unwrap_or_default();
        let (single_label, separate_label) = match screen.grouping {
            Grouping::Sections { .. } => (SECTIONS_SINGLE, SECTIONS_SEPARATE),
            _ => (SINGLE, SEPARATE),
        };
        let mut single_when = screen.extra_when.clone();
        if !matches!(screen.grouping, Grouping::Fixed(_)) {
            single_when.push(when(&group_id, single_label, json!({"group": "single"})));
        }
        let push = |questions: &mut Vec<Value>, mut question: Value, extra: &[Value]| {
            let conditions = single_when.iter().chain(extra).cloned().collect::<Vec<_>>();
            if !conditions.is_empty() {
                question["when"] = json!(conditions);
            }
            questions.push(question);
        };
        let grouping_question = match &screen.grouping {
            Grouping::Fixed(_) => None,
            Grouping::Ask {
                recommend_single,
                reason,
            } => {
                let single = choice(
                    SINGLE,
                    "One URL; the widths become devup-ui responsive arrays (responsiveTsx).",
                    json!({"group": "single"}),
                );
                let separate = choice(
                    SEPARATE,
                    "Each frame is its own page and gets its own route, asked next.",
                    json!({"group": "separate"}),
                );
                Some(question(
                    group_id.clone(),
                    header("Pages", &name),
                    format!(
                        "These frames share the name \"{name}\" and are {reason}: {}. One responsive page, or separate pages? A custom answer may list one route per frame, e.g. \"/a, /b\".",
                        describe_frames(&screen.frames)
                    ),
                    if *recommend_single {
                        vec![single, separate]
                    } else {
                        vec![separate, single]
                    },
                    "group",
                ))
            }
            Grouping::Sections {
                all_device,
                sections,
                why,
            } => {
                let separate = choice(
                    SECTIONS_SEPARATE,
                    "Each frame is its own page; its URL is asked next.",
                    json!({"group": "separate"}),
                );
                let single = choice(
                    SECTIONS_SINGLE,
                    if *all_device {
                        format!(
                            "Inferred from the section names ({}), which name only devices.",
                            sections.join(", ")
                        )
                    } else {
                        "The sections would only split one page by device.".to_owned()
                    },
                    json!({"group": "single"}),
                );
                let fix = choice(
                    FIX_FIGMA,
                    "These frames are not built. They are listed under Design fixes requested; everything else goes on.",
                    json!({"designFix": true}),
                );
                let mut asked = question(
                    group_id.clone(),
                    header("Sections", &name),
                    format!(
                        "Frames that look like the same screen sit in different sections ({}): {}. They look related because of {why}. Which is it? A custom answer may list one route per frame, e.g. \"/a, /b\".",
                        sections.join(", "),
                        describe_frames(&screen.frames)
                    ),
                    if *all_device {
                        vec![single, separate, fix]
                    } else {
                        vec![separate, single, fix]
                    },
                    "group",
                );
                asked["fix"] = json!({
                    "frames": screen.frames.iter().map(Frame::json).collect::<Vec<_>>(),
                    "text": format!(
                        "Frames {} share the name \"{name}\" but sit in sections {}. Move them into one section if they are one page, or rename them if they are different pages.",
                        describe_frames(&screen.frames),
                        sections.join(", ")
                    ),
                });
                Some(asked)
            }
        };
        if let Some(asked) = grouping_question {
            decisions.push("one-page-or-separate");
            questions.push(with_when(asked, &screen.extra_when));
            for frame in &screen.frames {
                let mut frame_notes = Vec::new();
                let frame_hits = route_hits(
                    std::slice::from_ref(frame),
                    &screen.words,
                    targets,
                    evidence,
                    &mut frame_notes,
                );
                let mut choices = Vec::new();
                for hit in frame_hits.iter().filter(|hit| hit.kind != NAME_MATCH) {
                    choices.push(route_choice(
                        &hit.route,
                        &format!("From {}.", hit.detail),
                        json!({}),
                    ));
                }
                for hit in frame_hits.iter().filter(|hit| hit.kind == NAME_MATCH) {
                    choices.push(route_choice(
                        &hit.route,
                        "Inferred from the layer name; only one frame can take it.",
                        json!({}),
                    ));
                }
                let section_slug = frame
                    .section_name
                    .as_deref()
                    .map(|section| slug(&base_words(section)))
                    .unwrap_or_default();
                if !section_slug.is_empty() && !screen_slug.is_empty() {
                    choices.push(route_choice(
                        &format!("/{section_slug}/{screen_slug}"),
                        "Named after its Figma section and layer.",
                        json!({}),
                    ));
                }
                for (_, route) in unclaimed_for(&screen.words, &candidates)
                    .iter()
                    .filter(|(score, _)| *score > 0)
                {
                    choices.push(route_choice(
                        route,
                        "Existing route with a similar name.",
                        json!({}),
                    ));
                }
                choices.truncate(MAX_OPTIONS - 1);
                choices.push(choice(
                    SKIP,
                    "Leave this frame out of this build.",
                    json!({"skip": true}),
                ));
                if choices.len() < 2 {
                    choices.push(choice(
                        NOT_A_PAGE,
                        "It is a section or state of another screen; it gets no URL.",
                        json!({"notPage": true}),
                    ));
                }
                let asked = question(
                    format!("split-route:{}", frame.node_id),
                    header("Route", &format!("{} {}", frame.name, frame.width.round())),
                    format!(
                        "As a separate page, which URL should \"{}\" ({}, {}px) be served at?",
                        frame.name,
                        frame.node_id,
                        frame.width.round()
                    ),
                    choices,
                    "route",
                );
                let separate_when = [
                    screen.extra_when.clone(),
                    vec![when(
                        &group_id,
                        separate_label,
                        json!({"group": "separate"}),
                    )],
                ]
                .concat();
                questions.push(with_when(asked, &separate_when));
            }
        }
        let route_evidence = match proposed {
            Some((_, kind)) if *kind == NAME_MATCH => json!(NAME_MATCH),
            Some(_) => json!(
                hits.iter()
                    .map(|hit| hit.detail.clone())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            None => json!("UNVERIFIED"),
        };
        if modal {
            decisions.push("modal-or-route");
            let mut hosts = page_screens
                .iter()
                .filter(|other| **other != index)
                .map(|other| &drafts[*other])
                .map(|host| {
                    let shared = host
                        .words
                        .iter()
                        .filter(|word| screen.words.contains(word))
                        .count();
                    (shared, host)
                })
                .collect::<Vec<_>>();
            hosts.sort_by_key(|host| std::cmp::Reverse(host.0));
            let state_choices = hosts
                .iter()
                .take(2)
                .map(|(_, host)| {
                    choice(
                        format!("State on {}", truncate(host.name(), 40)),
                        format!(
                            "Open/close state inside the page built from {} ({}); no URL of its own.",
                            host.name(),
                            host.id()
                        ),
                        json!({"state": host.id()}),
                    )
                })
                .collect::<Vec<_>>();
            let own_route = proposed
                .as_ref()
                .map(|(route, _)| route_choice(route, "A page with its own URL.", json!({})))
                .or_else(|| {
                    new_route.as_ref().map(|route| {
                        route_choice(
                            route,
                            "A page with its own URL, named after the layer.",
                            json!({}),
                        )
                    })
                });
            let mut choices = Vec::new();
            if proposed
                .as_ref()
                .is_some_and(|(_, kind)| *kind != NAME_MATCH)
            {
                choices.extend(own_route);
                choices.extend(state_choices);
            } else {
                choices.extend(state_choices);
                choices.extend(own_route);
            }
            choices.push(choice(
                SKIP,
                "Leave it out of this build.",
                json!({"skip": true}),
            ));
            let why = if kind_evidence == NAME_MATCH {
                "Its layer name suggests a modal or overlay (name-match)"
            } else {
                "Its layer name matches the project's modalKeywords"
            };
            push(
                &mut questions,
                question(
                    format!("modal:{id}"),
                    header("Modal", &name),
                    format!(
                        "\"{name}\" ({id}): {why}. Is it a state of another screen, or a page with its own URL?"
                    ),
                    choices,
                    "modal",
                ),
                &[],
            );
            if kind_evidence == NAME_MATCH {
                heuristics.push(json!({"screen": id, "name": name, "decision": "kind: modal", "evidence": NAME_MATCH, "askedIn": format!("modal:{id}")}));
            }
        } else if let Some((route, NAME_MATCH)) = proposed {
            decisions.push("route-from-name-match");
            heuristics.push(json!({"screen": id, "name": name, "decision": format!("route: {route}"), "evidence": NAME_MATCH, "askedIn": format!("review:{id}")}));
            let mut choices = vec![
                route_choice(
                    route,
                    "Keep the inferred route.",
                    json!({"collision": "merge"}),
                ),
                choice(
                    format!("Overwrite {route}"),
                    "Keep the inferred route, but replace the existing page file outright.",
                    json!({"route": route, "collision": "overwrite"}),
                ),
            ];
            if let Some(alternative) = new_route
                .as_ref()
                .filter(|candidate| *candidate != route && !claimed.contains(*candidate))
            {
                choices.push(route_choice(
                    alternative,
                    "Named after the layer.",
                    json!({}),
                ));
            }
            choices.push(choice(
                SKIP,
                "Leave it out of this build.",
                json!({"skip": true}),
            ));
            push(
                &mut questions,
                question(
                    format!("review:{id}"),
                    header("Check route", &name),
                    format!(
                        "\"{name}\" ({id}) was matched to {route} by its layer name only. Keep it?"
                    ),
                    choices,
                    "route",
                ),
                &[when("heuristics", REVIEW, json!({"heuristics": "review"}))],
            );
        } else if proposed.is_none() {
            decisions.push(if candidates.len() > 1 {
                "conflicting-route-evidence"
            } else {
                "no-route-evidence"
            });
            let mut choices = Vec::new();
            for route in &candidates {
                choices.push(route_choice(
                    route,
                    "Named by conflicting evidence.",
                    json!({}),
                ));
            }
            let unclaimed = unclaimed_for(&screen.words, &candidates);
            for (_, route) in unclaimed.iter().filter(|(score, _)| *score > 0) {
                choices.push(route_choice(
                    route,
                    "Existing route with a similar name.",
                    json!({}),
                ));
            }
            if let Some(route) = new_route.as_ref().filter(|route| !claimed.contains(*route)) {
                choices.push(route_choice(route, "Named after the layer.", json!({})));
            }
            for (_, route) in unclaimed.iter().filter(|(score, _)| *score == 0).take(1) {
                choices.push(route_choice(
                    route,
                    "Existing route not claimed by another screen.",
                    json!({}),
                ));
            }
            choices.truncate(MAX_OPTIONS - 1);
            choices.push(choice(
                SKIP,
                "Leave it out of this build.",
                json!({"skip": true}),
            ));
            if choices.len() < 2 {
                choices.push(choice(
                    NOT_A_PAGE,
                    "It is a section or state of another screen; it gets no URL.",
                    json!({"notPage": true}),
                ));
            }
            let reason = if candidates.len() > 1 {
                format!(
                    "The evidence names {} different routes.",
                    candidates.iter().cloned().collect::<Vec<_>>().join(", ")
                )
            } else {
                "No existing page, nav link, navigationTargets entry or routeAliases entry names it."
                    .to_owned()
            };
            push(
                &mut questions,
                question(
                    format!("route:{id}"),
                    header("Route", &name),
                    format!(
                        "Which URL should the Figma screen \"{name}\" ({id}, drawn at {}) be served at? {reason}",
                        width_list(&widths)
                    ),
                    choices,
                    "route",
                ),
                &[],
            );
        }
        let route = proposed.as_ref().map(|(route, _)| route.as_str());
        let existing = route.and_then(|route| existing_page(route, evidence));
        let name_matched = proposed
            .as_ref()
            .is_some_and(|(_, kind)| *kind == NAME_MATCH);
        if !modal
            && !name_matched
            && let (Some(route), Some(page)) = (route, existing)
        {
            decisions.push("collides-with-existing-page");
            push(
                &mut questions,
                question(
                    format!("collision:{id}"),
                    header("Existing page", route),
                    format!(
                        "{route} already has a page ({page}). What should building \"{name}\" ({id}) do to it?"
                    ),
                    vec![
                        choice(
                            "Merge into the existing page",
                            "Keep the page file and its data wiring; replace its markup with the screen.",
                            json!({"collision": "merge"}),
                        ),
                        choice(
                            "Overwrite the existing page",
                            "Replace the page file outright; existing wiring is lost.",
                            json!({"collision": "overwrite"}),
                        ),
                        choice(
                            SKIP,
                            "Leave the existing page as it is.",
                            json!({"collision": "skip"}),
                        ),
                    ],
                    "collision",
                ),
                &[],
            );
        }
        if let Some(route) = route.filter(|route| route.contains('[')) {
            decisions.push("dynamic-segment");
            push(
                &mut questions,
                question(
                    format!("dynamic:{id}"),
                    header("Dynamic route", route),
                    format!(
                        "{route} has a dynamic segment. Where does \"{name}\" get the record it shows?"
                    ),
                    vec![
                        choice(
                            "Sample data from the design",
                            "Render the Figma content as fixture data for one id; wire real data later.",
                            json!({"data": "sample"}),
                        ),
                        choice(
                            "Wire to the API now",
                            "Fetch the record by the segment value; needs the endpoint from devup_project_context scope api.",
                            json!({"data": "api"}),
                        ),
                    ],
                    "dynamic",
                ),
                &[],
            );
        }
        if widths.len() > 1 {
            decisions.push("several-widths");
        }
        let mut links = Vec::new();
        for (link_index, link) in screen.links.iter().enumerate() {
            let resolved = link
                .url
                .as_deref()
                .map(|url| json!({"url": url, "evidence": "figma-url"}))
                .or_else(|| {
                    let destination = link.destination_node_id.as_deref()?;
                    targets
                        .get(destination)
                        .map(|url| json!({"url": url, "evidence": format!("navigationTargets[{destination}]")}))
                        .or_else(|| {
                            frame_owner.get(destination).map(|owner| {
                                json!({"screen": drafts[*owner].id(), "evidence": "destination frame in this plan"})
                            })
                        })
                });
            if resolved.is_none() {
                decisions.push("unresolved-link");
                let mut choices = vec![choice(
                    "Leave unlinked",
                    "Render it without an href; wire it once the destination exists.",
                    json!({"link": null}),
                )];
                for (route, _) in evidence.routes.iter().take(MAX_OPTIONS - 1) {
                    choices.push(choice(
                        route.as_str(),
                        format!("Link to the existing route {route}."),
                        json!({"link": route}),
                    ));
                }
                if choices.len() < 2 {
                    choices.push(choice(
                        "Remove the link",
                        "Render the label as plain text.",
                        json!({"link": "remove"}),
                    ));
                }
                questions.push(with_when(question(
                    format!("link:{id}:{link_index}"),
                    header("Link", &link.label),
                    format!(
                        "\"{}\" on \"{name}\" ({id}) links somewhere this plan cannot see{}. Where should it go?",
                        link.label,
                        link.destination_node_id
                            .as_deref()
                            .map(|node| format!(" (Figma destination {node})"))
                            .unwrap_or_default()
                    ),
                    choices,
                    "link",
                ), &screen.extra_when));
            }
            links.push(json!({"label": link.label, "destinationNodeId": link.destination_node_id, "resolved": resolved}));
        }
        let mut shared = evidence.layout_parts.clone();
        for part in &screen.parts {
            part_screens
                .entry(part.clone())
                .or_default()
                .push(id.clone());
            if !shared.contains(part) {
                shared.push(part.clone());
            }
        }
        rows.push(json!({
            "id": id,
            "name": name,
            "kind": if modal { "modal" } else { "page" },
            "kindEvidence": if modal { json!(kind_evidence) } else { Value::Null },
            "frames": screen.frames.iter().map(Frame::json).collect::<Vec<_>>(),
            "drawnWidths": widths,
            "groupingQuestion": screen.grouping_question(),
            "structureQuestions": screen.structure,
            "grouping": match &screen.grouping {
                Grouping::Fixed(evidence) => json!({"mode": "decided", "evidence": evidence}),
                Grouping::Sections { all_device, sections, why } => json!({"mode": "ask-sections", "question": group_id,
                    "sections": sections, "allDeviceNames": all_device, "why": why}),
                Grouping::Ask { recommend_single, reason } => json!({"mode": "ask", "question": group_id,
                    "recommended": if *recommend_single { "single" } else { "separate" }, "reason": reason}),
            },
            "proposedRoute": route,
            "routeEvidence": route_evidence,
            "routeCandidates": candidates,
            "existingFile": existing,
            "sharedParts": shared,
            "links": links,
            "openDecisions": decisions,
            "notes": screen_notes[index],
        }));
    }
    let route_heuristics = heuristics
        .iter()
        .filter(|item| text(&item["askedIn"]).starts_with("review:"))
        .collect::<Vec<_>>();
    if !route_heuristics.is_empty() {
        let listed = route_heuristics
            .iter()
            .map(|item| format!("{} -> {}", text(&item["name"]), text(&item["decision"])))
            .collect::<Vec<_>>()
            .join(", ");
        questions.push(question(
            "heuristics".into(),
            "Inferred from layer names".into(),
            format!(
                "These were inferred from layer names only: {listed}. Accept all (merged into the existing pages), or review each one?"
            ),
            vec![
                choice(
                    ACCEPT_ALL,
                    "Use every inferred route; existing pages are merged into, not overwritten.",
                    json!({"heuristics": "accept"}),
                ),
                choice(
                    REVIEW,
                    "Ask one question per inferred route, in the next round.",
                    json!({"heuristics": "review"}),
                ),
            ],
            "heuristics",
        ));
    }
    for (part, owners) in part_screens.iter().filter(|(part, owners)| {
        owners.len() > 1
            && !evidence
                .layout_parts
                .iter()
                .any(|existing| compact(existing) == compact(part))
    }) {
        questions.push(question(
            format!("layout:{}", compact(part)),
            header("Shared part", part),
            format!(
                "\"{part}\" appears on {} screens and no layout renders it yet. Extract it once into a shared layout?",
                owners.len()
            ),
            vec![
                choice(
                    "Extract into a shared layout",
                    "Build it once and render it from the App Router layout.",
                    json!({"layout": "shared"}),
                ),
                choice(
                    "Keep it in each page",
                    "Every page renders its own copy.",
                    json!({"layout": "per-page"}),
                ),
            ],
            "layout",
        ));
    }
    // Questions about the whole plan wait on the linked area being planned.
    for asked in questions
        .iter_mut()
        .filter(|asked| asked["id"] == "heuristics" || text(&asked["id"]).starts_with("layout:"))
    {
        *asked = with_when(asked.clone(), &base_when);
    }
    let body = json!({"source": source, "screens": rows, "questions": questions, "notes": notes});
    let plan_id = format!(
        "sp-{}",
        &sha256(&serde_json::to_vec(&body).unwrap_or_default())[..16]
    );
    Ok(json!({
        "kind": PLAN_KIND,
        "schemaVersion": SCHEMA_VERSION,
        "planId": plan_id,
        "status": "draft",
        "source": body["source"],
        "notes": notes,
        "appDir": evidence.app_dir,
        "inventoryNote": evidence.inventory_note,
        "config": evidence.config_file,
        "screens": body["screens"],
        "heuristics": heuristics,
        "excluded": excluded_rows,
        "outOfScope": out_of_scope,
        "site": site,
        "defaults": defaults,
        "siteAssets": site_assets.iter().map(|(node, name, kind)| json!({"nodeId": node, "name": name, "kind": kind})).collect::<Vec<_>>(),
        "questions": body["questions"],
        "answers": {},
        "routes": [],
        "assumptions": [],
    }))
}

/// How deep a question sits behind the answers it depends on. A question is
/// asked in a round after every question it depends on.
fn depth(question: &Value, questions: &[Value]) -> usize {
    array(&question["when"])
        .iter()
        .filter_map(|condition| {
            questions
                .iter()
                .find(|other| other["id"] == condition["question"])
        })
        .map(|parent| depth(parent, questions) + 1)
        .max()
        .unwrap_or(0)
}

/// The questions split into rounds a question tool takes in one call each:
/// at most four questions of at most four options, every question exactly
/// once, and a conditional question always in a later round than the one it
/// depends on.
pub(super) fn question_rounds(questions: &[Value], per_round: usize) -> Vec<Value> {
    let mut by_depth = BTreeMap::<usize, Vec<&Value>>::new();
    for question in questions {
        by_depth
            .entry(depth(question, questions))
            .or_default()
            .push(question);
    }
    let mut rounds = Vec::new();
    for level in by_depth.values() {
        for chunk in level.chunks(per_round.max(1)) {
            let shaped = ask_shape_with(
                questions,
                &chunk
                    .iter()
                    .map(|question| (*question).clone())
                    .collect::<Vec<_>>(),
            );
            rounds.push(json!({"round": rounds.len() + 1, "questions": shaped}));
        }
    }
    rounds
}

fn plan_path(project_root: &str, relative: &str) -> String {
    Path::new(project_root)
        .join(relative)
        .to_string_lossy()
        .into_owned()
}

/// Writes the plan JSON and its Markdown twin in one transaction, so the two
/// never describe different plans.
fn write_plan(
    policy: &OutputPolicy,
    project_root: &str,
    plan: &Value,
) -> Result<Value, DevupError> {
    let json_target = policy.resolve(&plan_path(project_root, PLAN_FILE))?;
    let markdown_target = policy.resolve(&plan_path(project_root, PLAN_MARKDOWN))?;
    let written = json!([
        json_target.display_path().display().to_string(),
        markdown_target.display_path().display().to_string(),
    ]);
    let bytes = serde_json::to_vec_pretty(plan)
        .map_err(|error| invalid(format!("Cannot serialize the screen plan: {error}")))?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("screenPlan", json_target, &bytes)?;
    transaction.stage(
        "screenPlanMarkdown",
        markdown_target,
        markdown(plan).as_bytes(),
    )?;
    transaction.commit()?;
    Ok(written)
}

/// Reads `<projectRoot>/.devup/screen-plan.json`, or `None` when there is
/// none. A file that exists but is not a plan is an error, never "no plan".
pub(super) fn read_plan(project_root: &str) -> Result<Option<Value>, DevupError> {
    let path = Path::new(project_root).join(PLAN_FILE);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return Ok(None);
    };
    if metadata.len() > MAX_PLAN_BYTES as u64 {
        return Err(invalid(format!(
            "{} exceeds the plan size limit.",
            path.display()
        )));
    }
    let bytes = std::fs::read(&path)
        .map_err(|error| invalid(format!("Cannot read {}: {error}", path.display())))?;
    let plan: Value = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("{} is not valid JSON: {error}", path.display())))?;
    if plan["kind"] != PLAN_KIND || plan["schemaVersion"] != SCHEMA_VERSION {
        return Err(invalid(format!(
            "{} is not a devup screen plan; create one with devup_screen_plan action draft.",
            path.display()
        )));
    }
    Ok(Some(plan))
}

fn require_root(input: &ScreenPlanInput) -> Result<&str, DevupError> {
    input
        .project_root
        .as_deref()
        .filter(|root| !root.trim().is_empty())
        .ok_or_else(|| invalid(format!("action {} needs projectRoot.", input.action)))
}

/// `draft`: plan the given screens against the project and write both files.
#[cfg(test)]
pub(super) async fn draft(
    input: &ScreenPlanInput,
    screens: &[PlanScreenInput],
    source: Value,
    policy: &OutputPolicy,
) -> Result<Value, DevupError> {
    draft_with(input, screens, &input.assets, source, policy).await
}

/// [`draft`] with the Figma asset candidates an explore found beside the
/// screens.
pub(super) async fn draft_with(
    input: &ScreenPlanInput,
    screens: &[PlanScreenInput],
    assets: &[PlanAssetInput],
    source: Value,
    policy: &OutputPolicy,
) -> Result<Value, DevupError> {
    let root = require_root(input)?;
    let evidence = project_evidence(root).await?;
    let mut plan = draft_plan_with(
        screens,
        assets,
        &input.extra_questions,
        &input.navigation_targets,
        &evidence,
        source,
        input.notes.as_deref(),
    )?;
    let per_round = input
        .max_questions_per_round
        .unwrap_or(MAX_QUESTIONS_PER_ROUND);
    if !(1..=50).contains(&per_round) {
        return Err(invalid("maxQuestionsPerRound must be between 1 and 50."));
    }
    plan["maxQuestionsPerRound"] = json!(per_round);
    plan["createdAt"] = json!(now());
    let replaced = read_plan(root)
        .ok()
        .flatten()
        .filter(|previous| previous["status"] == "confirmed")
        .map(|previous| previous["planId"].clone());
    let written = write_plan(policy, root, &plan)?;
    let questions = ask_shape(array(&plan["questions"]));
    let rounds = question_rounds(array(&plan["questions"]), per_round);
    let unverified = array(&plan["screens"])
        .iter()
        .filter(|screen| screen["routeEvidence"] == "UNVERIFIED")
        .count();
    Ok(json!({
        "status": "draft",
        "planId": plan["planId"],
        "screens": screen_table(&plan),
        "questionRounds": rounds,
        "questions": questions,
        "questionCount": questions.len(),
        "askInstruction": ASK_INSTRUCTION,
        "maxQuestionsPerRound": per_round,
        "site": plan["site"],
        "defaults": plan["defaults"],
        "heuristics": plan["heuristics"],
        "excluded": plan["excluded"],
        "outOfScope": plan["outOfScope"],
        "config": plan["config"],
        "unverifiedRoutes": unverified,
        "inventoryNote": plan["inventoryNote"],
        "writtenTo": written,
        "replacedConfirmedPlan": replaced,
        "nextAction": if questions.is_empty() {
            json!({"tool": "devup_screen_plan", "arguments": {"action": "confirm", "projectRoot": root, "planId": plan["planId"], "answers": {}},
                "how": "Nothing is open. Show the user the screens table, then confirm with empty answers."})
        } else {
            json!({"tool": "devup_screen_plan", "arguments": {"action": "confirm", "projectRoot": root, "planId": plan["planId"], "answers": "<question id -> chosen label, for every question that applies>"},
                "how": ASK_INSTRUCTION})
        },
    }))
}

fn strip_recommended(label: &str) -> &str {
    label
        .trim()
        .strip_suffix(RECOMMENDED)
        .unwrap_or(label.trim())
}

/// The action an answer selects, or the custom text when it matches no
/// option. A custom answer to a question that places a screen must itself be
/// a route; anything else is recorded and the recommended action applies.
fn resolve_answer(
    question: &Value,
    answer: &AnswerInput,
) -> Result<(Value, Option<String>), DevupError> {
    let id = text(&question["id"]);
    let values = answer.values();
    if question["multiple"] == true {
        let labels = values
            .iter()
            .map(|value| strip_recommended(value))
            .collect::<Vec<_>>();
        return Ok((json!({"extra": labels}), None));
    }
    let [value] = values.as_slice() else {
        return Err(invalid(format!("{id} takes exactly one answer.")));
    };
    let label = strip_recommended(value);
    if let Some(found) = array(&question["resolve"])
        .iter()
        .find(|option| text(&option["label"]) == label)
    {
        return Ok((found["action"].clone(), None));
    }
    let route_of = |value: &str| normalize_route(value).filter(|route| valid_route(route));
    match text(&question["kind"]) {
        "route" | "modal" => route_of(label)
            .map(|route| (json!({"route": route}), Some(label.to_owned())))
            .ok_or_else(|| {
                invalid(format!(
                    "{id}: a custom answer must be an app route such as /about, not {label:?}."
                ))
            }),
        // One route is one page at that URL; one route per frame, in frame
        // order, is separate pages at those URLs.
        "group" => {
            let routes = label
                .split([',', ';', '\n'])
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(|part| route_of(part.rsplit('=').next().unwrap_or(part).trim()))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    invalid(format!(
                        "{id}: a custom answer must be one route, or one route per frame such as \"/a, /b\"; not {label:?}."
                    ))
                })?;
            match routes.as_slice() {
                [route] => Ok((
                    json!({"group": "single", "route": route}),
                    Some(label.to_owned()),
                )),
                _ => Ok((
                    json!({"group": "separate", "routes": routes}),
                    Some(label.to_owned()),
                )),
            }
        }
        "link" => Ok((
            json!({"link": normalize_route(label)}),
            Some(label.to_owned()),
        )),
        "extra" => Ok((json!({"extra": label}), Some(label.to_owned()))),
        // A custom site answer is the file, name or URL itself.
        "site" => {
            let field = text(&question["field"]);
            match field {
                "favicon" | "ogImage" => Ok((
                    json!({field: {"source": "provide", "path": label}}),
                    Some(label.to_owned()),
                )),
                "siteName" => Ok((
                    json!({field: {"source": "answer", "value": label}}),
                    Some(label.to_owned()),
                )),
                "siteUrl" if label.starts_with("https://") || label.starts_with("http://") => Ok((
                    json!({field: {"source": "answer", "value": label}}),
                    Some(label.to_owned()),
                )),
                "siteUrl" => Err(invalid(format!(
                    "{id}: a custom answer must be an http(s) URL, not {label:?}."
                ))),
                _ => Ok((
                    array(&question["resolve"])
                        .first()
                        .map(|option| option["action"].clone())
                        .unwrap_or(Value::Null),
                    Some(label.to_owned()),
                )),
            }
        }
        _ => Ok((
            array(&question["resolve"])
                .first()
                .map(|option| option["action"].clone())
                .unwrap_or(Value::Null),
            Some(label.to_owned()),
        )),
    }
}

fn per_round(plan: &Value) -> usize {
    plan["maxQuestionsPerRound"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(MAX_QUESTIONS_PER_ROUND)
}

/// Whether a question has to be answered, given the answers before it.
fn required(question: &Value, actions: &BTreeMap<String, Value>) -> bool {
    array(&question["when"])
        .iter()
        .all(|condition| actions.get(text(&condition["question"])) == Some(&condition["action"]))
}

/// `confirm`: every question that applies answered, in one call, or nothing
/// changes.
pub(super) fn confirm(input: &ScreenPlanInput, policy: &OutputPolicy) -> Result<Value, DevupError> {
    let root = require_root(input)?;
    let mut plan = read_plan(root)?.ok_or_else(|| {
        invalid("No screen plan exists in this project; run devup_screen_plan action draft first.")
    })?;
    let plan_id = input
        .plan_id
        .as_deref()
        .ok_or_else(|| invalid("action confirm needs planId."))?;
    if plan["planId"] != plan_id {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "planId does not match the plan on disk; it was redrafted. Ask the questions of the current draft.",
            false,
            json!({"planId": plan_id, "currentPlanId": plan["planId"]}),
        ));
    }
    let mut questions = array(&plan["questions"]).to_vec();
    let ordered = questions.clone();
    questions.sort_by_key(|question| depth(question, &ordered));
    let known = questions
        .iter()
        .map(|question| text(&question["id"]).to_owned())
        .collect::<BTreeSet<_>>();
    let unknown = input
        .answers
        .keys()
        .filter(|id| !known.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let mut actions = BTreeMap::new();
    let mut custom = Vec::new();
    let mut missing = Vec::new();
    for question in &questions {
        let id = text(&question["id"]).to_owned();
        if !required(question, &actions) {
            continue;
        }
        let Some(answer) = input.answers.get(&id) else {
            missing.push(id);
            continue;
        };
        let (action, note) = resolve_answer(question, answer)?;
        if let Some(note) = note {
            custom.push(json!({"question": id, "answer": note}));
        }
        actions.insert(id, action);
    }
    if !missing.is_empty() || !unknown.is_empty() {
        let pending = questions
            .iter()
            .filter(|question| missing.iter().any(|id| question["id"] == id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            format!(
                "confirm needs an answer to every question that applies. Missing: {}.",
                if missing.is_empty() {
                    "none".to_owned()
                } else {
                    missing.join(", ")
                }
            ),
            false,
            json!({"missing": missing, "unknown": unknown, "askInstruction": ASK_INSTRUCTION,
                "questionRounds": question_rounds(&pending, per_round(&plan))}),
        ));
    }
    let accepted = actions
        .get("heuristics")
        .is_none_or(|action| action["heuristics"] == "accept");
    let nothing_here = actions
        .get("scope")
        .is_some_and(|action| action["scope"] == "none");
    // Every "the Figma structure is wrong" answer: its frames are not built,
    // and the fix is reported. The work goes on without them.
    let design_fixes = questions
        .iter()
        .filter(|question| {
            actions
                .get(text(&question["id"]))
                .is_some_and(|action| action["designFix"] == true)
        })
        .map(|question| {
            json!({"question": question["id"], "frames": question["fix"]["frames"],
                "fix": question["fix"]["text"]})
        })
        .collect::<Vec<_>>();
    let fixed = |ids: &[&str]| {
        ids.iter().any(|id| {
            actions
                .get(*id)
                .is_some_and(|action| action["designFix"] == true)
        })
    };
    let app_dir = text(&plan["appDir"]).to_owned();
    let mut routes = Vec::new();
    let mut screen_routes = BTreeMap::new();
    let screens = array(&plan["screens"]).to_vec();
    for screen in &screens {
        let id = text(&screen["id"]).to_owned();
        let frames = array(&screen["frames"]).to_vec();
        let mut deciding = array(&screen["structureQuestions"])
            .iter()
            .map(text)
            .collect::<Vec<_>>();
        deciding.extend(screen["groupingQuestion"].as_str());
        if nothing_here || fixed(&deciding) {
            continue;
        }
        let group = screen["groupingQuestion"]
            .as_str()
            .and_then(|question| actions.get(question));
        let separate = group.is_some_and(|action| action["group"] == "separate");
        let data = actions
            .get(&format!("dynamic:{id}"))
            .map(|action| action["data"].clone());
        // Each placement is (frames, implementation, route, host, collision).
        let mut placements = Vec::new();
        if separate {
            let explicit = group.and_then(|action| action["routes"].as_array());
            if explicit.is_some_and(|routes| routes.len() != frames.len()) {
                return Err(invalid(format!(
                    "{}: list exactly one route per frame ({} frames).",
                    text(&screen["groupingQuestion"]),
                    frames.len()
                )));
            }
            for (index, frame) in frames.iter().enumerate() {
                let node = text(&frame["nodeId"]);
                let action = explicit
                    .map(|routes| json!({"route": routes[index]}))
                    .or_else(|| actions.get(&format!("split-route:{node}")).cloned())
                    .unwrap_or(Value::Null);
                placements.push((vec![frame.clone()], action, None));
            }
        } else {
            let placed = group
                .filter(|action| action["route"].is_string())
                .or_else(|| actions.get(&format!("route:{id}")))
                .or_else(|| actions.get(&format!("modal:{id}")))
                .or_else(|| actions.get(&format!("review:{id}")))
                .cloned();
            let action = match placed {
                Some(action) => action,
                None if screen["routeEvidence"] == NAME_MATCH && accepted => {
                    json!({"route": screen["proposedRoute"], "collision": "merge", "acceptedHeuristic": true})
                }
                None => json!({"route": screen["proposedRoute"]}),
            };
            let collision = actions
                .get(&format!("collision:{id}"))
                .map(|action| text(&action["collision"]).to_owned());
            placements.push((frames.clone(), action, collision));
        }
        for (group_frames, action, collision) in placements {
            let collision = collision.or_else(|| action["collision"].as_str().map(str::to_owned));
            let (implementation, route, host) =
                if action["skip"] == true || collision.as_deref() == Some("skip") {
                    ("skip", None, None)
                } else if action["notPage"] == true {
                    ("not-a-page", None, None)
                } else if let Some(host) = action["state"].as_str() {
                    ("state", None, Some(host.to_owned()))
                } else {
                    ("page", action["route"].as_str().map(str::to_owned), None)
                };
            let first = text(&group_frames[0]["nodeId"]).to_owned();
            if implementation == "page" && route.is_none() {
                return Err(invalid(format!(
                    "{first} has no route after the answers; answer its route question with a route."
                )));
            }
            if let Some(url) = &route {
                screen_routes
                    .entry(id.clone())
                    .or_insert_with(|| url.clone());
            }
            let existing = route.as_deref().and_then(|url| {
                (screen["proposedRoute"] == url && !screen["existingFile"].is_null())
                    .then(|| screen["existingFile"].clone())
            });
            routes.push(json!({
                "url": route,
                "screen": if separate { first.clone() } else { id.clone() },
                "name": group_frames[0]["name"],
                "nodeIds": group_frames.iter().map(|frame| frame["nodeId"].clone()).collect::<Vec<_>>(),
                "drawnWidths": group_frames.iter().filter_map(|frame| frame["width"].as_f64()).map(|width| width.round() as i64).collect::<Vec<_>>(),
                "implementation": implementation,
                "hostScreen": host,
                "file": existing.clone().unwrap_or_else(|| route.as_deref().map_or(Value::Null, |url| json!(planned_page(url, &app_dir)))),
                "fileExists": existing.is_some(),
                "collisionPolicy": collision,
                "data": data,
                "routeEvidence": if action["acceptedHeuristic"] == true { json!("name-match (accepted)") } else { screen["routeEvidence"].clone() },
            }));
        }
    }
    let mut excluded = Vec::new();
    for row in array(&plan["excluded"]) {
        let node = text(&row["nodeId"]).to_owned();
        let built = !nothing_here
            && actions
                .get(&format!("exclusion:{node}"))
                .is_some_and(|action| action["build"] == true);
        let action = actions.get(&format!("exclusion-route:{node}"));
        match action.and_then(|action| action["route"].as_str()) {
            Some(route) if built => routes.push(json!({
                "url": route, "screen": node, "name": row["name"], "nodeIds": [node],
                "drawnWidths": [row["width"].as_f64().map(|width| width.round() as i64)],
                "implementation": "page", "hostScreen": null,
                "file": planned_page(route, &app_dir), "fileExists": false,
                "collisionPolicy": null, "data": null,
                "routeEvidence": "confirmed answer (was excluded)",
            })),
            _ => {
                let mut row = row.clone();
                row["decision"] = json!("excluded");
                excluded.push(row);
            }
        }
    }
    if nothing_here {
        for screen in &screens {
            for frame in array(&screen["frames"]) {
                excluded.push(json!({"nodeId": frame["nodeId"], "name": frame["name"], "width": frame["width"],
                    "section": frame["sectionName"], "reason": "the linked area is not for implementation (answered)",
                    "decision": "excluded"}));
            }
        }
    }
    let mut seen = BTreeMap::<String, String>::new();
    for row in &routes {
        if let Some(url) = row["url"].as_str()
            && let Some(other) = seen.insert(url.to_owned(), text(&row["screen"]).to_owned())
            && other != text(&row["screen"])
        {
            return Err(invalid(format!(
                "Two screens resolve to {url} ({other} and {}); answer their route questions with different URLs.",
                text(&row["screen"])
            )));
        }
    }
    for row in routes
        .iter_mut()
        .filter(|row| row["implementation"] == "state")
    {
        row["hostUrl"] = json!(screen_routes.get(text(&row["hostScreen"])));
    }
    if let Some(rows) = plan["screens"].as_array_mut() {
        for screen in rows {
            let id = text(&screen["id"]).to_owned();
            let mut resolved = Vec::new();
            for (index, link) in array(&screen["links"]).iter().enumerate() {
                let mut link = link.clone();
                if let Some(action) = actions.get(&format!("link:{id}:{index}")) {
                    link["resolved"] =
                        json!({"url": action["link"], "evidence": "confirmed answer"});
                } else if let Some(target) = link["resolved"]["screen"].as_str() {
                    link["resolved"]["url"] = json!(screen_routes.get(target));
                }
                resolved.push(link);
            }
            screen["links"] = json!(resolved);
        }
    }
    plan["answers"] = json!(input.answers);
    plan["customAnswers"] = json!(custom);
    plan["heuristicsAccepted"] = json!(accepted);
    plan["layout"] = json!(
        actions
            .iter()
            .filter(|(id, _)| id.starts_with("layout:"))
            .map(|(id, action)| (id.clone(), action["layout"].clone()))
            .collect::<BTreeMap<_, _>>()
    );
    for asked in questions
        .iter()
        .filter(|asked| text(&asked["id"]).starts_with("site:"))
    {
        let field = text(&asked["field"]).to_owned();
        if let Some(action) = actions.get(text(&asked["id"])) {
            plan["site"][&field]["state"] = json!("answered");
            plan["site"][&field]["answer"] = action[&field].clone();
        }
    }
    plan["extraAnswers"] = json!(
        actions
            .iter()
            .filter(|(id, _)| id.starts_with("extra:"))
            .map(|(id, action)| (id.clone(), action["extra"].clone()))
            .collect::<BTreeMap<_, _>>()
    );
    plan["routes"] = json!(routes);
    plan["excluded"] = json!(excluded);
    plan["designFixes"] = json!(design_fixes);
    plan["status"] = json!("confirmed");
    plan["confirmedAt"] = json!(now());
    if !plan["assumptions"].is_array() {
        plan["assumptions"] = json!([]);
    }
    let written = write_plan(policy, root, &plan)?;
    Ok(json!({
        "status": "confirmed",
        "planId": plan["planId"],
        "routes": routes,
        "markdownTable": route_markdown(&routes),
        "excluded": excluded,
        "designFixes": design_fixes,
        "site": plan["site"],
        "defaults": plan["defaults"],
        "extraAnswers": plan["extraAnswers"],
        "customAnswers": custom,
        "rule": IMPLEMENT_INSTRUCTION,
        "writtenTo": written,
        "nextAction": {"how": IMPLEMENT_INSTRUCTION},
    }))
}

/// `assume`: what implementation decided without asking, appended to the
/// confirmed plan so status and the final report carry it.
pub(super) fn assume(input: &ScreenPlanInput, policy: &OutputPolicy) -> Result<Value, DevupError> {
    let root = require_root(input)?;
    let mut plan = read_plan(root)?.ok_or_else(|| {
        invalid("No screen plan exists in this project; run devup_screen_plan action draft first.")
    })?;
    if plan["status"] != "confirmed" {
        return Err(invalid(
            "Assumptions belong to implementation, after confirm. Before confirm, put the decision to the user in the question rounds instead.",
        ));
    }
    if input.assumptions.is_empty() {
        return Err(invalid("action assume needs assumptions."));
    }
    let recorded_at = now();
    let mut list = array(&plan["assumptions"]).to_vec();
    for assumption in &input.assumptions {
        if assumption.about.trim().is_empty() || assumption.decision.trim().is_empty() {
            return Err(invalid("Every assumption needs about and decision."));
        }
        list.push(
            json!({"about": assumption.about, "decision": assumption.decision,
            "reason": assumption.reason, "recordedAt": recorded_at}),
        );
    }
    plan["assumptions"] = json!(list);
    // An assumption never builds what the plan excluded or sent back for a
    // design fix; the export gate refuses those frames whatever is recorded.
    let blocked = array(&plan["excluded"])
        .iter()
        .map(|row| text(&row["nodeId"]).to_owned())
        .chain(array(&plan["designFixes"]).iter().flat_map(|fix| {
            array(&fix["frames"])
                .iter()
                .map(|frame| text(&frame["nodeId"]).to_owned())
                .collect::<Vec<_>>()
        }))
        .filter(|id| {
            !id.is_empty()
                && input
                    .assumptions
                    .iter()
                    .any(|assumption| assumption.about.contains(id.as_str()))
        })
        .collect::<BTreeSet<_>>();
    let written = write_plan(policy, root, &plan)?;
    Ok(json!({
        "status": "confirmed",
        "planId": plan["planId"],
        "stillNotBuilt": blocked.iter().map(|id| json!({"frame": id,
            "why": "Excluded or sent back for a design fix; an assumption cannot build it, and the export gate refuses it."})).collect::<Vec<_>>(),
        "assumptions": plan["assumptions"],
        "writtenTo": written,
        "how": "Keep implementing. Every assumption appears in status and must be in your final report.",
    }))
}

fn screen_table(plan: &Value) -> Vec<Value> {
    array(&plan["screens"])
        .iter()
        .map(|screen| {
            json!({
                "id": screen["id"],
                "name": screen["name"],
                "kind": screen["kind"],
                "drawnWidths": screen["drawnWidths"],
                "proposedRoute": screen["proposedRoute"],
                "routeEvidence": screen["routeEvidence"],
                "existingFile": screen["existingFile"],
                "sharedParts": screen["sharedParts"],
                "openDecisions": screen["openDecisions"],
            })
        })
        .collect()
}

fn cell(value: &Value) -> String {
    let rendered = match value {
        Value::Null => "-".to_owned(),
        Value::String(text) => text.clone(),
        Value::Array(items) if items.is_empty() => "-".to_owned(),
        Value::Array(items) => items.iter().map(cell).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    };
    rendered.replace('|', "\\|").replace('\n', " ")
}

fn route_markdown(routes: &[Value]) -> String {
    let mut out = String::from(
        "| URL | Figma screen | nodeIds | widths | file | implementation |\n|---|---|---|---|---|---|\n",
    );
    for row in routes {
        let url = if row["implementation"] == "state" {
            format!("(state on {})", cell(&row["hostUrl"]))
        } else {
            cell(&row["url"])
        };
        out.push_str(&format!(
            "| {url} | {} | {} | {} | {} | {} |\n",
            cell(&row["name"]),
            cell(&row["nodeIds"]),
            cell(&row["drawnWidths"]),
            cell(&row["file"]),
            cell(&row["implementation"]),
        ));
    }
    out
}

/// The Markdown twin of the plan: the same content, for a person.
fn markdown(plan: &Value) -> String {
    let mut out = format!(
        "# Screen plan {}\n\nStatus: **{}**. Source: {}.\n",
        cell(&plan["planId"]),
        cell(&plan["status"]),
        cell(&plan["source"]["url"]),
    );
    if let Some(notes) = plan["notes"].as_str() {
        out.push_str(&format!("\nNotes: {notes}\n"));
    }
    if let Some(note) = plan["inventoryNote"].as_str() {
        out.push_str(&format!("\n> {note}\n"));
    }
    out.push_str("\n## Screens\n\n| Figma screen | nodeId | widths | proposed route | evidence | existing file | shared parts | open decisions |\n|---|---|---|---|---|---|---|---|\n");
    for screen in array(&plan["screens"]) {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            cell(&screen["name"]),
            cell(&screen["id"]),
            cell(&screen["drawnWidths"]),
            cell(&screen["proposedRoute"]),
            cell(&screen["routeEvidence"]),
            cell(&screen["existingFile"]),
            cell(&screen["sharedParts"]),
            cell(&screen["openDecisions"]),
        ));
    }
    let questions = array(&plan["questions"]);
    if !questions.is_empty() {
        out.push_str(&format!("\n## Open questions\n\n{ASK_INSTRUCTION}\n\n"));
        for question in questions {
            let id = text(&question["id"]);
            out.push_str(&format!(
                "- **{}** (`{id}`): {}\n",
                cell(&question["header"]),
                text(&question["question"])
            ));
            for option in array(&question["options"]) {
                out.push_str(&format!(
                    "  - {}: {}\n",
                    text(&option["label"]),
                    text(&option["description"])
                ));
            }
            if let Some(answer) = plan["answers"].get(id) {
                out.push_str(&format!("  - Answer: {}\n", cell(answer)));
            }
        }
    }
    let routes = array(&plan["routes"]);
    if !routes.is_empty() {
        out.push_str("\n## Confirmed routes\n\n");
        out.push_str(&route_markdown(routes));
    }
    let lists = trailing_lists(plan);
    if !lists.is_empty() {
        out.push_str("\n## Not built, and why\n");
        out.push_str(&lists);
    }
    out
}

fn worst(a: &str, b: &str) -> &'static str {
    let rank = |status: &str| match status {
        "fail" => 3,
        "inconclusive" => 2,
        "not-run" => 1,
        _ => 0,
    };
    match rank(a).max(rank(b)) {
        3 => "fail",
        2 => "inconclusive",
        1 => "not-run",
        _ => "pass",
    }
}

/// `status`: the confirmed plan read against the routes on disk and, when
/// given, a runtime audit. A width without an audit item is `not-run`.
pub(super) async fn status(
    input: &ScreenPlanInput,
    policy: &OutputPolicy,
    artifacts: &ArtifactStore,
) -> Result<Value, DevupError> {
    let root = require_root(input)?;
    let plan = read_plan(root)?.ok_or_else(|| {
        invalid("No screen plan exists in this project; run devup_screen_plan action draft first.")
    })?;
    if plan["status"] != "confirmed" {
        return Ok(json!({
            "status": "not-confirmed",
            "planId": plan["planId"],
            "questionRounds": question_rounds(array(&plan["questions"]), per_round(&plan)),
            "questions": ask_shape(array(&plan["questions"])),
            "askInstruction": ASK_INSTRUCTION,
            "nextAction": {"tool": "devup_screen_plan", "arguments": {"action": "confirm", "projectRoot": root, "planId": plan["planId"]}, "how": ASK_INSTRUCTION},
        }));
    }
    let audit = match (&input.audit_plan_path, &input.audit_report_path) {
        (Some(plan_path), Some(report_path)) => Some(
            super::runtime_audit::ingest_with_artifacts(policy, plan_path, report_path, artifacts)
                .await?,
        ),
        (None, None) => None,
        _ => {
            return Err(invalid(
                "status needs both auditPlanPath and auditReportPath, or neither.",
            ));
        }
    };
    let evidence = project_evidence(root).await?;
    Ok(status_report(&plan, &evidence, audit.as_ref()))
}

/// The pure half of `status`, so the not-run rule is tested on its own.
pub(super) fn status_report(
    plan: &Value,
    evidence: &ProjectEvidence,
    audit: Option<&Value>,
) -> Value {
    let audit_rows = audit
        .map(|audit| array(&audit["screenTable"]))
        .unwrap_or_default();
    let mut widths = audit
        .map(|audit| {
            array(&audit["widthTable"])
                .iter()
                .filter_map(|row| row["width"].as_u64())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let audited = !widths.is_empty();
    let mut rows = Vec::new();
    let mut states = Vec::new();
    for route in array(&plan["routes"]) {
        if route["implementation"] == "state" {
            states.push(json!({"screen": route["screen"], "name": route["name"], "hostUrl": route["hostUrl"]}));
            continue;
        }
        let Some(url) = route["url"].as_str() else {
            continue;
        };
        let page = evidence
            .routes
            .iter()
            .find(|(pattern, _)| pattern == url || super::navigation::matches_route(pattern, url))
            .map(|(_, page)| page.clone());
        let mut row_widths = if audited {
            widths.clone()
        } else {
            DEFAULT_WIDTHS
                .iter()
                .map(|width| u64::from(*width))
                .chain(
                    array(&route["drawnWidths"])
                        .iter()
                        .filter_map(Value::as_u64),
                )
                .collect::<Vec<_>>()
        };
        row_widths.sort_unstable_by(|a, b| b.cmp(a));
        row_widths.dedup();
        let per_width = row_widths
            .iter()
            .map(|width| {
                let matching = audit_rows
                    .iter()
                    .filter(|row| {
                        row["width"] == *width
                            && row["routePath"]
                                .as_str()
                                .and_then(normalize_route)
                                .is_some_and(|path| path == url)
                    })
                    .collect::<Vec<_>>();
                if matching.is_empty() {
                    return json!({"width": width, "result": "not-run",
                        "reasons": [if audited { "no runtime audit item for this URL at this width" } else { "no runtime audit report supplied" }]});
                }
                let result = matching
                    .iter()
                    .fold("pass", |acc, row| worst(acc, text(&row["status"])));
                json!({
                    "width": width,
                    "result": result,
                    "reasons": matching.iter().flat_map(|row| array(&row["reasons"]).to_vec()).collect::<Vec<_>>(),
                    "screenshots": matching.iter().filter_map(|row| row["screenshot"].as_str()).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        rows.push(json!({
            "url": url,
            "figmaScreen": route["name"],
            "nodeIds": route["nodeIds"],
            "file": page.clone().map_or_else(|| route["file"].clone(), Value::from),
            "implemented": if page.is_some() { "yes" } else { "no" },
            "widths": per_width,
        }));
    }
    if !audited {
        widths = DEFAULT_WIDTHS
            .iter()
            .map(|width| u64::from(*width))
            .collect();
    }
    let implemented = rows
        .iter()
        .filter(|row| row["implemented"] == "yes")
        .count();
    let verified = rows
        .iter()
        .filter(|row| {
            array(&row["widths"])
                .iter()
                .all(|width| width["result"] == "pass")
        })
        .count();
    let mut problems = Vec::new();
    for row in &rows {
        let mut by_kind = BTreeMap::<&str, Vec<String>>::new();
        for width in array(&row["widths"]) {
            if width["result"] != "fail" {
                continue;
            }
            let reasons = array(&width["reasons"])
                .iter()
                .map(text)
                .collect::<Vec<_>>();
            let kind = if reasons
                .iter()
                .any(|reason| reason.starts_with("horizontal overflow"))
            {
                "overflows"
            } else if reasons.iter().any(|reason| {
                reason.starts_with("page error") || reason.starts_with("screen not reached")
            }) {
                "fails to load"
            } else {
                "fails checks"
            };
            by_kind
                .entry(kind)
                .or_default()
                .push(width["width"].to_string());
        }
        for (kind, at) in by_kind {
            problems.push(format!("{} {kind} at {}", text(&row["url"]), at.join(", ")));
        }
    }
    let mut summary = format!(
        "{} URLs: {implemented} implemented, {verified} verified at all widths",
        rows.len()
    );
    for problem in &problems {
        summary.push_str(&format!(", {problem}"));
    }
    if !audited {
        summary.push_str("; no runtime audit report, so every width is not-run");
    }
    let assumptions = array(&plan["assumptions"]);
    if !assumptions.is_empty() {
        summary.push_str(&format!(
            "; {} assumption{} made during implementation",
            assumptions.len(),
            if assumptions.len() == 1 { "" } else { "s" }
        ));
    }
    let design_fixes = array(&plan["designFixes"]);
    if !design_fixes.is_empty() {
        summary.push_str(&format!(
            "; {} design fix{} requested, not built",
            design_fixes.len(),
            if design_fixes.len() == 1 { "" } else { "es" }
        ));
    }
    let excluded = array(&plan["excluded"])
        .iter()
        .filter(|row| row["decision"] == "excluded")
        .cloned()
        .collect::<Vec<_>>();
    if !excluded.is_empty() {
        summary.push_str(&format!("; {} frames excluded, not built", excluded.len()));
    }
    let mut table = String::from(
        "| URL | Figma screen | file | status | widths verified | screenshots |\n|---|---|---|---|---|---|\n",
    );
    for row in &rows {
        let row_widths = array(&row["widths"]);
        let passed = row_widths
            .iter()
            .filter(|width| width["result"] == "pass")
            .map(|width| width["width"].to_string())
            .collect::<Vec<_>>();
        let failing = row_widths
            .iter()
            .filter(|width| width["result"] == "fail")
            .map(|width| width["width"].to_string())
            .collect::<Vec<_>>();
        let state = if row["implemented"] != "yes" {
            "not implemented".to_owned()
        } else if !failing.is_empty() {
            format!("fails at {}", failing.join(", "))
        } else if passed.len() == row_widths.len() {
            "verified".to_owned()
        } else {
            "implemented, not verified".to_owned()
        };
        let shots = row_widths
            .iter()
            .flat_map(|width| array(&width["screenshots"]).to_vec())
            .collect::<Vec<_>>();
        table.push_str(&format!(
            "| {} | {} | {} | {state} | {} of {} | {} |\n",
            cell(&row["url"]),
            cell(&row["figmaScreen"]),
            cell(&row["file"]),
            if passed.is_empty() {
                "none".to_owned()
            } else {
                passed.join(", ")
            },
            row_widths
                .iter()
                .map(|width| width["width"].to_string())
                .collect::<Vec<_>>()
                .join("/"),
            cell(&json!(shots)),
        ));
    }
    let completeness = site_completeness(plan, evidence);
    let incomplete = completeness
        .iter()
        .filter(|item| item["state"] == "missing")
        .count();
    if incomplete > 0 {
        summary.push_str(&format!("; site completeness: {incomplete} missing"));
    }
    table.push_str("\nSite completeness:\n");
    for item in &completeness {
        table.push_str(&format!(
            "- {}: {} ({})\n",
            text(&item["item"]),
            text(&item["state"]),
            cell(&item["detail"])
        ));
    }
    table.push_str(&trailing_lists(plan));
    json!({
        "status": "confirmed",
        "planId": plan["planId"],
        "summary": summary,
        "rows": rows,
        "states": states,
        "widths": widths,
        "audit": audit.map(|audit| json!({"verdict": audit["verdict"], "planId": audit["planId"], "reason": audit["reason"]})),
        "markdownTable": table,
        "assumptions": assumptions,
        "siteCompleteness": completeness,
        "defaults": plan["defaults"],
        "designFixes": design_fixes,
        "excluded": excluded,
        "inventoryNote": evidence.inventory_note,
        "rule": "A width without a runtime audit item is not-run, never pass. Implemented means an App Router page exists for the URL, not that it renders.",
    })
}

/// The lists every final report ends with: design fixes requested, frames
/// excluded, and assumptions made during implementation. Shared by the plan
/// file and `status`, so the two never disagree about what was not built.
fn trailing_lists(plan: &Value) -> String {
    let mut out = String::new();
    let defaults = array(&plan["defaults"]);
    if !defaults.is_empty() {
        out.push_str("\nFilled in by default (no question):\n");
        for default in defaults {
            out.push_str(&format!(
                "- {}: {}\n",
                text(&default["item"]),
                text(&default["decision"])
            ));
        }
    }
    let fixes = array(&plan["designFixes"]);
    if !fixes.is_empty() {
        out.push_str("\nDesign fixes requested (not built):\n");
        for fix in fixes {
            out.push_str(&format!("- {}\n", text(&fix["fix"])));
        }
    }
    let excluded = array(&plan["excluded"])
        .iter()
        .filter(|row| row["decision"] == "excluded" || row["decision"].is_null())
        .collect::<Vec<_>>();
    if !excluded.is_empty() {
        out.push_str("\nExcluded (never built):\n");
        for row in excluded {
            out.push_str(&format!(
                "- \"{}\" ({}): {}\n",
                text(&row["name"]),
                text(&row["nodeId"]),
                text(&row["reason"])
            ));
        }
    }
    let assumptions = array(&plan["assumptions"]);
    if !assumptions.is_empty() {
        out.push_str("\nAssumptions made during implementation:\n");
        for assumption in assumptions {
            out.push_str(&format!(
                "- {}: {}{}\n",
                text(&assumption["about"]),
                text(&assumption["decision"]),
                assumption["reason"]
                    .as_str()
                    .map(|reason| format!(" ({reason})"))
                    .unwrap_or_default()
            ));
        }
    }
    out
}

/// `app-icon` for `devup_ui_validate` with a projectRoot: an App Router app
/// with no icon or favicon. Read from disk here because the devup-ui checks
/// open no file.
pub(super) fn missing_app_icon(
    project_root: &str,
) -> Option<devup_mcp_devup_ui::ui_validate::Violation> {
    let root = Path::new(project_root);
    let app = ["src/app", "app"]
        .into_iter()
        .find(|app| join_relative(root, app).is_dir())?;
    let mut evidence = ProjectEvidence {
        app_dir: app.to_owned(),
        ..ProjectEvidence::default()
    };
    read_site_facts(root, &mut evidence);
    if !evidence.icon_files.is_empty() {
        return None;
    }
    let mut context = serde_json::Map::new();
    context.insert("appDir".into(), json!(app));
    Some(devup_mcp_devup_ui::ui_validate::Violation {
        rule: "app-icon",
        severity: devup_mcp_devup_ui::ui_validate::Severity::Warning,
        byte_range: [0, 0],
        context,
        message: format!("{app} has no icon.* or favicon, and public/ has no favicon either, so the browser tab shows a blank icon."),
        suggestion: Some("Add app/icon.svg (or favicon.ico) from a file the user provides or a Figma logo node exported as SVG; devup_screen_plan asks which. Never draw one.".into()),
    })
}

/// What the export gate decided.
pub(super) enum Gate {
    /// Not a page write, or no plan applies.
    Open,
    /// A confirmed plan covers the frame and names this route for pageScaffold.
    Route(String),
    /// Not written: the response to return instead.
    Blocked(Value),
}

/// The facts of one export call the gate decides on.
pub(super) struct ExportRequest<'a> {
    pub project_root: &'a str,
    pub url: Option<&'a str>,
    /// Frame ids, or the one node the url or artifact names when there are none.
    pub frames: Vec<String>,
    pub all_screens: bool,
    pub scaffold_write: bool,
    pub scaffold_route: Option<&'a str>,
    pub output_paths: Vec<&'a str>,
}

/// Whether a write lands in the App Router tree. Judged below the project
/// root, so a checkout that happens to sit in a folder named `app` is not one.
fn under_app(project_root: &str, path: &str) -> bool {
    let path = Path::new(path);
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .components()
        .any(|component| component.as_os_str() == "app")
}

fn plan_required(request: &ExportRequest<'_>, reason: &str, details: Value) -> Gate {
    let mut arguments = json!({"action": "draft", "projectRoot": request.project_root});
    if let Some(url) = request.url {
        arguments["url"] = json!(url);
    }
    Gate::Blocked(json!({
        "status": "plan_required",
        "written": false,
        "planRequired": {"reason": reason, "details": details},
        "nextAction": {"tool": "devup_screen_plan", "arguments": arguments,
            "how": "Draft the screen plan, ask every round of its questions back to back before implementing, confirm, then repeat this export. Nothing was collected or written."},
    }))
}

/// The soft gate in front of `devup_figma_export`. A call that would write a
/// page - a pageScaffold write, an outputPath under app/, or several screens
/// with outputPaths - needs a confirmed plan covering its frames. A single
/// component, or a preview that writes nothing, is never blocked.
pub(super) fn export_gate(request: &ExportRequest<'_>) -> Result<Gate, DevupError> {
    let several = request.all_screens || request.frames.len() > 1;
    let writes = request.scaffold_write || !request.output_paths.is_empty();
    let gated = request.scaffold_write
        || request
            .output_paths
            .iter()
            .any(|path| under_app(request.project_root, path))
        || (several && writes);
    if !gated && request.scaffold_route.is_none() {
        return Ok(Gate::Open);
    }
    let plan = match read_plan(request.project_root)? {
        Some(plan) if plan["status"] == "confirmed" => plan,
        Some(plan) if gated => {
            return Ok(plan_required(
                request,
                "The screen plan is a draft; its questions are unanswered.",
                json!({"planId": plan["planId"], "askInstruction": ASK_INSTRUCTION}),
            ));
        }
        None if gated => {
            return Ok(plan_required(
                request,
                "This call writes a page and the project has no confirmed screen plan.",
                json!({"planFile": PLAN_FILE}),
            ));
        }
        _ => return Ok(Gate::Open),
    };
    // Never built, whatever else the call says: not by this export, and not
    // by an assumption recorded during implementation.
    let refused = request
        .frames
        .iter()
        .filter_map(|frame| {
            let excluded = array(&plan["excluded"])
                .iter()
                .find(|row| row["nodeId"] == frame.as_str())
                .map(|row| format!("excluded: {}", text(&row["reason"])));
            let fix = array(&plan["designFixes"])
                .iter()
                .find(|fix| {
                    array(&fix["frames"])
                        .iter()
                        .any(|f| f["nodeId"] == frame.as_str())
                })
                .map(|fix| format!("design fix requested: {}", text(&fix["fix"])));
            excluded
                .or(fix)
                .map(|reason| json!({"frame": frame, "reason": reason}))
        })
        .collect::<Vec<_>>();
    if !refused.is_empty() && (gated || request.scaffold_route.is_some()) {
        return Ok(Gate::Blocked(json!({
            "status": "refused",
            "written": false,
            "refused": refused,
            "how": "Do not build these frames and do not record an assumption to build them. They are listed in devup_screen_plan status; continue with the rest of the plan.",
        })));
    }
    let routes = array(&plan["routes"]);
    let row_of = |frame: &str| {
        routes
            .iter()
            .find(|row| array(&row["nodeIds"]).iter().any(|id| id == frame))
    };
    if request.all_screens {
        let section = request
            .url
            .and_then(|url| FigmaTarget::parse(url).ok())
            .and_then(|target| target.node_id);
        let planned = plan["source"]["nodeId"].as_str();
        if section.is_some() && planned.is_some() && section.as_deref() != planned {
            return Ok(plan_required(
                request,
                "allScreens names a Section the confirmed plan was not drafted from.",
                json!({"requested": section, "planned": planned}),
            ));
        }
        return Ok(Gate::Open);
    } else if request.frames.is_empty() {
        if gated {
            return Ok(plan_required(
                request,
                "The frames this call writes are not named, so the plan cannot be checked. Pass frameIds or a frame url.",
                json!({}),
            ));
        }
        return Ok(Gate::Open);
    }
    let uncovered = request
        .frames
        .iter()
        .filter(|frame| {
            row_of(frame)
                .is_none_or(|row| !matches!(text(&row["implementation"]), "page" | "state"))
        })
        .cloned()
        .collect::<Vec<_>>();
    if !uncovered.is_empty() {
        if !gated {
            return Ok(Gate::Open);
        }
        // The plan is confirmed, so this is implementation: nobody is asked
        // now. The conservative option is to leave the frame unbuilt.
        return Ok(Gate::Blocked(json!({
            "status": "plan_required",
            "written": false,
            "planRequired": {"reason": "The confirmed screen plan does not place these frames on a page.",
                "details": {"uncoveredFrames": uncovered, "planId": plan["planId"]}},
            "nextAction": {"tool": "devup_screen_plan",
                "arguments": {"action": "assume", "projectRoot": request.project_root,
                    "assumptions": [{"about": uncovered.join(", "), "decision": "Not built: outside the confirmed screen plan."}]},
                "how": "Do not ask the user during implementation. Leave these frames unbuilt, record that with action assume, keep going, and list it in the final report."},
        })));
    }
    let Some(requested) = request.scaffold_route else {
        return Ok(Gate::Open);
    };
    let [frame] = request.frames.as_slice() else {
        return Ok(Gate::Open);
    };
    let Some(row) = row_of(frame) else {
        return Ok(Gate::Open);
    };
    let Some(planned) = row["url"].as_str() else {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            format!(
                "The confirmed plan builds {frame} as a state of another page, not as its own route; export it without pageScaffold."
            ),
            false,
            json!({"planId": plan["planId"], "hostUrl": row["hostUrl"]}),
        ));
    };
    if !requested.is_empty() && normalize_route(requested).as_deref() != Some(planned) {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            format!(
                "pageScaffold.route {requested} does not match the confirmed plan, which serves {frame} at {planned}."
            ),
            false,
            json!({"routeMismatch": {"requested": requested, "planned": planned, "frame": frame, "planId": plan["planId"]},
                "how": "Omit pageScaffold.route to take the plan's route, or redraft and confirm the plan if the URL should change."}),
        ));
    }
    Ok(Gate::Route(planned.to_owned()))
}

#[cfg(test)]
#[path = "screen_plan_tests.rs"]
mod tests;
