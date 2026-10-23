
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
pub(super) const ASK_INSTRUCTION: &str =
    include_str!("../text/screen_plan-ASK_INSTRUCTION-0285b268.md");
/// What implementation does instead of asking. Someone may have walked away
/// from the keyboard three hours ago; a question then stops all the work.
pub(super) const IMPLEMENT_INSTRUCTION: &str =
    include_str!("../text/screen_plan-IMPLEMENT_INSTRUCTION-0f8204de.md");
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

static MODAL_WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/screen-plan-modal-words.json"))
        .expect("vendored keyword JSON")
});
static DEVICE_WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/screen-plan-device-words.json"))
        .expect("vendored keyword JSON")
});
/// Device words that name a whole section rather than a frame suffix.
static SECTION_DEVICE_WORDS: std::sync::LazyLock<Vec<&'static str>> =
    std::sync::LazyLock::new(|| {
        serde_json::from_str(include_str!("../text/screen-plan-section-device-words.json"))
            .expect("vendored keyword JSON")
    });
static ROOT_WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/screen-plan-root-words.json"))
        .expect("vendored keyword JSON")
});

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct ScreenPlanInput {
    /// draft: map screens to routes and return every open question.
    /// confirm: apply all answers at once. status: URL x width report, the
    /// audit findings, the work queue and the final report. assume: after
    /// confirm, record a decision implementation made without asking, or
    /// take a pending decision. auditPlan: write the runtime-audit plan for
    /// every confirmed URL. defer: record a decision the work needs as
    /// pending, with its recommended option, and keep working.
    #[schemars(extend("enum" = ["draft", "confirm", "status", "assume", "auditPlan", "defer"]))]
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
    /// route that no page or nav link already shows. assume: remaps those frames
    /// onto the named URL in the confirmed plan.
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
    /// confirm: the user accepted the proposal table. Every applicable
    /// question missing from `answers` takes its recommended option, so only
    /// the rows the user changed need to be sent.
    #[serde(default)]
    pub accept_recommended: bool,
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
    /// assume: the outcome of the dependency step. Each pin is also recorded
    /// as an assumption.
    #[serde(default)]
    pub dependency_update: Option<DependencyUpdateInput>,
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
    /// auditPlan: where the app is served; default http://localhost:3000.
    #[serde(default)]
    pub base_url: Option<String>,
    /// auditPlan: app name -> base URL, when the plan spans several apps.
    #[serde(default)]
    pub base_urls: BTreeMap<String, String>,
    /// auditPlan: every devup_figma_export artifactId of this plan's frames
    /// (the TSX export, the referencePng export, the native renders). They
    /// are searched for each frame's geometry and references.
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// auditPlan: only these URLs; default every confirmed URL.
    #[serde(default)]
    pub urls: Vec<String>,
    /// status: the devup_visual_compare result for each visualComparisons
    /// call the last status returned.
    #[serde(default)]
    pub visual_results: Vec<super::audit_plan::VisualResultInput>,
    /// status, with auditPlanPath and no report: the runner could not be
    /// installed or started, and why. That URL's audit is inconclusive; the
    /// work goes on.
    #[serde(default)]
    pub tooling_unavailable: Option<String>,
    /// defer: the decision the work needs.
    #[serde(default)]
    pub decision: Option<PendingDecisionInput>,
}

/// A decision the work hit after confirm. It never stops the work: it is
/// recorded with the option the agent recommends, the rest goes on, and it
/// is taken with assume at the end - except a destructive one, which is
/// left unbuilt.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct PendingDecisionInput {
    /// A short id, e.g. contact-form-target.
    pub id: String,
    /// What it is about: a URL, a frame id, a file.
    pub about: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
    /// The option to take when the work is otherwise done.
    pub recommended: String,
    /// It deletes or overwrites something, or reaches outside the project.
    #[serde(default)]
    pub destructive: bool,
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
pub(super) fn extra_questions(extra: &[ExtraQuestionInput]) -> Result<Vec<Value>, DevupError> {
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

/// The outcome of the dependency step, recorded once it ran.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct DependencyUpdateInput {
    /// How many packages the update bumped.
    pub bumped: usize,
    /// Packages held back because no code fix exists, with the reason.
    #[serde(default)]
    pub pins: Vec<PinInput>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct PinInput {
    pub package: String,
    pub version: String,
    pub reason: String,
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
    /// Overrides app-selection keyword categories; omitted categories retain defaults.
    #[serde(default)]
    pub app_selection_keywords: BTreeMap<String, Vec<String>>,
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
    /// false for a project that forbids the dependency update step.
    #[serde(default)]
    pub update_dependencies: Option<bool>,
    /// vinext | next: pre-answers the migrate-to-vinext question.
    #[serde(default)]
    pub framework: Option<String>,
    /// false opts the project out of view transitions.
    #[serde(default)]
    pub view_transitions: Option<bool>,
    /// false stops exports into this project from writing
    /// data-figma-node-id on the page, section and asset elements.
    #[serde(default)]
    pub figma_node_ids: Option<bool>,
    /// How many fix-and-rerun rounds one audit finding gets before it is
    /// recorded as unresolved with its evidence; default 3.
    #[serde(default)]
    pub max_fix_attempts: Option<u32>,
    /// Changed-pixel ratios for the visual comparisons: page, section,
    /// asset, and nodes keyed by Figma node id for one asset that needs its
    /// own.
    #[serde(default)]
    pub visual_thresholds: Option<super::audit_plan::VisualThresholds>,
}

/// Whether exports into this project mark their elements with
/// data-figma-node-id: on unless the project config says false. An
/// unreadable config leaves it on; draft reports the config error.
pub(super) fn node_ids_enabled(project_root: &str) -> bool {
    !matches!(
        read_config(project_root),
        Ok(Some(PlanConfig {
            figma_node_ids: Some(false),
            ..
        }))
    )
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
    if let Some(framework) = config.framework.as_deref()
        && !matches!(framework, "vinext" | "next")
    {
        return Err(invalid(format!(
            "{}: framework must be vinext or next, not {framework:?}.",
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
    pub(super) fn values(&self) -> Vec<&str> {
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
    /// The Figma page that owns the frame (not the page the viewer has open),
    /// by id. Taken from the node itself; the name falls back to readcrumb[0].
    #[serde(default)]
    pub page_id: Option<String>,
    /// The owning Figma page's name.
    #[serde(default)]
    pub page_name: Option<String>,
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
    /// Measured navigation content width from exploration, before export.
    #[serde(default)]
    pub navigation_content_width: Option<f64>,
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

pub(super) fn invalid(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}

pub(super) fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

pub(super) fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn now() -> String {
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
    pub vinext_version: Option<(String, String)>,
    /// react-dom and react-server-dom-webpack, which a vinext app pins to
    /// the React it runs while `react` itself may say only `^19`.
    pub react_peers: Vec<(String, String)>,
    /// Every app the project holds, and what runs each.
    pub apps: Vec<super::framework::App>,
    /// The app this plan builds, relative to the root (`.` for the root).
    /// `None` with several apps: which one is asked.
    pub target_app: Option<super::framework::App>,
}

impl ProjectEvidence {
    /// The target app's directory, or `.`.
    fn app_root(&self) -> &str {
        self.target_app.as_ref().map_or(".", |app| app.dir.as_str())
    }

    /// The target app's framework, or `unknown`.
    fn framework(&self) -> &'static str {
        self.target_app
            .as_ref()
            .map_or("unknown", |app| app.framework)
    }
}

/// Picks the app a plan builds and points `app_dir` at its App Router.
fn choose_app(root: &Path, evidence: &mut ProjectEvidence) {
    evidence.apps = super::framework::detect(root);
    evidence.target_app = match evidence.apps.as_slice() {
        [only] => Some(only.clone()),
        apps => apps.iter().find(|app| app.dir == ".").cloned(),
    };
    let first = evidence
        .target_app
        .clone()
        .or_else(|| evidence.apps.first().cloned());
    if let Some(app) = first {
        evidence.app_dir = app.path(app.app_router.as_deref().unwrap_or("src/app"));
    }
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
        choose_app(&PathBuf::from(project_root), &mut evidence);
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
    choose_app(&root, &mut evidence);
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

pub(super) fn header(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        return truncate(name, HEADER_LIMIT);
    }
    truncate(&format!("{prefix}: {name}"), HEADER_LIMIT)
}

/// One option as the question tool shows it, and what choosing it does.
pub(super) struct Choice {
    label: String,
    description: String,
    action: Value,
}

pub(super) fn choice(
    label: impl Into<String>,
    description: impl Into<String>,
    action: Value,
) -> Choice {
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
pub(super) fn question(
    id: String,
    header: String,
    prompt: String,
    choices: Vec<Choice>,
    kind: &str,
) -> Value {
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
pub(super) fn ask_shape(questions: &[Value]) -> Vec<Value> {
    ask_shape_with(questions, questions)
}
