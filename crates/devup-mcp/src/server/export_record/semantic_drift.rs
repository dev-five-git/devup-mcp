fn semantic_drift(record: &Value, root: &Path, sources: &[String]) -> Option<Value> {
    let baseline = record["sourceSyncState"]["designBaseline"]
        .as_str()
        .or_else(|| record["generatedSource"].as_str())?;
    let original = super::source_identity::inventory(baseline).ok()?;
    let mut inventory = Vec::new();
    for file in sources {
        let path = dunce::canonicalize(root.join(file)).ok()?;
        let canonical_root = dunce::canonicalize(root).ok()?;
        if !path.starts_with(canonical_root)
            || std::fs::metadata(&path).ok()?.len() > MAX_RECORD_BYTES
        {
            return None;
        }
        let source = std::fs::read_to_string(path).ok()?;
        inventory.push((
            file.clone(),
            super::source_identity::inventory(&source).ok()?,
        ));
    }
    let matches = super::source_identity::match_elements(&original, &inventory);
    let mut mappings = Vec::new();
    let mut unresolved = Vec::new();
    let mut findings = Vec::new();
    let mut structural = false;
    for (before, matched) in original.iter().zip(matches) {
        let matched = match matched {
            Ok(m) => m,
            Err(reason) => {
                unresolved.push(reason);
                continue;
            }
        };
        structural |= matched.confidence != "high";
        let props = |element: &super::source_identity::Element| {
            element
                .attributes
                .iter()
                .filter(|a| style_prop(&a.name))
                .map(|a| (a.name.clone(), a.source.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        let expected = props(before);
        let actual = props(&matched.element);
        mappings.push(json!({"baselineElement":before.index,"file":matched.file,"elementIndex":matched.element.index,"confidence":matched.confidence,"reasons":matched.reasons,"runtimeVerified":false}));
        for (prop, value) in &expected {
            let current = actual.get(prop);
            if current.map(|v| style_value(v)) != Some(style_value(value)) {
                findings.push(json!({"kind":if current.is_none(){"prop-removed"}else if value.contains("{[")&&!current.is_some_and(|v|v.contains("{[")){"responsive-array-collapsed"}else{"prop-changed"},"file":matched.file,"element":matched.element.index,"baselineElement":before.index,"prop":prop,"generated":value,"actual":current,"mappingConfidence":matched.confidence}));
            }
        }
        for (prop, value) in actual {
            if !expected.contains_key(&prop) {
                findings.push(json!({"kind":"style-added","file":matched.file,"element":matched.element.index,"prop":prop,"actual":value}));
            }
        }
    }
    Some(
        json!({"state":if unresolved.is_empty(){if findings.is_empty(){"semantic-source-matched"}else{"mapped-style-differences"}}else{"unverified"},"designBaselineProvenance":record["sourceSyncState"]["incomingProvenance"],"method":"AST content, semantic attributes and supported child/ancestor structure; never ordinal identity alone.","runtimeVerified":false,"candidateOnly":!unresolved.is_empty(),"confidence":if !unresolved.is_empty(){"low"}else if structural{"medium"}else{"high"},"mappings":mappings,"unresolved":unresolved,"findings":findings}),
    )
}

#[cfg(test)]
mod semantic_mapping_tests {
    use super::*;
    #[test]
    fn extracted_reordered_files_match_copy_and_attribute_style_changes_to_the_right_file() {
        let root = std::env::temp_dir().join(format!(
            "devup-semantic-drift-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let baseline = "<Box><Text color=\"$primary\">Alpha</Text><Text color=\"$secondary\">Beta</Text></Box>";
        let record = json!({"generatedSource":baseline,"generatedStyles":style_elements(baseline)});
        std::fs::write(
            root.join("Alpha.tsx"),
            "export function Alpha(){return <Text color=\"$primary\" onClick={go}>Alpha</Text>}",
        )
        .unwrap();
        std::fs::write(
            root.join("Beta.tsx"),
            "export function Beta(){return <Text color=\"$accent\">Beta</Text>}",
        )
        .unwrap();
        let drift = generated_drift(&record, &root, &["Beta.tsx".into(), "Alpha.tsx".into()]);
        assert_eq!(drift["state"], "unverified");
        assert_eq!(drift["runtimeVerified"], false);
        assert_eq!(drift["findings"].as_array().unwrap().len(), 1, "{drift}");
        assert_eq!(drift["findings"][0]["file"], "Beta.tsx");
        assert_eq!(drift["findings"][0]["baselineElement"], 2);
        assert!(
            drift["mappings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|mapping| mapping["file"] == "Alpha.tsx" && mapping["confidence"] == "high")
        );
        std::fs::write(
            root.join("AlphaCopy.tsx"),
            "export function AlphaCopy(){return <Text color=\"$primary\">Alpha</Text>}",
        )
        .unwrap();
        let ambiguous = generated_drift(
            &record,
            &root,
            &["Alpha.tsx".into(), "AlphaCopy.tsx".into()],
        );
        assert!(ambiguous["mappings"].as_array().unwrap().is_empty());
        assert!(!ambiguous["unresolved"].as_array().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
