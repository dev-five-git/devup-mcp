fn validate_check(check: &CheckInput) -> Result<(), DevupError> {
    if !check.allowed_marquees.is_empty() {
        if check.kind != "no-clipped-content" || check.allowed_marquees.len()>16 {
            return Err(invalid("allowedMarquees is bounded to 16 selectors on no-clipped-content checks."));
        }
        for selector in &check.allowed_marquees { bounded_text("allowedMarquees selector",selector)?; }
    }
    if let Some(reason) = &check.unavailable_reason {
        return bounded_text("unavailableReason", reason);
    }
    let needs = |subject: bool, container: bool| -> Result<(), DevupError> {
        if subject != check.subject.is_some() || container != check.container.is_some() {
            return Err(invalid(format!(
                "A {} check takes {}subject and {}container.",
                check.kind,
                if subject { "a " } else { "no " },
                if container { "a " } else { "no " },
            )));
        }
        Ok(())
    };
    if check.kind == "motion" {
        needs(false, false)?;
        return validate_motion(check.motion.as_ref());
    }
    match check.kind.as_str() {
        "regression" => {
            needs(false, false)?;
            let config = check.regression.as_ref().ok_or_else(|| invalid("regression requires its browser contract."))?;
            validate_regression(config)?;
        }
        "data-binding" => {
            needs(false, false)?;
            let config=check.data_binding.as_ref().ok_or_else(||invalid("data-binding requires dataBinding configuration."))?;
            super::data_verification::validate_config(config)?;
        }
        "links-resolve" | "no-horizontal-overflow" | "page-health" | "no-clipped-content" => {
            needs(false, false)?
        }
        "visible" | "view-transition" | "hidden" => needs(true, false)?,
        "stale-resources" | "styles-applied" | "markup" => needs(false, false)?,
        "no-clipped-text" | "no-low-contrast-text" | "image-loaded" | "native-appearance"
        | "input-focus" => {
            if check.container.is_some() {
                return Err(invalid(format!(
                    "A {} check takes no container.",
                    check.kind
                )));
            }
        }
        "bounds" => {
            needs(true, true)?;
            if check.tolerance_px.is_some() {
                return Err(invalid(
                    "A bounds check is exact (half a pixel of rounding); it takes no tolerancePx. A mismatch is code drift or a converter bug, never noise.",
                ));
            }
            let Some(expected) = &check.expected else {
                return Err(invalid(
                    "A bounds check needs expected {x, y, width, height}.",
                ));
            };
            if [expected.x, expected.y, expected.width, expected.height]
                .iter()
                .any(|value| !value.is_finite() || value.abs() > 100_000.0)
                || expected.width < 0.0
                || expected.height < 0.0
            {
                return Err(invalid(
                    "bounds expected values must be finite px, width and height nonnegative.",
                ));
            }
        }
        "interaction" => {
            // Without a subject it sweeps every visible link and button.
            if check.container.is_some() {
                return Err(invalid("An interaction check takes no container."));
            }
            if !matches!(check.state.as_deref(), Some("hover" | "focus")) {
                return Err(invalid("An interaction check needs state hover or focus."));
            }
        }
        "capture" => {
            needs(true, false)?;
            let Some(reference) = &check.reference else {
                return Err(invalid(
                    "A capture check needs reference {scope, artifactId, assetId}.",
                ));
            };
            if !matches!(reference.scope.as_str(), "section" | "asset") {
                return Err(invalid("capture reference.scope must be section or asset."));
            }
            bounded_text("reference.artifactId", &reference.artifact_id)?;
            bounded_text("reference.assetId", &reference.asset_id)?;
            if reference
                .threshold
                .is_some_and(|value| !(0.0..=1.0).contains(&value))
            {
                return Err(invalid("reference.threshold must be between 0 and 1."));
            }
            if reference.max_luminance_delta.is_some_and(|limit| !(0.0..=255.0).contains(&limit)) {
                return Err(invalid("reference.maxLuminanceDelta must be between 0 and 255."));
            }
        }
        "dimensions" => {
            needs(true, false)?;
            if check.tolerance_px.is_none()
                || check.figma_dimensions.is_empty()
                || !matches!(check.measure.as_deref(), None | Some("box"))
            {
                return Err(invalid(
                    "A dimensions check needs tolerancePx, figmaDimensions and measure box.",
                ));
            }
        }
        "text-align" => {
            needs(true, false)?;
            if !matches!(
                check.edge.as_deref(),
                Some("left" | "right" | "center" | "justify")
            ) {
                return Err(invalid(
                    "A text-align check needs edge left, right, center or justify.",
                ));
            }
        }
        "sequence-order" => needs(false, false)?,
        "section-layout" => {
            if check.subject.is_some() {
                needs(true, true)?;
            } else {
                needs(false, false)?;
            }
        }
        "rendered-completeness" => {
            needs(false, false)?;
            let Some(completeness) = &check.completeness else {
                return Err(invalid(
                    "A rendered-completeness check needs completeness {frameHeight, sections, images}.",
                ));
            };
            validate_completeness(completeness)?;
            if check.widths.len() != 1 {
                return Err(invalid(
                    "A rendered-completeness check holds one frame, so it names exactly one width.",
                ));
            }
        }
        "contained" | "spans" => needs(true, true)?,
        "aligned" => {
            needs(true, true)?;
            if !matches!(check.edge.as_deref(), Some("left" | "right" | "center")) {
                return Err(invalid(
                    "An aligned check needs edge left, right or center.",
                ));
            }
        }
        other => return Err(invalid(format!("Unknown check kind {other}."))),
    }
    for (field, present, kind) in [
        ("dataBinding",check.data_binding.is_some(),"data-binding"),
        ("regression", check.regression.is_some(), "regression"),
        ("expected", check.expected.is_some(), "bounds"),
        ("state", check.state.is_some(), "interaction"),
        ("reference", check.reference.is_some(), "capture"),
        (
            "completeness",
            check.completeness.is_some(),
            "rendered-completeness",
        ),
    ] {
        if present && check.kind != kind {
            return Err(invalid(format!("{field} is only valid for {kind} checks.")));
        }
    }
    if let Some(node_id) = &check.node_id {
        bounded_text("checks[].nodeId", node_id)?;
    }
    if check.kind != "dimensions" && !check.figma_dimensions.is_empty() {
        return Err(invalid(
            "figmaDimensions is only valid for dimensions checks.",
        ));
    }
    let mut widths = std::collections::BTreeSet::new();
    for reference in &check.figma_dimensions {
        bounded_text("figmaDimensions.artifactId", &reference.artifact_id)?;
        bounded_text("figmaDimensions.nodeId", &reference.node_id)?;
        if !widths.insert(reference.viewport_width) {
            return Err(invalid(
                "A dimensions check needs at most one explicit reference per width.",
            ));
        }
    }
    for (field, locator) in [("subject", &check.subject), ("container", &check.container)] {
        if let Some(locator) = locator {
            validate_locator(&format!("checks[].{field}"), locator, true)?;
        }
    }
    match check.measure.as_deref() {
        None | Some("box") => {}
        Some("painted-mask") if check.subject.is_some() => {}
        Some(other) => {
            return Err(invalid(format!(
                "measure {other} is not box, or painted-mask on a check with a subject."
            )));
        }
    }
    for value in [check.offset_px, check.inset_px, check.tolerance_px]
        .into_iter()
        .flatten()
    {
        if !value.is_finite() || value.abs() > 10_000.0 {
            return Err(invalid(
                "Check offsets, insets and tolerances must be finite px values.",
            ));
        }
    }
    if check.tolerance_px.is_some_and(|value| value < 0.0) {
        return Err(invalid("tolerancePx must be nonnegative."));
    }
    if let Some(label) = &check.label {
        bounded_text("checks[].label", label)?;
    }
    Ok(())
}
