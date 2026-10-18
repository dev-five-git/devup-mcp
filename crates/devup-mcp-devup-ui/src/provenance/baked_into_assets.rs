

/// Nodes whose variable bindings no correct generator could write out.
///
/// An asset drawn in one colour becomes a Box masked to its shape with `bg`
/// set from that colour, so a token binding inside it survives as a token.
/// An asset drawn in more than one has no such form - a CSS mask carries
/// alpha only, and an SVG loaded through `<img src>` renders in its own
/// document where neither `currentColor` nor a CSS variable reaches it - so
/// it becomes an `<Image>` and every colour inside is baked into the file.
///
/// Counting those bindings as expected-but-missing asks the generator for
/// something the platform does not offer, and it is not a one-off: measured
/// on a real screen, two play buttons each hid a bound circle behind a white
/// glyph, and the shortfall they produced put the whole fidelity report on
/// the response with nothing in it anyone could act on.
fn baked_into_assets(snapshot: &Snapshot, root_id: &str) -> BTreeSet<String> {
    let mut baked = BTreeSet::new();
    let mut pending = vec![root_id.to_owned()];
    while let Some(node_id) = pending.pop() {
        let Some(node) = snapshot.nodes.get(&node_id) else {
            continue;
        };
        // A mask keeps its one colour in CSS, so keep looking inside it.
        if !matches!(asset_kind(snapshot, node), None | Some(AssetKind::SvgMask)) {
            let mut inside = vec![node_id];
            while let Some(current) = inside.pop() {
                let Some(node) = snapshot.nodes.get(&current) else {
                    continue;
                };
                inside.extend(node.typed_view().child_ids().map(str::to_owned));
                baked.insert(current);
            }
            continue;
        }
        pending.extend(node.typed_view().child_ids().map(str::to_owned));
    }
    baked
}

fn variable_sources(
    snapshot: &Snapshot,
    semantic_nodes: &BTreeSet<&str>,
) -> BTreeSet<(String, String)> {
    fn scan(node_id: &str, value: &serde_json::Value, output: &mut BTreeSet<(String, String)>) {
        match value {
            serde_json::Value::Object(object) => {
                if object.get("type").and_then(serde_json::Value::as_str) == Some("VARIABLE_ALIAS")
                    && let Some(id) = object
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty() && *id != "figma.mixed" && *id != "MIXED")
                {
                    output.insert((node_id.to_owned(), id.to_owned()));
                }
                for child in object.values() {
                    scan(node_id, child, output);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    scan(node_id, child, output);
                }
            }
            _ => {}
        }
    }

    let mut output = BTreeSet::new();
    for node_id in semantic_nodes {
        if let Some(node) = snapshot.nodes.get(*node_id) {
            let view = node.typed_view();
            // A mixed text-level fill is not a paint when every character is
            // covered by explicit segment fills. Retain all other bindings.
            let overridden = node.node_type == "TEXT"
                && view.value("fills").is_some_and(|v| !v.is_array())
                && view
                    .value("styledTextSegments")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|segments| {
                        !segments.is_empty()
                            && segments.iter().all(|segment| {
                                segment
                                    .get("fills")
                                    .and_then(serde_json::Value::as_array)
                                    .is_some_and(|fills| !fills.is_empty())
                            })
                            && segments
                                .iter()
                                .filter_map(|segment| {
                                    segment
                                        .get("characters")
                                        .and_then(serde_json::Value::as_str)
                                })
                                .collect::<String>()
                                == view.string("characters").unwrap_or("")
                    });
            for (field, value) in node.fields.iter().chain(node.extra.iter()) {
                if overridden && field == "fills" {
                    continue;
                }
                if overridden
                    && field == "boundVariables"
                    && let Some(bindings) = value.as_object()
                {
                    for (name, binding) in bindings {
                        if name != "fills" {
                            scan(node_id, binding, &mut output);
                        }
                    }
                } else {
                    scan(node_id, value, &mut output);
                }
            }
        }
    }
    output
}

fn text_sources(snapshot: &Snapshot, text_nodes: &BTreeSet<&str>) -> Vec<(String, String)> {
    let mut output = Vec::new();
    for node_id in text_nodes {
        let Some(node) = snapshot.nodes.get(*node_id) else {
            continue;
        };
        let segments = node
            .typed_view()
            .value("styledTextSegments")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|segment| {
                segment
                    .get("characters")
                    .and_then(serde_json::Value::as_str)
            })
            .filter(|characters| !characters.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if segments.is_empty() {
            if let Some(characters) = node
                .typed_view()
                .string("characters")
                .filter(|characters| !characters.is_empty())
            {
                output.push(((*node_id).to_owned(), characters.to_owned()));
            }
        } else {
            output.extend(
                segments
                    .into_iter()
                    .map(|characters| ((*node_id).to_owned(), characters)),
            );
        }
    }
    output
}

fn entry_range<'a>(entry: &ProvenanceEntry, tsx: &'a str) -> Option<&'a str> {
    let range = entry.generated_range.as_ref()?;
    (range.start < range.end
        && range.end <= tsx.len()
        && tsx.is_char_boundary(range.start)
        && tsx.is_char_boundary(range.end))
    .then(|| &tsx[range.start..range.end])
}

fn source_covers_text(source: &str, characters: &str) -> bool {
    if source == encode_jsx_text(characters) {
        return true;
    }
    if !characters.contains(['\r', '\n', '\u{2028}', '\u{2029}']) {
        return false;
    }
    let normalized = characters
        .replace("\r\n", "\n")
        .replace(['\r', '\u{2028}', '\u{2029}'], "\n");
    source
        .split("<br />")
        .map(str::trim)
        .eq(normalized.split('\n').map(str::trim).map(encode_jsx_text))
}

/// The text as the converter writes it, so a match is against the one rule
/// both sides follow rather than a copy of it kept here.
fn encode_jsx_text(input: &str) -> String {
    crate::codegen::escape_jsx_text(input)
}

fn empty_layout_leaf(output: &CodegenOutput, node_id: &str) -> bool {
    output.source_map.entries.iter().any(|entry| {
        entry.node_id.as_deref() == Some(node_id)
            && entry.property.is_none()
            && entry_range(entry, &output.tsx).is_some_and(|source| {
                let source = source.trim();
                ["<Box", "<VStack", "<Flex", "<Center", "<Grid"]
                    .iter()
                    .any(|tag| source.starts_with(tag))
                    && source.ends_with("/>")
                    && !source[1..].contains('<')
            })
    })
}

fn asset_reference_matches(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    entry: &ProvenanceEntry,
    node_id: &str,
    asset_id: &str,
    source: &str,
) -> bool {
    if !(source.starts_with("src=\"")
        || source.starts_with("maskImage=\"")
        || source.starts_with("bg=\""))
    {
        return false;
    }
    let Some(range) = entry.generated_range.as_ref() else {
        return false;
    };
    let owner = output
        .source_map
        .entries
        .iter()
        .filter(|e| e.property.is_none() && e.resolution == "node")
        .filter_map(|e| Some((e.node_id.as_deref()?, e.generated_range.as_ref()?)))
        .filter(|(_, r)| r.start <= range.start && r.end >= range.end)
        .min_by_key(|(_, r)| r.end - r.start)
        .map(|(id, _)| id);
    let Some(owner) = owner else {
        return false;
    };
    if owner != node_id {
        let parents = source_parents(snapshot);
        if !snapshot
            .nodes
            .get(owner)
            .is_some_and(|n| projects_as_asset(snapshot, n))
            || !has_asset_ancestor(node_id, &parents, &BTreeSet::from([owner.to_owned()]))
        {
            return false;
        }
    }
    if output.source_map.asset_css_mappings.iter().any(|mapping| {
        mapping["nodeId"] == owner
            && mapping["assetId"] == asset_id
            && mapping["assetPath"].as_str().is_some_and(|path| {
                source == format!("src=\"{path}\"")
                    || source.contains(&format!("url('{path}')"))
                    || source.contains(&format!("url({path})"))
            })
    }) {
        return true;
    }
    [false, true].into_iter().any(|per_node| {
        let path = crate::codegen::asset_path(snapshot, owner, per_node).or_else(|| {
            let index = asset_id.rsplit_once(":fills:")?.1.parse().ok()?;
            crate::codegen::image_fill_path(snapshot, owner, index, per_node)
        });
        path.is_some_and(|path| {
            source == format!("src=\"{path}\"")
                || source.contains(&format!("url('{path}')"))
                || source.contains(&format!("url({path})"))
        })
    })
}

// A property name or a source-map range alone does not prove size preservation.
pub(crate) fn node_opening<'a>(output: &'a CodegenOutput, node_id: &str) -> Option<&'a str> {
    output.source_map.entries.iter().find_map(|entry| {
        if entry.node_id.as_deref() != Some(node_id) || entry.property.is_some() {
            return None;
        }
        let source = entry_range(entry, &output.tsx)?;
        source.split_once('>').map(|(opening, _)| opening)
    })
}

fn fill_axis_is_established(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    node_id: &str,
    axis: &str,
) -> bool {
    fn established(
        snapshot: &Snapshot,
        output: &CodegenOutput,
        node_id: &str,
        axis: &str,
        seen: &mut BTreeSet<String>,
    ) -> bool {
        if !seen.insert(node_id.into()) {
            return false;
        }
        let Some(parent) = snapshot
            .nodes
            .values()
            .find(|parent| parent.typed_view().child_ids().any(|id| id == node_id))
        else {
            // Normal block roots have a host width, but no implicit height.
            return axis == "width";
        };
        let Some(opening) = node_opening(output, &parent.id) else {
            return false;
        };
        let prop = if axis == "width" { "w" } else { "h" };
        for prop in [prop, "boxSize"] {
            let needle = format!("{prop}=\"");
            if let Some(start) = find_prop(opening, &needle) {
                let Some((value, _)) = opening[start + needle.len()..].split_once('"') else {
                    return false;
                };
                if value == "100%" {
                    return established(snapshot, output, &parent.id, axis, seen);
                }
                return value
                    .strip_suffix("px")
                    .and_then(|value| value.parse::<f64>().ok())
                    .is_some_and(|value| value.is_finite() && value > 0.0);
            }
        }
        // A definite flex main size also provides the basis for nested FILL.
        // Follow the emitted flex chain; a HUG ancestor still ends this proof.
        if axis == "height" && sizing::has_vertical_flex_basis(snapshot, output, &parent.id) {
            return established(snapshot, output, &parent.id, axis, seen);
        }
        // HUG/shrink-to-fit width and percentage children are a cycle, not an
        // independent size anchor. Percentage height cannot use an auto parent.
        axis == "width"
            && parent.typed_view().string("layoutSizingHorizontal") != Some("HUG")
            && !opening.contains("pos=\"absolute\"")
            && !opening.contains("pos=\"fixed\"")
            && !opening.contains("display=\"inline")
            && !opening.contains("float=")
            && established(snapshot, output, &parent.id, axis, seen)
    }
    established(snapshot, output, node_id, axis, &mut BTreeSet::new())
}

fn dimension_value_matches(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    node_id: &str,
    axis: &str,
    source: &str,
) -> bool {
    if !matches!(axis, "width" | "height") {
        return true;
    }
    let Some(node) = snapshot.nodes.get(node_id) else {
        return false;
    };
    let view = node.typed_view();
    let sizing = if axis == "width" {
        "layoutSizingHorizontal"
    } else {
        "layoutSizingVertical"
    };
    let strict_leaf = empty_layout_leaf(output, node_id)
        && (view.string(sizing) == Some("HUG")
            || view
                .value("absoluteRenderBounds")
                .is_some_and(|v| v.is_object()));
    let requires_measured_size = projects_as_asset(snapshot, node) || strict_leaf;
    let Some(value) = source
        .split_once("=\"")
        .and_then(|(_, v)| v.strip_suffix('"'))
    else {
        return !requires_measured_size;
    };
    if view.string(sizing) == Some("FILL") && requires_measured_size {
        // A measured pixel width is not equivalent to filling the parent.
        return value == "100%" && fill_axis_is_established(snapshot, output, node_id, axis);
    }
    if source.starts_with("aspectRatio=") && requires_measured_size {
        let Some((w, h)) = crate::codegen::folded_mask_dimensions(snapshot, node) else {
            return false;
        };
        let Some((x, y)) = value.split_once('/').and_then(|(x, y)| {
            Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?))
        }) else {
            return false;
        };
        let (opposite, opposite_sizing, prop) = if axis == "height" {
            ("width", "layoutSizingHorizontal", "w")
        } else {
            ("height", "layoutSizingVertical", "h")
        };
        return view.string(sizing) == Some("HUG")
            && view.string(opposite_sizing) == Some("FILL")
            && x.is_finite()
            && y.is_finite()
            && x > 0.0
            && y > 0.0
            && (x / y - w / h).abs() < 1e-9
            && node_opening(output, node_id).is_some_and(|opening| {
                find_prop(opening, &format!("{prop}=\"100%\"")).is_some()
                    && find_prop(opening, if axis == "height" { "h=" } else { "w=" }).is_none()
                    && find_prop(opening, "boxSize=").is_none()
            })
            && fill_axis_is_established(snapshot, output, node_id, opposite);
    }
    if let Some(px) = value.strip_suffix("px") {
        let Ok(actual) = px.parse::<f64>() else {
            return false;
        };
        if !actual.is_finite() {
            return false;
        }
        let view = node.typed_view();
        if view.number(axis).is_some_and(|v| v > 0.0) && actual <= 0.0 {
            return false;
        }
        if requires_measured_size {
            let parent = snapshot
                .nodes
                .values()
                .find(|candidate| candidate.typed_view().child_ids().any(|id| id == node_id));
            let canvas = parent
                .map(|p| p.node_type.as_str())
                .or_else(|| view.string("parentType"))
                .is_some_and(|kind| matches!(kind, "SECTION" | "PAGE" | "COMPONENT_SET"));
            let absolute = view.string("layoutPositioning") == Some("ABSOLUTE")
                || placed_by_a_free_layout(snapshot, node, parent, canvas);
            let bounds = if projects_as_asset(snapshot, node) && absolute {
                crate::codegen::export_box(snapshot, node)
            } else if view.number("rotation").is_some_and(|r| r.abs() > 0.01) {
                crate::codegen::layout_box(node)
            } else {
                None
            };
            let expected = bounds
                .map(|b| if axis == "width" { b.w } else { b.h })
                .or_else(|| view.number(axis))
                .or_else(|| view.value("absoluteBoundingBox")?.get(axis)?.as_f64());
            return expected
                .is_some_and(|expected| actual > 0.0 && (actual - expected).abs() < 0.01);
        }
        return true;
    }
    // Percentages, expressions and a ratio without a proven opposing axis
    // depend on a host. They cannot prove an exported leaf's measured size.
    !requires_measured_size
}

fn layout_source_matches(property: &str, source: &str) -> bool {
    let is_prop = |name: &str| {
        source.starts_with(&format!("{name}=\""))
            || source.starts_with(&format!("{name}={{"))
            || source.starts_with(&format!("\"{name}\":"))
    };
    match property {
        "layoutMode" => {
            is_prop("flexDir") || matches!(source, "VStack" | "Flex" | "Grid" | "Center")
        }
        "layoutPositioning" => is_prop("pos"),
        "width" => is_prop("w") || is_prop("boxSize") || is_prop("aspectRatio"),
        "height" => is_prop("h") || is_prop("boxSize") || is_prop("aspectRatio"),
        "itemSpacing" => is_prop("gap") || is_prop("m"),
        "paddingTop" => is_prop("p") || is_prop("py") || is_prop("pt"),
        "paddingRight" => is_prop("p") || is_prop("px") || is_prop("pr"),
        "paddingBottom" => is_prop("p") || is_prop("py") || is_prop("pb"),
        "paddingLeft" => is_prop("p") || is_prop("px") || is_prop("pl"),
        _ => false,
    }
}

fn active_nodes(snapshot: &Snapshot, root_id: &str) -> Vec<(String, Option<String>)> {
    let mut output = Vec::new();
    let Some(root) = snapshot.nodes.get(root_id) else {
        return output;
    };
    output.push((root_id.to_owned(), None));
    let start = if root.node_type == "SECTION" {
        root.typed_view().child_ids().next()
    } else {
        Some(root_id)
    };
    let Some(start) = start else {
        return output;
    };
    let mut stack = if start == root_id {
        let mut children = root
            .typed_view()
            .child_ids()
            .map(|child| (child.to_owned(), Some(root_id.to_owned())))
            .collect::<Vec<_>>();
        children.reverse();
        children
    } else {
        vec![(start.to_owned(), Some(root_id.to_owned()))]
    };
    let mut seen = BTreeSet::from([root_id.to_owned()]);
    while let Some((node_id, parent_id)) = stack.pop() {
        if !seen.insert(node_id.clone()) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(&node_id) else {
            continue;
        };
        output.push((node_id.clone(), parent_id));
        let mut children = node.typed_view().child_ids().collect::<Vec<_>>();
        children.reverse();
        for child_id in children {
            stack.push((child_id.to_owned(), Some(node_id.clone())));
        }
    }
    output
}

fn typography_sources<'a>(
    snapshot: &'a Snapshot,
    text_nodes: &BTreeSet<&'a str>,
) -> BTreeSet<(&'a str, &'a str)> {
    let mut styles = BTreeSet::new();
    for node_id in text_nodes {
        let Some(node) = snapshot.nodes.get(*node_id) else {
            continue;
        };
        if let Some(style_id) = node
            .typed_view()
            .string("textStyleId")
            .filter(|id| !id.is_empty())
        {
            styles.insert((*node_id, style_id));
        }
        if let Some(segments) = node
            .typed_view()
            .value("styledTextSegments")
            .and_then(serde_json::Value::as_array)
        {
            for style_id in segments.iter().filter_map(|segment| {
                segment
                    .get("textStyleId")
                    .and_then(serde_json::Value::as_str)
                    .filter(|id| !id.is_empty())
            }) {
                styles.insert((*node_id, style_id));
            }
        }
    }
    styles
}

const LAYOUT_FIELDS: &[&str] = &[
    "layoutSizingHorizontal",
    "layoutSizingVertical",
    "layoutGrow",
    "layoutMode",
    "layoutPositioning",
    "width",
    "height",
    "itemSpacing",
    "paddingTop",
    "paddingRight",
    "paddingBottom",
    "paddingLeft",
];

pub(crate) fn mark_node(node_id: &str, rendered: String) -> String {
    format!("{START}{node_id}{CLOSE}{rendered}{END}{node_id}{CLOSE}")
}

pub(crate) fn finalize_tsx(
    marked: &str,
    snapshot: &Snapshot,
    variable_tokens: &BTreeMap<String, String>,
    style_tokens: &BTreeMap<String, String>,
) -> (String, SourceMap) {
    let (tsx, node_ranges) = strip_markers(marked);
    let emitted_ranges = node_ranges
        .iter()
        .cloned()
        .collect::<BTreeMap<String, GeneratedRange>>();
    let mut assets_by_node = BTreeMap::<String, Vec<_>>::new();
    for asset in discover_asset_manifest(snapshot).assets {
        assets_by_node
            .entry(asset.node_id.clone())
            .or_default()
            .push(asset);
    }
    let mut entries = Vec::new();
    for (node_id, range) in node_ranges {
        let Some(node) = snapshot.nodes.get(&node_id) else {
            continue;
        };
        entries.push(ProvenanceEntry {
            element_index: None,
            calculation: None,
            generated_property: None,
            generated_range: Some(range.clone()),
            json_pointer: None,
            node_id: Some(node_id.clone()),
            property: None,
            variable_id: None,
            style_id: None,
            asset_id: None,
            resolution: "node".to_owned(),
        });
        if node.node_type == "COMPONENT_SET" {
            continue;
        }
        let node_source = &tsx[range.start..range.end];
        let Some(open_relative) = node_source.find('<') else {
            continue;
        };
        let Some(close_relative) = node_source[open_relative..].find('>') else {
            continue;
        };
        let opening_end = open_relative + close_relative + 1;
        let opening = &node_source[open_relative..opening_end];
        let component_start = open_relative + 1;
        let component_end = opening[1..]
            .find(|character: char| character.is_whitespace() || matches!(character, '>' | '/'))
            .map_or(open_relative + opening.len(), |offset| {
                open_relative + offset + 1
            });
        if component_end > component_start {
            entries.push(generated_entry(
                range.start + component_start,
                range.start + component_end,
                &node_id,
                "type",
                None,
                None,
                "exact",
            ));
            if matches!(
                node.typed_view().string("layoutMode"),
                Some("HORIZONTAL" | "VERTICAL" | "GRID")
            ) {
                entries.push(generated_entry(
                    range.start + component_start,
                    range.start + component_end,
                    &node_id,
                    "layoutMode",
                    None,
                    None,
                    "raw-fallback",
                ));
            }
        }

        let mut prop_sources = PROP_SOURCES.to_vec();
        if crate::codegen::asset_kind(snapshot, node).is_none()
            && crate::codegen::single_outside_stroke(&node.typed_view()).is_some()
        {
            prop_sources.push(("boxShadow", "strokes"));
            if node
                .typed_view()
                .value("effects")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|effects| !effects.is_empty())
            {
                prop_sources.push(("boxShadow", "effects"));
            }
        }
        let mut selector_properties = BTreeSet::new();
        if let Some(selector) = non_default_variant_selector(snapshot, node) {
            for (prop, property) in &prop_sources {
                let Some((start, end)) = selector_prop_range(opening, &selector, prop) else {
                    continue;
                };
                selector_properties.insert(*property);
                entries.push(generated_entry(
                    range.start + open_relative + start,
                    range.start + open_relative + end,
                    &node_id,
                    property,
                    None,
                    None,
                    "variant-selector",
                ));
            }
        }

        for (prop, property) in &prop_sources {
            if *property == "overflowDirection"
                && node.typed_view().string("overflowDirection").is_none()
            {
                continue;
            }
            if *prop == "aspectRatio" && node.typed_view().value("targetAspectRatio").is_none() {
                continue;
            }
            if selector_properties.contains(property) {
                continue;
            }
            let needle = format!("{prop}=\"");
            let Some(start) = find_prop(opening, &needle) else {
                let expression = format!("{prop}={{");
                if let Some(start) = find_prop(opening, &expression) {
                    entries.push(generated_entry(
                        range.start + open_relative + start,
                        range.start + open_relative + start + expression.len(),
                        &node_id,
                        property,
                        None,
                        None,
                        "raw-fallback",
                    ));
                }
                continue;
            };
            let value_start = start + needle.len();
            let Some(value_end) = opening[value_start..].find('"') else {
                continue;
            };
            let end = value_start + value_end + 1;
            let value = &opening[value_start..value_start + value_end];
            let variable_id = find_resource_id(value, variable_tokens, '$');
            let style_id = (*prop == "typography")
                .then(|| {
                    style_tokens
                        .iter()
                        .find_map(|(id, token)| (token == value).then(|| id.clone()))
                })
                .flatten();
            let resolution = if *prop == "whiteSpace"
                && value == "pre-wrap"
                && crate::codegen::preserves_hard_break_spaces(&node.typed_view())
            {
                "derived-hard-break-whitespace"
            } else if *prop == "boxShadow"
                && crate::codegen::asset_kind(snapshot, node).is_none()
                && crate::codegen::single_outside_stroke(&node.typed_view()).is_some()
            {
                "derived-single-edge-outside-stroke"
            } else if variable_id.is_some() {
                "variable-token"
            } else if style_id.is_some() {
                "style-token"
            } else if *prop == "justifyContent"
                && value == "center"
                && crate::codegen::centers_lone_space_between_child(snapshot, node)
            {
                "derived-lone-child-center"
            } else if matches!(*property, "width" | "height")
                && !node.field_errors.contains_key(*property)
                && matches!(*prop, "w" | "h" | "boxSize")
                && value
                    .strip_suffix("px")
                    .and_then(|v| v.parse::<f64>().ok())
                    .zip(node.typed_view().number(property))
                    .is_some_and(|(generated, original)| {
                        original.is_finite() && original == generated
                    })
            {
                "verified-explicit-dimension"
            } else {
                "raw-fallback"
            };
            let source_property = if *prop == "whiteSpace"
                && value == "pre-wrap"
                && crate::codegen::preserves_hard_break_spaces(&node.typed_view())
            {
                if node.typed_view().string("characters").is_some() {
                    "characters"
                } else {
                    "styledTextSegments"
                }
            } else if *prop == "minW"
                && value == "0"
                && node.typed_view().number("minWidth").is_none()
                && node.typed_view().string("layoutSizingHorizontal") == Some("FILL")
            {
                "layoutSizingHorizontal"
            } else if *prop == "fontWeight"
                && node.node_type == "TEXT"
                && node.typed_view().number(property).is_none()
            {
                "styledTextSegments"
            } else {
                property
            };
            entries.push(generated_entry(
                range.start + open_relative + start,
                range.start + open_relative + end,
                &node_id,
                source_property,
                variable_id,
                style_id,
                resolution,
            ));
        }

        if crate::codegen::folded_mask_dimensions(snapshot, node).is_some() {
            for (axis, sizing, prop) in [
                ("width", "layoutSizingHorizontal", "w"),
                ("height", "layoutSizingVertical", "h"),
            ] {
                if node.typed_view().string(sizing) != Some("HUG") {
                    continue;
                }
                let derived_prop = if find_prop(opening, "aspectRatio=\"").is_some() {
                    "aspectRatio"
                } else {
                    prop
                };
                let needle = format!("{derived_prop}=\"");
                if let Some(start) = find_prop(opening, &needle)
                    && let Some(end) = opening[start + needle.len()..].find('"')
                {
                    let end = start + needle.len() + end + 1;
                    for field in [axis, "width", "height", sizing, "childrenIds"] {
                        let mut entry = generated_entry(
                            range.start + open_relative + start,
                            range.start + open_relative + end,
                            &node_id,
                            field,
                            None,
                            None,
                            "restored-hug-after-mask-child-folding",
                        );
                        if matches!(field, "width" | "height")
                            && node.typed_view().number(field).is_none()
                        {
                            entry.resolution =
                                "restored-hug-after-mask-child-folding-from-absoluteBoundingBox"
                                    .into();
                        }
                        if !entries.contains(&entry) {
                            entries.push(entry);
                        }
                    }
                }
            }
        }

        if let Some(asset) = assets_by_node
            .get(&node_id)
            .and_then(|assets| assets.first())
            && let Some((start, end)) = asset_prop_range(opening)
        {
            entries.push(ProvenanceEntry {
                element_index: None,
                calculation: None,
                generated_property: None,
                generated_range: Some(GeneratedRange {
                    start: range.start + open_relative + start,
                    end: range.start + open_relative + end,
                }),
                json_pointer: None,
                node_id: Some(node_id.clone()),
                property: Some(asset.field.clone()),
                variable_id: None,
                style_id: None,
                asset_id: Some(asset.asset_id.clone()),
                resolution: "asset".to_owned(),
            });
        }

        if node.node_type == "TEXT" {
            add_text_entries(
                &tsx,
                &range,
                &node_id,
                node,
                variable_tokens,
                style_tokens,
                &mut entries,
            );
        }
    }
    add_flattened_resource_entries(
        &tsx,
        snapshot,
        &emitted_ranges,
        variable_tokens,
        &assets_by_node,
        &mut entries,
    );
    entries.sort_by(|left, right| {
        let left_range = left.generated_range.as_ref();
        let right_range = right.generated_range.as_ref();
        left_range
            .map(|range| (range.start, range.end))
            .cmp(&right_range.map(|range| (range.start, range.end)))
            .then_with(|| left.node_id.cmp(&right.node_id))
            .then_with(|| left.property.cmp(&right.property))
    });
    (
        tsx,
        SourceMap {
            version: 2,
            entries,
            asset_css_mappings: Vec::new(),
            variable_tokens: variable_tokens.clone(),
            style_tokens: style_tokens.clone(),
        },
    )
}