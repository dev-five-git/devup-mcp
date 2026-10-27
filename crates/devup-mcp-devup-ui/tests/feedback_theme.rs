use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    theme::{
        Completeness, ThemeScope, TokenUsage, VariableSnapshot, generate_devup_json,
        missing_token_diff, parse_project_theme, restrict_devup_json,
    },
};
use devup_mcp_figma::{Snapshot, UpstreamResult};
use serde_json::{Value, json};

fn resources() -> VariableSnapshot {
    serde_json::from_value(json!({
        "collections":[{"id":"c","name":"Theme","defaultModeId":"m","modes":[{"modeId":"m","name":"Default"}]}],
        "variables":[
            {"id":"brand","name":"primary","resolvedType":"COLOR","variableCollectionId":"c","valuesByMode":{"m":{"r":1,"g":0,"b":0,"a":1}}},
            {"id":"unused","name":"unused","resolvedType":"COLOR","variableCollectionId":"c","valuesByMode":{"m":{"r":0,"g":1,"b":0,"a":1}}},
            {"id":"size","name":"bodySize","resolvedType":"FLOAT","variableCollectionId":"c","valuesByMode":{"m":16}}
        ],
        "styles":[
            {"id":"body","name":"Body","styleType":"TEXT","value":{"fontName":{"family":"Inter","style":"Regular"},"fontSize":16,"lineHeight":{"unit":"PERCENT","value":140},"boundVariables":{"fontSize":{"id":"size"}}}},
            {"id":"title","name":"Heading","styleType":"TEXT","value":{"fontName":{"family":"Inter","style":"Bold"},"fontSize":32,"lineHeight":{"unit":"PERCENT","value":150}}}
        ],"localComplete":true,"usedRemoteComplete":true
    })).unwrap()
}

#[test]
fn unreadable_style_is_isolated_and_percentage_fallback_survives() {
    let mut snapshot = resources();
    snapshot.styles.push(serde_json::from_value(json!({"id":"broken","name":"Unreadable heading","styleType":"TEXT","value":{"lineHeight":{"$error":"not read"}}})).unwrap());
    snapshot.styles.push(serde_json::from_value(json!({"id":"missing-size","name":"Caption","styleType":"TEXT","value":{"lineHeight":{"unit":"PERCENT","value":140}}})).unwrap());
    let output = generate_devup_json(&snapshot, ThemeScope::File).unwrap();
    let value: Value = serde_json::from_str(&output.json).unwrap();
    assert_eq!(value["theme"]["typography"]["body"]["lineHeight"], "140%");
    assert_eq!(
        value["theme"]["typography"]["caption"]["lineHeight"],
        "140%"
    );
    assert_eq!(
        value["theme"]["typography"]["heading"]["lineHeight"],
        "48px"
    );
    assert!(
        value["theme"]["typography"]
            .get("unreadableHeading")
            .is_none()
    );
    assert_eq!(output.skipped_styles[0].name, "Unreadable heading");
    assert!(
        output
            .diagnostics
            .iter()
            .any(
                |diagnostic| diagnostic.resource_id.as_deref() == Some("broken")
                    && diagnostic.message.contains("Unreadable heading")
            )
    );
    assert_eq!(output.completeness, Completeness::ResolvedValuesOnly);
}

#[test]
fn used_only_reads_jsx_attributes_and_includes_typography_dependencies() {
    let mut output = generate_devup_json(&resources(), ThemeScope::File).unwrap();
    let mut usage = TokenUsage::default();
    usage.add_tsx(r#"// <Box color="$unused" />
        export const X = <Text color={["$primary", null, "$primary"]} typography="body">$unused</Text>"#).unwrap();
    restrict_devup_json(&mut output, &usage).unwrap();
    let theme: Value = serde_json::from_str(&output.json).unwrap();
    assert!(theme["theme"]["colors"]["default"].get("primary").is_some());
    assert!(theme["theme"]["colors"]["default"].get("unused").is_none());
    assert_eq!(theme["theme"]["length"]["default"]["bodySize"], "16px");
    assert!(theme["theme"]["typography"].get("heading").is_none());
    assert_eq!(output.counts.variables, 2);
    assert_eq!(output.counts.styles, 1);
    assert!(output.source_map.entries.iter().all(|entry| {
        theme
            .pointer(entry.json_pointer.as_deref().unwrap())
            .is_some()
    }));
}

#[test]
fn diff_preserves_existing_values_and_reports_typography_conflict() {
    let output = generate_devup_json(&resources(), ThemeScope::File).unwrap();
    let existing = parse_project_theme(r##"{"theme":{"colors":{"default":{"primary":"#123456"}},"typography":{"body":{"fontSize":"18px","lineHeight":"150%"}}}}"##).unwrap();
    let mut usage = TokenUsage::default();
    usage
        .add_tsx(r#"<Text color="$primary" typography="body" />"#)
        .unwrap();
    let diff = missing_token_diff(&output.json, &usage, &existing).unwrap();
    assert!(
        diff["additions"]["theme"]["colors"]["default"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert!(
        diff["additions"]["theme"]["typography"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert!(
        diff["additions"]["theme"]["length"]["default"]
            .as_object()
            .unwrap()
            .is_empty(),
        "an existing body token does not need Figma body's dependencies"
    );
    assert_eq!(diff["existingTypographyConflicts"][0]["token"], "body");
    assert_eq!(
        diff["existingTypographyConflicts"][0]["existingValue"]["fontSize"],
        "18px"
    );
    assert_eq!(existing.colors["default"]["primary"], "#123456");
    assert_eq!(diff["visualFidelity"], "unverified");
}

#[test]
fn diff_adds_missing_style_dependency_and_names_uncollected_references() {
    let output = generate_devup_json(&resources(), ThemeScope::File).unwrap();
    let existing = parse_project_theme("{\"theme\":{}}").unwrap();
    let mut usage = TokenUsage::default();
    usage
        .add_tsx(r#"<Text typography={["body", null, "missingStyle"]} color="$notCollected" />"#)
        .unwrap();
    let diff = missing_token_diff(&output.json, &usage, &existing).unwrap();
    assert_eq!(
        diff["additions"]["theme"]["length"]["default"]["bodySize"],
        "16px"
    );
    assert_eq!(diff["state"], "partial");
    assert_eq!(diff["unresolvedTokens"].as_array().unwrap().len(), 2);
}

#[test]
fn used_export_resolves_alias_closure_and_keeps_style_dependencies_outside_used_id_hints() {
    let mut snapshot = resources();
    snapshot.used_variable_ids = vec!["brand".into()];
    snapshot.variables[0]
        .values_by_mode
        .insert("m".into(), json!({"type":"VARIABLE_ALIAS","id":"unused"}));
    let mut usage = TokenUsage::default();
    usage
        .add_tsx(r#"<Text color="$primary" typography="body" />"#)
        .unwrap();
    let output = devup_mcp_devup_ui::theme::generate_used_devup_json(&snapshot, &usage).unwrap();
    let theme: Value = serde_json::from_str(&output.json).unwrap();
    assert_eq!(theme["theme"]["colors"]["default"]["primary"], "#00ff00");
    assert!(theme["theme"]["colors"]["default"].get("unused").is_none());
    assert_eq!(theme["theme"]["length"]["default"]["bodySize"], "16px");
    assert!(output.unresolved_variables.is_empty());
}

#[test]
fn breakpoint_only_attribute_tokens_survive_used_export() {
    let mut usage = TokenUsage::default();
    usage
        .add_tsx(r#"<Text color={[null, null, "$primary"]} typography={[null, "heading"]} />"#)
        .unwrap();
    let output = devup_mcp_devup_ui::theme::generate_used_devup_json(&resources(), &usage).unwrap();
    let theme: Value = serde_json::from_str(&output.json).unwrap();
    assert!(theme["theme"]["colors"]["default"].get("primary").is_some());
    assert!(theme["theme"]["typography"].get("heading").is_some());
    assert!(theme["theme"]["typography"].get("body").is_none());
}

#[test]
fn dependency_cycles_terminate_and_missing_dependencies_are_reported() {
    let source = r#"{"theme":{"typography":{"body":{"fontSize":"$a"}},"length":{"default":{"a":"$b","b":"$a"}}}}"#;
    let mut usage = TokenUsage::default();
    usage.add_tsx(r#"<Text typography="body" />"#).unwrap();
    let diff = missing_token_diff(source, &usage, &Default::default()).unwrap();
    assert_eq!(diff["additions"]["theme"]["length"]["default"]["b"], "$a");
    let missing = missing_token_diff(
        r#"{"theme":{"typography":{"body":{"fontSize":"$missing"}}}}"#,
        &usage,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(missing["state"], "partial");
    assert_eq!(missing["unresolvedTokens"][0]["token"], "$missing");
}

fn text(weight: u32) -> Snapshot {
    serde_json::from_value(json!({"fileKey":"test","roots":["t"],"diagnostics":[],"nodes":{
        "t":{"id":"t","type":"TEXT","fields":{"name":"Title","textStyleId":"title","characters":"Example",
            "fontSize":32,"fontWeight":weight,"lineHeight":{"unit":"PERCENT","value":150},
            "layoutSizingHorizontal":"HUG","layoutSizingVertical":"HUG","textTruncation":"DISABLED"}}}})).unwrap()
}

#[test]
fn collected_equal_typography_properties_are_elided_but_local_overrides_stay() {
    let raw = serde_json::to_value(resources()).unwrap();
    let result = UpstreamResult { raw };
    let options = CodegenOptions::default().with_resource_results(Some(&result), Some(&result));
    let output = generate_component(&text(700), "t", &options).unwrap();
    assert!(output.tsx.contains("typography=\"heading\""));
    for property in ["fontSize=", "fontWeight=", "lineHeight="] {
        assert!(!output.tsx.contains(property), "{}", output.tsx);
    }
    let overridden = generate_component(&text(400), "t", &options).unwrap();
    assert!(
        overridden.tsx.contains("fontWeight=\"400\""),
        "{}",
        overridden.tsx
    );
    let names_only = CodegenOptions {
        text_style_tokens: options.text_style_tokens,
        ..Default::default()
    };
    assert!(
        generate_component(&text(700), "t", &names_only)
            .unwrap()
            .tsx
            .contains("fontSize=\"32px\"")
    );
}

#[test]
fn responsive_typography_elides_values_the_token_defines_at_any_slot() {
    let options = CodegenOptions {
        text_style_tokens: [("title".into(), "heading".into())].into(),
        typography_values: [(
            "heading".into(),
            json!([{"fontSize":"16px","fontWeight":700},null,{"fontSize":"32px","fontWeight":700}]),
        )]
        .into(),
        ..Default::default()
    };
    let output = generate_component(&text(700), "t", &options).unwrap();
    assert!(!output.tsx.contains("fontWeight="), "{}", output.tsx);
    assert!(!output.tsx.contains("fontSize="), "{}", output.tsx);
    let mut overridden = text(700);
    overridden
        .nodes
        .get_mut("t")
        .unwrap()
        .fields
        .insert("fontSize".into(), json!(24));
    let overridden = generate_component(&overridden, "t", &options).unwrap();
    assert!(
        overridden.tsx.contains("fontSize=\"24px\""),
        "{}",
        overridden.tsx
    );
    assert!(
        generate_component(&text(400), "t", &options)
            .unwrap()
            .tsx
            .contains("fontWeight=\"400\"")
    );
}

#[test]
fn a_different_rich_text_token_elides_equal_metrics_without_losing_overrides() {
    let result = UpstreamResult {
        raw: serde_json::to_value(resources()).unwrap(),
    };
    let options = CodegenOptions::default().with_resource_results(Some(&result), Some(&result));
    let mut snapshot = text(700);
    let fields = &mut snapshot.nodes.get_mut("t").unwrap().fields;
    for name in ["fontSize", "fontWeight", "lineHeight", "textStyleId"] {
        fields.remove(name);
    }
    fields.insert("styledTextSegments".into(), json!([
        {"characters":"Title","textStyleId":"title","fontSize":32,"fontWeight":700,"lineHeight":{"unit":"PERCENT","value":150}},
        {"characters":" longer body paragraph","textStyleId":"body","fontSize":16,"fontWeight":400,"lineHeight":{"unit":"PERCENT","value":140}}
    ]));
    fields.insert("characters".into(), json!("Title longer body paragraph"));
    let output = generate_component(&snapshot, "t", &options).unwrap();
    let tag = output
        .tsx
        .split("<Text")
        .find(|tag| tag.contains("typography=\"heading\""))
        .unwrap()
        .split('>')
        .next()
        .unwrap();
    for field in ["fontSize=", "fontWeight=", "lineHeight="] {
        assert!(!tag.contains(field), "{tag}");
    }
}

#[test]
fn six_award_cards_keep_main_axis_fill_and_cross_axis_hug() {
    let cards = (0..6).map(|i| format!("card{i}")).collect::<Vec<_>>();
    let mut nodes = serde_json::Map::new();
    nodes.insert("root".into(), json!({"id":"root","type":"FRAME","fields":{"name":"Awards","width":1260,"height":40,
        "layoutMode":"HORIZONTAL","layoutSizingHorizontal":"FIXED","layoutSizingVertical":"HUG","counterAxisAlignItems":"CENTER","childrenIds":cards}}));
    for (i, id) in cards.iter().enumerate() {
        let child = format!("text{i}");
        nodes.insert(id.clone(), json!({"id":id,"type":"FRAME","fields":{"name":"Award","parentId":"root","width":210,"height":40,"maxWidth":null,"maxHeight":null,
            "layoutMode":"VERTICAL","layoutSizingHorizontal":"FILL","layoutSizingVertical":"HUG","childrenIds":[child]}}));
        nodes.insert(child.clone(), json!({"id":child,"type":"TEXT","fields":{"characters":"Award","parentId":id,"width":40,"height":20,"fontSize":16,
            "layoutSizingHorizontal":"HUG","layoutSizingVertical":"HUG","textAutoResize":"WIDTH_AND_HEIGHT"}}));
    }
    for node in nodes.values_mut() {
        if node["fields"].get("layoutMode").is_some() {
            node["fields"]["inferredAutoLayout"] = node["fields"].clone();
        }
    }
    let snapshot: Snapshot = serde_json::from_value(
        json!({"fileKey":"test","roots":["root"],"diagnostics":[],"nodes":nodes}),
    )
    .unwrap();
    let output = generate_component(&snapshot, "root", &CodegenOptions::default()).unwrap();
    assert_eq!(
        output.tsx.matches("flex=\"1\"").count(),
        6,
        "{}",
        output.tsx
    );
    assert!(!output.tsx.contains("210px"), "{}", output.tsx);
    for id in cards {
        let entry = output
            .source_map
            .entries
            .iter()
            .find(|entry| entry.node_id.as_deref() == Some(&id) && entry.property.is_none())
            .unwrap();
        let range = entry.generated_range.as_ref().unwrap();
        let opening = output.tsx[range.start..range.end]
            .split('>')
            .next()
            .unwrap();
        assert!(opening.contains("flex=\"1\""), "{opening}");
        assert!(
            !opening.contains("h="),
            "HUG height stays intrinsic: {opening}"
        );
    }
}
