fn rewrite_asset_references(code: &str, paths: &BTreeMap<String, String>) -> String {
    // One pass prevents a replacement from becoming another replacement's input.
    let mut result = String::with_capacity(code.len());
    let mut cursor = 0;
    while cursor < code.len() {
        let rest = &code[cursor..];
        let matched = paths.iter().find(|(old, _)| {
            rest.starts_with(old.as_str())
                && (cursor == 0
                    || matches!(
                        code.as_bytes()[cursor - 1],
                        b'"' | b'\'' | b'(' | b' ' | b','
                    ))
                && rest
                    .as_bytes()
                    .get(old.len())
                    .is_none_or(|next| matches!(next, b'"' | b'\'' | b')' | b' ' | b','))
        });
        if let Some((old, new)) = matched {
            result.push_str(new);
            cursor += old.len();
        } else {
            let character = rest.chars().next().expect("nonempty remainder");
            result.push(character);
            cursor += character.len_utf8();
        }
    }
    result
}

fn rewrite_result_asset_references(value: &mut Value, paths: &BTreeMap<String, String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(
                    key.as_str(),
                    "tsx"
                        | "componentTsx"
                        | "responsiveTsx"
                        | "generatedProperty"
                        | "generatedSource"
                        | "srcSet"
                ) {
                    match value {
                        Value::String(code) => {
                            *code = if key == "srcSet" {
                                rewrite_asset_references(code, paths)
                            } else {
                                rewrite_asset_references(&with_host_safe_asset_paths(code), paths)
                            }
                        }
                        Value::Array(items) => {
                            for item in items {
                                if let Value::String(code) = item {
                                    *code = rewrite_asset_references(
                                        &with_host_safe_asset_paths(code),
                                        paths,
                                    );
                                } else {
                                    rewrite_result_asset_references(item, paths);
                                }
                            }
                        }
                        _ => rewrite_result_asset_references(value, paths),
                    }
                } else if matches!(key.as_str(), "assetPath" | "path") {
                    if let Value::String(path) = value
                        && let Some(replacement) = paths.get(path)
                    {
                        *path = replacement.clone();
                    }
                } else if !matches!(key.as_str(), "rawSnapshot" | "rawPayload" | "originalValue") {
                    rewrite_result_asset_references(value, paths);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                rewrite_result_asset_references(value, paths);
            }
        }
        _ => {}
    }
}
