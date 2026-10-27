use devup_mcp_devup_ui::codegen::{
    CodegenOptions, generate_component, instance_overrides::instance_overrides,
};
use devup_mcp_figma::{RawNode, Snapshot};
use serde_json::json;

fn button() -> (Snapshot, CodegenOptions) {
    let mut snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/chondrogen_button_set.json")).unwrap();
    let mut instance = snapshot.nodes["24:1853"].clone();
    instance.id = "51:5705".into();
    instance.node_type = "INSTANCE".into();
    instance
        .fields
        .insert("mainComponentId".into(), json!("24:1853"));
    instance
        .fields
        .insert("mainComponentSetId".into(), json!("23:1613"));
    instance.fields.insert(
        "variantProperties".into(),
        json!({"effect":"default","size":"md","type":"primary"}),
    );
    instance.fields.insert("childrenIds".into(), json!([]));
    instance.fields.insert(
        "overrides".into(),
        json!([{"id":"51:5705","overriddenFields":["fills"]}]),
    );
    instance.fields.insert("fills".into(), json!([{"type":"SOLID","color":{"r":0.0235294118,"g":0.51372549,"b":0.56862745},"boundVariables":{"color":{"type":"VARIABLE_ALIAS","id":"VariableID:9:346"}}}]));
    snapshot.nodes.insert(instance.id.clone(), instance);
    snapshot.roots = vec!["51:5705".into()];
    let options = CodegenOptions {
        variable_tokens: [("VariableID:9:346".into(), "secondary".into())].into(),
        inline_instances: true,
        ..CodegenOptions::default()
    };
    (snapshot, options)
}

#[test]
fn chondrogen_primary_button_fill_override_requires_preservation() {
    let (snapshot, options) = button();
    let records = instance_overrides(&snapshot, "51:5705", &options);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["classification"], "not-expressible");
    assert_eq!(records[0]["mainComponentId"], "24:1853");
    assert_eq!(records[0]["mainComponentSetId"], "23:1613");
    assert_eq!(records[0]["differsFromMaster"], true);
    assert!(
        records[0]["generatedProps"]
            .as_array()
            .unwrap()
            .contains(&json!("bg=\"$secondary\""))
    );
    let output = generate_component(&snapshot, "51:5705", &options).unwrap();
    assert!(output.tsx.contains("// @devup-override "));
    assert!(
        output
            .diagnostics
            .iter()
            .any(|issue| issue.code == "DEVUP_CODEGEN_INSTANCE_OVERRIDE")
    );
}

#[test]
fn text_characters_bound_to_text_component_property_is_not_loss() {
    let (mut snapshot, options) = button();
    let child: RawNode =
        serde_json::from_value(json!({"id":"I51:5705;24:1852","type":"TEXT","fields":{
        "characters":"Custom label","componentPropertyReferences":{"characters":"Label#1:2"}}}))
        .unwrap();
    snapshot.nodes.insert(child.id.clone(), child);
    let instance = snapshot.nodes.get_mut("51:5705").unwrap();
    instance
        .fields
        .insert("childrenIds".into(), json!(["I51:5705;24:1852"]));
    instance.fields.insert(
        "componentProperties".into(),
        json!({"Label#1:2":{"type":"TEXT","value":"Custom label"}}),
    );
    instance.fields.insert(
        "overrides".into(),
        json!([{"id":"I51:5705;24:1852","overriddenFields":["characters"]}]),
    );
    assert!(instance_overrides(&snapshot, "51:5705", &options).is_empty());
}

#[test]
fn unbound_characters_and_missing_descendants_are_reported() {
    let (mut snapshot, options) = button();
    snapshot.nodes.get_mut("51:5705").unwrap().fields.insert(
        "overrides".into(),
        json!([
        {"id":"missing","overriddenFields":["characters"]}]),
    );
    let records = instance_overrides(&snapshot, "51:5705", &options);
    assert_eq!(records[0]["resolution"], "unresolved");
    assert_eq!(records[0]["generatedProps"], json!([]));
}
