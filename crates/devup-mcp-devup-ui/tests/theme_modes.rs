use devup_mcp_devup_ui::theme::{ThemeScope, VariableSnapshot, generate_devup_json};
use serde_json::{Value, json};

fn incident() -> VariableSnapshot {
    serde_json::from_value(json!({
        "collections": [
            {"id":"primitives","name":"Primitives","defaultModeId":"value","modes":[{"modeId":"value","name":"value"}]},
            {"id":"leftover","name":"Leftover","defaultModeId":"one","modes":[{"modeId":"one","name":"Mode 1"}]},
            {"id":"site","name":"Site","defaultModeId":"light","modes":[{"modeId":"light","name":"Light"},{"modeId":"dark","name":"Dark"}]},
            {"id":"sds","name":"Simple Design System","remote":true,"defaultModeId":"sl","modes":[{"modeId":"sl","name":"SDS Light"},{"modeId":"sd","name":"SDS Dark"}]},
            {"id":"empty","name":"Empty","defaultModeId":"empty","modes":[{"modeId":"empty","name":"Empty"}]}
        ],
        "variables": [
            {"id":"primitive","name":"white","resolvedType":"COLOR","variableCollectionId":"primitives","valuesByMode":{"value":{"r":1,"g":1,"b":1}}},
            {"id":"leftover","name":"accent","resolvedType":"COLOR","variableCollectionId":"leftover","valuesByMode":{"one":{"r":0,"g":0,"b":1}}},
            {"id":"bg","name":"background","resolvedType":"COLOR","variableCollectionId":"site","valuesByMode":{"light":{"type":"VARIABLE_ALIAS","id":"primitive"},"dark":{"r":0,"g":0,"b":0}}}
        ],
        "usedRemoteVariables":[
            {"id":"sds-icon","name":"sdsIcon","resolvedType":"COLOR","variableCollectionId":"sds","valuesByMode":{"sl":{"r":1,"g":1,"b":1},"sd":{"r":0,"g":0,"b":0}}}
        ],
        "usedVariableIds":["bg"],"styles":[],"localComplete":true,"usedRemoteComplete":true
    })).unwrap()
}

#[test]
fn incident_exports_only_site_themes_with_drop_reasons() {
    let output = generate_devup_json(&incident(), ThemeScope::File).unwrap();
    let theme: Value = serde_json::from_str(&output.json).unwrap();
    assert_eq!(
        theme["theme"]["colors"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["light", "dark"]
    );
    assert_eq!(theme["theme"]["colors"]["light"]["accent"], "#0000ff");
    assert_eq!(theme["theme"]["colors"]["light"]["white"], "#ffffff");
    let output = serde_json::to_value(output).unwrap();
    let dropped = output["themeModesDropped"].as_array().unwrap();
    assert!(
        dropped.iter().any(|mode| mode["collectionId"] == "sds"
            && mode["reason"] == "unreferenced-remote-collection")
    );
    assert!(
        dropped
            .iter()
            .any(|mode| mode["collectionId"] == "empty"
                && mode["reason"] == "no-defined-color-values")
    );
    assert!(
        dropped
            .iter()
            .any(|mode| mode["modeName"] == "Mode 1" && mode["reason"] == "single-default-mode")
    );
}

#[test]
fn referenced_library_modes_merge_by_meaning() {
    let mut snapshot = incident();
    snapshot.used_variable_ids.push("sds-icon".into());
    let output = generate_devup_json(&snapshot, ThemeScope::File).unwrap();
    let theme: Value = serde_json::from_str(&output.json).unwrap();
    assert_eq!(
        theme["theme"]["colors"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["light", "dark"]
    );
    assert_eq!(theme["theme"]["colors"]["dark"]["sdsIcon"], "#000000");
}

#[test]
fn used_only_drops_unused_modes_and_missing_only_preserves_existing_values() {
    use devup_mcp_devup_ui::theme::{
        TokenUsage, generate_used_devup_json, missing_token_diff, parse_project_theme,
    };
    let mut usage = TokenUsage::default();
    usage.add_tsx(r#"<Box bg="$background" />"#).unwrap();
    let output = generate_used_devup_json(&incident(), &usage).unwrap();
    let theme: Value = serde_json::from_str(&output.json).unwrap();
    assert_eq!(
        theme["theme"]["colors"],
        json!({"light":{"background":"#ffffff"},"dark":{"background":"#000000"}})
    );
    assert_eq!(
        theme["theme"]["colorScheme"],
        json!({"light":"light","dark":"dark"})
    );
    let existing =
        parse_project_theme(r##"{"theme":{"colors":{"light":{"background":"#123456"}}}}"##)
            .unwrap();
    let diff = missing_token_diff(&output.json, &usage, &existing).unwrap();
    assert!(
        diff["additions"]["theme"]["colors"]["light"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert_eq!(existing.colors["light"]["background"], "#123456");
}

#[test]
fn incompatible_same_meaning_tokens_report_conflict() {
    let mut snapshot = incident();
    snapshot.used_variable_ids.push("sds-icon".into());
    snapshot.used_remote_variables[0].name = "background".into();
    snapshot.used_remote_variables[0]
        .values_by_mode
        .insert("sd".into(), json!({"r":1,"g":0,"b":0}));
    let output = generate_devup_json(&snapshot, ThemeScope::File).unwrap();
    assert!(
        output
            .conflicts
            .iter()
            .any(|conflict| conflict.mode == "dark" && conflict.token == "background")
    );
}

#[test]
fn custom_dark_mode_has_explicit_color_scheme() {
    let mut snapshot = incident();
    snapshot.collections[2].modes[1].name = "Dark Brand".into();
    let output = generate_devup_json(&snapshot, ThemeScope::File).unwrap();
    let theme: Value = serde_json::from_str(&output.json).unwrap();
    assert_eq!(theme["theme"]["colorScheme"]["darkBrand"], "dark");
}
