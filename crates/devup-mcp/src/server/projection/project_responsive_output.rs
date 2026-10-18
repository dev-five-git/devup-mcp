use super::*;

pub(super) struct ResponsiveOutput<'a> {
    pub(super) payload: &'a CollectedPayload,
    pub(super) outputs: &'a Vec<String>,
    pub(super) include_diagnostics: bool,
    pub(super) root_layout: devup_mcp_devup_ui::codegen::RootLayout,
    pub(super) asset_names_per_node: bool,
    pub(super) theme_options: &'a theme_export::ThemeExportOptions,
    pub(super) output_paths: &'a BTreeMap<String, String>,
    pub(super) result: Map<String, Value>,
    pub(super) asset_policies:
        &'a BTreeMap<String, devup_mcp_devup_ui::codegen::AssetRepresentationPolicy>,
    pub(super) selected_projection_roots: Option<Vec<String>>,
    pub(super) projection_diagnostics: Vec<Diagnostic>,
    pub(super) pending_text_outputs: BTreeMap<String, String>,
    pub(super) component_name_for_components: &'a Option<String>,
}

type ResponsiveProjection = Result<
    (
        Map<String, Value>,
        Vec<Diagnostic>,
        BTreeMap<String, String>,
        Vec<f64>,
    ),
    DevupError,
>;

pub(super) fn project_responsive_output(input: ResponsiveOutput<'_>) -> ResponsiveProjection {
    let ResponsiveOutput {
        payload,
        outputs,
        include_diagnostics,
        root_layout,
        asset_names_per_node,
        theme_options,
        output_paths,
        mut result,
        asset_policies,
        selected_projection_roots,
        mut projection_diagnostics,
        mut pending_text_outputs,
        component_name_for_components,
    } = input;
    let mut responsive_snapshot = std::borrow::Cow::Borrowed(&payload.snapshot);
    if let Some(roots) = selected_projection_roots {
        responsive_snapshot.to_mut().roots = roots;
    }
    let mut responsive_widths = Vec::new();
    // A screen captured with its other widths is convertible as one
    // tree. It is offered whenever those widths are present rather than
    // only on request, because a caller asking for a screen that has
    // them almost always wants the responsive form and cannot know from
    // the node id alone whether it exists.
    if outputs
        .iter()
        .any(|output| output == "tsx" || output == "responsiveTsx")
        && let Some(mut merged) = merge_breakpoints(
            &responsive_snapshot,
            &CodegenOptions {
                asset_policies: asset_policies.clone(),
                component_name: component_name_for_components.clone(),
                include_diagnostics,
                inline_instances: false,
                root_layout,
                asset_names_per_node,
                ..CodegenOptions::default()
            }
            .with_payload_tokens(payload),
        )?
        && (outputs.iter().any(|output| output == "responsiveTsx")
            || !merged.slots.windows(2).any(|pair| pair[0] == pair[1]))
    {
        // Named as the plugin names it: after the Section the widths
        // sit in, with `Page` on the end — `AboutPage` for a Section
        // called `about`. The Section is outside the collected
        // subtree, so each width carries its name as `parentName`.
        // A caller's own name wins; without one and without a
        // Section name the page is `ResponsivePage`.
        let section_name = payload
            .snapshot
            .roots
            .iter()
            .filter_map(|root| payload.snapshot.nodes.get(root))
            .find_map(|root| root.typed_view().string("parentName").map(str::to_owned));
        let name = component_name_for_components.clone().map_or_else(
            || {
                section_name.map_or_else(
                    || "ResponsivePage".to_owned(),
                    |section| format!("{}Page", normalize_component_name(&section)),
                )
            },
            |name| normalize_component_name(&name),
        );
        if merged
            .slots
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != merged.slots.len()
        {
            return Err(unavailable_output(
                payload,
                "responsiveTsx",
                "Two frames occupy the same breakpoint slot. Request tsx separately, or select one frame per breakpoint slot.",
            ));
        }
        if let Some(root) = theme_options.project_root.as_deref()
            && super::super::screen_plan::node_ids_enabled(root)
        {
            let written = devup_mcp_devup_ui::codegen::node_ids::annotate_merged(&mut merged);
            let ids = result.entry("figmaNodeIds").or_insert_with(
                || json!({"attribute": devup_mcp_devup_ui::codegen::node_ids::NODE_ID_ATTRIBUTE}),
            );
            ids["perWidthAttribute"] =
                json!(devup_mcp_devup_ui::codegen::node_ids::NODE_IDS_ATTRIBUTE);
            ids["responsive"] = json!(written);
            ids["responsiveFormat"] = json!(include_str!(
                "../text/projection-project_operation-8a156710.md"
            ));
        }
        let module = merged.module(&name);
        // The responsive tree builder does not run the ordinary
        // finalizer. Preserve each breakpoint's source limitations.
        let mut breakpoint_fidelity = Vec::new();
        for root_id in &responsive_snapshot.roots {
            let mut source_output = generate_component(
                &responsive_snapshot,
                root_id,
                &CodegenOptions {
                    asset_policies: asset_policies.clone(),
                    inline_instances: true,
                    root_layout,
                    asset_names_per_node,
                    ..CodegenOptions::default()
                }
                .with_payload_tokens(payload),
            )?;
            scope_output(&mut source_output, "responsiveTsx");
            breakpoint_fidelity
                .push(json!({"rootId":root_id,"fidelity":source_output.fidelity_report}));
            for mut issue in source_output.diagnostics.into_iter().filter(|d| {
                d.fidelity_impact() != devup_mcp_figma::FidelityImpact::None
                    || d.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"
            }) {
                if let Some(details) = issue.details.as_mut().and_then(Value::as_object_mut) {
                    details.insert(
                        "evidenceScope".into(),
                        json!("breakpoint-source-projection"),
                    );
                    details.insert("breakpointRootId".into(), json!(root_id));
                    details.insert("output".into(), json!("responsiveTsx"));
                }
                if !projection_diagnostics.contains(&issue) {
                    projection_diagnostics.push(issue);
                }
            }
        }
        if output_paths.contains_key("responsiveTsx") {
            pending_text_outputs.insert("responsiveTsx".to_owned(), module.clone());
        }
        result.insert("responsiveTsx".to_owned(), json!(module));
        result.insert("responsiveSlots".to_owned(), json!(merged.slots));
        result.insert(
            "responsiveEvidence".into(),
            responsive_groups::width_evidence(&responsive_snapshot),
        );
        result.insert(
            "responsiveReviewChecklist".into(),
            responsive_groups::review(&responsive_snapshot),
        );
        responsive_widths = responsive_snapshot
            .roots
            .iter()
            .filter_map(|id| {
                responsive_snapshot
                    .nodes
                    .get(id)?
                    .typed_view()
                    .number("width")
            })
            .collect();
        let responsive_issues: Vec<_> = projection_diagnostics
            .iter()
            .filter(|d| {
                d.details
                    .as_ref()
                    .is_some_and(|v| v["output"] == "responsiveTsx")
            })
            .cloned()
            .collect();
        let responsive_quality = if merged.unrepresented.is_empty() {
            projection_quality(true, &responsive_issues)
        } else {
            ProjectionQuality::Lossy
        };
        result.entry("outputResults").or_insert_with(|| json!({}))["responsiveTsx"] = json!({
        "state":"produced","projection":responsive_quality,
        "fidelity":{"scope":"breakpoint-source-projections","sourceOutput":"tsx","breakpoints":breakpoint_fidelity,
            "syntaxValid":devup_mcp_devup_ui::validation::validate_tsx(&module).is_ok(),
            "mergedMappingVerified":merged.verified,"unrepresentedCount":merged.unrepresented.len(),
            "mergeGapCount":merged.gaps.len(),
            "mergeGaps":merged.gaps.iter().take(20).collect::<Vec<_>>(),
            "note":if merged.verified {
                "Read at each collected width's slot, the merged module writes that width's own verified tsx: the same elements, values and text. This reads the code; it is not a rendering."
            } else {
                include_str!("../text/projection-project_operation-9a2b2731.md")
            }},
        "_propertyContract":merged.contract});

        if !merged.unrepresented.is_empty() {
            for note in &merged.unrepresented {
                projection_diagnostics.push(Diagnostic {
                code:"DEVUP_CODEGEN_RESPONSIVE_LOSS".into(),
                message:note.detail.clone(),
                node_id:Some(note.node_id.clone()),
                property:Some("childrenIds".into()),
                details:Some(json!({"originalValue":payload.snapshot.nodes.get(&note.node_id).map(|n| &n.fields),
                    "appliedValue":null,"appliedValueReason":"Content is unrepresented in the merged output.","stage":"projection","output":"responsiveTsx"})),
                fidelity_impact:Some(devup_mcp_figma::FidelityImpact::Lossy),
                severity:Some(DiagnosticSeverity::Warning),
                ..Diagnostic::default()
            });
            }
            result.insert(
                "responsiveUnrepresented".to_owned(),
                json!(
                    merged
                        .unrepresented
                        .iter()
                        .map(|note| json!({
                            "nodeId": note.node_id,
                            "detail": note.detail
                        }))
                        .collect::<Vec<_>>()
                ),
            );
        }
    }

    if outputs.iter().any(|output| output == "responsiveTsx")
        && !result.contains_key("responsiveTsx")
    {
        return Err(unavailable_output(
            payload,
            "responsiveTsx",
            "The snapshot has no mergeable breakpoint frames. Request tsx for individual screens, or re-collect a screen with its breakpoint siblings.",
        ));
    }
    Ok((
        result,
        projection_diagnostics,
        pending_text_outputs,
        responsive_widths,
    ))
}
