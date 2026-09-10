use rmcp::model::CallToolResult;
use serde_json::Value;

/// Execution errors now travel as MCP tool results. Keep negative tests
/// checking failure, and additionally require both representations to agree.
pub fn tool_error(result: CallToolResult) -> String {
    assert_eq!(result.is_error, Some(true));
    let wire = serde_json::to_value(&result).unwrap();
    let value = result.structured_content.expect("structured tool error");
    assert_eq!(
        serde_json::from_str::<Value>(wire["content"][0]["text"].as_str().unwrap()).unwrap(),
        value
    );
    assert!(value["error"]["code"].is_string());
    assert!(value["error"]["message"].is_string());
    assert!(value["error"]["retryable"].is_boolean());
    value.to_string()
}
