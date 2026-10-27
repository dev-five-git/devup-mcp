

fn component_variants(
    snapshot: &Snapshot,
    root: &RawNode,
    default_id: &str,
) -> Vec<(String, String)> {
    let definitions = root
        .typed_view()
        .value("componentPropertyDefinitions")
        .and_then(serde_json::Value::as_object);
    let mut variants = Vec::new();
    let mut added = BTreeSet::new();
    let add = |raw_key: &str,
               definition: &serde_json::Value,
               variants: &mut Vec<(String, String)>,
               added: &mut BTreeSet<String>| {
        let key = component_property_name(raw_key);
        if added.contains(&key) {
            return;
        }
        let value = match definition.get("type").and_then(serde_json::Value::as_str) {
            Some("VARIANT") => definition
                .get("variantOptions")
                .and_then(serde_json::Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(|option| format!("'{option}'"))
                        .collect::<Vec<_>>()
                        .join(" | ")
                }),
            Some("BOOLEAN") => Some("boolean".to_owned()),
            Some("INSTANCE_SWAP") => Some("React.ReactNode".to_owned()),
            _ => None,
        };
        if let Some(value) = value {
            added.insert(key.clone());
            variants.push((key, value));
        }
    };
    if let Some(definitions) = definitions {
        for (key, definition) in definitions {
            if definition.get("type").and_then(serde_json::Value::as_str) == Some("VARIANT") {
                add(key, definition, &mut variants, &mut added);
            }
        }
        fn visit_refs(
            snapshot: &Snapshot,
            id: &str,
            definitions: &serde_json::Map<String, serde_json::Value>,
            variants: &mut Vec<(String, String)>,
            added: &mut BTreeSet<String>,
            add: &impl Fn(&str, &serde_json::Value, &mut Vec<(String, String)>, &mut BTreeSet<String>),
        ) {
            let Some(node) = snapshot.nodes.get(id) else {
                return;
            };
            let view = node.typed_view();
            if let Some(references) = view
                .value("componentPropertyReferences")
                .and_then(serde_json::Value::as_object)
            {
                for field in ["mainComponent", "visible"] {
                    if let Some(key) = references.get(field).and_then(serde_json::Value::as_str)
                        && let Some(definition) = definitions.get(key)
                    {
                        add(key, definition, variants, added);
                    }
                }
            }
            for child in view.child_ids() {
                visit_refs(snapshot, child, definitions, variants, added, add);
            }
        }
        visit_refs(
            snapshot,
            default_id,
            definitions,
            &mut variants,
            &mut added,
            &add,
        );
        for (key, definition) in definitions {
            add(key, definition, &mut variants, &mut added);
        }
    }
    variants
}

fn component_property_name(raw: &str) -> String {
    raw.split('#').next().unwrap_or(raw).to_owned()
}

pub(super) fn legacy_component_name(input: &str) -> String {
    if !input.is_empty()
        && input
            .chars()
            .all(|character| !character.is_alphabetic() || character.is_uppercase())
    {
        let mut characters = input.chars();
        if let Some(first) = characters.next() {
            let candidate = first
                .to_uppercase()
                .chain(characters.flat_map(char::to_lowercase))
                .collect::<String>();
            return sanitize_legacy_identifier(&candidate);
        }
    }
    let mut output = String::new();
    for part in input
        .split(['-', '_', '/', ' '])
        .filter(|part| !part.is_empty())
    {
        let acronym = part
            .chars()
            .filter(|character| character.is_alphabetic())
            .all(|character| character.is_uppercase());
        let normalized = if acronym {
            part.to_lowercase()
        } else {
            part.to_owned()
        };
        let mut characters = normalized.chars();
        if let Some(first) = characters.next() {
            output.extend(first.to_uppercase());
            output.extend(characters);
        }
    }
    sanitize_legacy_identifier(&output)
}

fn sanitize_legacy_identifier(candidate: &str) -> String {
    let mut output = candidate
        .chars()
        .filter(|character| character.is_alphanumeric() || *character == '_')
        .collect::<String>();
    if output.is_empty() {
        return "FigmaComponent".to_owned();
    }
    if output
        .chars()
        .next()
        .is_some_and(|character| character.is_numeric())
    {
        output.insert(0, '_');
    }
    output
}

pub fn generate_node(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    finalize_codegen_output(
        generate_node_marked(snapshot, root_id, options)?,
        snapshot,
        options,
        root_id,
    )
}

fn generate_node_marked(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Node to convert was not found in the Figma snapshot.",
            false,
        )
    })?;
    let render_root = if root.typed_view().node_type() == "SECTION" {
        root.typed_view()
            .child_ids()
            .next()
            .and_then(|id| snapshot.nodes.get(id))
            .unwrap_or(root)
    } else {
        root
    };
    let mut context = Context {
        asset_policies: options.asset_policies.clone(),
        asset_names_per_node: options.asset_names_per_node,
        inline_instances: options.inline_instances,
        text_style_tokens: options.text_style_tokens.clone(),
        typography_values: options.typography_values.clone(),
        variable_tokens: options.variable_tokens.clone(),
        length_tokens: options.length_tokens.clone(),
        root_layout: options.root_layout,
        width_policy: options.width_policy.clone(),
        ..Context::default()
    };
    let jsx = render_node(snapshot, render_root, 0, &mut context, &mut HashSet::new())?;
    let imports = context.imports.iter().cloned().collect::<Vec<_>>();
    Ok(CodegenOutput {
        tsx: jsx,
        imports,
        used_tokens: context.used_tokens,
        diagnostics: context.diagnostics,
        source_map: SourceMap::empty(),
        projection_trace: ProjectionTrace::default(),
        fidelity_report: FidelityReport::default(),
        width_policy: options.width_policy.clone(),
    })
}

fn finalize_codegen_output(
    mut output: CodegenOutput,
    snapshot: &Snapshot,
    options: &CodegenOptions,
    root_id: &str,
) -> Result<CodegenOutput, DevupError> {
    let (tsx, source_map) = finalize_tsx(
        &output.tsx,
        snapshot,
        &options.variable_tokens,
        &options.text_style_tokens,
    );
    output.tsx = tsx;
    output.source_map = source_map;
    for record in super::instance_overrides::instance_overrides(snapshot, root_id, options) {
        output.tsx.push_str(&format!("\n// @devup-override {}\n", record.to_string()
            .replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029")));
        output.diagnostics.push(super::instance_overrides::diagnostic(record));
    }
    super::image::attach_evidence(snapshot, options, &mut output);
    crate::provenance::account_for_sizing(snapshot, &mut output);
    validate_tsx(&output.tsx)?;
    if let Some(contract) = super::evidence::placement_contract(snapshot, &output, options, root_id)
    {
        output.diagnostics.push(contract);
    }
    // Background and variant projections share paint suppression in style;
    // attach evidence for those paths as well as ordinary asset leaves.
    for entry in output
        .source_map
        .entries
        .iter()
        .filter(|e| e.property.is_none() && e.resolution == "node")
    {
        let Some(node) = entry.node_id.as_ref().and_then(|id| snapshot.nodes.get(id)) else {
            continue;
        };
        let Some(reason) = style::non_rendering_asset_reason(snapshot, node) else {
            continue;
        };
        if output.diagnostics.iter().any(|d| {
            d.code == "DEVUP_CODEGEN_NON_RENDERING_ASSET" && d.node_id.as_deref() == Some(&node.id)
        }) {
            continue;
        }
        let Some(source) = entry
            .generated_range
            .as_ref()
            .and_then(|r| output.tsx.get(r.start..r.end))
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        output.diagnostics.push(non_rendering_diagnostic(
            node,
            reason,
            source,
            "layout-only",
        ));
    }
    output
        .diagnostics
        .extend(unresolved_token_bindings(snapshot, root_id, options));
    output.projection_trace =
        build_projection_trace(snapshot, root_id, &output.tsx, &output.source_map);
    for diagnostic in &output.diagnostics {
        if diagnostic.code == "DEVUP_CODEGEN_VARIANT_CHILD_FALLBACK"
            && let Some(node_id) = &diagnostic.node_id
            && !output.projection_trace.entries.iter().any(|entry| &entry.node_id == node_id)
        {
            output.projection_trace.entries.push(crate::provenance::ProjectionTraceEntry {
                node_id: node_id.clone(),
                disposition: crate::provenance::ProjectionDisposition::Ignored,
                reason: "unrepresented-variant-child".into(),
                generated_range: None,
            });
        }
    }
    output.fidelity_report = validate_fidelity(snapshot, root_id, &output)?;
    output
        .diagnostics
        .retain(|d| d.code != "DEVUP_CODEGEN_LAYOUT_UNCOVERED");
    // A coverage shortfall has no proven intentional-exclusion classification.
    // Report the actual field instead of allowing an exact quality grade.
    for pair in &output.fidelity_report.uncovered_layout {
        if let Some((node_id, property)) = pair.rsplit_once('#') {
            output.diagnostics.push(Diagnostic {
                code: "DEVUP_CODEGEN_LAYOUT_UNCOVERED".into(),
                message: "No verified property mapping accounts for this layout field; generated evidence describes what was emitted.".into(),
                node_id: Some(node_id.into()),
                property: Some(property.into()),
                fidelity_impact: Some(devup_mcp_figma::FidelityImpact::Lossy),
                details: Some(super::evidence::uncovered_layout_details(snapshot, &output, options, node_id, property)),
                ..Diagnostic::default()
            });
        }
    }
    let absolute_components = output
        .source_map
        .entries
        .iter()
        .filter(|e| e.property.is_none())
        .filter_map(|e| e.node_id.as_deref())
        .filter(|id| {
            snapshot.nodes.get(*id).is_some_and(|n| {
                n.typed_view().string("layoutPositioning") == Some("ABSOLUTE")
                    && n.typed_view().bool("visible") != Some(false)
            })
        })
        .map(|id| {
            (
                id.to_owned(),
                crate::provenance::absolute_component_verification(snapshot, &output, id),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    // Every emitted ABSOLUTE node receives the same generated-property audit.
    for id in absolute_components.keys() {
        if !output.diagnostics.iter().any(|d| {
            d.code == "DEVUP_CODEGEN_ABSOLUTE_FALLBACK" && d.node_id.as_deref() == Some(id)
        }) {
            output.diagnostics.push(Diagnostic {
                code: "DEVUP_CODEGEN_ABSOLUTE_FALLBACK".into(),
                node_id: Some(id.clone()),
                fidelity_impact: Some(devup_mcp_figma::FidelityImpact::Approximated),
                ..Diagnostic::default()
            });
        }
    }
    for diagnostic in &mut output.diagnostics {
        let property = match diagnostic.code.as_str() {
            "DEVUP_CODEGEN_ABSOLUTE_FALLBACK" => Some("layoutPositioning"),
            "DEVUP_CODEGEN_MASK_FALLBACK" => Some("isMask"),
            "DEVUP_CODEGEN_EFFECT_FALLBACK" => Some("effects"),
            "DEVUP_CODEGEN_IMAGE_COMPOSITION_UNSUPPORTED" => Some("fills"),
            "DEVUP_CODEGEN_ANIMATION_UNREACHABLE" => Some("reactions"),
            "DEVUP_CODEGEN_VARIANT_CHILD_FALLBACK" => Some("childrenIds"),
            "DEVUP_CODEGEN_TOKEN_NAME_UNRESOLVED" => diagnostic.property.as_deref(),
            _ => None,
        };
        if let Some(property) = property.map(str::to_owned) {
            diagnostic.property = Some(property.clone());
            let node_id = diagnostic.node_id.as_deref().unwrap_or(root_id);
            let generated = output
                .source_map
                .entries
                .iter()
                .filter(|entry| {
                    entry.node_id.as_deref() == Some(node_id) && entry.property.is_none()
                })
                .filter_map(|entry| entry.generated_range.as_ref())
                .filter_map(|range| output.tsx.get(range.start..range.end))
                .collect::<Vec<_>>();
            diagnostic.details = Some(serde_json::json!({
                "originalValue": super::evidence::fallback_original(snapshot, node_id, &property),
                "originalResourceId": diagnostic.resource_id,
                "appliedValue": {"generatedSource": generated, "fallback": diagnostic.fallback,
                    "heightPreservation": if property == "layoutPositioning" {
                        snapshot.nodes.get(node_id).map(|node| {
                            let view = node.typed_view();
                            let expected = view.number("height").map(|h|format!("{}px",layout::format_number(h)));
                            let preserved = expected.as_ref().is_some_and(|height|generated.iter().any(|source| {
                                let tag = source.split('>').next().unwrap_or(source);
                                tag.contains(&format!("h=\"{height}\"")) || tag.contains(&format!("boxSize=\"{height}\""))
                            }));
                            serde_json::json!({"sourceSizing":view.string("layoutSizingVertical"),"sourceHeight":view.number("height"),
                                "state":if view.string("layoutSizingVertical") != Some("FIXED") {"not-fixed"} else if preserved {"preserved"} else {"unverified"},
                                "basis":"Generated explicit height; derived padding alone is not a fixed-height guarantee."})
                        })
                    } else {None},
                    "derivedPadding": if property == "layoutPositioning" {
                        snapshot.nodes.get(node_id).and_then(|node|layout::derived_padding(snapshot,node))
                            .map(|[top,right,bottom,left]|serde_json::json!({"top":top,"right":right,"bottom":bottom,"left":left,
                                "basis":"Generator-derived inset from visible child geometry; not the source padding fields."}))
                    } else { None }},
                "evidenceLimit": "Generated source and collected geometry are comparison evidence, not measured browser layout or proof of responsive equivalence.",
                "stage": "projection"
            }));
            if property == "layoutPositioning"
                && let Some(components) = absolute_components.get(node_id)
            {
                let unresolved = ["height","width","horizontal","vertical"].into_iter()
                    .filter(|name|components[*name]["fidelityImpact"] != "none")
                    .map(|name|serde_json::json!({"component":name,"condition":components[name]["resolutionCondition"]}))
                    .collect::<Vec<_>>();
                let verified = unresolved.is_empty();
                diagnostic.message = if verified {"Absolute dimensions and constraint props verified against collected geometry."} else {"Absolute conversion is partially verified; components identify preserved values and remaining approximations."}.into();
                if verified {
                    diagnostic.code = "DEVUP_CODEGEN_ABSOLUTE_VERIFIED".into();
                    diagnostic.fidelity_impact = Some(devup_mcp_figma::FidelityImpact::None);
                }
                let details = diagnostic.details.as_mut().unwrap();
                details["components"] = components.clone();
                details["appliedValue"]["widthPreservation"] = components["width"].clone();
                details["componentSummary"] = serde_json::json!({
                    "verified": (["height", "width", "horizontal", "vertical"].into_iter()
                        .filter(|name| components[*name]["fidelityImpact"] == "none").collect::<Vec<_>>()),
                    "unresolved": (["height", "width", "horizontal", "vertical"].into_iter()
                        .filter(|name| components[*name]["fidelityImpact"] != "none").collect::<Vec<_>>())
                });
                details["resolutionConditions"] = serde_json::json!(unresolved);
                details["classification"] = serde_json::json!(if verified {
                    "verified-absolute-components"
                } else {
                    "partially-verified-absolute-components"
                });
            }
        }
    }
    output.source_map.describe_properties(&output.tsx);
    output.source_map.describe_conversions(snapshot);
    crate::provenance::attributes::audit_properties(snapshot, &mut output, options, root_id);
    output
        .diagnostics
        .extend(super::evidence::image_layout_diagnostics(snapshot, &output));
    output.fidelity_report = validate_fidelity(snapshot, root_id, &output)?;
    Ok(output)
}

/// Reports every binding the generated code had to write out as a value.
///
/// A fill bound to a variable, or a text carrying a style, is the design
/// saying "this is a token". The generator writes the token when the resource
/// catalog carried that variable or style, and the resolved value when it did
/// not - and it has to write something, because the module still has to
/// compile and render. What it must not do is stay quiet about it: a
/// hardcoded `#7d7f83` where the design says `$caption` renders identically
/// today and stops following the theme tomorrow, and a caller reading a
/// response graded `exact` has no way to know one is in there.
///
/// Run as a pass over the collected subtree rather than inside rendering, so
/// it sees the node a binding belongs to and needs no argument threaded
/// through the render functions to reach it.
fn unresolved_token_bindings(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Vec<Diagnostic> {
    fn paint_variable_ids(fills: Option<&serde_json::Value>) -> Vec<String> {
        fills
            .and_then(serde_json::Value::as_array)
            .map(|paints| {
                paints
                    .iter()
                    .filter_map(|paint| {
                        Some(
                            paint
                                .get("boundVariables")?
                                .get("color")?
                                .get("id")?
                                .as_str()?
                                .to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    let mut reported = BTreeSet::new();
    let mut diagnostics = Vec::new();
    let mut pending = vec![root_id.to_owned()];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(&id) else {
            continue;
        };
        let view = node.typed_view();
        if view.bool("visible") == Some(false) && !node.field_errors.contains_key("visible") {
            // No paint or typography token is required by an omitted subtree.
            continue;
        }
        pending.extend(view.child_ids().map(str::to_owned));

        let mut lost = Vec::new();
        for variable_id in paint_variable_ids(view.value("fills")) {
            if !options.variable_tokens.contains_key(&variable_id) {
                lost.push(("variable", "fills", variable_id));
            }
        }
        if let Some(segments) = view
            .value("styledTextSegments")
            .and_then(serde_json::Value::as_array)
        {
            for segment in segments {
                for variable_id in paint_variable_ids(segment.get("fills")) {
                    if !options.variable_tokens.contains_key(&variable_id) {
                        lost.push(("variable", "fills", variable_id));
                    }
                }
                if let Some(style_id) = segment
                    .get("textStyleId")
                    .and_then(serde_json::Value::as_str)
                    && !style_id.is_empty()
                    && !options.text_style_tokens.contains_key(style_id)
                {
                    lost.push(("textStyle", "textStyleId", style_id.to_owned()));
                }
            }
        }
        if let Some(style_id) = view.string("textStyleId")
            && !style_id.is_empty()
            && !options.text_style_tokens.contains_key(style_id)
        {
            lost.push(("textStyle", "textStyleId", style_id.to_owned()));
        }

        for (kind, property, resource_id) in lost {
            if !reported.insert((id.clone(), property, resource_id.clone())) {
                continue;
            }
            diagnostics.push(Diagnostic {
                code: "DEVUP_CODEGEN_TOKEN_NAME_UNRESOLVED".to_owned(),
                message: format!(
                    include_str!(
                        "../../text/component-unresolved_token_bindings_Diagnostic-08be2867.md"
                    ),
                    id = id,
                    kind = kind,
                    property = property
                ),
                severity: Some(DiagnosticSeverity::Warning),
                node_id: Some(id.clone()),
                property: Some(property.to_owned()),
                resource_id: Some(resource_id.clone()),
                resource_kind: Some(kind.to_owned()),
                details: Some(serde_json::json!({
                    "nodeId": id,
                    "property": property,
                    "resourceId": resource_id
                })),
                ..Diagnostic::default()
            });
        }
    }
    diagnostics.sort_by(|left, right| left.message.cmp(&right.message));
    diagnostics
}

#[derive(Default)]
struct Context {
    asset_policies: BTreeMap<String, super::AssetRepresentationPolicy>,
    asset_names_per_node: bool,
    imports: BTreeSet<String>,
    used_tokens: BTreeSet<String>,
    diagnostics: Vec<Diagnostic>,
    inline_instances: bool,
    instance_boolean_properties: Vec<BTreeMap<String, bool>>,
    text_style_tokens: std::collections::BTreeMap<String, String>,
    typography_values: BTreeMap<String, serde_json::Value>,
    variable_tokens: std::collections::BTreeMap<String, String>,
    length_tokens: BTreeMap<String, LengthToken>,
    root_layout: RootLayout,
    width_policy: Option<super::width::WidthPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PropValue {
    String(String),
}

pub(super) type Prop = (String, PropValue);
