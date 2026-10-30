

/// Variant/responsive trees use the same representation and paint geometry
/// as the ordinary renderer. Synthetic paint elements inherit provenance
/// from their outer source node rather than inventing Figma node identities.
pub(super) fn project_tree(
    mut tree: super::variant::Tree,
    snapshot: &Snapshot,
    node: &RawNode,
    options: &super::CodegenOptions,
) -> super::variant::Tree {
    if style::non_rendering_asset_reason(snapshot, node).is_some() {
        return tree;
    }
    use super::variant::Tree;
    let synthetic =
        |component: &str, props: BTreeMap<String, String>, children: Vec<Tree>, name: &str| Tree {
            node_id: String::new(),
            node_name: name.into(),
            source_node_ids: BTreeSet::new(),
            component: component.into(),
            props,
            children,
            content: None,
            is_component: false,
            visible_when: None,
            drawn_when: Vec::new(),
            leading_comment: None,
            slots: Vec::new(),
            scalar_sources: Vec::new(),
        };
    let visible = node.typed_view().value("fills").and_then(Value::as_array);
    if tree.component == "Image" {
        let index = visible.and_then(|fs| {
            fs.iter()
                .position(|p| p["type"] == "IMAGE" && p["visible"] != false)
        });
        if index.is_none() {
            return tree;
        }
        let mapping = map_for(
            snapshot,
            node,
            index,
            &options.asset_policies,
            options.asset_names_per_node,
        );
        if mapping["css"]["image"]["transform"].is_null() {
            return tree;
        }
        let image_names = [
            "src",
            "srcSet",
            "sizes",
            "width",
            "height",
            "objectFit",
            "objectPos",
            "alt",
        ];
        let mut image = tree
            .props
            .iter()
            .filter(|(n, _)| image_names.contains(&n.as_str()))
            .map(|(n, v)| (n.clone(), v.clone()))
            .collect::<BTreeMap<_, _>>();
        tree.props.retain(|n, _| !image_names.contains(&n.as_str()));
        tree.component = "Box".into();
        tree.props.entry("pos".into()).or_insert("relative".into());
        for (n, v) in mapping["css"]["image"].as_object().into_iter().flatten() {
            if let Some(v) = v.as_str() {
                image.insert(n.clone(), v.into());
            }
        }
        for (n, v) in [
            ("w", "100%"),
            ("h", "100%"),
            ("pos", "absolute"),
            ("left", "0"),
            ("top", "0"),
            ("maxW", "none"),
            ("display", "block"),
        ] {
            image.insert(n.into(), v.into());
        }
        let mut clip: BTreeMap<String, String> = [
            ("w", "100%"),
            ("h", "100%"),
            ("pos", "relative"),
            ("overflow", "hidden"),
            ("borderRadius", "inherit"),
        ]
        .into_iter()
        .map(|(n, v)| (n.into(), v.into()))
        .collect();
        for (dimension, css, axis) in [("width", "w", "horizontal"), ("height", "h", "vertical")] {
            if mapping["layoutSizing"][axis] == "HUG"
                && !(options.root_layout == super::RootLayout::Embedded
                    && snapshot.roots.contains(&node.id))
                && !tree.props.contains_key(css)
                && !tree.props.contains_key("boxSize")
                && let Some(value) = mapping["nodeBox"][dimension].as_f64()
            {
                clip.insert(css.into(), format!("{}px", number(value)));
            }
        }
        tree.children = vec![synthetic(
            "Box",
            clip,
            vec![synthetic("Image", image, vec![], "source-image")],
            "image-clip",
        )];
        return tree;
    }
    if tree.component == "Text" {
        return tree;
    }
    let Some(paints) = visible else {
        return tree;
    };
    if !paints.iter().enumerate().any(|(i, p)| {
        p["type"] == "IMAGE"
            && p["visible"] != false
            && policy_for(
                node,
                &format!("{}:fills:{i}", node.id),
                &options.asset_policies,
            )
            .original()
    }) {
        return tree;
    }
    let style_options = style::StyleOptions {
        reserve_image_content: !matches!(options.root_layout, super::RootLayout::Embedded),
        asset_policies: &options.asset_policies,
        variable_tokens: &options.variable_tokens,
        length_tokens: &options.length_tokens,
        asset_names_per_node: options.asset_names_per_node,
        width_policy: options.width_policy.as_ref(),
    };
    let mut layers = Vec::new();
    let mut used = BTreeSet::new();
    for (index, p) in paints
        .iter()
        .enumerate()
        .filter(|(_, p)| p["visible"] != false && p["opacity"] != 0)
    {
        let mut props: BTreeMap<String, String> = [
            ("aria-hidden", "true"),
            ("pos", "absolute"),
            ("inset", "0"),
            ("zIndex", "-1"),
            ("pointerEvents", "none"),
            ("overflow", "hidden"),
            ("borderRadius", "inherit"),
        ]
        .into_iter()
        .map(|(n, v)| (n.into(), v.into()))
        .collect();
        if let Some(blend) = p["blendMode"].as_str().and_then(style::blend_mode) {
            props.insert("mixBlendMode".into(), blend);
        }
        let children = if p["type"] == "IMAGE" && p["scaleMode"] != "TILE" {
            let mapping = map_for(
                snapshot,
                node,
                Some(index),
                &options.asset_policies,
                options.asset_names_per_node,
            );
            let mut image_props = Vec::new();
            apply_image_props(&mut image_props, &mapping);
            let mut image = image_props
                .into_iter()
                .filter(|(n, _)| !matches!(n.as_str(), "aspectRatio" | "minW" | "minH"))
                .map(|(n, super::component::PropValue::String(v))| (n, v))
                .collect::<BTreeMap<_, _>>();
            image.insert(
                "src".into(),
                mapping["assetPath"].as_str().unwrap_or_default().into(),
            );
            image.insert("alt".into(), String::new());
            for (n, v) in mapping["css"]["image"].as_object().into_iter().flatten() {
                if n == "opacity" {
                    continue;
                }
                if let Some(v) = v.as_str() {
                    image.insert(n.clone(), v.into());
                }
            }
            for (n, v) in [
                ("pos", "absolute"),
                ("inset", "0"),
                ("w", "100%"),
                ("h", "100%"),
                ("maxW", "none"),
                ("display", "block"),
            ] {
                image.insert(n.into(), v.into());
            }
            if let Some(opacity) = p["opacity"].as_f64()
                && opacity < 1.
            {
                props.insert("opacity".into(), number(opacity));
            }
            vec![synthetic(
                "Image",
                image,
                vec![],
                &format!("paint-{index}-image"),
            )]
        } else {
            if let Some(css) =
                style::paint_css(snapshot, node, p, index, false, &mut used, style_options)
            {
                props.insert("bg".into(), css);
            }
            vec![]
        };
        layers.push(synthetic("Box", props, children, &format!("paint-{index}")));
    }
    tree.props
        .retain(|n, _| !matches!(n.as_str(), "bg" | "bgBlendMode" | "objectFit" | "objectPos"));
    tree.props.entry("pos".into()).or_insert("relative".into());
    tree.props.insert("isolation".into(), "isolate".into());
    tree.children.splice(0..0, layers);
    tree
}

pub(super) fn map_for(
    snapshot: &Snapshot,
    node: &RawNode,
    index: Option<usize>,
    policies: &BTreeMap<String, AssetRepresentationPolicy>,
    per_node: bool,
) -> Value {
    let id = index
        .map(|i| format!("{}:fills:{i}", node.id))
        .unwrap_or_else(|| node.id.clone());
    let default_path = index
        .and_then(|i| style::image_fill_path(snapshot, &node.id, i, per_node))
        .or_else(|| style::asset_path(snapshot, &node.id, per_node))
        .unwrap_or_default();
    asset_css_map(
        &serde_json::to_value(node).expect("RawNode serializes"),
        &id,
        &default_path,
        &policy_for(node, &id, policies),
    )
}

pub(super) fn attach_evidence(
    snapshot: &Snapshot,
    options: &super::CodegenOptions,
    output: &mut super::CodegenOutput,
) {
    use crate::provenance::{GeneratedRange, ProvenanceEntry};
    let attrs = crate::provenance::attributes::generated_attributes(&output.tsx);
    let emitted = output
        .source_map
        .entries
        .iter()
        .filter(|e| e.property.is_none() && e.resolution == "node")
        .filter_map(|e| Some((e.node_id.clone()?, e.generated_range.clone()?)))
        .collect::<Vec<_>>();
    for (id, range) in emitted {
        let Some(node) = snapshot.nodes.get(&id) else {
            continue;
        };
        let indices = node
            .typed_view()
            .value("fills")
            .and_then(Value::as_array)
            .map(|fs| {
                fs.iter()
                    .enumerate()
                    .filter(|(_, p)| p["type"] == "IMAGE" && p["visible"] != false)
                    .map(|(i, _)| Some(i))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let indices = if indices.is_empty()
            && style::asset_kind(snapshot, node) == Some(style::AssetKind::Png)
        {
            vec![None]
        } else {
            indices
        };
        for index in indices {
            let mut mapping = map_for(
                snapshot,
                node,
                index,
                &options.asset_policies,
                options.asset_names_per_node,
            );
            let path = mapping["assetPath"].as_str().unwrap_or_default().to_owned();
            let asset_id = mapping["assetId"].as_str().unwrap_or(&id).to_owned();
            // Nested nodes have their own range; only this node's properties
            // can substantiate its mapping, even if another instance reuses bytes.
            let own = attrs
                .iter()
                .filter(|a| range.start <= a.start && a.end <= range.end)
                .filter(|a| {
                    !output.source_map.entries.iter().any(|e| {
                        e.property.is_none()
                            && e.node_id.as_deref() != Some(&id)
                            && e.generated_range.as_ref().is_some_and(|r| {
                                range.start <= r.start
                                    && r.end <= range.end
                                    && r.start <= a.start
                                    && a.end <= r.end
                            })
                    })
                })
                .collect::<Vec<_>>();
            let references = own
                .iter()
                .filter(|a| {
                    matches!(a.name.as_str(), "src" | "bg")
                        && !path.is_empty()
                        && literal_attribute(&a.source).is_some_and(|value| {
                            value == path || background_urls(value).iter().any(|url| *url == path)
                        })
                })
                .collect::<Vec<_>>();
            if references.is_empty() {
                let policy = policy_for(node, &asset_id, &options.asset_policies);
                if !policy.incompatible_capture(&id) {
                    continue;
                }
                mapping["emitted"] = json!(false);
                mapping["effectsAppliedBy"] = Value::Null;
                mapping["emittedProperties"] = json!([]);
                mapping["propertyEvidence"] = json!([]);
                output.diagnostics.push(Diagnostic{code:"DEVUP_CODEGEN_IMAGE_MAPPING_UNSUPPORTED".into(),message:"A visible capture of a different node cannot be composed into this leaf's layout box.".into(),node_id:Some(id.clone()),property:Some("fills".into()),fidelity_impact:Some(devup_mcp_figma::FidelityImpact::Lossy),details:Some(json!({"assetId":asset_id,"reason":"incompatible-visible-capture","captureNodeId":policy.capture_node_id,"captureBox":policy.capture_box,"emittedAssetReference":false})),..Diagnostic::default()});
                output.source_map.asset_css_mappings.push(mapping);
                continue;
            }
            mapping["emitted"] = json!(true);
            let properties=own.iter().map(|a|json!({"elementIndex":a.element,"name":a.name,"generatedProperty":a.source})).collect::<Vec<_>>();
            mapping["emittedProperties"] = json!(properties);
            // The public source map exposes verified emission, while the
            // reusable recipe remains useful before code generation.
            mapping["propertyEvidence"]=json!(own.iter().filter_map(|a| {
                let source=match a.name.as_str(){"filter"|"boxShadow"|"backdropFilter"=>"effects","borderRadius"|"borderTopLeftRadius"|"borderTopRightRadius"|"borderBottomLeftRadius"|"borderBottomRightRadius"=>"radii","width"|"height"=>"acquired-pixel-dimensions","transform"|"transformOrigin"|"objectFit"|"objectPos"=>"image-paint-geometry","srcSet"|"sizes"=>"acquired-variants","opacity"=>"node-or-paint-opacity","mixBlendMode"|"bgBlendMode"=>"node-or-paint-blend-mode","bg"=>"paint-stack-gradient-or-solid",_=>return None};
                Some(json!({"property":a.name,"generatedProperty":a.source,"source":source,"elementIndex":a.element,"scope":"node-paint-stack","effectsAppliedBy":mapping["effectsAppliedBy"]}))
            }).collect::<Vec<_>>());
            for a in own.iter().filter(|a| {
                matches!(
                    a.name.as_str(),
                    "width"
                        | "height"
                        | "srcSet"
                        | "sizes"
                        | "objectFit"
                        | "objectPos"
                        | "transform"
                        | "transformOrigin"
                        | "aspectRatio"
                        | "minW"
                        | "minH"
                        | "filter"
                        | "boxShadow"
                        | "backdropFilter"
                )
            }) {
                let property = match a.name.as_str() {
                    "width" | "height" => "sourcePixelSize",
                    "srcSet" | "sizes" => "assetVariants",
                    "filter" | "boxShadow" | "backdropFilter" => "effects",
                    _ => "fills",
                };
                output.source_map.entries.push(ProvenanceEntry {generated_range:Some(GeneratedRange{start:a.start,end:a.end}),calculation:Some("Acquisition representation policy and image-paint geometry; see assetCssMappings for dimensions, coordinate semantics and limits.".into()),generated_property:Some(a.source.clone()),json_pointer:None,node_id:Some(id.clone()),property:Some(property.into()),variable_id:None,style_id:None,asset_id:Some(asset_id.clone()),resolution:"asset-css-projection".into(),element_index:Some(a.element)});
            }
            for a in &references {
                output.source_map.entries.push(ProvenanceEntry {
                    generated_range: Some(GeneratedRange {
                        start: a.start,
                        end: a.end,
                    }),
                    calculation: Some("Instance-specific acquired image path.".into()),
                    generated_property: Some(a.source.clone()),
                    json_pointer: None,
                    node_id: Some(id.clone()),
                    property: Some("fills".into()),
                    variable_id: None,
                    style_id: None,
                    asset_id: Some(asset_id.clone()),
                    resolution: "asset".into(),
                    element_index: Some(a.element),
                });
                if mapping["effectsBaked"] == true {
                    let mut entry = output.source_map.entries.last().unwrap().clone();
                    entry.property = Some("effects".into());
                    entry.resolution = "effects-baked-into-asset".into();
                    entry.calculation=Some("Supplied acquisition evidence confirms baked effects; the same CSS effects are suppressed.".into());
                    output.source_map.entries.push(entry);
                }
            }
            for reason in mapping["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let unsupported = mapping["unsupported"]
                    .as_array()
                    .is_some_and(|xs| xs.iter().any(|x| x == reason));
                output.diagnostics.push(Diagnostic{code:if unsupported{"DEVUP_CODEGEN_IMAGE_MAPPING_UNSUPPORTED"}else{"DEVUP_CODEGEN_IMAGE_MAPPING_EVIDENCE"}.into(),message:format!("Image mapping: {reason}."),node_id:Some(id.clone()),property:Some("fills".into()),severity:Some(DiagnosticSeverity::Warning),fidelity_impact:Some(if unsupported{devup_mcp_figma::FidelityImpact::Lossy}else{devup_mcp_figma::FidelityImpact::Approximated}),details:Some(json!({"assetId":asset_id,"reason":reason,"renderMode":mapping["renderMode"],"evidenceLimit":mapping["coordinateSemantics"]["scope"]})),..Diagnostic::default()});
            }
            output.source_map.asset_css_mappings.push(mapping);
        }
    }
    output
        .diagnostics
        .extend(validate_asset_effects(&output.tsx, &options.asset_policies));
}
