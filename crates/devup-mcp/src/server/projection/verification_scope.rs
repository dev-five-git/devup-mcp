/// Which widths the generated code is checked at, and how: the mapping and the
/// width-change report are static, and nothing here was rendered. Said apart
/// so an exact grade at the drawn width is not read as a claim about others.
fn attach_verification_scope(value: &mut Value, responsive_widths: &[f64]) {
    let mut widths = value["widthBehavior"]["designWidth"]
        .as_f64()
        .into_iter()
        .chain(
            value["frames"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|frame| frame["widthBehavior"]["designWidth"].as_f64()),
        )
        .chain(responsive_widths.iter().copied())
        .collect::<Vec<_>>();
    widths.sort_by(f64::total_cmp);
    widths.dedup();
    if widths.is_empty() {
        return;
    }
    let mut scope = json!({
        "collectedWidths": widths,
        "staticallyChecked": {
            "mapping": "Each generated property against its source field, at the collected widths (quality.projection).",
            "widthChange": "Anchors and sizing intent when a container is not its drawn width (widthBehavior); read from constraints and geometry, not rendered.",
        },
        "rendered": {
            "state": "not-performed",
            "note": "Nothing was rendered. devup_visual_compare compares a consumer-rendered PNG at a drawn width; devup_runtime_audit plans and ingests a consumer-side browser run at any widths.",
        },
    });
    if !responsive_widths.is_empty() {
        scope["betweenCollectedWidths"] = json!(
            "responsiveTsx switches values at breakpoints; widths between and beyond the collected ones were never drawn."
        );
    }
    value["verificationScope"] = scope;
}

/// Which further outputs this artifactId can project without calling Figma,
/// and which need the original url again. Each output keeps its own
/// preconditions (responsiveTsx still needs several widths); this only says
/// whether the capture holds what it reads.
fn reuse_summary(capabilities: &super::artifacts::ArtifactCapabilities) -> Value {
    use super::artifacts::ArtifactKind;
    use devup_mcp_figma::ResourceScope;
    let mut with_artifact = Vec::new();
    let mut needs_url = Vec::new();
    match capabilities.kind {
        ArtifactKind::Design => {
            with_artifact.extend([
                "outline",
                "tsx",
                "componentTsx",
                "responsiveTsx",
                "sourceMap",
                "assetManifest",
                "designFingerprints",
                "designChanges",
                "pageScaffold",
                "rawSnapshot",
                "rawPayload",
            ]);
            if capabilities.resource_scope == ResourceScope::None {
                needs_url.push("devupJson");
            } else {
                with_artifact.push("devupJson");
            }
            if capabilities.resource_scope != ResourceScope::File {
                needs_url.push("devupJson:scope=file");
            }
            if capabilities.reference_png {
                with_artifact.push("referencePng");
            } else {
                needs_url.push("referencePng");
            }
            needs_url.push("assetRequests-not-yet-captured");
        }
        ArtifactKind::ThemeOnly => {
            with_artifact.push("devupJson");
            needs_url.push("design-outputs");
        }
        ArtifactKind::SectionIndex => {}
        ArtifactKind::Measurement | ArtifactKind::Search | ArtifactKind::Explore => {
            needs_url.push("export-outputs")
        }
    }
    let mut reuse = json!({
        "withArtifactId": with_artifact,
        "needsUrl": needs_url,
        "processBound": "Held in this devup-mcp process's memory until expiresAt; a restarted or different process cannot serve this artifactId.",
    });
    if capabilities.kind == ArtifactKind::SectionIndex {
        reuse["selection"] = json!(
            "frameIds or allScreens with this artifactId collects the selected screens; nothing is projected from the index itself."
        );
    }
    reuse
}
