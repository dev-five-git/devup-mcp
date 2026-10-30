fn style_prop(name: &str) -> bool {
    LAYOUT_PROPS.contains(&name) || matches!(name,
        "bg" | "background" | "backgroundColor" | "backgroundImage" | "color" | "border" |
        "borderColor" | "borderWidth" | "borderRadius" | "rounded" | "boxShadow" | "opacity" |
        "textAlign" | "whiteSpace" | "wordBreak" | "zIndex" | "maskImage" | "objectPosition")
}

fn style_elements(tsx: &str) -> Vec<Value> {
    let mut elements = BTreeMap::<usize, (String, BTreeMap<String, String>)>::new();
    for attribute in generated_attributes(tsx).into_iter().filter(|attribute| style_prop(&attribute.name)) {
        let element = elements.entry(attribute.element).or_insert_with(|| (attribute.tag, BTreeMap::new()));
        element.1.insert(attribute.name, normalized(&attribute.source));
    }
    elements.into_values().map(|(tag, props)| json!({"tag": tag, "props": props})).collect()
}

fn style_value(source: &str) -> String {
    source.split_once('=').map_or(source, |(_, value)| value).chars()
        .filter(|character| !character.is_whitespace()).map(|character| if character == '\'' { '"' } else { character }).collect()
}

pub(super) fn generated_drift(record: &Value, root: &Path, sources: &[String]) -> Value {
    let Some(expected) = record["generatedStyles"].as_array() else {
        return json!({"state": "unverified", "confidence": "none", "reason": "Export predates the style baseline; re-export responsiveTsx."});
    };
    if expected.is_empty() {
        return json!({"state": "unverified", "confidence": "none", "reason": "The export has no comparable literal style props."});
    }
    let mut actual = Vec::new();
    let mut files = Vec::new();
    for source in sources {
        let path = root.join(source);
        if !path.starts_with(root) || source.split(['/', '\\']).any(|part| part == "..") { continue; }
        if !dunce::canonicalize(&path).ok().zip(dunce::canonicalize(root).ok()).is_some_and(|(path, root)| path.starts_with(root)) { continue; }
        if std::fs::metadata(&path).map_or(true, |metadata| metadata.len() > MAX_RECORD_BYTES) { continue; }
        let Ok(text) = std::fs::read_to_string(&path) else { continue; };
        files.push(source.clone());
        actual.extend(style_elements(&text).into_iter().map(|element| (source, element)));
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
        let Some(props) = element["props"].as_object() else { continue; };
        for (prop, generated) in props {
            let value = &current["props"][prop];
            if value.as_str().map(style_value) != generated.as_str().map(style_value) {
                let kind = if value.is_null() { "prop-removed" }
                    else if generated.as_str().is_some_and(|value| value.contains("{[")) && !value.as_str().is_some_and(|value| value.contains("{[")) { "responsive-array-collapsed" }
                    else { "prop-changed" };
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
    json!({"state": if findings.is_empty() { "matched-heuristic" } else { "drift" }, "confidence": "medium",
        "method": "Ordered style-bearing JSX elements across mapped URL files; wiring-only wrappers and attributes are ignored. Component extraction or reordering can require mapping review; not a runtime proof.",
        "files": files, "findings": findings})
}

#[cfg(test)]
mod generated_drift_tests {
    use super::*;

    #[test]
    fn style_baseline_ignores_wiring_but_preserves_responsive_arrays() {
        let generated = style_elements("<Box w={['360px', null, '1440px']}><Text color=\"$text\" /></Box>");
        let wired = style_elements("<Link href=\"/next\"><Box w={['360px', null, '1440px']} onClick={go} aria-label=\"Next\" key=\"next\"><Text color=\"$text\" /></Box></Link>");
        assert_eq!(generated, wired);
        assert_ne!(generated, style_elements("<Box w=\"1440px\" h=\"640px\"><Text /></Box>"));
    }
}
