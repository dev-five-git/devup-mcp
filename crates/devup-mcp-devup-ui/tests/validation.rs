use devup_mcp_devup_ui::validation::validate_tsx;
use devup_mcp_figma::ErrorCode;

#[test]
fn syntax_accepts_nested_typescript_jsx() {
    let source = r#"
        import { Text, VStack } from "@devup-ui/react";
        export function Proofread(): JSX.Element {
            return <VStack><Text typography="body">Body</Text></VStack>;
        }
    "#;
    let report = validate_tsx(source).expect("valid TSX");
    assert_eq!(report.byte_len, source.len());
}

#[test]
fn syntax_rejects_invalid_tsx_without_echoing_source_text() {
    for source in [
        "export function Broken() { return <VStack><Text>secret body</VStack>; }",
        "export function Broken() { return <Text color=\"red>secret body</Text>; }",
        "export function Broken() { return <Text>{secret body + }</Text>; }",
    ] {
        let error = validate_tsx(source).expect_err("invalid TSX");
        assert_eq!(error.code, ErrorCode::DevupCodegenFailed);
        assert!(!error.to_string().contains("secret body"));
        assert!(
            error.details["errorCount"]
                .as_u64()
                .is_some_and(|count| count > 0)
        );
        assert!(error.details["errors"][0]["start"].as_u64().is_some());
        assert!(error.details["errors"][0]["end"].as_u64().is_some());
        assert_eq!(error.details["errors"][0]["category"], "syntax");
    }
}
