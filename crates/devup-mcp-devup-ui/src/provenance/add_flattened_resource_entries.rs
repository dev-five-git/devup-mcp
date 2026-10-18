

fn add_flattened_resource_entries(
    tsx: &str,
    snapshot: &Snapshot,
    emitted_ranges: &BTreeMap<String, GeneratedRange>,
    variable_tokens: &BTreeMap<String, String>,
    assets_by_node: &BTreeMap<String, Vec<devup_mcp_figma::AssetManifestEntry>>,
    entries: &mut Vec<ProvenanceEntry>,
) {
    let parents = source_parents(snapshot);
    let all_nodes = snapshot
        .nodes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for (node_id, variable_id) in variable_sources(snapshot, &all_nodes) {
        if entries.iter().any(|entry| {
            entry.node_id.as_deref() == Some(node_id.as_str())
                && entry.variable_id.as_deref() == Some(variable_id.as_str())
                && entry.resolution == "variable-token"
        }) {
            continue;
        }
        let Some(token) = variable_tokens.get(&variable_id) else {
            continue;
        };
        let needle = format!("${token}");
        let mut represented_by = Some(node_id.as_str());
        while let Some(candidate_id) = represented_by {
            if let Some(range) = emitted_ranges.get(candidate_id)
                && (candidate_id == node_id
                    || snapshot
                        .nodes
                        .get(candidate_id)
                        .is_some_and(|n| projects_as_asset(snapshot, n)))
            {
                let source = &tsx[range.start..range.end];
                // Original images can move their solid/gradient token into an
                // owned paint layer. Only inspect this node's opening elements;
                // a descendant's identical token cannot prove its parent's fill.
                let token_range = attributes::opening_elements(source)
                    .into_iter()
                    .filter(|opening| {
                        !emitted_ranges.iter().any(|(other, child)| {
                            other != candidate_id
                                && range.start <= child.start
                                && child.end <= range.end
                                && child.start <= range.start + opening.start
                                && range.start + opening.end <= child.end
                        })
                    })
                    .find_map(|opening| {
                        token_prop_range(&source[opening.start..opening.end], &needle)
                            .map(|(start, end)| (opening.start + start, opening.start + end))
                    });
                if let Some((start, end)) = token_range {
                    entries.push(generated_entry(
                        range.start + start,
                        range.start + end,
                        &node_id,
                        variable_property(snapshot, &node_id, &variable_id),
                        Some(variable_id.clone()),
                        None,
                        "variable-token",
                    ));
                    break;
                }
            }
            represented_by = parents.get(candidate_id).map(String::as_str);
        }
    }

    for (node_id, assets) in assets_by_node {
        for asset in assets {
            if entries.iter().any(|entry| {
                entry.node_id.as_deref() == Some(node_id.as_str())
                    && entry.asset_id.as_deref() == Some(asset.asset_id.as_str())
                    && entry.resolution == "asset"
            }) {
                continue;
            }
            let mut represented_by = parents.get(node_id.as_str()).map(String::as_str);
            while let Some(candidate_id) = represented_by {
                if let Some(range) = emitted_ranges.get(candidate_id)
                    && snapshot
                        .nodes
                        .get(candidate_id)
                        .is_some_and(|n| projects_as_asset(snapshot, n))
                {
                    let source = &tsx[range.start..range.end];
                    if let Some((start, end)) = asset_range_in_node_source(source) {
                        entries.push(ProvenanceEntry {
                            element_index: None,
                            calculation: None,
                            generated_property: None,
                            generated_range: Some(GeneratedRange {
                                start: range.start + start,
                                end: range.start + end,
                            }),
                            json_pointer: None,
                            node_id: Some(node_id.clone()),
                            property: Some(asset.field.clone()),
                            variable_id: None,
                            style_id: None,
                            asset_id: Some(asset.asset_id.clone()),
                            resolution: "asset".to_owned(),
                        });
                        break;
                    }
                }
                represented_by = parents.get(candidate_id).map(String::as_str);
            }
        }
    }
}

fn source_parents(snapshot: &Snapshot) -> BTreeMap<String, String> {
    let mut parents = BTreeMap::new();
    for node in snapshot.nodes.values() {
        for child_id in node.typed_view().child_ids() {
            parents
                .entry(child_id.to_owned())
                .or_insert_with(|| node.id.clone());
        }
    }
    for node in snapshot.nodes.values() {
        if let Some(parent_id) = node.typed_view().string("parentId") {
            parents
                .entry(node.id.clone())
                .or_insert_with(|| parent_id.to_owned());
        }
    }
    parents
}

fn variable_property<'a>(snapshot: &'a Snapshot, node_id: &str, variable_id: &str) -> &'a str {
    fn contains_alias(value: &serde_json::Value, variable_id: &str) -> bool {
        match value {
            serde_json::Value::Object(object) => {
                (object.get("type").and_then(serde_json::Value::as_str) == Some("VARIABLE_ALIAS")
                    && object.get("id").and_then(serde_json::Value::as_str) == Some(variable_id))
                    || object
                        .values()
                        .any(|value| contains_alias(value, variable_id))
            }
            serde_json::Value::Array(values) => values
                .iter()
                .any(|value| contains_alias(value, variable_id)),
            _ => false,
        }
    }

    let Some(node) = snapshot.nodes.get(node_id) else {
        return "boundVariables";
    };
    for field in ["fills", "strokes", "effects", "boundVariables"] {
        if node
            .typed_view()
            .value(field)
            .is_some_and(|value| contains_alias(value, variable_id))
        {
            return field;
        }
    }
    node.fields
        .iter()
        .chain(&node.extra)
        .find_map(|(field, value)| contains_alias(value, variable_id).then_some(field.as_str()))
        .unwrap_or("boundVariables")
}

fn non_default_variant_selector(
    snapshot: &Snapshot,
    node: &devup_mcp_figma::RawNode,
) -> Option<String> {
    let parent = snapshot.nodes.get(node.typed_view().string("parentId")?)?;
    if parent.node_type != "COMPONENT_SET" {
        return None;
    }
    let definitions = parent
        .typed_view()
        .value("componentPropertyDefinitions")?
        .as_object()?;
    let (effect_name, definition) = definitions.iter().find(|(name, definition)| {
        name.eq_ignore_ascii_case("effect")
            && definition.get("type").and_then(serde_json::Value::as_str) == Some("VARIANT")
    })?;
    let default = definition
        .get("defaultValue")
        .and_then(serde_json::Value::as_str)?;
    let value = node
        .typed_view()
        .value("variantProperties")?
        .get(effect_name)?
        .as_str()?;
    (value != default).then(|| format!("_{value}"))
}

fn selector_prop_range(opening: &str, selector: &str, prop: &str) -> Option<(usize, usize)> {
    let selector_needle = format!("{selector}={{{{");
    let selector_start = find_prop(opening, &selector_needle)?;
    let block = &opening[selector_start..];
    let mut depth = 0_u32;
    let mut quoted = false;
    let mut escaped = false;
    let mut block_end = None;
    for (offset, character) in block.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => quoted = true,
            '{' => depth += 1,
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    block_end = Some(offset + character.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }
    let block = &block[..block_end?];
    let needle = format!("\"{prop}\":");
    let relative = block.find(&needle)?;
    Some((
        selector_start + relative,
        selector_start + relative + needle.len(),
    ))
}

const PROP_SOURCES: &[(&str, &str)] = &[
    ("alignContent", "textAlignVertical"),
    ("alignItems", "counterAxisAlignItems"),
    ("aspectRatio", "targetAspectRatio"),
    ("bg", "fills"),
    ("border", "strokes"),
    ("borderBottom", "strokes"),
    ("borderLeft", "strokes"),
    ("borderRight", "strokes"),
    ("borderTop", "strokes"),
    ("borderRadius", "cornerRadius"),
    ("bottom", "y"),
    ("boxSize", "width"),
    ("boxSize", "height"),
    ("color", "fills"),
    ("flex", "layoutGrow"),
    ("flexDir", "layoutMode"),
    ("fontFamily", "fontName"),
    ("fontSize", "fontSize"),
    ("fontStyle", "fontName"),
    ("fontWeight", "fontWeight"),
    ("gap", "itemSpacing"),
    ("h", "height"),
    ("justifyContent", "primaryAxisAlignItems"),
    ("left", "x"),
    ("letterSpacing", "letterSpacing"),
    ("lineHeight", "lineHeight"),
    ("m", "itemSpacing"),
    ("maxH", "maxHeight"),
    ("maxW", "maxWidth"),
    ("minH", "minHeight"),
    ("minW", "minWidth"),
    ("opacity", "opacity"),
    ("outline", "strokes"),
    ("outlineOffset", "strokeWeight"),
    ("overflow", "clipsContent"),
    ("overflow", "overflowDirection"),
    ("overflowX", "overflowDirection"),
    ("overflowY", "overflowDirection"),
    ("p", "paddingTop"),
    ("p", "paddingRight"),
    ("p", "paddingBottom"),
    ("p", "paddingLeft"),
    ("pb", "paddingBottom"),
    ("pl", "paddingLeft"),
    ("pos", "layoutPositioning"),
    ("pr", "paddingRight"),
    ("pt", "paddingTop"),
    ("px", "paddingLeft"),
    ("px", "paddingRight"),
    ("py", "paddingTop"),
    ("py", "paddingBottom"),
    ("right", "x"),
    ("textAlign", "textAlignHorizontal"),
    ("top", "y"),
    ("typography", "textStyleId"),
    ("w", "width"),
    ("whiteSpace", "textAutoResize"),
    ("wordBreak", "characters"),
];

fn add_text_entries(
    tsx: &str,
    range: &GeneratedRange,
    node_id: &str,
    node: &devup_mcp_figma::RawNode,
    variable_tokens: &BTreeMap<String, String>,
    style_tokens: &BTreeMap<String, String>,
    entries: &mut Vec<ProvenanceEntry>,
) {
    let source = &tsx[range.start..range.end];
    let view = node.typed_view();
    let mut text_segments = view
        .value("styledTextSegments")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|segment| {
            segment
                .get("characters")
                .and_then(serde_json::Value::as_str)
                .map(|characters| (characters, Some(segment)))
        })
        .filter(|(characters, _)| !characters.is_empty())
        .collect::<Vec<_>>();
    if text_segments.is_empty()
        && let Some(characters) = view
            .string("characters")
            .filter(|characters| !characters.is_empty())
    {
        text_segments.push((characters, None));
    }
    let mut cursor = 0;
    for (characters, segment) in text_segments {
        if let Some((start, end)) = find_text_span(source, characters, cursor) {
            // A rich-text wrapper can carry its own advance and weight.
            // Attribute ownership belongs to the segment whose text directly
            // follows this opening, rather than the node's default metrics.
            if let Some(segment) = segment
                && let Some(open_start) = source[..start].rfind("<Text")
                && source[..open_start].contains("<Text")
                && let Some(close) = source[open_start..start].find('>')
                && source[open_start + close + 1..start].trim().is_empty()
            {
                let opening = &source[open_start..open_start + close];
                for field in ["fontSize", "lineHeight", "fontWeight"] {
                    if segment.get(field).is_none() {
                        continue;
                    }
                    let needle = format!("{field}=\"");
                    if let Some(attr_start) = find_prop(opening, &needle)
                        && let Some(attr_end) = opening[attr_start + needle.len()..].find('"')
                    {
                        entries.push(generated_entry(
                            range.start + open_start + attr_start,
                            range.start + open_start + attr_start + needle.len() + attr_end + 1,
                            node_id,
                            "styledTextSegments",
                            None,
                            None,
                            "raw-fallback",
                        ));
                    }
                }
            }
            entries.push(generated_entry(
                range.start + start,
                range.start + end,
                node_id,
                "characters",
                None,
                None,
                "exact",
            ));
            cursor = end;
        }
    }
    for (variable_id, token) in variable_tokens {
        let needle = format!("${token}");
        for start in match_indices(source, &needle) {
            if source[start + needle.len()..]
                .chars()
                .next()
                .is_some_and(is_token_character)
            {
                continue;
            }
            let prop_start = source[..start]
                .rfind(|character: char| character.is_whitespace() || character == '<')
                .map_or(start, |offset| offset + 1);
            let prop_end = source[start..]
                .find('"')
                .map_or(start + needle.len(), |offset| start + offset + 1);
            entries.push(generated_entry(
                range.start + prop_start,
                range.start + prop_end,
                node_id,
                "styledTextSegments",
                Some(variable_id.clone()),
                None,
                "variable-token",
            ));
        }
    }
    for (style_id, token) in style_tokens {
        let needle = format!("typography=\"{token}\"");
        for start in match_indices(source, &needle) {
            entries.push(generated_entry(
                range.start + start,
                range.start + start + needle.len(),
                node_id,
                "styledTextSegments",
                None,
                Some(style_id.clone()),
                "style-token",
            ));
        }
    }
}

fn find_text_span(source: &str, characters: &str, search_start: usize) -> Option<(usize, usize)> {
    let rendered = encode_jsx_text(characters);
    if let Some(start) = source[search_start..].find(&rendered) {
        let start = search_start + start;
        return Some((start, start + rendered.len()));
    }
    let fragments = characters
        .split(['\r', '\n', '\u{2028}', '\u{2029}'])
        .map(str::trim)
        .filter(|fragment| !fragment.is_empty())
        .map(encode_jsx_text)
        .collect::<Vec<_>>();
    let mut cursor = search_start;
    let mut start = None;
    let mut end = None;
    for fragment in fragments {
        let found = source[cursor..].find(&fragment)? + cursor;
        start.get_or_insert(found);
        end = Some(found + fragment.len());
        cursor = found + fragment.len();
    }
    Some((start?, end?))
}

fn asset_prop_range(opening: &str) -> Option<(usize, usize)> {
    for prop in ["src", "maskImage", "bg"] {
        let needle = format!("{prop}=\"");
        let Some(start) = find_prop(opening, &needle) else {
            continue;
        };
        let value_start = start + needle.len();
        let Some(value_end) = opening[value_start..].find('"') else {
            continue;
        };
        let end = value_start + value_end + 1;
        if prop != "bg" || opening[value_start..value_start + value_end].contains("url(") {
            return Some((start, end));
        }
    }
    None
}

fn asset_range_in_node_source(source: &str) -> Option<(usize, usize)> {
    let opening_start = source.find('<')?;
    let opening_end = opening_start + source[opening_start..].find('>')? + 1;
    let (start, end) = asset_prop_range(&source[opening_start..opening_end])?;
    Some((opening_start + start, opening_start + end))
}

fn token_prop_range(source: &str, needle: &str) -> Option<(usize, usize)> {
    let token_start = source.find(needle)?;
    let prop_start = source[..token_start]
        .rfind(|character: char| character.is_whitespace() || character == '<')
        .map_or(token_start, |offset| offset + 1);
    let prop_end = source[token_start..]
        .find('"')
        .map_or(token_start + needle.len(), |offset| {
            token_start + offset + 1
        });
    Some((prop_start, prop_end))
}

fn find_resource_id(
    value: &str,
    tokens: &BTreeMap<String, String>,
    prefix: char,
) -> Option<String> {
    tokens.iter().find_map(|(id, token)| {
        let needle = format!("{prefix}{token}");
        match_indices(value, &needle)
            .into_iter()
            .any(|start| {
                value[start + needle.len()..]
                    .chars()
                    .next()
                    .is_none_or(|character| !is_token_character(character))
            })
            .then(|| id.clone())
    })
}

fn find_prop(opening: &str, needle: &str) -> Option<usize> {
    match_indices(opening, needle).into_iter().find(|start| {
        *start > 0
            && opening[..*start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
    })
}

fn is_token_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-')
}

fn match_indices(source: &str, needle: &str) -> Vec<usize> {
    let mut result = Vec::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(needle) {
        let absolute = offset + found;
        result.push(absolute);
        offset = absolute + needle.len();
    }
    result
}

fn generated_entry(
    start: usize,
    end: usize,
    node_id: &str,
    property: &str,
    variable_id: Option<String>,
    style_id: Option<String>,
    resolution: &str,
) -> ProvenanceEntry {
    ProvenanceEntry {
        element_index: None,
        calculation: None,
        generated_property: None,
        generated_range: Some(GeneratedRange { start, end }),
        json_pointer: None,
        node_id: Some(node_id.to_owned()),
        property: Some(property.to_owned()),
        variable_id,
        style_id,
        asset_id: None,
        resolution: resolution.to_owned(),
    }
}

fn strip_markers(marked: &str) -> (String, Vec<(String, GeneratedRange)>) {
    let mut output = String::with_capacity(marked.len());
    let mut stack = Vec::<(String, usize)>::new();
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < marked.len() {
        let rest = &marked[cursor..];
        let next_start = rest.find(START);
        let next_end = rest.find(END);
        let next = match (next_start, next_end) {
            (Some(start), Some(end)) => start.min(end),
            (Some(start), None) => start,
            (None, Some(end)) => end,
            (None, None) => {
                output.push_str(rest);
                break;
            }
        };
        output.push_str(&rest[..next]);
        cursor += next;
        let is_start = marked[cursor..].starts_with(START);
        let prefix = if is_start { START } else { END };
        cursor += prefix.len();
        let Some(close) = marked[cursor..].find(CLOSE) else {
            output.push_str(&marked[cursor - prefix.len()..]);
            break;
        };
        let node_id = marked[cursor..cursor + close].to_owned();
        cursor += close + CLOSE.len_utf8();
        if is_start {
            stack.push((node_id, output.len()));
        } else if let Some(index) = stack.iter().rposition(|(id, _)| id == &node_id) {
            let (_, start) = stack.remove(index);
            ranges.push((
                node_id,
                GeneratedRange {
                    start,
                    end: output.len(),
                },
            ));
        }
    }
    ranges.sort_by(|left, right| {
        left.1
            .start
            .cmp(&right.1.start)
            .then_with(|| right.1.end.cmp(&left.1.end))
            .then_with(|| left.0.cmp(&right.0))
    });
    (output, ranges)
}

pub(crate) fn json_pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod line_height_calculation_tests {
    use super::*;
    use serde_json::json;

    fn entry(node: &str) -> ProvenanceEntry {
        ProvenanceEntry {
            element_index: None,
            calculation: None,
            generated_property: Some("lineHeight=\"26px\"".into()),
            generated_range: None,
            json_pointer: None,
            node_id: Some(node.into()),
            property: Some("lineHeight".into()),
            variable_id: None,
            style_id: None,
            asset_id: None,
            resolution: "raw-fallback".into(),
        }
    }

    #[test]
    fn percent_line_height_records_how_px_was_derived() {
        let snapshot: devup_mcp_figma::Snapshot = serde_json::from_value(json!({
            "fileKey":"k","roots":["t","b"],"diagnostics":[],"nodes":{
                "t":{"id":"t","type":"TEXT","fields":{"fontSize":16.25,"lineHeight":{"unit":"PERCENT","value":160}}},
                "b":{"id":"b","type":"TEXT","fields":{"fontSize":16,"lineHeight":{"unit":"PERCENT","value":150},
                    "boundVariables":{"fontSize":{"id":"v"}}}}}
        })).unwrap();
        let mut map = SourceMap::empty();
        map.entries = vec![entry("t"), entry("b")];
        map.describe_conversions(&snapshot);
        assert!(
            map.entries[0]
                .calculation
                .as_deref()
                .unwrap()
                .ends_with("= 26px")
        );
        assert!(
            map.entries[1]
                .calculation
                .as_deref()
                .unwrap()
                .contains("kept as a percentage")
        );
    }
}
