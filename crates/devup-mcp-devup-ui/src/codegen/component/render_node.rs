

fn render_node(
    snapshot: &Snapshot,
    node: &RawNode,
    depth: usize,
    context: &mut Context,
    visiting: &mut HashSet<String>,
) -> Result<String, DevupError> {
    if !visiting.insert(node.id.clone()) {
        return Err(DevupError::new(
            ErrorCode::DevupCodegenFailed,
            "Figma node tree contains a circular reference.",
            false,
        ));
    }
    let view = node.typed_view();
    // Hidden snapshot subtrees have no deliverable assets. Do not emit their
    // image/background/mask references, even under display:none.
    if view.bool("visible") == Some(false) {
        context.diagnostics.push(non_rendering_diagnostic(
            node,
            "visible-false",
            "",
            "omitted",
        ));
        visiting.remove(&node.id);
        return Ok(mark_node(
            &node.id,
            if depth == 0 {
                "<></>".into()
            } else {
                String::new()
            },
        ));
    }
    add_fallback_diagnostics(snapshot, node, context);
    let concrete_visibility = if context.inline_instances {
        view.value("componentPropertyReferences")
            .and_then(serde_json::Value::as_object)
            .and_then(|references| references.get("visible"))
            .and_then(serde_json::Value::as_str)
            .and_then(|property| {
                context
                    .instance_boolean_properties
                    .iter()
                    .rev()
                    .find_map(|properties| properties.get(property).copied())
            })
    } else {
        None
    };
    if concrete_visibility == Some(false) {
        visiting.remove(&node.id);
        return Ok(mark_node(&node.id, String::new()));
    }
    if view.node_type() == "INSTANCE"
        && !context.inline_instances
        && style::non_rendering_asset_reason(snapshot, node).is_none()
    {
        let references = view
            .value("componentPropertyReferences")
            .and_then(serde_json::Value::as_object);
        let main_property = references
            .and_then(|references| references.get("mainComponent"))
            .and_then(serde_json::Value::as_str)
            .map(component_property_name);
        let visible_property = references
            .and_then(|references| references.get("visible"))
            .and_then(serde_json::Value::as_str)
            .map(component_property_name);
        let component = snapshot
            .nodes
            .values()
            .filter(|candidate| candidate.typed_view().node_type() == "COMPONENT")
            .filter_map(|candidate| candidate.typed_view().name())
            .filter(|name| {
                view.name()
                    .is_some_and(|instance| instance.starts_with(name))
            })
            .max_by_key(|name| name.len())
            .map(legacy_component_name)
            .unwrap_or_else(|| legacy_component_name(view.name().unwrap_or("Component")));
        let content = main_property.unwrap_or_else(|| format!("<{component} />"));
        let expression = if let Some(visible) = visible_property {
            format!("{{{visible} && {content}}}")
        } else if references.is_some_and(|references| references.contains_key("mainComponent")) {
            format!("{{{content}}}")
        } else {
            content
        };
        if view.string("layoutPositioning") == Some("ABSOLUTE") {
            let mut props = Vec::new();
            layout::push_layout_props(
                snapshot,
                node,
                "Box",
                &mut props,
                context.root_layout,
                depth == 0,
                context.width_policy.as_ref(),
                super::image::original_image(node, &context.asset_policies),
            );
            props.retain(|(name, value)| {
                matches!(
                    name.as_str(),
                    "pos" | "left" | "right" | "top" | "bottom" | "transform" | "transformOrigin"
                ) || (name == "w" && matches!(value, PropValue::String(value) if value == "100%"))
            });
            context.imports.insert("Box".to_owned());
            let indent = "  ".repeat(depth);
            let (opening_props, multiline_props) = render_props(&props, depth);
            let close_open = if multiline_props {
                format!("{opening_props}\n{indent}>")
            } else {
                format!("{opening_props}>")
            };
            visiting.remove(&node.id);
            return Ok(mark_node(
                &node.id,
                format!(
                    "{indent}<Box{close_open}\n{}{}\n{indent}</Box>",
                    "  ".repeat(depth + 1),
                    expression
                ),
            ));
        }
        visiting.remove(&node.id);
        return Ok(mark_node(
            &node.id,
            format!("{}{expression}", "  ".repeat(depth)),
        ));
    }
    let asset = super::image::asset_kind(snapshot, node, &context.asset_policies);
    if asset.is_some()
        && let Some(reason) = style::non_rendering_asset_reason(snapshot, node)
    {
        // A transparent/clipped asset still occupies layout space. Keep the
        // asset sizing policy, but emit no external paint or descendant asset.
        let mut props = Vec::new();
        layout::push_layout_props(
            snapshot,
            node,
            "Image",
            &mut props,
            context.root_layout,
            depth == 0,
            context.width_policy.as_ref(),
            super::image::original_image(node, &context.asset_policies),
        );
        layout::string_prop(&mut props, "visibility", "hidden");
        let (opening, multiline) = render_props(&props, depth);
        let indent = "  ".repeat(depth);
        let rendered = if multiline {
            format!("{indent}<Box{opening}\n{indent}/>")
        } else {
            format!("{indent}<Box{opening} />")
        };
        context.imports.insert("Box".into());
        context.diagnostics.push(non_rendering_diagnostic(
            node,
            reason,
            &rendered,
            "layout-only",
        ));
        visiting.remove(&node.id);
        return Ok(mark_node(&node.id, rendered));
    }
    let inferred_mode = view
        .value("inferredAutoLayout")
        .and_then(serde_json::Value::as_object)
        .and_then(|layout| layout.get("layoutMode"))
        .and_then(serde_json::Value::as_str);
    let inferred_align = |name: &str| {
        view.string(name).or_else(|| {
            view.value("inferredAutoLayout")
                .and_then(serde_json::Value::as_object)
                .and_then(|layout| layout.get(name))
                .and_then(serde_json::Value::as_str)
        })
    };
    let component = if asset == Some(style::AssetKind::SvgMask) {
        "Box"
    } else if asset.is_some() {
        "Image"
    } else if view.node_type() == "TEXT" {
        "Text"
    } else if layout::centres_its_only_child(snapshot, node) {
        "Center"
    } else {
        match inferred_mode {
            Some("GRID") => "Grid",
            Some("HORIZONTAL" | "VERTICAL")
                if inferred_align("primaryAxisAlignItems") == Some("CENTER")
                    && inferred_align("counterAxisAlignItems") == Some("CENTER") =>
            {
                "Center"
            }
            Some("VERTICAL") => "VStack",
            Some("HORIZONTAL") => "Flex",
            _ => "Box",
        }
    };
    context.imports.insert(component.to_owned());

    let mut props = Vec::new();
    layout::push_layout_props(
        snapshot,
        node,
        component,
        &mut props,
        context.root_layout,
        depth == 0,
        context.width_policy.as_ref(),
        super::image::original_image(node, &context.asset_policies),
    );
    // A frame with no auto-layout places its children itself, and this keeps
    // them resolvable. Once the gap around them is measurable it is emitted as
    // padding instead, which puts them where they belong on its own — so the
    // anchor is only still needed where nothing could be measured, as when the
    // child fills the frame exactly or carries no position of its own.
    // Positioned children, including those of page and embedded roots, are
    // anchored by push_layout_props above. This legacy AUTO fallback only
    // applies to other frames.
    let page_root = snapshot
        .nodes
        .values()
        .find(|candidate| {
            candidate
                .typed_view()
                .child_ids()
                .any(|child| child == node.id)
        })
        .map(|parent| parent.typed_view().node_type())
        .or_else(|| view.string("parentType"))
        .is_some_and(|kind| matches!(kind, "SECTION" | "PAGE" | "COMPONENT_SET"));
    if !(depth == 0 && context.root_layout == RootLayout::Embedded)
        && !page_root
        && asset.is_none()
        && view.value("inferredAutoLayout").is_none()
        && view.string("layoutPositioning") == Some("AUTO")
        && layout::derived_padding(snapshot, node).is_none()
        && !layout::centres_its_only_child(snapshot, node)
        && view.child_ids().any(|child| {
            snapshot
                .nodes
                .get(child)
                .is_some_and(|child| child.typed_view().string("layoutPositioning") == Some("AUTO"))
        })
    {
        layout::string_prop(&mut props, "pos", "relative");
    }
    style::push_style_props(
        snapshot,
        node,
        component,
        asset,
        &mut props,
        &mut context.used_tokens,
        style::StyleOptions {
            reserve_image_content: !(depth == 0 && context.root_layout == RootLayout::Embedded),
            asset_policies: &context.asset_policies,
            variable_tokens: &context.variable_tokens,
            length_tokens: &context.length_tokens,
            asset_names_per_node: context.asset_names_per_node,
            width_policy: context.width_policy.as_ref(),
        },
    );
    text::validate_line_metrics(&view, &context.text_style_tokens)?;
    text::push_text_props(
        &view,
        &context.text_style_tokens,
        &context.typography_values,
        &context.variable_tokens,
        &mut context.used_tokens,
        &mut props,
    );
    style::tokenize_lengths(
        snapshot,
        node,
        &mut props,
        &context.length_tokens,
        &mut context.used_tokens,
    );
    // Last, as the plugin merges `getReactionProps` last: what a timed Smart
    // Animate changes becomes keyframes on the child it changes, or on the
    // frame itself.
    let before = props.len();
    animation::push_animation_props(snapshot, node, &context.variable_tokens, &mut props);
    if props.len() > before {
        context.imports.insert("keyframes".to_owned());
    }
    if asset.is_some() {
        props.retain(|(name, _)| {
            !matches!(
                name.as_str(),
                "alignItems"
                    | "justifyContent"
                    | "flexDir"
                    | "gap"
                    | "outline"
                    | "outlineOffset"
                    | "overflow"
                    | "p"
                    | "px"
                    | "py"
                    | "pt"
                    | "pr"
                    | "pb"
                    | "pl"
            )
        });
    }
    let indent = "  ".repeat(depth);
    let instance_properties =
        (context.inline_instances && view.node_type() == "INSTANCE").then(|| {
            view.value("componentProperties")
                .and_then(serde_json::Value::as_object)
                .map(|properties| {
                    properties
                        .iter()
                        .filter_map(|(key, property)| {
                            (property.get("type").and_then(serde_json::Value::as_str)
                                == Some("BOOLEAN"))
                            .then(|| {
                                property
                                    .get("value")
                                    .and_then(serde_json::Value::as_bool)
                                    .map(|value| (key.clone(), value))
                            })
                            .flatten()
                        })
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default()
        });
    if let Some(properties) = instance_properties {
        context.instance_boolean_properties.push(properties);
    }
    let children_result = if asset.is_some() {
        Ok(Vec::new())
    } else {
        for child in view
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .filter(|child| child.typed_view().bool("visible") == Some(false))
        {
            context.diagnostics.push(non_rendering_diagnostic(
                child,
                "visible-false",
                "",
                "omitted",
            ));
        }
        view.child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .filter(|child| child.typed_view().bool("visible") != Some(false))
            .map(|child| render_node(snapshot, child, depth + 1, context, visiting))
            .collect::<Result<Vec<_>, _>>()
    };
    if context.inline_instances && view.node_type() == "INSTANCE" {
        context.instance_boolean_properties.pop();
    }
    let mut children = children_result?;
    if asset.is_none()
        && component != "Text"
        && let Some(layers) = super::image::render_paint_layers(
            snapshot,
            node,
            style::StyleOptions {
                reserve_image_content: !(depth == 0 && context.root_layout == RootLayout::Embedded),
                asset_policies: &context.asset_policies,
                variable_tokens: &context.variable_tokens,
                length_tokens: &context.length_tokens,
                asset_names_per_node: context.asset_names_per_node,
                width_policy: context.width_policy.as_ref(),
            },
            &mut context.used_tokens,
            depth + 1,
        )
    {
        props.retain(|(name, _)| {
            !matches!(
                name.as_str(),
                "bg" | "bgBlendMode" | "objectFit" | "objectPos"
            )
        });
        if !props.iter().any(|(name, _)| name == "pos") {
            layout::string_prop(&mut props, "pos", "relative");
        }
        layout::string_prop(&mut props, "isolation", "isolate");
        context.imports.insert("Image".into());
        context.imports.insert("Box".into());
        children.splice(0..0, layers);
    }

    let (opening_props, multiline_props) = render_props(&props, depth);
    let image_index = super::image::selected_image_index(node, &context.asset_policies);
    let affine = if asset == Some(style::AssetKind::Png) && image_index.is_some() {
        let mapping = super::image::map_for(
            snapshot,
            node,
            image_index,
            &context.asset_policies,
            context.asset_names_per_node,
        );
        super::image::render_affine_image(
            &props,
            &mapping,
            depth,
            !(depth == 0 && context.root_layout == RootLayout::Embedded),
        )
    } else {
        None
    };
    if affine.is_some() {
        context.imports.insert("Box".into());
    }
    let rendered = if let Some(rendered) = affine {
        rendered
    } else if component == "Text" {
        let children = text::render_text_children(
            &view,
            &context.text_style_tokens,
            &context.typography_values,
            &context.variable_tokens,
            &mut context.used_tokens,
            depth + 1,
        );
        let close_open = if multiline_props {
            format!("{opening_props}\n{indent}>")
        } else {
            format!("{opening_props}>")
        };
        format!("{indent}<Text{close_open}\n{children}\n{indent}</Text>")
    } else if children.is_empty() {
        if multiline_props {
            format!("{indent}<{component}{opening_props}\n{indent}/>")
        } else {
            format!("{indent}<{component}{opening_props} />")
        }
    } else {
        let close_open = if multiline_props {
            format!("{opening_props}\n{indent}>")
        } else {
            format!("{opening_props}>")
        };
        format!(
            "{indent}<{component}{close_open}\n{}\n{indent}</{component}>",
            children.join("\n")
        )
    };
    let rendered = if concrete_visibility == Some(true)
        || (view.node_type() == "INSTANCE" && context.inline_instances)
    {
        rendered
    } else {
        view.value("componentPropertyReferences")
            .and_then(serde_json::Value::as_object)
            .and_then(|references| references.get("visible"))
            .and_then(serde_json::Value::as_str)
            .map(component_property_name)
            .map(|property| {
                let content = rendered.strip_prefix(&indent).unwrap_or(&rendered);
                format!("{indent}{{{property} && {content}}}")
            })
            .unwrap_or(rendered)
    };
    visiting.remove(&node.id);
    Ok(mark_node(&node.id, rendered))
}

fn non_rendering_diagnostic(node: &RawNode, reason: &str, source: &str, state: &str) -> Diagnostic {
    let view = node.typed_view();
    let field = if view.bool("visible") == Some(false) {
        "visible"
    } else {
        "absoluteRenderBounds"
    };
    let confirmed = !node.field_errors.contains_key(field)
        && if field == "visible" {
            view.bool(field) == Some(false)
        } else {
            view.value(field).is_some_and(serde_json::Value::is_null)
        };
    Diagnostic {
        code: "DEVUP_CODEGEN_NON_RENDERING_ASSET".into(),
        node_id: Some(node.id.clone()), property: Some("assetReference".into()),
        message: if confirmed { "Confirmed non-rendering node accounted for without an asset reference." } else { "Asset paint was omitted, but non-rendering could not be confirmed from collected fields." }.into(),
        fidelity_impact: Some(if confirmed { devup_mcp_figma::FidelityImpact::None } else { devup_mcp_figma::FidelityImpact::Approximated }),
        details: Some(serde_json::json!({
            "originalValue":{"visible":view.value("visible"),"opacity":view.value("opacity"),"absoluteBoundingBox":view.value("absoluteBoundingBox"),"absoluteRenderBounds":view.value("absoluteRenderBounds")},
            "appliedValue":{"state":state,"generatedSource":source.chars().take(800).collect::<String>(),"sourceTruncated":source.chars().count()>800},
            "classification":if confirmed {"accounted-for-non-rendering"} else {"unverified-non-rendering"},"reason":reason,
            "verification":{"state":if confirmed {"accounted-for"} else {"unverified"},"field":field,"fieldPresent":view.value(field).is_some(),"value":view.value(field),"readError":node.field_errors.get(field)},
            "nextAction":if confirmed {"No asset bytes are required for this confirmed non-rendering node. Correct visibility or clipping in Figma if it should render."} else {"Recapture visible and absoluteRenderBounds without field errors; an absent field is not evidence of non-rendering."}
        })),
        ..Diagnostic::default()
    }
}

/// A style name without a leading group that is only a number.
///
/// `0/` and `3/` in front of a style name are how a Figma library is made to
/// sort in the picker; they are not part of what the style is called, and the
/// reference does not carry them into the token. A group that names something
/// (`typography/`) is part of the name and stays.
fn named_tokens(result: Option<&UpstreamResult>, collection: &str) -> BTreeMap<String, String> {
    fn visit(value: &serde_json::Value, collection: &str, tokens: &mut BTreeMap<String, String>) {
        if let Some(values) = value.get(collection).and_then(serde_json::Value::as_array) {
            for value in values {
                if let (Some(id), Some(name)) = (
                    value.get("id").and_then(serde_json::Value::as_str),
                    value.get("name").and_then(serde_json::Value::as_str),
                ) {
                    let token = if collection == "variables" {
                        variable_token(
                            name,
                            value
                                .get("codeSyntax")
                                .and_then(serde_json::Value::as_object)
                                .and_then(|syntax| syntax.get("WEB"))
                                .and_then(serde_json::Value::as_str),
                        )
                    } else {
                        // The rule `devup.json` names the style by, so the
                        // `typography="…"` written here is a key that exists
                        // there: a leading breakpoint or number says where the
                        // style applies and is dropped, any other group is
                        // kept. See `theme::style_token`.
                        crate::theme::style_token(name).1
                    };
                    tokens.insert(id.to_owned(), token);
                }
            }
        }
        match value {
            serde_json::Value::Object(object) => {
                object
                    .values()
                    .for_each(|value| visit(value, collection, tokens));
            }
            serde_json::Value::Array(values) => {
                values
                    .iter()
                    .for_each(|value| visit(value, collection, tokens));
            }
            _ => {}
        }
    }
    let mut tokens = BTreeMap::new();
    if let Some(result) = result {
        visit(&result.raw, collection, &mut tokens);
    }
    tokens
}

pub(super) fn render_props(props: &[Prop], depth: usize) -> (String, bool) {
    if props.is_empty() {
        return (String::new(), false);
    }
    let mut sorted = props.to_vec();
    sorted.sort_by(|left, right| left.0.cmp(&right.0));
    let rendered = sorted
        .into_iter()
        .map(|(name, value)| match value {
            PropValue::String(value) => render_static_attribute(&name, &value),
        })
        .collect::<Vec<_>>();
    // Five props, or one that spans lines — a `keyframes({...})` — and the
    // props go one to a line, which is the plugin's `propsToString` rule.
    let multiline =
        rendered.len() >= 5 || rendered.iter().any(|attribute| attribute.contains('\n'));
    if multiline {
        let prefix = "  ".repeat(depth + 1);
        let padded = rendered
            .iter()
            .map(|attribute| attribute.replace('\n', &format!("\n{prefix}")))
            .collect::<Vec<_>>();
        (
            format!("\n{prefix}{}", padded.join(&format!("\n{prefix}"))),
            true,
        )
    } else {
        (format!(" {}", rendered.join(" ")), false)
    }
}

/// A prop as JSX. A value is a quoted string, except `animationName` holding
/// a `keyframes({...})` call, which is the expression itself — the plugin's
/// `propsToString` makes the same exception, and it is how devup-ui's
/// `keyframes` is meant to be written.
pub(super) fn render_static_attribute(name: &str, value: &str) -> String {
    if name == "animationName" && value.starts_with("keyframes(") {
        return format!("{name}={{{value}}}");
    }
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(character),
        }
    }
    format!("{name}=\"{escaped}\"")
}

fn add_fallback_diagnostics(snapshot: &Snapshot, node: &RawNode, context: &mut Context) {
    use devup_mcp_figma::FidelityImpact;

    let view = node.typed_view();
    if let Some(diagnostic) = text::whitespace_collapse_diagnostic(&view) {
        context.diagnostics.push(diagnostic);
    }
    let candidates = [
        (
            super::image::original_image(node, &context.asset_policies)
                && matches!(
                    view.node_type(),
                    "VECTOR" | "STAR" | "POLYGON" | "BOOLEAN_OPERATION"
                ),
            "DEVUP_CODEGEN_IMAGE_SHAPE_UNSUPPORTED",
            "The original image source does not encode this vector's shape. Use the vector rendition or an explicit CSS mask; source-image rectangle equivalence is unverified.",
            FidelityImpact::Lossy,
        ),
        (
            view.value("fills")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|fills| {
                    fills.iter().any(|p| {
                        p["visible"] != false
                            && p["type"]
                                .as_str()
                                .is_some_and(|t| t.starts_with("GRADIENT_"))
                            && p["gradientStops"].as_array().is_none_or(|s| s.len() < 2)
                    })
                }),
            "DEVUP_CODEGEN_IMAGE_COMPOSITION_UNSUPPORTED",
            include_str!("../../text/component-add_fallback_diagnostics-9333dc00.md"),
            FidelityImpact::Lossy,
        ),
        (
            style::asset_kind(snapshot, node).is_none()
                && view
                    .value("fills")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|fills| {
                        fills.iter().enumerate().any(|(i, p)| {
                            p["type"] == "IMAGE"
                                && p["visible"] != false
                                && !super::image::policy_for(
                                    node,
                                    &format!("{}:fills:{i}", node.id),
                                    &context.asset_policies,
                                )
                                .original()
                        })
                    })
                && (view.child_ids().next().is_some()
                    || view
                        .value("fills")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|fs| fs.iter().filter(|p| p["visible"] != false).count() > 1)
                    || view
                        .value("effects")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|es| es.iter().any(|e| e["visible"] != false))),
            "DEVUP_CODEGEN_IMAGE_COMPOSITION_UNSUPPORTED",
            include_str!("../../text/component-add_fallback_diagnostics-b5ee2878.md"),
            FidelityImpact::Lossy,
        ),
        (
            view.bool("isMask") == Some(true),
            "DEVUP_CODEGEN_MASK_FALLBACK",
            "Mask is preserved as a plain Box rendering.",
            FidelityImpact::Lossy,
        ),
        (
            view.string("layoutPositioning") == Some("ABSOLUTE")
                && !layout::absolute_layout_is_exact(snapshot, node),
            "DEVUP_CODEGEN_ABSOLUTE_FALLBACK",
            "Absolute positioning is converted to position props with limited fidelity.",
            FidelityImpact::Approximated,
        ),
        (
            view.value("effects")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|effects| !effects.is_empty())
                && !style::effects_are_exact(&view),
            "DEVUP_CODEGEN_EFFECT_FALLBACK",
            "Some Figma effects may not be converted into computed CSS.",
            FidelityImpact::Lossy,
        ),
        (
            animation::has_unreachable_destination(snapshot, node),
            "DEVUP_CODEGEN_ANIMATION_UNREACHABLE",
            "A timed Smart Animate points at a frame that was not collected, so no keyframes are written for it.",
            FidelityImpact::Lossy,
        ),
    ];
    for (enabled, code, message, fidelity_impact) in candidates {
        if enabled {
            context.diagnostics.push(Diagnostic {
                code: code.to_owned(),
                message: message.to_owned(),
                node_id: Some(node.id.clone()),
                fidelity_impact: Some(fidelity_impact),
                ..Diagnostic::default()
            });
        }
    }
}

pub fn normalize_component_name(input: &str) -> String {
    let mut result = String::new();
    let mut segment = String::new();
    for character in input.chars().chain(std::iter::once(' ')) {
        if character.is_alphanumeric() || character == '_' {
            segment.push(character);
            continue;
        }
        if !segment.is_empty() {
            let all_ascii_upper = segment
                .chars()
                .filter(|character| character.is_ascii_alphabetic())
                .all(|character| character.is_ascii_uppercase());
            let mut characters = segment.chars();
            if let Some(first) = characters.next() {
                result.extend(first.to_uppercase());
                for rest in characters {
                    if all_ascii_upper {
                        result.extend(rest.to_lowercase());
                    } else {
                        result.push(rest);
                    }
                }
            }
            segment.clear();
        }
    }
    if result.is_empty() {
        return "FigmaComponent".to_owned();
    }
    if result
        .chars()
        .next()
        .is_some_and(|character| character.is_numeric())
    {
        result.insert(0, '_');
    }
    result
}

/// The custom components a rendered body refers to, in the order a reader meets
/// them, deduplicated. A devup-ui primitive is imported from the library and is
/// not one of these; anything else opening in PascalCase is.
fn referenced_components(body: &str) -> Vec<String> {
    const PRIMITIVES: [&str; 8] = [
        "Box", "Center", "Flex", "Grid", "Image", "Text", "VStack", "Input",
    ];
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    for (index, _) in body.match_indices('<') {
        let rest = &body[index + 1..];
        let name = rest
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect::<String>();
        if name.is_empty()
            || !name.starts_with(|character: char| character.is_ascii_uppercase())
            || PRIMITIVES.contains(&name.as_str())
            || !seen.insert(name.clone())
        {
            continue;
        }
        found.push(name);
    }
    found
}
