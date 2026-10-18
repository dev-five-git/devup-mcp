/// Compare the generated text, removing only known asset references in codegen's
/// src and CSS URL forms. Literal text, layout, imports and component names stay
/// significant. Separate segments avoid collisions with a placeholder string.
fn generated_structure(code: &str, paths: &[String]) -> Vec<String> {
    let mut ranges = Vec::new();
    for attribute in ["src", "bg", "backgroundImage", "maskImage"] {
        let opening = format!("{attribute}=\"");
        for (at, _) in code.match_indices(&opening) {
            // An attribute name must start at a word boundary, not inside an
            // unrelated prop. Codegen quotes literal text containing quotes.
            if !code[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
            {
                continue;
            }
            let start = at + opening.len();
            let Some(end) = code[start..].find('"') else {
                continue;
            };
            let value = &code[start..start + end];
            for path in paths {
                if attribute == "src" {
                    if value == path {
                        ranges.push((start, start + path.len()));
                    }
                    continue;
                }
                // Preserve all paint layers and positions while recognizing
                // every image, including URLs after gradients or other images.
                for (prefix, suffix) in [("url(", ")"), ("url('", "')")] {
                    let reference = format!("{prefix}{path}{suffix}");
                    for (offset, _) in value.match_indices(&reference) {
                        let asset_start = start + offset + prefix.len();
                        ranges.push((asset_start, asset_start + path.len()));
                    }
                }
            }
        }
    }
    ranges.sort_unstable();
    ranges.dedup();
    let mut segments = Vec::new();
    let mut previous = 0;
    for (start, end) in ranges {
        if start < previous {
            continue;
        }
        segments.push(code[previous..start].to_owned());
        previous = end;
    }
    segments.push(code[previous..].to_owned());
    segments
}

fn annotate_identical_frames(frames: &mut [Value], payload: &CollectedPayload, per_node: bool) {
    let paths = devup_mcp_figma::discover_asset_manifest(&payload.snapshot)
        .assets
        .iter()
        .filter_map(|asset| {
            match asset
                .field
                .strip_prefix("fills/")
                .and_then(|n| n.parse::<usize>().ok())
            {
                Some(index) => devup_mcp_devup_ui::codegen::image_fill_path(
                    &payload.snapshot,
                    &asset.node_id,
                    index,
                    per_node,
                ),
                None => devup_mcp_devup_ui::codegen::asset_path(
                    &payload.snapshot,
                    &asset.node_id,
                    per_node,
                ),
            }
        })
        .map(|path| host_safe_asset_path(&path))
        .collect::<Vec<_>>();
    let mut seen = BTreeMap::new();
    for frame in frames {
        // Fidelity warnings may make a frame partial without changing the
        // generated text. Compare exactly the outputs that exist; the key and
        // comparedOutputs prevent missing outputs from implying equivalence.
        let generated = ["tsx", "componentTsx"]
            .into_iter()
            .filter_map(|field| frame[field].as_str().map(|code| (field, code.to_owned())))
            .collect::<Vec<_>>();
        if generated.is_empty() {
            continue;
        }
        let key = generated
            .iter()
            .map(|(field, code)| (*field, generated_structure(code, &paths)))
            .collect::<Vec<_>>();
        if let Some((node_id, original)) = seen.get(&key) {
            frame["structurallyIdenticalTo"] = json!(node_id);
            frame["differsOnly"] = if original == &generated {
                json!([])
            } else {
                json!(["assetReferences"])
            };
            frame["comparedOutputs"] = json!(
                generated
                    .iter()
                    .map(|(field, _)| *field)
                    .collect::<Vec<_>>()
            );
        } else {
            seen.insert(key, (frame["nodeId"].clone(), generated));
        }
    }
}
