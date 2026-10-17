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
pub(super) const ASK_INSTRUCTION: &str = "Ask every round in questionRounds now, before implementing, in consecutive question-tool calls with no other work in between; skip a question whose `when` was not met; never ask in prose. Then confirm. After confirm never ask the user anything: take the recommended or most conservative option and record it with action assume.";
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
    /// responsive | separate | ask (the default).
    #[serde(default)]
    pub width_grouping: Option<String>,
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
    if inventory["found"] != true {
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
    questions
        .iter()
        .map(|question| {
            let mut shaped = question.clone();
            if let Some(object) = shaped.as_object_mut() {
                object.remove("resolve");
                object.remove("kind");
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
    /// Asked per group; the recommendation follows the evidence.
    Ask {
        recommend_single: bool,
        reason: String,
    },
}

#[derive(Debug, Clone)]
struct DraftScreen {
    frames: Vec<Frame>,
    words: Vec<String>,
    parts: Vec<String>,
    links: Vec<PlanLinkInput>,
    grouping: Grouping,
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

/// Whether every section name says only which device it is for, e.g. `PC`
/// and `Mobile`: a designer splitting one page by device, not two pages.
fn device_sections(frames: &[Frame]) -> bool {
    frames.iter().all(|frame| {
        frame.section_name.as_deref().is_some_and(|name| {
            let lowered = name.to_lowercase();
            let words = lowered
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>();
            !words.is_empty()
                && words.iter().all(|word| {
                    DEVICE_WORDS.contains(word) || word.chars().all(|c| c.is_ascii_digit())
                })
        })
    })
}

/// Frames that share a base name are candidates for one page. Whether they
/// are one is not assumed: designers differ, so a group that could go either
/// way is asked about on its own, with the recommendation the evidence gives.
///
/// - several widths in one section that can merge into one responsive tree:
///   one responsive page is recommended;
/// - the same name in different sections: separate pages are recommended,
///   unless the sections are named only for devices (`PC`, `Mobile`);
/// - the same name at the same width in one section is several screens, not a
///   width family, and is not grouped at all.
///
/// `widthGrouping` in the project config decides instead of asking.
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
        let same_section = sections.len() == 1;
        if same_section && !family {
            for member in members {
                result.push(make(&[member], Grouping::Fixed("single-frame")));
            }
            continue;
        }
        match config.width_grouping.as_deref() {
            Some("responsive") if family => {
                result.push(make(members, Grouping::Fixed("config:widthGrouping")));
            }
            Some("separate") => {
                for member in members {
                    result.push(make(&[member], Grouping::Fixed("config:widthGrouping")));
                }
            }
            _ => {
                let probe = make(members, Grouping::Fixed(""));
                let by_device = device_sections(&probe.frames);
                let recommend_single = family && (same_section || by_device);
                let reason = if same_section {
                    "drawn at several widths in one section with matching names".to_owned()
                } else if recommend_single {
                    "split across sections named only for devices".to_owned()
                } else {
                    format!("the same name in {} different sections", sections.len())
                };
                result.push(make(
                    members,
                    Grouping::Ask {
                        recommend_single,
                        reason,
                    },
                ));
            }
        }
    }
    result
}

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
pub(super) fn draft_plan(
    screens: &[PlanScreenInput],
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
    let drafts = group_screens(screens, &evidence.config);
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
    let mut questions = Vec::new();
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
        let group_id = format!("group:{id}");
        let single_when = match &screen.grouping {
            Grouping::Ask { .. } => vec![when(&group_id, SINGLE, json!({"group": "single"}))],
            Grouping::Fixed(_) => Vec::new(),
        };
        let push = |questions: &mut Vec<Value>, mut question: Value, extra: &[Value]| {
            let conditions = single_when.iter().chain(extra).cloned().collect::<Vec<_>>();
            if !conditions.is_empty() {
                question["when"] = json!(conditions);
            }
            questions.push(question);
        };
        if let Grouping::Ask {
            recommend_single,
            reason,
        } = &screen.grouping
        {
            decisions.push("one-page-or-separate");
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
            let frames = screen
                .frames
                .iter()
                .map(|frame| {
                    format!(
                        "{} ({}, {}px{})",
                        frame.name,
                        frame.node_id,
                        frame.width.round(),
                        frame
                            .section_name
                            .as_deref()
                            .or(frame.section.as_deref())
                            .map(|section| format!(", section {section}"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            questions.push(question(
                group_id.clone(),
                header("Pages", &name),
                format!(
                    "These frames share the name \"{name}\" and are {reason}: {frames}. One responsive page, or separate pages? A custom answer may list one route per frame, e.g. \"/a, /b\"."
                ),
                if *recommend_single {
                    vec![single, separate]
                } else {
                    vec![separate, single]
                },
                "group",
            ));
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
                let mut asked = question(
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
                asked["when"] = json!([when(&group_id, SEPARATE, json!({"group": "separate"}))]);
                questions.push(asked);
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
                questions.push(question(
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
                ));
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
            "grouping": match &screen.grouping {
                Grouping::Fixed(evidence) => json!({"mode": "decided", "evidence": evidence}),
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
pub(super) fn question_rounds(questions: &[Value]) -> Vec<Value> {
    let mut by_depth = BTreeMap::<usize, Vec<&Value>>::new();
    for question in questions {
        by_depth
            .entry(depth(question, questions))
            .or_default()
            .push(question);
    }
    let mut rounds = Vec::new();
    for level in by_depth.values() {
        for chunk in level.chunks(MAX_QUESTIONS_PER_ROUND) {
            let shaped = ask_shape(
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
pub(super) async fn draft(
    input: &ScreenPlanInput,
    screens: &[PlanScreenInput],
    source: Value,
    policy: &OutputPolicy,
) -> Result<Value, DevupError> {
    let root = require_root(input)?;
    let evidence = project_evidence(root).await?;
    let mut plan = draft_plan(
        screens,
        &input.navigation_targets,
        &evidence,
        source,
        input.notes.as_deref(),
    )?;
    plan["createdAt"] = json!(now());
    let replaced = read_plan(root)
        .ok()
        .flatten()
        .filter(|previous| previous["status"] == "confirmed")
        .map(|previous| previous["planId"].clone());
    let written = write_plan(policy, root, &plan)?;
    let questions = ask_shape(array(&plan["questions"]));
    let rounds = question_rounds(array(&plan["questions"]));
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
        "heuristics": plan["heuristics"],
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
        _ => Ok((
            array(&question["resolve"])
                .first()
                .map(|option| option["action"].clone())
                .unwrap_or(Value::Null),
            Some(label.to_owned()),
        )),
    }
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
                "questionRounds": question_rounds(&pending)}),
        ));
    }
    let accepted = actions
        .get("heuristics")
        .is_none_or(|action| action["heuristics"] == "accept");
    let app_dir = text(&plan["appDir"]).to_owned();
    let mut routes = Vec::new();
    let mut screen_routes = BTreeMap::new();
    let screens = array(&plan["screens"]).to_vec();
    for screen in &screens {
        let id = text(&screen["id"]).to_owned();
        let frames = array(&screen["frames"]).to_vec();
        let group = actions.get(&format!("group:{id}"));
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
                    "group:{id}: list exactly one route per frame ({} frames).",
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
    plan["routes"] = json!(routes);
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
    let written = write_plan(policy, root, &plan)?;
    Ok(json!({
        "status": "confirmed",
        "planId": plan["planId"],
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
    let assumptions = array(&plan["assumptions"]);
    if !assumptions.is_empty() {
        out.push_str("\n## Assumptions made during implementation\n\n");
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
            "questionRounds": question_rounds(array(&plan["questions"])),
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
    json!({
        "status": "confirmed",
        "planId": plan["planId"],
        "summary": summary,
        "rows": rows,
        "states": states,
        "widths": widths,
        "audit": audit.map(|audit| json!({"verdict": audit["verdict"], "planId": audit["planId"], "reason": audit["reason"]})),
        "markdownTable": if assumptions.is_empty() {
            table
        } else {
            format!(
                "{table}
Assumptions made during implementation:
{}",
                assumptions
                    .iter()
                    .map(|assumption| format!("- {}: {}", text(&assumption["about"]), text(&assumption["decision"])))
                    .collect::<Vec<_>>()
                    .join("
    ")
            )
        },
        "assumptions": assumptions,
        "inventoryNote": evidence.inventory_note,
        "rule": "A width without a runtime audit item is not-run, never pass. Implemented means an App Router page exists for the URL, not that it renders.",
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
