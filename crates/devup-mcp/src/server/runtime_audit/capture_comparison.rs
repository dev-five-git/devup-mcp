/// The `devup_visual_compare` call for one element capture against the
/// Figma node's own native render.
fn capture_comparison(
    screen: &ScreenInput,
    index: usize,
    check: &CheckInput,
    reference: &CaptureReferenceInput,
    viewport: &ViewportInput,
    shot: &VerifiedShot,
) -> Value {
    let class = if reference.scope == "asset" {
        "asset-mismatch"
    } else {
        "section-visual"
    };
    let subject = check.node_id.clone().unwrap_or_else(|| format!("#{index}"));
    let id = comparison_id(screen, &reference.scope, &subject, viewport.width);
    let threshold = reference
        .threshold
        .unwrap_or(if reference.scope == "asset" {
            ASSET_THRESHOLD
        } else {
            SECTION_THRESHOLD
        });
    let mut blockers = Vec::new();
    if (viewport.device_scale_factor - 1.0).abs() > f64::EPSILON {
        blockers.push(format!(
            "Captured at deviceScaleFactor {}; the native render is at scale 1.",
            viewport.device_scale_factor
        ));
    }
    json!({
        "id": id,
        "class": class,
        "screen": screen.id,
        "routePath": screen.route_path,
        "nodeId": check.node_id,
        "label": check.label,
        "check": index,
        "codeFile": screen.component_path,
        "viewport": viewport,
        "screenshot": {"path": shot.path, "sha256": shot.sha256, "width": shot.width, "height": shot.height},
        "call": {
            "tool": "devup_visual_compare",
            "arguments": {
                "actual": shot.path,
                "reference": {"artifactId": reference.artifact_id, "assetId": reference.asset_id},
                "threshold": threshold,
                "diffPath": diff_path(&shot.path, &id),
            },
        },
        "ifItFails": {"class": class, "fixHint": fix_hint(class, "capture", &Value::Null)},
        "blockers": blockers,
        "note": "A size mismatch with the native render is reported as dimensions, not pixels: fix the element's size (its bounds check says by how much) and compare again.",
    })
}

/// The `devup_visual_compare` call for one screenshot taken at a width its
/// Figma frame was drawn at, with every field the audit itself knows filled
/// in, the ones only the consumer can state named, and what would still make
/// it inconclusive said up front.
fn visual_comparison(
    screen: &ScreenInput,
    node_id: &str,
    viewport: &ViewportInput,
    shot: &VerifiedShot,
    report: &Value,
    environment_valid: bool,
) -> Value {
    let mut environment = json!({
        "schemaVersion": 1,
        "actualPng": shot.path,
        "viewport": {"width": viewport.width, "height": viewport.height,
            "deviceScaleFactor": viewport.device_scale_factor},
        "environmentStatus": if environment_valid { "valid" } else { "environment-invalid" },
    });
    let mut must_add = vec![
        "reference.artifactId",
        "environment.tsxResource",
        "environment.themePath",
        "environment.assetDirectory",
        "environment.devupUiVersion",
        "environment.fontManifest",
    ];
    let browser = &report["executor"]["browser"];
    match (browser["name"].as_str(), browser["version"].as_str()) {
        (Some(name), Some(version)) => {
            environment["renderer"] =
                json!({"name": format!("playwright-{name}"), "version": version});
        }
        _ => must_add.push("environment.renderer"),
    }
    for (field, value) in [
        ("os", &report["environment"]["os"]),
        ("locale", &report["environment"]["locale"]),
        ("timezone", &report["environment"]["timezone"]),
    ] {
        match value.as_str() {
            Some(text) => environment[field] = json!(text),
            None => must_add.push(match field {
                "os" => "environment.os",
                "locale" => "environment.locale",
                _ => "environment.timezone",
            }),
        }
    }
    let mut blockers = Vec::new();
    if (viewport.device_scale_factor - 1.0).abs() > f64::EPSILON {
        blockers.push(format!(
            "Captured at deviceScaleFactor {}; the Figma reference is at scale 1, so plan this width at 1.",
            viewport.device_scale_factor
        ));
    }
    if !environment_valid {
        blockers.push(
            "This run reported environment errors or an unverified screenshot; the comparison stays inconclusive."
                .to_owned(),
        );
    }
    let frame = screen
        .figma
        .iter()
        .flat_map(|figma| &figma.frames)
        .find(|frame| frame.width == viewport.width);
    if let Some(height) = frame.and_then(|frame| frame.height)
        && (shot.width != viewport.width || shot.height != height)
    {
        blockers.push(format!(
            include_str!("../text/runtime_audit-visual_comparison-8010a061.md"),
            shot.width, shot.height, viewport.width, height
        ));
    }
    let node_id = frame.map_or(node_id, |frame| frame.node_id.as_str());
    let artifact = frame.and_then(|frame| frame.artifact_id.clone());
    if artifact.is_some() {
        must_add.retain(|field| *field != "reference.artifactId");
    }
    let id = comparison_id(screen, "page", node_id, viewport.width);
    let threshold = frame
        .and_then(|frame| frame.threshold)
        .unwrap_or(PAGE_THRESHOLD);
    json!({
        "id": id,
        "class": "page-visual",
        "screen": screen.id,
        "routePath": screen.route_path,
        "nodeId": node_id,
        "codeFile": screen.component_path,
        "viewport": viewport,
        "screenshot": {"path": shot.path, "sha256": shot.sha256, "width": shot.width, "height": shot.height},
        "call": {
            "tool": "devup_visual_compare",
            "arguments": {
                "actual": shot.path,
                "reference": {"artifactId": artifact.unwrap_or_else(|| format!("<artifactId of devup_figma_export for node {node_id} with outputs [\"referencePng\"]>"))},
                "threshold": threshold,
                "diffPath": diff_path(&shot.path, &id),
                "environment": environment,
            },
        },
        "ifItFails": {"class": "page-visual", "fixHint": fix_hint("page-visual", "", &Value::Null)},
        "consumerMustAdd": must_add,
        "blockers": blockers,
        "note": include_str!("../text/runtime_audit-visual_comparison-8418b2cf.md"),
    })
}
/// What a page must render to count as built from its frame: the height the
/// frame was drawn at, every section, every image, and text a person can
/// see. Code on disk that renders a blank, a cut-short page, a broken image
/// or white-on-white text is written, not implemented.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct CompletenessInput {
    /// The frame's height at this width, in Figma logical px.
    pub frame_height: f64,
    /// The rendered page must be at least this share of frameHeight tall;
    /// default 0.9.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_height_ratio: Option<f64>,
    /// Figma node ids of the frame's sections; each must render visible with
    /// a non-empty box.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<String>,
    /// Figma node ids of the frame's images and icons; each must render with
    /// a non-empty box, an <img> decoded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    /// Lowest contrast ratio text may have against the background painted
    /// behind it; default 1.5, below which text cannot be seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_contrast: Option<f64>,
}

/// Most node ids one completeness check lists per kind.
pub(crate) const MAX_COMPLETENESS_NODES: usize = 200;

fn validate_completeness(completeness: &CompletenessInput) -> Result<(), DevupError> {
    if !completeness.frame_height.is_finite()
        || completeness.frame_height <= 0.0
        || completeness.frame_height > 100_000.0
    {
        return Err(invalid(
            "completeness.frameHeight must be a positive finite px value.",
        ));
    }
    if completeness
        .min_height_ratio
        .is_some_and(|ratio| !(ratio > 0.0 && ratio <= 1.0))
    {
        return Err(invalid("completeness.minHeightRatio must be in (0, 1]."));
    }
    if completeness
        .min_contrast
        .is_some_and(|ratio| !(1.0..=21.0).contains(&ratio))
    {
        return Err(invalid("completeness.minContrast must be in [1, 21]."));
    }
    for (field, ids) in [
        ("completeness.sections", &completeness.sections),
        ("completeness.images", &completeness.images),
    ] {
        if ids.len() > MAX_COMPLETENESS_NODES {
            return Err(invalid(format!(
                "{field} lists at most {MAX_COMPLETENESS_NODES} node ids."
            )));
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in ids {
            bounded_text(&format!("{field}[]"), id)?;
            if id.contains(['"', '\\', ']']) {
                return Err(invalid(format!(
                    "{field}[] must be a Figma node id, without quotes, backslashes or brackets."
                )));
            }
            if !seen.insert(id.as_str()) {
                return Err(invalid(format!("{field} repeats {id}.")));
            }
        }
    }
    Ok(())
}

/// At a drawn width, checks that pass on code rendering a blank or half a
/// page prove nothing: without a rendered-completeness check the width stays
/// not-run.
fn require_completeness(
    row: &mut Value,
    screen: &ScreenInput,
    viewport: &ViewportInput,
    judged: &[(&str, &CheckInput)],
) {
    let drawn_here = screen
        .figma
        .as_ref()
        .is_some_and(|figma| figma.drawn_widths.contains(&viewport.width));
    if drawn_here
        && row["status"] == "pass"
        && !judged
            .iter()
            .any(|(_, check)| check.kind == "rendered-completeness")
    {
        row["status"] = json!("not-run");
        row["reasons"] = json!([format!(
            "no rendered-completeness check at {}px, a width the frame was drawn at; passing checks do not show the page renders its frame",
            viewport.width
        )]);
    }
}
