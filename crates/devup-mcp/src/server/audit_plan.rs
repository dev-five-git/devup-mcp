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
/// Sections and cards hold text, whose line boxes differ between Figma and
/// a browser by a few pixels; an asset's box does not.
const LAYOUT_TOLERANCE_PX: f64 = 4.0;
const ASSET_TOLERANCE_PX: f64 = 2.0;
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
                    "The {width}px frame {node_id} is {height}px tall, over the {MAX_VIEWPORT_SIDE}px a viewport can be; its page comparison stays inconclusive and its sections carry the visual check."
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
                    if is_asset {
                        ASSET_TOLERANCE_PX
                    } else {
                        LAYOUT_TOLERANCE_PX
                    },
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
                                LAYOUT_TOLERANCE_PX,
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
                        ASSET_TOLERANCE_PX,
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
        fonts: Vec::new(),
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

fn bounds_check(
    name: &str,
    subject: &str,
    container: &str,
    container_box: Bounds,
    subject_box: Bounds,
    width: u32,
    tolerance: f64,
) -> CheckInput {
    let round = |value: f64| (value * 100.0).round() / 100.0;
    check(
        json!({"kind": "bounds", "label": format!("{name} size and position"),
        "subject": node(subject), "container": node(container), "nodeId": subject,
        "expected": {"x": round(subject_box.x - container_box.x), "y": round(subject_box.y - container_box.y),
            "width": round(subject_box.width), "height": round(subject_box.height)},
        "tolerancePx": tolerance, "widths": [width]}),
    )
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
        "subject": node(subject), "nodeId": subject, "widths": [width],
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
    let urls = routes
        .iter()
        .filter_map(|route| route["url"].as_str())
        .filter(|url| !url.contains('['))
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
    let mut not_audited = Vec::new();
    for route in &routes {
        let url = route["url"].as_str().unwrap_or_default();
        if !only.is_empty() && !only.iter().any(|wanted| wanted == url) {
            continue;
        }
        if url.contains('[') {
            not_audited.push(json!({"url": url, "why": "A dynamic route has no path to open; audit a real path once the data gives one."}));
            continue;
        }
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
        let target = Target {
            url,
            screen: screen.clone(),
            file: route["file"].as_str(),
            frames,
            next_url,
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
    Ok(json!({
        "plans": written,
        "notAudited": not_audited,
        "missingReferences": missing,
        "expiredArtifacts": expired,
        "runner": {"directory": runner.display().to_string(), "state": template_state,
            "install": [["npm", "ci"], ["npx", "playwright", "install", "chromium"]],
            "ifInstallFails": "Record it with devup_screen_plan action status, auditPlanPath and toolingUnavailable: the URL's audit stays inconclusive and the work goes on."},
        "nodeIdentity": "Sections, cards and assets are found by data-figma-node-id, which devup_figma_export writes into a project's TSX. A check whose element is missing reports missingNodes; re-export with projectRoot.",
        "nextAction": "For each URL in order: export what missingReferences names and run auditPlan again, start the app, run the plan, then call devup_screen_plan action status with auditPlanPath and auditReportPath (report/report.json). Run each visualComparisons call, pass the results back as visualResults, fix every finding, and repeat until the URL is done.",
    }))
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
        record["status"] = json!(if fix_attempts >= u64::from(max_fix_attempts) {
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
            json!({"url": url, "items": items, "done": done})
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
        "| URL | width | health | geometry | page | section | asset | interaction | screenshot | diffs |\n|---|---|---|---|---|---|---|---|---|---|\n",
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
            out.push_str(&format!(
                "| {url} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                width["width"],
                cell(&columns["health"]),
                cell(&columns["geometry"]),
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
