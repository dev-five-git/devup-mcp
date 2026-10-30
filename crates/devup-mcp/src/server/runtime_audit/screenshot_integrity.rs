enum Integrity {
    Tampered(String),
    Unverified(String),
}
struct VerifiedShot {
    path: String,
    sha256: String,
    width: u32,
    height: u32,
}
fn verify_screenshot(
    policy: &OutputPolicy,
    report_dir: &Path,
    shot: &Value,
    viewport: &ViewportInput,
) -> Result<VerifiedShot, Integrity> {
    verify_capture(policy, report_dir, shot, Some(viewport))
}

/// An element capture has the element's size, not the viewport's; only its
/// bytes and PNG decoding are checked.
fn verify_capture(
    policy: &OutputPolicy,
    report_dir: &Path,
    shot: &Value,
    viewport: Option<&ViewportInput>,
) -> Result<VerifiedShot, Integrity> {
    let path = shot["path"]
        .as_str()
        .ok_or_else(|| Integrity::Unverified("no path".into()))?;
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(Integrity::Unverified(format!(
            "{path} must be relative to the report directory, without .. or a root"
        )));
    }
    let joined = report_dir.join(relative);
    let target = policy
        .resolve(&joined.to_string_lossy())
        .map_err(|error| Integrity::Unverified(format!("{path}: {}", error.message)))?;
    let bytes = target
        .read_bounded(MAX_SCREENSHOT_BYTES)
        .map_err(|error| Integrity::Unverified(format!("{path}: {}", error.message)))?;
    let digest = sha256(&bytes);
    if shot["sha256"].as_str() != Some(digest.as_str())
        || shot["bytes"].as_u64() != Some(bytes.len() as u64)
    {
        return Err(Integrity::Tampered(format!(
            "{path}: its SHA-256 or length differs from the report"
        )));
    }
    let [width, height] = devup_mcp_visual::png_dimensions(&bytes).map_err(|_| {
        Integrity::Unverified(format!(
            "{path}: not a decodable PNG within 8192px per side"
        ))
    })?;
    if let Some(viewport) = viewport {
        let expected = (f64::from(viewport.width) * viewport.device_scale_factor).round() as u32;
        let minimum = (f64::from(viewport.height) * viewport.device_scale_factor).round() as u32;
        if width != expected || height < minimum {
            return Err(Integrity::Unverified(format!(
                "{path}: {width}x{height} is not {expected} wide and at least {minimum} tall; the capture does not show this viewport"
            )));
        }
    }
    Ok(VerifiedShot {
        path: target.display_path().display().to_string(),
        sha256: digest,
        width,
        height,
    })
}

/// The one place a finding's identity is made, so a later run of the same
/// check at the same width finds its earlier attempts.
fn comparison_id(screen: &ScreenInput, kind: &str, subject: &str, width: u32) -> String {
    format!("{}|{kind}|{subject}|{width}", screen.id)
}

fn diff_path(actual: &str, id: &str) -> String {
    let directory = Path::new(actual).parent().unwrap_or(Path::new(""));
    let name = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    directory
        .join("diff")
        .join(format!("{name}.png"))
        .display()
        .to_string()
}

fn array_text(value: &Value) -> String {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("; ")
}

/// What kind of problem a failed check is, so a fix starts in the right
/// place: a page that does not render, content wider than its box, a box in
/// the wrong place or of the wrong size, a state or a transition missing.
pub(crate) fn failure_class(kind: &str, reason: &str) -> &'static str {
    match kind {
        "page-health" => "runtime-error",
        "image-loaded" | "stale-resources" => "asset-mismatch",
        "no-low-contrast-text" | "native-appearance" => "section-visual",
        "rendered-completeness" => "page-visual",
        "no-horizontal-overflow" | "no-clipped-text" => "overflow",
        "bounds" if reason.starts_with("position") => "layout-position",
        "bounds" | "dimensions" | "visible" => "layout-size",
        "contained" | "aligned" | "spans" => "layout-position",
        "interaction" => "interaction",
        "view-transition" => "view-transition",
        _ => "layout-position",
    }
}

/// A concrete first step for each class. The measured evidence names the
/// element where the executor reported one.
pub(crate) fn fix_hint(class: &str, kind: &str, measured: &Value) -> String {
    match class {
        "runtime-error" => include_str!("../text/runtime_audit-fix_hint-3a12f919.md").to_owned(),
        "overflow" if kind == "no-clipped-text" => {
            include_str!("../text/runtime_audit-fix_hint-b3941912.md").to_owned()
        }
        "overflow" => format!(
            include_str!("../text/runtime_audit-fix_hint-975acb57.md"),
            measured["widest"]
                .as_str()
                .map(|widest| format!(" (widest: {widest})"))
                .unwrap_or_default()
        ),
        "layout-position" | "layout-size" if kind == "bounds" => {
            include_str!("../text/runtime_audit-fix_hint-1f874fed.md").to_owned()
        }
        "layout-position" => include_str!("../text/runtime_audit-fix_hint-516a692f.md").to_owned(),
        "layout-size" => include_str!("../text/runtime_audit-fix_hint-1ca5b7eb.md").to_owned(),
        "code-drift" => include_str!("../text/runtime_audit-fix_hint-60ba4f27.md").to_owned(),
        "converter-bug" => include_str!("../text/runtime_audit-fix_hint-0e677049.md").to_owned(),
        "asset-mismatch" => include_str!("../text/runtime_audit-fix_hint-650b18d4.md").to_owned(),
        "section-visual" => include_str!("../text/runtime_audit-fix_hint-a13b2885.md").to_owned(),
        "page-visual" => include_str!("../text/runtime_audit-fix_hint-5960cebc.md").to_owned(),
        "interaction" => include_str!("../text/runtime_audit-fix_hint-e19dc0a1.md").to_owned(),
        "view-transition" => include_str!("../text/runtime_audit-fix_hint-7a003bba.md").to_owned(),
        _ => "Re-run the check and read its reason.".to_owned(),
    }
}

fn finding(
    screen: &ScreenInput,
    viewport: &ViewportInput,
    check: Option<(usize, &CheckInput)>,
    class: &str,
    reason: &str,
    measured: &Value,
    screenshot: Option<&str>,
) -> Value {
    let kind = check.map_or("screen", |(_, check)| check.kind.as_str());
    let subject = check
        .and_then(|(index, check)| {
            check
                .node_id
                .clone()
                .or_else(|| check.label.clone())
                .or_else(|| Some(format!("#{index}")))
        })
        .unwrap_or_else(|| "page".to_owned());
    json!({
        "id": comparison_id(screen, kind, &subject, viewport.width),
        "class": class,
        "screen": screen.id,
        "routePath": screen.route_path,
        "width": viewport.width,
        "check": check.map(|(index, _)| index),
        "kind": kind,
        "label": check.and_then(|(_, check)| check.label.clone()),
        "nodeId": check.and_then(|(_, check)| check.node_id.clone()),
        "codeFile": screen.component_path,
        "reason": reason,
        "fixHint": fix_hint(class, kind, measured),
        "evidence": {"screenshot": screenshot, "measured": measured},
    })
}
