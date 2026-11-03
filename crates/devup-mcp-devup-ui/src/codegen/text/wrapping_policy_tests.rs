#[cfg(test)]
mod wrapping_policy_tests {
    use super::*;

    #[test]
    fn explicit_existing_word_break_policy_is_not_overwritten() {
        let snapshot: devup_mcp_figma::Snapshot = serde_json::from_str(include_str!(
            "../../../tests/fixtures/vision-wrapping-text.json"
        )).unwrap();
        let mut props = vec![("wordBreak".into(), PropValue::String("break-all".into()))];
        push_korean_wrap_policy(&snapshot.nodes["I44:3228;44:3621"].typed_view(), None, &mut props);
        assert_eq!(props.len(), 1);
        assert!(matches!(&props[0].1, PropValue::String(value) if value == "break-all"));
    }
}

