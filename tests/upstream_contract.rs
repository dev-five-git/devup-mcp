use devup_mcp::figma::{BuiltinScript, ReadToolCall};

#[test]
fn maps_every_read_call_to_the_fixed_figma_tool_contract() {
    let calls = [
        (
            ReadToolCall::metadata("file-key", Some("1:2")),
            "get_metadata",
        ),
        (
            ReadToolCall::variable_defs("file-key", "1:2"),
            "get_variable_defs",
        ),
        (
            ReadToolCall::design_context("file-key", "1:2"),
            "get_design_context",
        ),
        (
            ReadToolCall::code_connect_map("file-key", "1:2"),
            "get_code_connect_map",
        ),
        (
            ReadToolCall::screenshot("file-key", "1:2"),
            "get_screenshot",
        ),
        (
            ReadToolCall::snapshot("file-key", "1:2", BuiltinScript::NodeSnapshot),
            "use_figma",
        ),
    ];

    for (call, expected_name) in calls {
        assert_eq!(call.tool_name(), expected_name);
        let arguments = call.arguments();
        assert_eq!(
            arguments.get("fileKey").and_then(|v| v.as_str()),
            Some("file-key")
        );
    }
}

#[test]
fn snapshot_accepts_only_compiled_in_scripts() {
    let call = ReadToolCall::snapshot("file-key", "1:2", BuiltinScript::NodeSnapshot);
    let arguments = call.arguments();
    let code = arguments
        .get("code")
        .and_then(|value| value.as_str())
        .expect("built-in script");

    assert!(code.contains("figma.getNodeByIdAsync"));
    assert!(!code.contains("eval("));
    assert!(!code.contains("Function("));
}
