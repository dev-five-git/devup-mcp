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
static NAV_WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../text/audit-nav-words.json")).expect("vendored nav words")
});

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
                    include_str!("../text/audit_plan-url_plan-a1031a26.md"),
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
        check(json!({"kind":"links-resolve", "label":"all connected URLs resolve", "plannedTargets":[target.url]})),
        check(json!({"kind": "no-clipped-content", "label": "navigation and card content not cut off"})),
        check(json!({"kind": "sequence-order", "label": "numbered cards in reading order"})),
        check(json!({"kind": "section-layout", "label": "responsive sections and text columns"})),
        check(json!({"kind": "page-health", "label": "page health"})),
        check(json!({"kind": "styles-applied", "label": "page styles delivered"})),
        check(json!({"kind": "no-low-contrast-text", "label": "visible text contrast"})),
        check(json!({"kind": "image-loaded", "label": "images and backgrounds loaded"})),
        check(json!({"kind": "native-appearance", "label": "controls drawn without native browser chrome"})),
        check(json!({"kind": "stale-resources", "label": "no stale or missing build resources"})),
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
            checks.push(check(json!({"kind": "page-health", "label": "design reference unavailable", "widths": [width], "unavailableReason": "Design comparison inconclusive: frame artifact missing or expired; export the frame again."})));
            missing.push(json!({"kind": "frame", "nodeId": frame_id, "width": width,
                "why": "No artifact passed holds this frame; section, asset and geometry checks for this width are missing.",
                "call": {"tool": "devup_figma_export", "arguments": {"url": format!("<link to node {frame_id}>"), "outputs": ["tsx", "referencePng"], "projectRoot": "<projectRoot>"}}}));
            frame_refs.push(json!({"width": width, "nodeId": frame_id}));
            continue;
        };
        let reference = sources.reference_png(frame_id);
        if reference.is_none() {
            checks.push(check(json!({"kind": "page-health", "label": "design reference unavailable", "widths": [width], "unavailableReason": "Design comparison inconclusive: referencePng missing or expired; export the frame again."})));
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
        let mut images = Vec::new();
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
            if !is_asset && bounds(payload, section).is_some_and(|section_box| section_box.width >= frame_box.width * 0.5) {
                layout.push(check(json!({"kind": "section-layout", "label": format!("{name} remains a visible section"), "subject": node_at(*width, section), "container": node_at(*width, frame_id),
                    "widths": viewports.iter().filter(|viewport| {
                        target.frames.iter().filter(|(drawn, _, _)| *drawn <= viewport.width).max_by_key(|(drawn, _, _)| *drawn)
                            .or_else(|| target.frames.iter().min_by_key(|(drawn, _, _)| *drawn))
                            .is_some_and(|(drawn, _, _)| drawn == width)
                    }).map(|viewport| viewport.width).collect::<Vec<_>>(), "nodeId": section})));
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
            if is_asset {
                images.push(section.clone());
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
                    if let Some(alignment) = text_alignment_check(payload, &child, *width) {
                        parts.push(alignment);
                    }
                    stack.push((child, section.clone()));
                    continue;
                }
                images.push(child.clone());
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
        checks.push(completeness_check(
            *width,
            frame_id,
            frame_box.height,
            &sections,
            images,
            &mut notes,
        ));
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

fn gate_source_anchors(built: &mut UrlPlan, app: &Path) {
    fn scan(path: &Path, text: &mut String) {
        let Ok(entries) = std::fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
                continue;
            }
            if path.is_dir() {
                scan(&path, text);
            } else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("tsx" | "jsx" | "ts" | "js"))
                && let Ok(source) = std::fs::read_to_string(path) {
                text.push_str(&source);
                text.push('\n');
            }
        }
    }
    let mut source = String::new();
    scan(&app.join("src"), &mut source);
    if !app.join("src").exists() {
        scan(&app.join("app"), &mut source);
        scan(&app.join("components"), &mut source);
    }
    let mut anchors = BTreeSet::new();
    for tail in source.split("data-figma-node-id").skip(1) {
        let (plural, tail) = tail.strip_prefix('s').map_or((false, tail), |tail| (true, tail));
        let Some(tail) = tail.trim_start().strip_prefix('=') else { continue };
        let tail = tail.trim_start();
        let Some(quote @ ('\'' | '"')) = tail.chars().next() else { continue };
        if let Some(value) = tail[1..].split(quote).next() {
            for value in value.split_whitespace() {
                let id = if plural { value.split_once(':').map_or(value, |(_, id)| id) } else { value };
                anchors.insert(id.to_owned());
            }
        }
    }
    for check in &mut built.plan.screens[0].checks {
        let anchored = [&check.subject, &check.container].into_iter().flatten()
            .filter_map(|locator| locator.css.as_deref())
            .any(|css| css.contains("data-figma-node-id"));
        let selectors = [&check.subject, &check.container].into_iter().flatten().filter_map(|locator| locator.css.as_deref()).collect::<Vec<_>>();
        let missing = selectors.iter().any(|css| {
            css.split('"').enumerate().filter(|(index, _)| index % 2 == 1)
                .all(|(_, id)| !anchors.contains(id) && !id.split_once(':').is_some_and(|(_, node)| anchors.contains(node)))
        });
        let completeness_missing = check.completeness.as_ref().is_some_and(|complete| complete.sections.iter().chain(&complete.images).any(|id| !anchors.contains(id)));
        if (anchored && missing) || completeness_missing {
            check.unavailable_reason = Some("not-run: source anchors absent; data-figma-node-id is off by default. Export with figmaNodeIds:true and preserve anchors, or use explicit role/testId locators.".into());
            check.subject = None;
            check.container = None;
        }
    }
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
    let evidence = super::screen_plan::project_evidence(root).await?;
    let audit_routes = super::screen_plan::audit_routes(plan);
    let routes = audit_routes.iter()
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
        == Some(include_bytes!("../../../../../harness/runtime-audit/run.mjs").as_slice())
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
        let mut screen = if several_apps && !app.is_empty() {
            format!("{}-{}", slug(app), slug(url))
        } else {
            slug(url)
        };
        if let Some(node) = route["state"]["nodeId"].as_str() {
            screen.push_str(&format!("-state-{}", slug(node)));
        }
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
        let mut built = url_plan(&target, base, &sources, thresholds)?;
        gate_source_anchors(&mut built, &Path::new(root).join(app));
        if let Some(node) = route["state"]["nodeId"].as_str() {
            built.notes.push(format!("State {} ({node}) on {url}: configure the runtime screen's steps to reach this state before running; no interaction is inferred from the frame name.", route["state"]["name"].as_str().unwrap_or_default()));
            for check in &mut built.plan.screens[0].checks {
                check.label = Some(format!("State {node}: {}", check.label.as_deref().unwrap_or(&check.kind)));
            }
        }
        for check in &mut built.plan.screens[0].checks {
            if check.kind == "links-resolve" {
                check.planned_targets = routes.iter().filter_map(|route| route["url"].as_str().map(str::to_owned)).collect();
                check.existing_routes = evidence.routes.iter().map(|(route, _)| route.clone()).collect();
            }
        }
        let plan_path = Path::new(root).join(AUDIT_DIR).join(&screen).join("plan.json");
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
            "state": route["state"],
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
        "nodeIdentity": include_str!("../text/audit_plan-write-7f9aea99.md"),
        "nextAction": include_str!("../text/audit_plan-write-925db929.md"),
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
