use super::*;

pub(super) struct NodeAndThemeOutputs<'a> {
    pub(super) payload: &'a CollectedPayload,
    pub(super) outputs: &'a Vec<String>,
    pub(super) component_name: Option<String>,
    pub(super) include_diagnostics: bool,
    pub(super) root_layout: devup_mcp_devup_ui::codegen::RootLayout,
    pub(super) asset_names_per_node: bool,
    pub(super) scope: &'a String,
    pub(super) theme_options: &'a theme_export::ThemeExportOptions,
    pub(super) strict: bool,
    pub(super) output_paths: &'a BTreeMap<String, String>,
    pub(super) width_policy: &'a Option<devup_mcp_devup_ui::codegen::width::WidthPolicy>,
    pub(super) result: Map<String, Value>,
    pub(super) failures: &'a mut Vec<Value>,
    pub(super) warnings: Vec<Value>,
    pub(super) asset_policies:
        &'a BTreeMap<String, devup_mcp_devup_ui::codegen::AssetRepresentationPolicy>,
    pub(super) scaffold_requested: bool,
    pub(super) keep_tsx: bool,
    pub(super) section_tsx_projected: bool,
    pub(super) tsx_source_map: Option<devup_mcp_devup_ui::provenance::SourceMap>,
    pub(super) generated_output_target: &'static str,
    pub(super) devup_json_source_map: Option<devup_mcp_devup_ui::provenance::SourceMap>,
    pub(super) projection_diagnostics: Vec<Diagnostic>,
    pub(super) fidelity_reports: Vec<devup_mcp_devup_ui::provenance::FidelityReport>,
    pub(super) theme_conflict_count: usize,
    pub(super) theme_unresolved_count: usize,
    pub(super) pending_text_outputs: BTreeMap<String, String>,
    pub(super) component_name_for_components: &'a Option<String>,
}

type NodeAndThemeProjection = Result<
    (
        Map<String, Value>,
        Vec<Diagnostic>,
        Vec<devup_mcp_devup_ui::provenance::FidelityReport>,
        BTreeMap<String, String>,
        Option<devup_mcp_devup_ui::provenance::SourceMap>,
        &'static str,
        Option<devup_mcp_devup_ui::provenance::SourceMap>,
        usize,
        usize,
        Vec<Value>,
    ),
    DevupError,
>;

pub(super) fn project_node_and_theme_outputs(
    input: NodeAndThemeOutputs<'_>,
) -> NodeAndThemeProjection {
    let NodeAndThemeOutputs {
        payload,
        outputs,
        component_name,
        include_diagnostics,
        root_layout,
        asset_names_per_node,
        scope,
        theme_options,
        strict,
        output_paths,
        width_policy,
        mut result,
        failures,
        mut warnings,
        asset_policies,
        scaffold_requested,
        keep_tsx,
        section_tsx_projected,
        mut tsx_source_map,
        mut generated_output_target,
        mut devup_json_source_map,
        mut projection_diagnostics,
        mut fidelity_reports,
        mut theme_conflict_count,
        mut theme_unresolved_count,
        mut pending_text_outputs,
        component_name_for_components,
    } = input;
    if (keep_tsx || scaffold_requested) && !section_tsx_projected {
        let node_id = payload.target.node_id.as_deref().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "A TSX export payload requires a node ID.",
                false,
            )
        })?;
        let mut output = generate_component(
            &payload.snapshot,
            node_id,
            &CodegenOptions {
                asset_policies: asset_policies.clone(),
                component_name,
                include_diagnostics,
                inline_instances: true,
                root_layout,
                asset_names_per_node,
                width_policy: width_policy.clone(),
                ..CodegenOptions::default()
            }
            .with_payload_tokens(payload),
        )?;
        if theme_options.figma_node_ids {
            let annotated = annotate_node_ids(&mut output, &payload.snapshot, node_id);
            result.insert("figmaNodeIds".to_owned(), annotated);
        }
        let coverage = super::super::asset_guard::export_coverage(
            &payload.snapshot,
            node_id,
            &mut output,
            true,
            theme_options.figma_node_ids,
        );
        scope_output(&mut output, "tsx");
        result.entry("outputResults").or_insert_with(|| json!({}))["tsx"] = output_result(&output);
        result["outputResults"]["tsx"]["sectionCoverage"] = coverage;
        result.insert(
            "widthBehavior".to_owned(),
            devup_mcp_devup_ui::codegen::width::width_behavior(&payload.snapshot, node_id, &output),
        );
        projection_diagnostics.extend(output.diagnostics.iter().cloned());
        fidelity_reports.push(output.fidelity_report.clone());
        generated_output_target = "tsx";
        tsx_source_map = Some(output.source_map.clone());
        attach_overlay_anchor_delivery(
            &payload.snapshot,
            &[node_id.to_owned()],
            &output.tsx,
            "tsx",
            &mut result,
        );
        if keep_tsx && output_paths.contains_key("tsx") {
            pending_text_outputs.insert("tsx".to_owned(), output.tsx.clone());
        }
        result.insert("tsx".to_owned(), json!(output.tsx));
        if include_diagnostics {
            result.insert(
                "diagnostics".to_owned(),
                json!(
                    output
                        .diagnostics
                        .iter()
                        .filter(|d| d.code != "DEVUP_CODEGEN_PROPERTY_EVIDENCE")
                        .collect::<Vec<_>>()
                ),
            );
        }
    }

    if outputs.iter().any(|output| output == "componentTsx") && !section_tsx_projected {
        let node_id = payload.target.node_id.as_deref().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "A component TSX export payload requires a node ID.",
                false,
            )
        })?;
        let output = generate_collected_component(
            payload,
            node_id,
            &CodegenOptions {
                asset_policies: asset_policies.clone(),
                component_name: component_name_for_components.clone(),
                include_diagnostics: false,
                inline_instances: false,
                root_layout,
                asset_names_per_node,
                width_policy: width_policy.clone(),
                ..CodegenOptions::default()
            }
            .with_payload_tokens(payload),
        );
        match output {
            Ok(mut output) => {
                if theme_options.figma_node_ids {
                    let annotated = annotate_node_ids(&mut output, &payload.snapshot, node_id);
                    result.insert("figmaNodeIds".to_owned(), annotated);
                }
                let definitions = component_definition_actions(payload, &output.tsx, &result);
                if !definitions.is_empty() {
                    result.insert("componentDefinitions".to_owned(), json!(definitions));
                }
                let coverage = super::super::asset_guard::export_coverage(
                    &payload.snapshot,
                    node_id,
                    &mut output,
                    false,
                    theme_options.figma_node_ids,
                );
                scope_output(&mut output, "componentTsx");
                result.entry("outputResults").or_insert_with(|| json!({}))["componentTsx"] =
                    output_result(&output);
                result["outputResults"]["componentTsx"]["sectionCoverage"] = coverage;
                projection_diagnostics.extend(output.diagnostics.iter().cloned());
                fidelity_reports.push(output.fidelity_report.clone());
                if tsx_source_map.is_none() {
                    generated_output_target = "componentTsx";
                    tsx_source_map = Some(output.source_map.clone());
                }
                attach_overlay_anchor_delivery(
                    &payload.snapshot,
                    &[node_id.to_owned()],
                    &output.tsx,
                    "componentTsx",
                    &mut result,
                );
                if output_paths.contains_key("componentTsx") {
                    pending_text_outputs.insert("componentTsx".to_owned(), output.tsx.clone());
                }
                result.insert("componentTsx".to_owned(), json!(output.tsx));
                if let Some(summary) = component_set_variants(&payload.snapshot, node_id) {
                    result.insert("componentSetVariants".to_owned(), summary);
                }
            }
            Err(error) => {
                failures.push(projection_failure(payload, node_id, "componentTsx", &error))
            }
        }
    }

    if (keep_tsx || scaffold_requested || outputs.iter().any(|output| output == "componentTsx"))
        && !section_tsx_projected
        && let Some(node_id) = payload.target.node_id.as_deref()
        && let Some(warning) = hidden_master_warning(&payload.snapshot, node_id)
    {
        warnings.push(warning);
    }

    if outputs.iter().any(|output| output == "devupJson") {
        let variables = payload.variables.as_ref().ok_or_else(|| unavailable_output(payload, "devupJson",
        "No variable/style resources were collected. Re-collect from the URL with devupJson in outputs and the desired scope."))?;
        let variables = variable_snapshot_from_result(variables)?;
        let mut output =
            theme_export::generate(&variables, parse_scope(scope)?, theme_options, &result)?;
        theme_export::prepare(&mut output, theme_options, &mut result, payload, strict)?;
        theme_conflict_count = output.conflicts.len();
        theme_unresolved_count = output.unresolved_variables.len() + output.skipped_styles.len();
        theme_unresolved_count += result
            .get("unresolvedUsedTokens")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if let Some(diff) = result.get("themeDiff") {
            let conflicts = diff["existingTypographyConflicts"].as_array();
            theme_conflict_count += conflicts.map_or(0, Vec::len);
            for conflict in conflicts.into_iter().flatten() {
                warnings.push(json!({"code":"DEVUP_THEME_EXISTING_TYPOGRAPHY_CONFLICT",
                "message":format!("Existing typography token '{}' differs from Figma; the project's value is preserved and visual fidelity needs review.", conflict["token"].as_str().unwrap_or_default()),
                "details":conflict,"severity":"warning"}));
            }
        }
        warnings.extend(
            output
                .diagnostics
                .iter()
                .filter(|d| d.code == "DEVUP_THEME_STYLE_SKIPPED")
                .map(|d| json!(d)),
        );
        projection_diagnostics.extend(
            output
                .diagnostics
                .iter()
                .filter(|d| d.code == "DEVUP_THEME_STYLE_SKIPPED")
                .cloned(),
        );
        devup_json_source_map = Some(output.source_map.clone());
        if output_paths.contains_key("devupJson") {
            pending_text_outputs.insert("devupJson".to_owned(), output.json.clone());
        }
        result.insert("devupJson".to_owned(), json!(output.json));
        result.insert("themeCounts".to_owned(), json!(output.counts));
        result.insert("themeCompleteness".to_owned(), json!(output.completeness));
        result.insert("conflicts".to_owned(), json!(output.conflicts));
        result.insert(
            "unresolvedVariables".to_owned(),
            json!(output.unresolved_variables),
        );
        if include_diagnostics && !result.contains_key("diagnostics") {
            result.insert(
                "diagnostics".to_owned(),
                json!(
                    output
                        .diagnostics
                        .iter()
                        .filter(|d| d.code != "DEVUP_CODEGEN_PROPERTY_EVIDENCE")
                        .collect::<Vec<_>>()
                ),
            );
        }
    }
    Ok((
        result,
        projection_diagnostics,
        fidelity_reports,
        pending_text_outputs,
        tsx_source_map,
        generated_output_target,
        devup_json_source_map,
        theme_conflict_count,
        theme_unresolved_count,
        warnings,
    ))
}

fn component_set_variants(snapshot: &devup_mcp_figma::Snapshot, node_id: &str) -> Option<Value> {
    let set = snapshot.nodes.get(node_id)?;
    let view = set.typed_view();
    if view.node_type() != "COMPONENT_SET" {
        return None;
    }
    let variants: Vec<Value> = view
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .map(|node| {
            json!({
                "nodeId": node.id,
                "name": node.typed_view().name(),
                "variantProperties": node.typed_view().value("variantProperties"),
                "visible": !hidden(set) && !hidden(node),
            })
        })
        .collect();
    let all_hidden = !variants.is_empty() && variants.iter().all(|v| v["visible"] == false);
    Some(json!({
        "nodeId": node_id,
        "variantCount": variants.len(),
        "defaultVariant": view.value("defaultVariant").and_then(|v| v.get("name")),
        "variants": variants,
        "allVariantsHidden": all_hidden,
        "note": "This node is a component set. Export a single variant by nodeId to get that variant's own tsx.",
    }))
}

fn hidden(node: &devup_mcp_figma::RawNode) -> bool {
    node.typed_view().bool("visible") == Some(false)
}

/// A library master can be hidden on its own page while every placed instance
/// is visible. Its generated code is then `display="none"` at every variant
/// and breakpoint, which looks like a finished component but renders nothing
/// and carries none of the page's text/visibility overrides.
fn hidden_master_warning(snapshot: &devup_mcp_figma::Snapshot, node_id: &str) -> Option<Value> {
    let node = snapshot.nodes.get(node_id)?;
    let view = node.typed_view();
    let (hidden_ids, total) = match view.node_type() {
        "COMPONENT_SET" => {
            let variants = view
                .child_ids()
                .filter_map(|id| snapshot.nodes.get(id))
                .filter(|child| child.typed_view().node_type() == "COMPONENT")
                .collect::<Vec<_>>();
            let hidden_ids = variants
                .iter()
                .filter(|variant| hidden(node) || hidden(variant))
                .map(|variant| variant.id.clone())
                .collect::<Vec<_>>();
            (hidden_ids, variants.len())
        }
        "COMPONENT" => {
            let set_hidden = view
                .string("parentId")
                .and_then(|id| snapshot.nodes.get(id))
                .is_some_and(|parent| {
                    parent.typed_view().node_type() == "COMPONENT_SET" && hidden(parent)
                });
            let ids = if hidden(node) || set_hidden {
                vec![node.id.clone()]
            } else {
                Vec::new()
            };
            (ids, 1)
        }
        _ => return None,
    };
    if total == 0 || hidden_ids.len() != total {
        return None;
    }
    let instances = snapshot
        .nodes
        .values()
        .filter(|other| {
            let other = other.typed_view();
            other.node_type() == "INSTANCE"
                && (other
                    .string("mainComponentId")
                    .is_some_and(|id| id == node_id || hidden_ids.iter().any(|h| h == id))
                    || other.string("mainComponentSetId") == Some(node_id))
        })
        .map(|instance| instance.id.clone())
        .collect::<Vec<_>>();
    Some(json!({
        "code": "DEVUP_HIDDEN_MASTER_EXPORT",
        "severity": "warning",
        "nodeId": node_id,
        "message": "Every variant of this exported master is hidden in Figma, so its generated code renders display=\"none\" at every variant and breakpoint. It is not usable screen code.",
        "details": {
            "nodeType": view.node_type(),
            "hiddenVariantIds": hidden_ids,
            "variantCount": total,
            "visibleInstanceIdsInSnapshot": instances,
            "nextAction": "Export the visible page INSTANCE that uses this component (its own node id, which carries its text/visibility/variant overrides) instead of the hidden master. Do not delete display=\"none\" by hand to make the master appear."
        }
    }))
}

#[cfg(test)]
mod r32_tests {
    use super::*;

    #[test]
    fn component_set_export_lists_its_variants() {
        let snapshot: devup_mcp_figma::Snapshot = serde_json::from_value(json!({
            "fileKey": "k", "version": null, "roots": ["1:0"], "diagnostics": [],
            "nodes": {
                "1:0": {"id":"1:0","type":"COMPONENT_SET","fields":{"name":"Button","childrenIds":["1:1","1:2"],"defaultVariant":{"name":"State=Default"}}},
                "1:1": {"id":"1:1","type":"COMPONENT","fields":{"name":"State=Default","variantProperties":{"State":"Default"}}},
                "1:2": {"id":"1:2","type":"COMPONENT","fields":{"name":"State=Hover","variantProperties":{"State":"Hover"}}}
            }
        }))
        .unwrap();
        let summary = component_set_variants(&snapshot, "1:0").unwrap();
        assert_eq!(summary["variantCount"], 2);
        assert_eq!(summary["defaultVariant"], "State=Default");
        assert_eq!(summary["variants"][1]["nodeId"], "1:2");
        assert!(component_set_variants(&snapshot, "1:1").is_none());
        assert_eq!(summary["allVariantsHidden"], false);
        assert_eq!(summary["variants"][0]["visible"], true);
        assert!(hidden_master_warning(&snapshot, "1:0").is_none());
        assert!(hidden_master_warning(&snapshot, "1:1").is_none());
    }

    fn master(set_visible: bool, variants_visible: [bool; 2]) -> devup_mcp_figma::Snapshot {
        serde_json::from_value(json!({
            "fileKey": "k", "version": null, "roots": ["1:0"], "diagnostics": [],
            "nodes": {
                "1:0": {"id":"1:0","type":"COMPONENT_SET","fields":{"name":"Card","visible":set_visible,"childrenIds":["1:1","1:2"]}},
                "1:1": {"id":"1:1","type":"COMPONENT","fields":{"name":"Breakpoint=Desktop","parentId":"1:0","visible":variants_visible[0],"variantProperties":{"Breakpoint":"Desktop"}}},
                "1:2": {"id":"1:2","type":"COMPONENT","fields":{"name":"Breakpoint=Mobile","parentId":"1:0","visible":variants_visible[1],"variantProperties":{"Breakpoint":"Mobile"}}},
                "9:1": {"id":"9:1","type":"INSTANCE","fields":{"name":"Card","mainComponentId":"1:2"}}
            }
        }))
        .unwrap()
    }

    /// Feedback 008: a master hidden at every variant/breakpoint is flagged and
    /// the agent is sent to the visible page instance.
    #[test]
    fn all_hidden_master_variants_warn_and_point_to_the_visible_instance() {
        for snapshot in [master(true, [false, false]), master(false, [true, true])] {
            let summary = component_set_variants(&snapshot, "1:0").unwrap();
            assert_eq!(summary["allVariantsHidden"], true);
            assert_eq!(summary["variants"][1]["visible"], false);
            let warning = hidden_master_warning(&snapshot, "1:0").unwrap();
            assert_eq!(warning["code"], "DEVUP_HIDDEN_MASTER_EXPORT");
            assert_eq!(warning["details"]["hiddenVariantIds"], json!(["1:1", "1:2"]));
            assert_eq!(warning["details"]["visibleInstanceIdsInSnapshot"], json!(["9:1"]));
            assert!(warning["details"]["nextAction"].as_str().unwrap().contains("INSTANCE"));
            // A single variant export of a hidden master is flagged as well.
            assert!(hidden_master_warning(&snapshot, "1:2").is_some());
        }
        let partly = master(true, [false, true]);
        assert_eq!(component_set_variants(&partly, "1:0").unwrap()["allVariantsHidden"], false);
        assert!(hidden_master_warning(&partly, "1:0").is_none());
        assert!(hidden_master_warning(&partly, "1:2").is_none());
        assert!(hidden_master_warning(&partly, "1:1").is_some());
    }
}
