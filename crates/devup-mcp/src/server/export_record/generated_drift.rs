fn style_prop(name: &str) -> bool {
    LAYOUT_PROPS.contains(&name)
        || matches!(
            name,
            "bg" | "background"
                | "backgroundColor"
                | "backgroundImage"
                | "color"
                | "border"
                | "borderColor"
                | "borderWidth"
                | "borderRadius"
                | "rounded"
                | "boxShadow"
                | "opacity"
                | "textAlign"
                | "whiteSpace"
                | "wordBreak"
                | "zIndex"
                | "maskImage"
                | "objectPosition"
        )
}

pub(super) fn style_elements(tsx: &str) -> Vec<Value> {
    let mut elements = BTreeMap::<usize, (String, BTreeMap<String, String>)>::new();
    for attribute in generated_attributes(tsx)
        .into_iter()
        .filter(|attribute| style_prop(&attribute.name))
    {
        let element = elements
            .entry(attribute.element)
            .or_insert_with(|| (attribute.tag, BTreeMap::new()));
        element
            .1
            .insert(attribute.name, normalized(&attribute.source));
    }
    elements
        .into_values()
        .map(|(tag, props)| json!({"tag": tag, "props": props}))
        .collect()
}

fn style_value(source: &str) -> String {
    source
        .split_once('=')
        .map_or(source, |(_, value)| value)
        .chars()
        .filter(|character| !character.is_whitespace())
        .map(|character| if character == '\'' { '"' } else { character })
        .collect()
}

pub(super) fn generated_drift(record: &Value, root: &Path, sources: &[String]) -> Value {
    // Rebind extracted/reordered source by real AST evidence before considering
    // the legacy positional review fallback. Exact complete files retain the
    // older source-matched label below.
    let mut unresolved = Value::Null;
    if record["generatedSource"].as_str().is_some_and(|generated| {
        !sources.iter().any(|file| {
            std::fs::read_to_string(root.join(file)).is_ok_and(|source| source == generated)
        })
    }) && let Some(evidence) = semantic_drift(record, root, sources)
    {
        if evidence["mappings"]
            .as_array()
            .is_some_and(|mappings| !mappings.is_empty())
            || evidence["unresolved"].as_array().is_none_or(Vec::is_empty)
        {
            return evidence;
        }
        // Anonymous layout-only elements have no semantic identity. Keep
        // positional style differences as explicit review candidates; never
        // silently turn absence of a mapping into a clean style baseline.
        unresolved = evidence["unresolved"].clone();
    }
    let Some(expected) = record["generatedStyles"].as_array() else {
        return json!({"state": "unverified", "confidence": "none", "reason": "Export predates the style baseline; re-export responsiveTsx."});
    };
    if expected.is_empty() {
        return json!({"state": "unverified", "confidence": "none", "reason": "The export has no comparable literal style props."});
    }
    let mut actual = Vec::new();
    let mut files = Vec::new();
    let mut exact_source_matches = 0usize;
    for source in sources {
        let path = root.join(source);
        if !path.starts_with(root) || source.split(['/', '\\']).any(|part| part == "..") {
            continue;
        }
        if !dunce::canonicalize(&path)
            .ok()
            .zip(dunce::canonicalize(root).ok())
            .is_some_and(|(path, root)| path.starts_with(root))
        {
            continue;
        }
        if std::fs::metadata(&path).map_or(true, |metadata| metadata.len() > MAX_RECORD_BYTES) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if record["generatedSource"].as_str() == Some(text.as_str()) {
            exact_source_matches += 1;
        }
        files.push(source.clone());
        actual.extend(
            style_elements(&text)
                .into_iter()
                .map(|element| (source, element)),
        );
    }
    if files.is_empty() {
        return json!({"state": "unverified", "confidence": "none", "reason": "No mapped URL source files were readable."});
    }
    let mut findings = Vec::new();
    for (index, element) in expected.iter().enumerate() {
        let Some((file, current)) = actual.get(index) else {
            findings.push(json!({"kind": "element-removed", "element": index, "files": files, "generated": element}));
            continue;
        };
        let Some(props) = element["props"].as_object() else {
            continue;
        };
        for (prop, generated) in props {
            let value = &current["props"][prop];
            if value.as_str().map(style_value) != generated.as_str().map(style_value) {
                let kind = if value.is_null() {
                    "prop-removed"
                } else if generated.as_str().is_some_and(|value| value.contains("{["))
                    && !value.as_str().is_some_and(|value| value.contains("{["))
                {
                    "responsive-array-collapsed"
                } else {
                    "prop-changed"
                };
                findings.push(json!({"kind": kind, "file": file, "element": index, "prop": prop, "generated": generated, "actual": value}));
            }
        }
        for (prop, value) in current["props"].as_object().into_iter().flatten() {
            if !props.contains_key(prop) {
                findings.push(json!({"kind": "style-added", "file": file, "element": index, "prop": prop, "actual": value}));
            }
        }
    }
    for (file, element) in actual.iter().skip(expected.len()) {
        findings.push(json!({"kind": "styled-element-added", "file": file, "actual": element}));
    }
    let source_matched = files.len() == 1 && exact_source_matches == 1;
    json!({"state": if source_matched { "source-matched" } else { "unverified" }, "confidence": if source_matched { "high" } else { "low" },
        "candidateOnly": !source_matched,"runtimeVerified":false,"mappings":[],"unresolved":unresolved,
        "reason":if source_matched { "The complete source is identical to the recorded generated source; browser behavior remains unverified." } else { "Source identity is unavailable after edits or component extraction. Findings are positional review candidates, not verified node drift." },
        "method": "Ordered style-bearing JSX comparison; wiring-only wrappers and attributes are ignored. Only an identical complete generated file establishes source identity. Review candidate mapping after extraction or reordering; never infer runtime DOM identities from ordinals.",
        "files": files, "findings": findings})
}

#[cfg(test)]
mod generated_drift_tests {
    use super::*;

    #[test]
    fn style_baseline_ignores_wiring_but_preserves_responsive_arrays() {
        let generated =
            style_elements("<Box w={['360px', null, '1440px']}><Text color=\"$text\" /></Box>");
        let wired = style_elements(
            "<Link href=\"/next\"><Box w={['360px', null, '1440px']} onClick={go} aria-label=\"Next\" key=\"next\"><Text color=\"$text\" /></Box></Link>",
        );
        assert_eq!(generated, wired);
        assert_ne!(
            generated,
            style_elements("<Box w=\"1440px\" h=\"640px\"><Text /></Box>")
        );
    }

    #[test]
    fn changed_source_is_a_review_candidate_without_verified_node_identity() {
        let root = std::env::temp_dir().join(format!(
            "devup-drift-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let code = "<Box w=\"40px\"><Text color=\"$text\" /></Box>";
        let record = json!({"generatedSource":code,"generatedStyles":style_elements(code)});
        let sources = vec!["Screen.tsx".into()];
        std::fs::write(root.join("Screen.tsx"), code).unwrap();
        assert_eq!(
            generated_drift(&record, &root, &sources)["state"],
            "source-matched"
        );
        std::fs::write(root.join("Screen.tsx"), code.replace("40px", "80px")).unwrap();
        let drift = generated_drift(&record, &root, &sources);
        assert_eq!(drift["state"], "unverified");
        assert_eq!(drift["candidateOnly"], true);
        assert_eq!(drift["runtimeVerified"], false);
        assert!(!drift["findings"].as_array().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
