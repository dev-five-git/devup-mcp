use devup_mcp_figma::{AssetSelection, ErrorCode, Snapshot, resolve_asset_selections};
use serde_json::json;

#[test]
fn missing_asset_reports_acquired_scope_without_claiming_node_absent_from_figma() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"fixture", "version":null, "roots":["section"],
        "nodes":{"section":{"id":"section","type":"SECTION",
            "fields":{"name":"Products"},"extra":{},"fieldErrors":{}}},
        "diagnostics":[]
    }))
    .unwrap();
    let selection: AssetSelection = serde_json::from_value(json!({
        "assetId":"I81:11810;81:11808:node", "format":"png",
        "scale":1, "renderMode":"node"
    }))
    .unwrap();
    let error = resolve_asset_selections(&snapshot, &[selection]).unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    assert!(!error.retryable);
    assert_eq!(error.details["acquiredRootIds"], json!(["section"]));
    assert_eq!(error.details["nodeExistence"], "unverified");
    assert_eq!(error.details["requestedAssetId"], "I81:11810;81:11808:node");
    assert_eq!(error.details["recovery"]["tool"], "devup_figma_export");
}
