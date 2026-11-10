/// Feedback 008: a master hidden at every variant reaches the export response
/// as a warning naming the visible page instance, not as silent display=none.
#[tokio::test]
async fn feedback_hidden_master_export_warns_with_the_visible_instance() {
    let mut data = payload();
    data.target.node_id = Some("1:0".into());
    data.snapshot = serde_json::from_value(json!({
        "fileKey": "W1Fixture", "version": null, "roots": ["1:0"], "diagnostics": [],
        "nodes": {
            "1:0": {"id":"1:0","type":"COMPONENT_SET","fields":{"name":"Card","visible":false,"width":390,"height":200,"childrenIds":["1:1","1:2"]}},
            "1:1": {"id":"1:1","type":"COMPONENT","fields":{"name":"Breakpoint=Desktop","parentId":"1:0","width":390,"height":100,"variantProperties":{"Breakpoint":"Desktop"},"childrenIds":[]}},
            "1:2": {"id":"1:2","type":"COMPONENT","fields":{"name":"Breakpoint=Mobile","parentId":"1:0","width":390,"height":100,"variantProperties":{"Breakpoint":"Mobile"},"childrenIds":[]}},
            "9:1": {"id":"9:1","type":"INSTANCE","fields":{"name":"Card","mainComponentId":"1:1","childrenIds":[]}}
        }
    }))
    .unwrap();
    let result = project(data, operation(&["tsx", "componentTsx"])).await.unwrap();
    assert_eq!(result["componentSetVariants"]["allVariantsHidden"], true, "{result:#}");
    let warning = result["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|warning| warning["code"] == "DEVUP_HIDDEN_MASTER_EXPORT")
        .unwrap_or_else(|| panic!("{result:#}"));
    assert_eq!(warning["details"]["visibleInstanceIdsInSnapshot"], json!(["9:1"]));
}
