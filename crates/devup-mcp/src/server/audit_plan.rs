//! `devup_screen_plan action: "auditPlan"`: one runtime-audit plan per
//! confirmed URL that checks everything the Figma frames can be compared
//! with - page health at every width, the page, each section and each asset
//! against their Figma renders at the drawn widths, size and position of
//! every section, card and asset, containment and clipped text between the
//! widths, interaction states, and the view transition to the next URL.
//!
//! A plan per URL, because a page comparison needs the viewport as tall as
//! that URL's frame, and because the work goes URL by URL.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use devup_mcp_figma::{AssetFormat, AssetStatus, CollectedPayload, DevupError, ErrorCode};
use serde_json::{Value, json};

use super::artifacts::ArtifactStore;
use super::output::OutputPolicy;
use super::runtime_audit::{
    self, ASSET_THRESHOLD, CheckInput, PAGE_THRESHOLD, PlanInput, SECTION_THRESHOLD, ScreenInput,
    ViewportInput,
};

pub(super) const AUDIT_DIR: &str = ".devup/runtime-audit";
const RUNNER_DIR: &str = ".devup/runtime-audit/runner";
const DEFAULT_WIDTHS: [u32; 4] = [1440, 1100, 858, 390];
const DEFAULT_HEIGHT: u32 = 900;
const MAX_VIEWPORT_SIDE: u32 = 4096;
const MAX_VIEWPORTS: usize = 8;
const MAX_CHECKS: usize = 200;
/// Most concrete URLs audited for one dynamic route.
const MAX_DYNAMIC_URLS: usize = 10;
/// Font CDNs a page commonly loads; anything else outside the app is
/// blocked and recorded, which leaves the run inconclusive.
const FONT_ORIGINS: [&str; 3] = [
    "https://fonts.googleapis.com",
    "https://fonts.gstatic.com",
    "https://cdn.jsdelivr.net",
];
const NAV_WORDS: [&str; 5] = ["nav", "header", "gnb", "menu", "헤더"];

fn invalid(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}

/// Changed-pixel ratios from `.devup/screen-plan.config.json`
/// `visualThresholds`: `page`, `section`, `asset`, and `nodes` keyed by Figma
/// node id for one asset that needs its own.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VisualThresholds {
    #[serde(default)]
    pub page: Option<f64>,
    #[serde(default)]
    pub section: Option<f64>,
    #[serde(default)]
    pub asset: Option<f64>,
    #[serde(default)]
    pub nodes: BTreeMap<String, f64>,
}

impl VisualThresholds {
    fn for_node(&self, node_id: &str, scope: &str) -> f64 {
        self.nodes
            .get(node_id)
            .copied()
            .or(match scope {
                "page" => self.page,
                "section" => self.section,
                _ => self.asset,
            })
            .unwrap_or(match scope {
                "page" => PAGE_THRESHOLD,
                "section" => SECTION_THRESHOLD,
                _ => ASSET_THRESHOLD,
            })
    }
}

/// The export artifacts the agent holds, searched for each frame's snapshot,
/// its referencePng and the native renders of its sections and assets.
pub(super) struct Sources {
    payloads: Vec<(String, Arc<CollectedPayload>)>,
}

impl Sources {
    pub(super) async fn load(ids: &[String], store: &ArtifactStore) -> (Self, Vec<Value>) {
        let mut payloads = Vec::new();
        let mut missing = Vec::new();
        for id in ids {
            match store.get(id).await {
                Some(found) => payloads.push((id.clone(), found.payload)),
                None => missing.push(json!({"artifactId": id,
                    "issue": "missing or expired; export the frame again and pass the new artifactId"})),
            }
        }
        (Self { payloads }, missing)
    }

    #[cfg(test)]
    pub(super) fn from_payloads(payloads: Vec<(String, CollectedPayload)>) -> Self {
        Self {
            payloads: payloads
                .into_iter()
                .map(|(id, payload)| (id, Arc::new(payload)))
                .collect(),
        }
    }

    /// The artifact whose snapshot holds the frame, preferring the one
    /// exported for that frame and carrying its referencePng.
    fn frame(&self, node_id: &str) -> Option<(&str, &CollectedPayload)> {
        self.payloads
            .iter()
            .filter(|(_, payload)| payload.snapshot.nodes.contains_key(node_id))
            .max_by_key(|(_, payload)| {
                (
                    payload.target.node_id.as_deref() == Some(node_id),
                    payload.reference_png.is_some(),
                )
            })
            .map(|(id, payload)| (id.as_str(), payload.as_ref()))
    }

    fn reference_png(&self, node_id: &str) -> Option<&str> {
        self.payloads
            .iter()
            .find(|(_, payload)| {
                payload.target.node_id.as_deref() == Some(node_id)
                    && payload.reference_png.is_some()
            })
            .map(|(id, _)| id.as_str())
    }

    /// A native-node PNG of the node at scale 1, the only render an element
    /// capture at deviceScaleFactor 1 can be compared with pixel for pixel.
    fn native_render(&self, node_id: &str) -> Option<(&str, &str)> {
        self.payloads.iter().find_map(|(id, payload)| {
            payload
                .assets
                .iter()
                .find(|asset| {
                    asset.node_id == node_id
                        && asset.status == AssetStatus::Exported
                        && asset.format == Some(AssetFormat::Png)
                        && asset.mime_type.as_deref() == Some("image/png")
                        && asset.scale == Some(1.0)
                        && asset
                            .render_info
                            .as_ref()
                            .is_some_and(|info| info["representation"] == "native-node")
                })
                .map(|asset| (id.as_str(), asset.asset_id.as_str()))
        })
    }

    /// Nodes the export found as images or icons. A render the agent asked
    /// for (sourceKind node) says nothing about what the node is.
    fn asset_nodes(&self) -> BTreeSet<String> {
        self.payloads
            .iter()
            .flat_map(|(_, payload)| {
                payload
                    .assets
                    .iter()
                    .filter(|asset| asset.source_kind != "node")
                    .map(|asset| asset.node_id.clone())
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn bounds(payload: &CollectedPayload, node_id: &str) -> Option<Bounds> {
    let view = payload.snapshot.nodes.get(node_id)?.typed_view();
    let bbox = view.value("absoluteBoundingBox")?;
    let number = |key: &str| bbox.get(key).and_then(Value::as_f64);
    Some(Bounds {
        x: number("x")?,
        y: number("y")?,
        width: number("width")?,
        height: number("height")?,
    })
}

fn visible_children(payload: &CollectedPayload, node_id: &str) -> Vec<String> {
    payload
        .snapshot
        .nodes
        .get(node_id)
        .map(|node| {
            node.typed_view()
                .child_ids()
                .filter(|id| {
                    payload
                        .snapshot
                        .nodes
                        .get(*id)
                        .is_some_and(|child| child.typed_view().bool("visible") != Some(false))
                })
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn name_of(payload: &CollectedPayload, node_id: &str) -> String {
    payload
        .snapshot
        .nodes
        .get(node_id)
        .and_then(|node| node.typed_view().name())
        .unwrap_or(node_id)
        .to_owned()
}

fn node(id: &str) -> Value {
    json!({"css": format!("[data-figma-node-id=\"{id}\"]")})
}

/// The element a frame drawn at `width` made from `id`: on a single-frame
/// page it carries the id itself, on a page merged from one frame per width
/// the id is in its `data-figma-node-ids` map under that width.
fn node_at(width: u32, id: &str) -> Value {
    json!({"css": format!(
        "[data-figma-node-id=\"{id}\"], [data-figma-node-ids~=\"{width}:{id}\"]"
    )})
}

fn check(value: Value) -> CheckInput {
    serde_json::from_value(value).expect("audit checks are built from the schema")
}

/// One confirmed URL and what the plan knows about it.
pub(super) struct Target<'a> {
    pub url: &'a str,
    pub screen: String,
    pub file: Option<&'a str>,
    /// (width, frame node id, frame height) for each drawn width.
    pub frames: Vec<(u32, String, Option<f64>)>,
    /// The URL a view-transition check follows a link to.
    pub next_url: Option<&'a str>,
    /// Elements of a merged page a drawn width does not show: (width, the
    /// element's id at its widest width).
    pub hidden: Vec<(u32, String)>,
}

/// The runtime-audit plan for one URL, the references it could not find,
/// and what it left out.
pub(super) struct UrlPlan {
    pub plan: PlanInput,
    pub missing: Vec<Value>,
    pub notes: Vec<String>,
}

pub(super) fn url_plan(
    target: &Target<'_>,
    base_url: &str,
    sources: &Sources,
    thresholds: &VisualThresholds,
) -> Result<UrlPlan, DevupError> {
    let origin = url::Url::parse(base_url)
        .map_err(|_| invalid(format!("baseUrl {base_url} is not a URL.")))?
        .origin()
        .ascii_serialization();
    let mut notes = Vec::new();
    let mut missing = Vec::new();
    let mut viewports = Vec::<ViewportInput>::new();
    for (width, node_id, height) in &target.frames {
        let height = height.map(|height| height.round() as u32);
        let height = match height {
            Some(height) if (1..=MAX_VIEWPORT_SIDE).contains(&height) => height,
            Some(height) => {
                notes.push(format!(
                    include_str!("text/audit_plan-url_plan-a1031a26.md"),
                    MAX_VIEWPORT_SIDE = MAX_VIEWPORT_SIDE,
                    height = height,
                    node_id = node_id,
                    width = width
                ));
                DEFAULT_HEIGHT
            }
            None => DEFAULT_HEIGHT,
        };
        if !viewports.iter().any(|viewport| viewport.width == *width) {
            viewports.push(viewport(*width, height));
        }
    }
    for width in DEFAULT_WIDTHS {
        if viewports.len() >= MAX_VIEWPORTS {
            notes.push(format!(
                "{width}px left out: a plan holds at most {MAX_VIEWPORTS} widths."
            ));
            continue;
        }
        if !viewports.iter().any(|viewport| viewport.width == width) {
            viewports.push(viewport(width, DEFAULT_HEIGHT));
        }
    }
    viewports.sort_by_key(|viewport| std::cmp::Reverse(viewport.width));
    let widest = viewports.first().map_or(1440, |viewport| viewport.width);
    let drawn = target
        .frames
        .iter()
        .map(|(width, _, _)| *width)
        .collect::<Vec<_>>();

    // Health and containment at every width.
    let mut checks = vec![
        check(json!({"kind": "page-health", "label": "page health"})),
        check(json!({"kind": "no-horizontal-overflow", "label": "no horizontal overflow"})),
        check(json!({"kind": "no-clipped-text", "label": "no clipped text"})),
    ];
    let mut layout = Vec::new();
    let mut parts = Vec::new();
    let mut frame_refs = Vec::new();
    let assets_everywhere = sources.asset_nodes();
    let mut nav_checked = false;
    for (width, frame_id, _) in &target.frames {
        let Some((artifact_id, payload)) = sources.frame(frame_id) else {
            missing.push(json!({"kind": "frame", "nodeId": frame_id, "width": width,
                "why": "No artifact passed holds this frame; section, asset and geometry checks for this width are missing.",
                "call": {"tool": "devup_figma_export", "arguments": {"url": format!("<link to node {frame_id}>"), "outputs": ["tsx", "referencePng"], "projectRoot": "<projectRoot>"}}}));
            frame_refs.push(json!({"width": width, "nodeId": frame_id}));
            continue;
        };
        let reference = sources.reference_png(frame_id);
        if reference.is_none() {
            missing.push(json!({"kind": "referencePng", "nodeId": frame_id, "width": width,
                "why": format!("No referencePng for the frame drawn at {width}px, so its page comparison cannot run."),
                "call": {"tool": "devup_figma_export", "arguments": {"url": format!("<link to node {frame_id}>"), "outputs": ["referencePng"]}}}));
        }
        frame_refs.push(
            json!({"width": width, "nodeId": frame_id, "artifactId": reference,
            "threshold": thresholds.for_node(frame_id, "page")}),
        );
        let _ = artifact_id;
        let Some(frame_box) = bounds(payload, frame_id) else {
            notes.push(format!(
                "Frame {frame_id} has no absoluteBoundingBox; its geometry is not checked."
            ));
            continue;
        };
        let sections = visible_children(payload, frame_id);
        let mut renders_missing = Vec::new();
        for section in &sections {
            let name = name_of(payload, section);
            let is_asset = assets_everywhere.contains(section)
                || payload.snapshot.nodes[section].typed_view().bool("isAsset") == Some(true);
            if !nav_checked
                && NAV_WORDS
                    .iter()
                    .any(|word| name.to_lowercase().contains(word))
            {
                nav_checked = true;
                checks.push(check(
                    json!({"kind": "contained", "label": "navigation not cut off",
                    "subject": {"css": "nav >> nth=0"}, "container": {"css": "body"}}),
                ));
            }
            // Between the drawn widths only containment holds; at a drawn
            // width the frame says exactly where the section sits.
            if target.frames.len() == 1 {
                layout.push(check(
                    json!({"kind": "contained", "label": format!("{name} inside the page"),
                    "subject": node(section), "container": node(frame_id), "nodeId": section}),
                ));
            }
            if let Some(section_box) = bounds(payload, section) {
                layout.push(bounds_check(
                    &name,
                    section,
                    frame_id,
                    frame_box,
                    section_box,
                    *width,
                ));
                if !is_asset {
                    for part in visible_children(payload, section) {
                        if let Some(part_box) = bounds(payload, &part) {
                            parts.push(bounds_check(
                                &name_of(payload, &part),
                                &part,
                                section,
                                section_box,
                                part_box,
                                *width,
                            ));
                        }
                    }
                }
            }
            let scope = if is_asset { "asset" } else { "section" };
            match sources.native_render(section) {
                Some((artifact, asset)) => layout.push(capture(
                    &name,
                    section,
                    scope,
                    artifact,
                    asset,
                    *width,
                    thresholds.for_node(section, scope),
                )),
                None => renders_missing.push(section.clone()),
            }
        }
        // Assets below the sections, each against its own render and in
        // its section.
        let mut stack = sections
            .iter()
            .map(|section| (section.clone(), section.clone()))
            .collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        while let Some((id, section)) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            for child in visible_children(payload, &id) {
                let is_asset = assets_everywhere.contains(&child)
                    || payload.snapshot.nodes[&child].typed_view().bool("isAsset") == Some(true);
                if !is_asset {
                    stack.push((child, section.clone()));
                    continue;
                }
                let name = name_of(payload, &child);
                if let (Some(section_box), Some(asset_box)) =
                    (bounds(payload, &section), bounds(payload, &child))
                {
                    layout.push(bounds_check(
                        &name,
                        &child,
                        &section,
                        section_box,
                        asset_box,
                        *width,
                    ));
                }
                match sources.native_render(&child) {
                    Some((artifact, asset)) => layout.push(capture(
                        &name,
                        &child,
                        "asset",
                        artifact,
                        asset,
                        *width,
                        thresholds.for_node(&child, "asset"),
                    )),
                    None => renders_missing.push(child.clone()),
                }
            }
        }
        for batch in renders_missing.chunks(6) {
            missing.push(json!({"kind": "nativeRender", "frame": frame_id, "width": width, "nodeIds": batch,
                "why": "No native-node PNG at scale 1 for these sections or assets, so their visual comparison cannot run.",
                "call": {"tool": "devup_figma_export", "arguments": {"url": format!("<link to node {frame_id}>"), "outputs": ["tsx"],
                    "assetRequests": batch.iter().map(|id| json!({"nodeId": id, "renderMode": "node", "format": "png", "scale": 1})).collect::<Vec<_>>()}}}));
        }
    }
    let room = MAX_CHECKS.saturating_sub(checks.len() + layout.len() + 3);
    if parts.len() > room {
        notes.push(format!(
            "{} card and child position checks left out: a screen holds at most {MAX_CHECKS} checks.",
            parts.len() - room
        ));
        parts.truncate(room);
    }
    checks.extend(layout);
    checks.extend(parts);
    for (width, id) in &target.hidden {
        checks.push(check(
            json!({"kind": "hidden", "label": format!("{id} hidden at {width}"),
            "subject": node(id), "nodeId": id, "widths": [width]}),
        ));
    }
    checks.truncate(MAX_CHECKS - 3);
    checks.push(check(json!({"kind": "interaction", "label": "hover on every link and button", "state": "hover", "widths": [widest]})));
    checks.push(check(json!({"kind": "interaction", "label": "focus on every link and button", "state": "focus", "widths": [widest]})));
    match target.next_url {
        Some(next) => checks.push(check(
            json!({"kind": "view-transition", "label": format!("view transition to {next}"),
            "subject": {"css": format!("a[href=\"{next}\"] >> nth=0")}, "widths": [widest]}),
        )),
        None => notes.push("A single URL: no navigation to prove a view transition on.".to_owned()),
    }
    let first = target
        .frames
        .first()
        .map(|(_, id, _)| id.clone())
        .unwrap_or_default();
    let screen = ScreenInput {
        id: target.screen.clone(),
        route_path: Some(target.url.to_owned()),
        component_path: target.file.map(str::to_owned),
        steps: Vec::new(),
        expect: None,
        checks,
        figma: (!first.is_empty()).then(|| {
            serde_json::from_value(
                json!({"nodeId": first, "drawnWidths": drawn, "frames": frame_refs}),
            )
            .expect("figma anchor from the schema")
        }),
    };
    let mut allowed = vec![origin];
    allowed.extend(FONT_ORIGINS.iter().map(|origin| (*origin).to_owned()));
    let mut plan = PlanInput {
        base_url: base_url.to_owned(),
        allowed_origins: allowed,
        viewports,
        screens: vec![screen],
        readiness: None,
        fonts: fonts(sources, &target.frames),
        locale: Some("en-US".to_owned()),
        timezone: Some("UTC".to_owned()),
        step_timeout_ms: None,
    };
    runtime_audit::order_checks(&mut plan);
    runtime_audit::validate_plan(&plan)?;
    Ok(UrlPlan {
        plan,
        missing,
        notes,
    })
}

fn viewport(width: u32, height: u32) -> ViewportInput {
    ViewportInput {
        width,
        height,
        device_scale_factor: 1.0,
        mobile: false,
        touch: false,
    }
}

/// Size and position against the frame, exact: the executor allows only
/// half a pixel of rounding and takes no tolerance.
fn bounds_check(
    name: &str,
    subject: &str,
    container: &str,
    container_box: Bounds,
    subject_box: Bounds,
    width: u32,
) -> CheckInput {
    let round = |value: f64| (value * 1000.0).round() / 1000.0;
    check(
        json!({"kind": "bounds", "label": format!("{name} size and position"),
        "subject": node_at(width, subject), "container": node_at(width, container), "nodeId": subject,
        "expected": {"x": round(subject_box.x - container_box.x), "y": round(subject_box.y - container_box.y),
            "width": round(subject_box.width), "height": round(subject_box.height)},
        "widths": [width]}),
    )
}

/// The font families the frames' text uses: they must be loaded before a
/// text box is measured, and a missing one is an environment error rather
/// than a layout failure.
fn fonts(sources: &Sources, frames: &[(u32, String, Option<f64>)]) -> Vec<String> {
    let mut families = BTreeSet::new();
    for (_, frame, _) in frames {
        let Some((_, payload)) = sources.frame(frame) else {
            continue;
        };
        let mut stack = vec![frame.clone()];
        let mut seen = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(node) = payload.snapshot.nodes.get(&id) else {
                continue;
            };
            let view = node.typed_view();
            if let Some(family) = view
                .value("fontName")
                .and_then(|font| font.get("family"))
                .and_then(Value::as_str)
            {
                families.insert(family.to_owned());
            }
            stack.extend(view.child_ids().map(str::to_owned));
        }
    }
    families.into_iter().take(20).collect()
}

fn capture(
    name: &str,
    subject: &str,
    scope: &str,
    artifact: &str,
    asset: &str,
    width: u32,
    threshold: f64,
) -> CheckInput {
    check(
        json!({"kind": "capture", "label": format!("{name} against its Figma render"),
        "subject": node_at(width, subject), "nodeId": subject, "widths": [width],
        "reference": {"scope": scope, "artifactId": artifact, "assetId": asset, "threshold": threshold}}),
    )
}

pub(super) fn slug(url: &str) -> String {
    let trimmed = url.trim_matches('/');
    if trimmed.is_empty() {
        return "home".to_owned();
    }
    trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

/// What auditPlan was asked for.
pub(super) struct AuditRequest<'a> {
    pub root: &'a str,
    pub base_url: &'a str,
    pub base_urls: &'a BTreeMap<String, String>,
    /// Only these URLs; empty means every confirmed URL.
    pub only: &'a [String],
    pub artifact_ids: &'a [String],
    pub thresholds: &'a VisualThresholds,
}

/// Writes every URL's plan, the runner once, and returns what to run.
pub(super) async fn write(
    plan: &Value,
    request: &AuditRequest<'_>,
    policy: &OutputPolicy,
    store: &ArtifactStore,
) -> Result<Value, DevupError> {
    let AuditRequest {
        root,
        base_url,
        base_urls,
        only,
        artifact_ids,
        thresholds,
    } = *request;
    let (sources, expired) = Sources::load(artifact_ids, store).await;
    let routes = plan["routes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|route| route["implementation"] == "page" && route["url"].is_string())
        .collect::<Vec<_>>();
    let records = super::export_record::records(Path::new(root));
    // Every route as the concrete URLs it is audited at: a dynamic route at
    // the URLs its params, the site's links or its Figma frame give.
    let mut concrete = Vec::<(usize, String, Option<Value>)>::new();
    let mut not_audited = Vec::new();
    for (index, route) in routes.iter().enumerate() {
        let url = route["url"].as_str().unwrap_or_default();
        if !only.is_empty() && !only.iter().any(|wanted| wanted == url) {
            continue;
        }
        if !url.contains('[') {
            concrete.push((index, url.to_owned(), None));
            continue;
        }
        let resolved = dynamic_urls(root, route, &plan["audit"]["links"], &sources);
        if resolved.urls.is_empty() {
            not_audited.push(json!({"url": url, "why": resolved.why}));
            continue;
        }
        for sample in resolved.urls {
            concrete.push((
                index,
                sample,
                Some(json!({"pattern": url, "from": resolved.source,
                "assumption": resolved.assumption})),
            ));
        }
    }
    let urls = concrete
        .iter()
        .map(|(_, url, _)| url.as_str())
        .collect::<Vec<_>>();
    let several_apps = routes
        .iter()
        .map(|route| route["app"].as_str().unwrap_or_default())
        .collect::<BTreeSet<_>>()
        .len()
        > 1;
    let heights = plan["screens"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|screen| screen["frames"].as_array().into_iter().flatten())
        .filter_map(|frame| {
            Some((
                frame["nodeId"].as_str()?.to_owned(),
                frame["height"].as_f64(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let runner = Path::new(root).join(RUNNER_DIR);
    let mut template = (!runner.join("run.mjs").exists()).then(|| runner.display().to_string());
    let template_state = if template.is_some() {
        "written"
    } else if std::fs::read(runner.join("run.mjs")).ok().as_deref()
        == Some(include_bytes!("../../../../harness/runtime-audit/run.mjs").as_slice())
    {
        "current"
    } else {
        "stale: delete .devup/runtime-audit/runner and run auditPlan again to get this server's runner"
    };
    let mut written = Vec::new();
    for (index, url, dynamic) in &concrete {
        let route = routes[*index];
        let url = url.as_str();
        let app = route["app"].as_str().unwrap_or_default();
        let screen = if several_apps && !app.is_empty() {
            format!("{}-{}", slug(app), slug(url))
        } else {
            slug(url)
        };
        let frames = route["nodeIds"]
            .as_array()
            .into_iter()
            .flatten()
            .zip(route["drawnWidths"].as_array().into_iter().flatten())
            .filter_map(|(node, width)| {
                let node = node.as_str()?.to_owned();
                let height = heights.get(&node).copied().flatten();
                Some((u32::try_from(width.as_u64()?).ok()?, node, height))
            })
            .collect::<Vec<_>>();
        let position = urls.iter().position(|candidate| *candidate == url);
        let next_url = position
            .filter(|_| urls.len() > 1)
            .map(|index| urls[(index + 1) % urls.len()]);
        let hidden = hidden_elements(&records, &frames);
        let target = Target {
            url,
            screen: screen.clone(),
            file: route["file"].as_str(),
            frames,
            next_url,
            hidden,
        };
        let base = base_urls.get(app).map_or(base_url, String::as_str);
        let built = url_plan(&target, base, &sources, thresholds)?;
        let plan_path = Path::new(root)
            .join(AUDIT_DIR)
            .join(&screen)
            .join("plan.json");
        let report_dir = Path::new(root).join(AUDIT_DIR).join(&screen).join("report");
        let counts = built.plan.screens[0].checks.iter().fold(
            BTreeMap::<String, usize>::new(),
            |mut counts, check| {
                *counts.entry(check.kind.clone()).or_default() += 1;
                counts
            },
        );
        let viewports = built
            .plan
            .viewports
            .iter()
            .map(|viewport| json!({"width": viewport.width, "height": viewport.height}))
            .collect::<Vec<_>>();
        let created = runtime_audit::create_plan(
            built.plan,
            policy,
            Some(&plan_path.to_string_lossy()),
            template.take().as_deref(),
            store,
        )
        .await?;
        written.push(json!({
            "url": url,
            "screen": screen,
            "planPath": created["writtenTo"],
            "planId": created["planId"],
            "reportDirectory": report_dir.display().to_string(),
            "viewports": viewports,
            "checks": counts,
            "missingReferences": built.missing,
            "notes": built.notes,
            "dynamic": dynamic,
            "run": ["node", "run.mjs", "--plan", plan_path.display().to_string(), "--out", report_dir.display().to_string()],
        }));
    }
    let missing = written
        .iter()
        .flat_map(|plan| {
            plan["missingReferences"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let strip = production_strip(&routes, root, policy)?;
    Ok(json!({
        "plans": written,
        "notAudited": not_audited,
        "productionStrip": strip,
        "missingReferences": missing,
        "expiredArtifacts": expired,
        "runner": {"directory": runner.display().to_string(), "state": template_state,
            "install": [["npm", "ci"], ["npx", "playwright", "install", "chromium"]],
            "ifInstallFails": "Record it with devup_screen_plan action status, auditPlanPath and toolingUnavailable: the URL's audit stays inconclusive and the work goes on."},
        "nodeIdentity": include_str!("text/audit_plan-write-7f9aea99.md"),
        "nextAction": include_str!("text/audit_plan-write-925db929.md"),
    }))
}

/// Elements of a merged page that a route's drawn widths do not show, from
/// the export records: each must be hidden at those widths.
fn hidden_elements(records: &[Value], frames: &[(u32, String, Option<f64>)]) -> Vec<(u32, String)> {
    let drawn = frames
        .iter()
        .map(|(width, _, _)| *width)
        .collect::<BTreeSet<_>>();
    let roots = frames
        .iter()
        .map(|(_, id, _)| id.as_str())
        .collect::<BTreeSet<_>>();
    let mut hidden = Vec::new();
    for record in records {
        let elements = record["responsive"].as_array().cloned().unwrap_or_default();
        let ours = elements.iter().any(|element| {
            element["ids"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|id| id["nodeId"].as_str().is_some_and(|id| roots.contains(id)))
        });
        if !ours {
            continue;
        }
        for element in &elements {
            let Some(widest) = element["ids"][0]["nodeId"].as_str() else {
                continue;
            };
            for width in element["hiddenAt"].as_array().into_iter().flatten() {
                if let Some(width) = width.as_u64().and_then(|width| u32::try_from(width).ok())
                    && drawn.contains(&width)
                {
                    hidden.push((width, widest.to_owned()));
                }
            }
        }
    }
    hidden
}

/// The concrete URLs a dynamic route is audited at, and where they came from.
pub(super) struct Dynamic {
    pub urls: Vec<String>,
    pub source: &'static str,
    pub why: String,
    pub assumption: Option<String>,
}

/// `[slug]` -> ("slug", false); `[...slug]` and `[[...slug]]` -> ("slug", true).
fn params(pattern: &str) -> Vec<(String, bool)> {
    pattern
        .split('/')
        .filter(|segment| segment.starts_with('[') && segment.ends_with(']'))
        .map(|segment| {
            let inner = segment.trim_matches(|c| c == '[' || c == ']');
            match inner.strip_prefix("...") {
                Some(name) => (name.to_owned(), true),
                None => (inner.to_owned(), false),
            }
        })
        .collect()
}

fn fill(pattern: &str, values: &BTreeMap<String, String>) -> Option<String> {
    let mut out = Vec::new();
    for segment in pattern.split('/') {
        if segment.starts_with('[') && segment.ends_with(']') {
            let inner = segment.trim_matches(|c| c == '[' || c == ']');
            out.push(values.get(inner.trim_start_matches("...")).cloned()?);
        } else {
            out.push(segment.to_owned());
        }
    }
    Some(out.join("/"))
}

/// Every literal value of `key` in `text`: `key: 'a'`, `"key": "a"`, or
/// `key: ['a', 'b']` (joined with `/` for a catch-all).
pub(super) fn values_of(text: &str, key: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(at) = text[from..].find(key) {
        let start = from + at;
        from = start + key.len();
        let before = start.checked_sub(1).map(|i| bytes[i] as char);
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.') {
            continue;
        }
        let mut rest = text[from..].trim_start_matches(['"', '\'']).trim_start();
        let Some(after) = rest.strip_prefix(':') else {
            continue;
        };
        rest = after.trim_start();
        let quoted = |rest: &str| -> Option<(String, usize)> {
            let quote = rest
                .chars()
                .next()
                .filter(|c| matches!(c, '"' | '\'' | '`'))?;
            let end = rest[1..].find(quote)?;
            Some((rest[1..=end].to_owned(), end + 2))
        };
        if let Some((value, _)) = quoted(rest) {
            found.push(value);
        } else if let Some(list) = rest.strip_prefix('[') {
            let end = list.find(']').unwrap_or(list.len());
            let parts = list[..end]
                .split(',')
                .filter_map(|part| quoted(part.trim()).map(|(value, _)| value))
                .collect::<Vec<_>>();
            if !parts.is_empty() {
                found.push(parts.join("/"));
            }
        }
    }
    found.dedup();
    found
}

/// Local modules a page imports: `./x`, `../x` or `@/x`, with the names
/// imported from each.
fn local_imports(source: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("import ") else {
            continue;
        };
        let Some((names, module)) = rest.rsplit_once(" from ") else {
            continue;
        };
        let module = module
            .trim()
            .trim_end_matches(';')
            .trim_matches(['"', '\'']);
        if module.starts_with("./") || module.starts_with("../") || module.starts_with("@/") {
            found.push((names.to_owned(), module.to_owned()));
        }
    }
    found
}

fn read_module(base: &Path, module: &str) -> Option<String> {
    for suffix in [
        "",
        ".ts",
        ".tsx",
        ".js",
        ".mjs",
        ".json",
        "/index.ts",
        "/index.js",
    ] {
        let path = base.join(format!("{module}{suffix}"));
        if path.is_file()
            && std::fs::metadata(&path).is_ok_and(|meta| meta.len() <= 512 * 1024)
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            return Some(text);
        }
    }
    None
}

/// `generateStaticParams` in the route's page: literal params in its body,
/// else the param's values in a local data file it reads.
pub(super) fn static_params(
    root: &Path,
    app_dir: &str,
    page_file: &str,
    pattern: &str,
) -> Vec<String> {
    let page = root.join(page_file);
    let Ok(source) = std::fs::read_to_string(&page) else {
        return Vec::new();
    };
    let Some(at) = source.find("generateStaticParams") else {
        return Vec::new();
    };
    let body = &source[at..];
    let body = body[1..]
        .find("\nexport ")
        .map_or(body, |end| &body[..=end]);
    let names = params(pattern);
    let collect = |text: &str| {
        let lists = names
            .iter()
            .map(|(name, _)| values_of(text, name))
            .collect::<Vec<_>>();
        let count = lists.iter().map(Vec::len).min().unwrap_or(0);
        (0..count)
            .filter_map(|index| {
                let values = names
                    .iter()
                    .zip(&lists)
                    .map(|((name, _), list)| (name.clone(), list[index].clone()))
                    .collect::<BTreeMap<_, _>>();
                fill(pattern, &values)
            })
            .collect::<Vec<_>>()
    };
    let literal = collect(body);
    if !literal.is_empty() {
        return literal;
    }
    let page_dir = page.parent().unwrap_or(root);
    for (imported, module) in local_imports(&source) {
        let used = imported
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
            .filter(|name| !name.is_empty() && *name != "as")
            .any(|name| body.contains(name));
        if !used {
            continue;
        }
        let text = match module.strip_prefix("@/") {
            Some(rest) => read_module(&root.join(app_dir).join("src"), rest)
                .or_else(|| read_module(&root.join(app_dir), rest)),
            None => read_module(page_dir, &module),
        };
        if let Some(text) = text {
            let from_data = collect(&text);
            if !from_data.is_empty() {
                return from_data;
            }
        }
    }
    Vec::new()
}

/// The first text of a frame, depth first: a detail page's title.
fn first_text(sources: &Sources, frame: &str) -> Option<String> {
    let (_, payload) = sources.frame(frame)?;
    let mut stack = vec![frame.to_owned()];
    while let Some(id) = stack.pop() {
        let node = payload.snapshot.nodes.get(&id)?;
        let view = node.typed_view();
        if let Some(text) = view
            .string("characters")
            .filter(|text| !text.trim().is_empty())
        {
            return Some(text.to_owned());
        }
        let mut children = view.child_ids().map(str::to_owned).collect::<Vec<_>>();
        children.reverse();
        stack.extend(children);
    }
    None
}

fn sample_segment(text: &str) -> String {
    let words = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(6)
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if words.is_empty() {
        "sample".to_owned()
    } else {
        words.join("-")
    }
}

/// A dynamic route's concrete URLs: its `generateStaticParams`, else the
/// links the audited pages hold into it, else a sample from its Figma detail
/// frame (an assumption). At most ten.
pub(super) fn dynamic_urls(root: &str, route: &Value, links: &Value, sources: &Sources) -> Dynamic {
    let pattern = route["url"].as_str().unwrap_or_default();
    let app_dir = route["app"]
        .as_str()
        .filter(|app| !app.is_empty())
        .unwrap_or(".");
    let cap = |mut urls: Vec<String>| {
        urls.sort();
        urls.dedup();
        urls.truncate(MAX_DYNAMIC_URLS);
        urls
    };
    if let Some(file) = route["file"].as_str() {
        let found = static_params(Path::new(root), app_dir, file, pattern);
        if !found.is_empty() {
            return Dynamic {
                urls: cap(found),
                source: "generateStaticParams",
                why: String::new(),
                assumption: None,
            };
        }
    }
    let linked = links
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, hrefs)| hrefs.as_array().cloned().unwrap_or_default())
        .filter_map(|href| href.as_str().map(str::to_owned))
        .filter(|href| super::navigation::matches_route(pattern, href))
        .collect::<Vec<_>>();
    if !linked.is_empty() {
        return Dynamic {
            urls: cap(linked),
            source: "links on audited pages",
            why: String::new(),
            assumption: None,
        };
    }
    let frame = route["nodeIds"][0].as_str().unwrap_or_default();
    if let Some(text) = first_text(sources, frame) {
        let segment = sample_segment(&text);
        let values = params(pattern)
            .into_iter()
            .map(|(name, _)| (name, segment.clone()))
            .collect::<BTreeMap<_, _>>();
        if let Some(url) = fill(pattern, &values) {
            return Dynamic {
                assumption: Some(format!(
                    "{pattern} audited at {url}: the sample comes from the detail frame {frame}'s title \"{text}\"; the app must serve it."
                )),
                urls: vec![url],
                source: "Figma detail frame",
                why: String::new(),
            };
        }
    }
    Dynamic {
        urls: Vec::new(),
        source: "none",
        why: "No generateStaticParams with literal or local-data params, no link to it on an audited page (run the audit once and auditPlan again), and no detail frame text to sample.".to_owned(),
        assumption: None,
    }
}

const STRIP_PLUGIN: &[u8] =
    include_bytes!("../../../../harness/runtime-audit/strip-figma-node-ids.mjs");
const STRIP_FILE: &str = "vite-plugin-strip-figma-node-ids.mjs";

/// Puts the plugin that strips the audit attributes from production builds
/// next to each app's vite.config, once, and says whether the config uses
/// it yet.
fn production_strip(
    routes: &[&Value],
    root: &str,
    policy: &OutputPolicy,
) -> Result<Vec<Value>, DevupError> {
    let apps = routes
        .iter()
        .map(|route| {
            route["app"]
                .as_str()
                .filter(|app| !app.is_empty())
                .unwrap_or(".")
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
    let mut out = Vec::new();
    for app in apps {
        let dir = Path::new(root).join(&app);
        let file = dir.join(STRIP_FILE);
        let written = if file.exists() {
            false
        } else {
            let target = policy.resolve(&file.to_string_lossy())?;
            let mut transaction = super::output::OutputTransaction::new();
            transaction.stage("stripPlugin", target, STRIP_PLUGIN)?;
            transaction.commit()?;
            true
        };
        let config = [
            "vite.config.ts",
            "vite.config.mts",
            "vite.config.js",
            "vite.config.mjs",
        ]
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file());
        let wired = config
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .is_some_and(|text| text.contains(STRIP_FILE));
        out.push(json!({
            "app": app,
            "plugin": file.display().to_string(),
            "written": written,
            "viteConfig": config.map(|path| path.display().to_string()),
            "wired": wired,
            "add": format!("import stripFigmaNodeIds from './{STRIP_FILE}'  // then plugins: [stripFigmaNodeIds(), vinext()]"),
            "nextCli": "On the next CLI instead: compiler.reactRemoveProperties: process.env.DEVUP_AUDIT === '1' ? false : { properties: ['^data-figma-node-ids?$'] }",
            "audit": "Audit against `bun run dev` or an audit build: DEVUP_AUDIT=1 bun run build.",
            "verify": "After `bun run build`, `grep -rl data-figma-node-id dist` (or the app's build output) must find nothing.",
        }));
    }
    Ok(out)
}

/// One devup_visual_compare result for a comparison status returned.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct VisualResultInput {
    /// The visualComparisons id.
    pub id: String,
    /// The comparison's verdict.
    #[schemars(extend("enum" = ["pass", "fail", "inconclusive"]))]
    pub verdict: String,
    /// visual.changedRatio from the result.
    #[serde(default)]
    pub changed_ratio: Option<f64>,
    /// The diffPath the result wrote.
    #[serde(default)]
    pub diff_path: Option<String>,
    /// Why it was inconclusive, when it was.
    #[serde(default)]
    pub reason: Option<String>,
}

/// The order a URL's failures are worked in: a page that does not render
/// first, then overflow, then layout, then pixels, then states.
fn class_rank(class: &str) -> usize {
    [
        "runtime-error",
        "overflow",
        "layout-position",
        "layout-size",
        "asset-mismatch",
        "section-visual",
        "page-visual",
        "interaction",
        "view-transition",
    ]
    .iter()
    .position(|candidate| *candidate == class)
    .unwrap_or(9)
}

/// Folds one audit run (and the visual results for it) into the plan's
/// finding records. A finding counts a failed run once per new evidence -
/// a new report for a check, a new screenshot for a comparison - so calling
/// status twice on one report does not burn an attempt. After
/// `max_fix_attempts` fix-and-rerun rounds that still fail it is
/// unresolved, with its evidence, and leaves the work queue.
pub(super) fn record_round(
    plan: &mut Value,
    audit: &Value,
    report_sha: &str,
    visual: &[VisualResultInput],
    node_files: &BTreeMap<String, (String, String)>,
    max_fix_attempts: u32,
) -> Value {
    let audited = audit["screenTable"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some((
                row["routePath"].as_str()?.to_owned(),
                row["width"].as_u64()?,
            ))
        })
        .collect::<BTreeSet<_>>();
    let comparisons = audit["visualComparisons"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut current = Vec::<(Value, String)>::new();
    for finding in audit["findings"].as_array().into_iter().flatten() {
        current.push((finding.clone(), report_sha.to_owned()));
    }
    let mut compared = BTreeSet::new();
    let mut inconclusive = Vec::new();
    for result in visual {
        let Some(comparison) = comparisons.iter().find(|c| c["id"] == result.id.as_str()) else {
            inconclusive.push(
                json!({"id": result.id, "why": "no comparison with this id in this audit run"}),
            );
            continue;
        };
        compared.insert(result.id.clone());
        match result.verdict.as_str() {
            "fail" => {
                let threshold = comparison["call"]["arguments"]["threshold"].as_f64();
                let reason = match (result.changed_ratio, threshold) {
                    (Some(ratio), Some(threshold)) => format!(
                        "{:.1}% of pixels differ from the Figma render (allowed {:.1}%)",
                        ratio * 100.0,
                        threshold * 100.0
                    ),
                    _ => "differs from the Figma render".to_owned(),
                };
                current.push((
                    json!({
                        "id": result.id,
                        "class": comparison["class"],
                        "screen": comparison["screen"],
                        "routePath": comparison["routePath"],
                        "width": comparison["viewport"]["width"],
                        "kind": "visual",
                        "label": comparison["label"],
                        "nodeId": comparison["nodeId"],
                        "codeFile": comparison["codeFile"],
                        "reason": reason,
                        "fixHint": comparison["ifItFails"]["fixHint"],
                        "evidence": {"screenshot": comparison["screenshot"]["path"], "diff": result.diff_path,
                            "reference": comparison["call"]["arguments"]["reference"]},
                    }),
                    comparison["screenshot"]["sha256"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                ));
            }
            "inconclusive" => inconclusive.push(json!({"id": result.id, "class": comparison["class"],
                "why": result.reason.clone().unwrap_or_else(|| "the comparison was inconclusive".to_owned())})),
            _ => {}
        }
    }
    let pending = comparisons
        .iter()
        .filter(|comparison| {
            !compared.contains(comparison["id"].as_str().unwrap_or_default())
                && comparison["blockers"].as_array().is_none_or(Vec::is_empty)
        })
        .map(|comparison| comparison["id"].clone())
        .collect::<Vec<_>>();
    // A converter bug is reported once and kept, whatever later runs show.
    for (finding, _) in &current {
        if finding["class"] == "converter-bug"
            && let Some(node) = finding["nodeId"].as_str()
        {
            plan["audit"]["converterBugs"][node] = json!({
                "nodeId": node, "routePath": finding["routePath"], "width": finding["width"],
                "reason": finding["reason"], "repro": finding["evidence"]["repro"],
                "codeFile": finding["codeFile"],
            });
        }
    }
    let records = &mut plan["audit"]["findings"];
    if !records.is_object() {
        *records = json!({});
    }
    let records = records.as_object_mut().expect("an object");
    let failing = current
        .iter()
        .filter_map(|(finding, _)| finding["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for (mut finding, key) in current {
        let id = finding["id"].as_str().unwrap_or_default().to_owned();
        if let Some((_, file)) = finding["nodeId"]
            .as_str()
            .and_then(|node| node_files.get(node))
        {
            finding["codeFile"] = json!(file);
        }
        let record = records
            .entry(id)
            .or_insert_with(|| json!({"failedRuns": 0}));
        if record["lastEvidence"] != key.as_str() {
            record["failedRuns"] = json!(record["failedRuns"].as_u64().unwrap_or(0) + 1);
            record["lastEvidence"] = json!(key);
        }
        let fix_attempts = record["failedRuns"].as_u64().unwrap_or(1).saturating_sub(1);
        for field in [
            "class",
            "screen",
            "routePath",
            "width",
            "kind",
            "label",
            "nodeId",
            "codeFile",
            "reason",
            "fixHint",
            "evidence",
        ] {
            record[field] = finding[field].clone();
        }
        record["fixAttempts"] = json!(fix_attempts);
        // A deterministic result does not change by retrying: a converter
        // bug leaves the fix loop at once and never spends attempts.
        record["status"] = json!(if finding["class"] == "converter-bug" {
            "converter-bug"
        } else if fix_attempts >= u64::from(max_fix_attempts) {
            "unresolved"
        } else {
            "open"
        });
    }
    // Anything this run measured again and did not see fail is fixed. A
    // comparison is fixed only by a passing result for it.
    let passed = visual
        .iter()
        .filter(|result| result.verdict == "pass")
        .map(|result| result.id.as_str())
        .collect::<BTreeSet<_>>();
    for (id, record) in records.iter_mut() {
        if failing.contains(id) || record["status"] == "resolved" {
            continue;
        }
        let measured_again = if record["kind"] == "visual" {
            passed.contains(id.as_str())
        } else {
            record["routePath"]
                .as_str()
                .zip(record["width"].as_u64())
                .is_some_and(|(route, width)| audited.contains(&(route.to_owned(), width)))
        };
        if measured_again {
            record["status"] = json!("resolved");
        }
    }
    json!({"pendingComparisons": pending, "inconclusiveComparisons": inconclusive})
}

/// The open and unresolved findings in work order, most basic first.
pub(super) fn findings_by_status(plan: &Value, status: &str) -> Vec<Value> {
    let mut list = plan["audit"]["findings"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, record)| record["status"] == status)
        .map(|(id, record)| {
            let mut record = record.clone();
            record["id"] = json!(id);
            record
        })
        .collect::<Vec<_>>();
    list.sort_by_key(|record| {
        (
            record["routePath"].as_str().unwrap_or_default().to_owned(),
            class_rank(record["class"].as_str().unwrap_or_default()),
            std::cmp::Reverse(record["width"].as_u64().unwrap_or(0)),
        )
    });
    list
}

/// Per URL: what to do next, blocked items last. Nothing here waits on
/// the user; a blocked item waits on a pending decision taken at the end.
pub(super) fn work_queue(
    plan: &Value,
    rows: &[Value],
    pending_comparisons: &[Value],
) -> Vec<Value> {
    let open = findings_by_status(plan, "open");
    let decisions = plan["pendingDecisions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let audited_urls = plan["audit"]["plans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["url"].as_str())
        .collect::<BTreeSet<_>>();
    rows.iter()
        .map(|row| {
            let url = row["url"].as_str().unwrap_or_default();
            let screen = audit_screen(plan, url);
            let mut items = Vec::new();
            let mut blocked = Vec::new();
            for decision in &decisions {
                if decision["status"] == "open"
                    && decision["about"].as_str().is_some_and(|about| about.contains(url))
                {
                    blocked.push(json!({"do": "blocked", "on": decision["id"],
                        "why": decision["question"], "meanwhile": "work the rest; it is taken with assume at the end"}));
                }
            }
            if row["implemented"] != "yes" {
                items.push(json!({"do": "build", "file": row["file"]}));
            }
            if let Some(reason) = plan["audit"]["inconclusive"][url].as_str() {
                items.push(json!({"do": "audit inconclusive", "why": reason,
                    "how": "install the runner when it can be; the rest of the work goes on"}));
            } else if !audited_urls.contains(url) {
                items.push(json!({"do": "audit", "how": "devup_screen_plan action auditPlan, run the plan, then status"}));
            } else if row["widths"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|width| width["result"] == "not-run")
            {
                items.push(json!({"do": "run the audit", "why": "some widths have no audit result yet"}));
            }
            for finding in open.iter().filter(|finding| finding["routePath"] == url) {
                items.push(json!({"do": "fix", "finding": finding["id"], "class": finding["class"],
                    "width": finding["width"], "codeFile": finding["codeFile"], "fixHint": finding["fixHint"],
                    "attempt": finding["fixAttempts"].as_u64().unwrap_or(0) + 1}));
            }
            let comparisons = pending_comparisons
                .iter()
                .filter_map(Value::as_str)
                .filter(|id| screen.as_deref().is_some_and(|screen| id.starts_with(&format!("{screen}|"))))
                .count();
            if comparisons > 0 {
                items.push(json!({"do": "compare", "count": comparisons,
                    "how": "run each visualComparisons call and pass the results back as visualResults"}));
            }
            let done = items.is_empty();
            items.extend(blocked);
            let reported = plan["audit"]["converterBugs"]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, bug)| bug["routePath"] == url)
                .map(|(_, bug)| json!({"converterBug": bug["nodeId"], "repro": bug["repro"],
                    "note": "reported, not retried; a local override needs a comment naming the repro"}))
                .collect::<Vec<_>>();
            json!({"url": url, "items": items, "reported": reported, "done": done})
        })
        .collect()
}

fn audit_screen(plan: &Value, url: &str) -> Option<String> {
    plan["audit"]["plans"]
        .as_array()?
        .iter()
        .find(|entry| entry["url"] == url)?["screen"]
        .as_str()
        .map(str::to_owned)
}

/// The report the job ends with: every URL at every width by health,
/// geometry, the three visual levels and interaction, with screenshots and
/// diffs, then the unresolved findings with their evidence and the pending
/// decisions taken. The plan's own lists (assumptions, design fixes,
/// exclusions, defaults) and the dependency line follow.
pub(super) fn final_report(plan: &Value, rows: &[Value], trailing: &str) -> String {
    let cell = |value: &Value| match value {
        Value::Null => "-".to_owned(),
        Value::String(text) => text.replace('|', "\\|"),
        other => other.to_string(),
    };
    let mut out = String::from(
        "| URL | width | health | geometry | drift / converter bugs | page | section | asset | interaction | screenshot | diffs |\n|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    let records = plan["audit"]["findings"].as_object();
    for row in rows {
        let url = row["url"].as_str().unwrap_or_default();
        for width in row["widths"].as_array().into_iter().flatten() {
            let columns = &width["columns"];
            let diffs = records
                .into_iter()
                .flatten()
                .filter(|(_, record)| {
                    record["routePath"] == url
                        && record["width"] == width["width"]
                        && record["status"] != "resolved"
                })
                .filter_map(|(_, record)| record["evidence"]["diff"].as_str())
                .collect::<Vec<_>>();
            let count = |class: &str| {
                records
                    .into_iter()
                    .flatten()
                    .filter(|(_, record)| {
                        record["routePath"] == url
                            && record["width"] == width["width"]
                            && record["class"] == class
                            && record["status"] != "resolved"
                    })
                    .count()
            };
            out.push_str(&format!(
                "| {url} | {} | {} | {} | {} / {} | {} | {} | {} | {} | {} | {} |\n",
                width["width"],
                cell(&columns["health"]),
                cell(&columns["geometry"]),
                count("code-drift"),
                count("converter-bug"),
                cell(&columns["page"]),
                cell(&columns["section"]),
                cell(&columns["asset"]),
                cell(&columns["interaction"]),
                width["screenshots"]
                    .as_array()
                    .and_then(|shots| shots.first())
                    .map_or("-".to_owned(), cell),
                if diffs.is_empty() {
                    "-".to_owned()
                } else {
                    diffs.join(", ")
                },
            ));
        }
    }
    let unresolved = findings_by_status(plan, "unresolved");
    out.push_str("\nUnresolved findings (after the fix-attempt budget):\n");
    if unresolved.is_empty() {
        out.push_str("- none\n");
    }
    for finding in &unresolved {
        out.push_str(&format!(
            "- {} at {}px, {} {}: {} (code: {}; screenshot: {}; diff: {})\n",
            cell(&finding["routePath"]),
            finding["width"],
            cell(&finding["class"]),
            cell(&finding["nodeId"]),
            cell(&finding["reason"]),
            cell(&finding["codeFile"]),
            cell(&finding["evidence"]["screenshot"]),
            cell(&finding["evidence"]["diff"]),
        ));
    }
    out.push_str("\nConverter bugs (filed, not retried):\n");
    let bugs = plan["audit"]["converterBugs"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if bugs.is_empty() {
        out.push_str("- none\n");
    }
    for bug in bugs.values() {
        out.push_str(&format!(
            "- {} at {}px, {}: {} (repro: {}; code: {})\n",
            cell(&bug["routePath"]),
            bug["width"],
            cell(&bug["nodeId"]),
            cell(&bug["reason"]),
            cell(&bug["repro"]),
            cell(&bug["codeFile"]),
        ));
    }
    out.push_str("\nPending decisions:\n");
    let decisions = plan["pendingDecisions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if decisions.is_empty() {
        out.push_str("- none\n");
    }
    for decision in &decisions {
        out.push_str(&format!(
            "- {}: {} -> {} ({})\n",
            cell(&decision["id"]),
            cell(&decision["question"]),
            cell(decision.get("taken").unwrap_or(&decision["recommended"])),
            match decision["status"].as_str() {
                Some("taken") => "taken at the end",
                Some("left-unbuilt") => "destructive: left unbuilt",
                _ => "still open",
            }
        ));
    }
    out.push_str(trailing);
    let dependencies = &plan["dependencies"];
    out.push_str(&if dependencies.is_object() {
        format!(
            "\nDependencies: `{}` bumped {} packages; pins: {}.\n",
            cell(&dependencies["command"]),
            dependencies["bumped"],
            dependencies["pins"]
                .as_array()
                .filter(|pins| !pins.is_empty())
                .map_or("none".to_owned(), |pins| pins
                    .iter()
                    .map(|pin| format!(
                        "{}@{} ({})",
                        cell(&pin["package"]),
                        cell(&pin["version"]),
                        cell(&pin["reason"])
                    ))
                    .collect::<Vec<_>>()
                    .join(", "))
        )
    } else {
        "\nDependencies: the update step has not been recorded.\n".to_owned()
    });
    out
}

#[cfg(test)]
#[path = "audit_plan_tests.rs"]
mod tests;

/// Geometry is exact, so a box that fails at a drawn width is classified by
/// cause: the code drifted from what the export wrote, or it did not and the
/// converter is wrong. A converter bug gets its repro written under
/// `.devup/converter-bugs/`.
pub(super) fn classify_geometry(
    audit: &mut Value,
    project_root: &str,
    sources: &[String],
    policy: &OutputPolicy,
) {
    let Some(findings) = audit["findings"].as_array_mut() else {
        return;
    };
    for finding in findings {
        if !matches!(finding["kind"].as_str(), Some("bounds" | "dimensions")) {
            continue;
        }
        let Some(node) = finding["nodeId"].as_str().map(str::to_owned) else {
            continue;
        };
        match super::export_record::geometry_cause(Path::new(project_root), &node, sources) {
            super::export_record::Cause::Drift {
                file,
                missing,
                added,
            } => {
                finding["class"] = json!("code-drift");
                finding["codeFile"] = json!(file);
                finding["evidence"]["drift"] = json!({"missing": missing, "added": added});
                finding["fixHint"] = json!(format!(
                    "{} Missing or changed: {}. Added layout props: {}.",
                    runtime_audit::fix_hint("code-drift", "bounds", &Value::Null),
                    if missing.is_empty() {
                        "none".to_owned()
                    } else {
                        missing.join(", ")
                    },
                    if added.is_empty() {
                        "none".to_owned()
                    } else {
                        added.join(", ")
                    },
                ));
            }
            super::export_record::Cause::ConverterBug {
                file,
                record,
                exported,
            } => {
                let width = finding["width"].clone();
                match super::export_record::write_repro(
                    policy,
                    project_root,
                    &node,
                    &width,
                    finding,
                    &file,
                    &record,
                    &exported,
                ) {
                    Ok((path, _)) => {
                        finding["class"] = json!("converter-bug");
                        finding["codeFile"] = json!(file);
                        finding["evidence"]["repro"] = json!(path);
                        finding["fixHint"] = json!(runtime_audit::fix_hint(
                            "converter-bug",
                            "bounds",
                            &Value::Null
                        ));
                    }
                    Err(error) => {
                        finding["evidence"]["reproError"] = json!(error.message);
                    }
                }
            }
            super::export_record::Cause::Unknown(why) => {
                finding["evidence"]["cause"] = json!(why);
            }
        }
    }
}
