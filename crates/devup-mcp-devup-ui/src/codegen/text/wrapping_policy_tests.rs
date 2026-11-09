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

    fn props_for(characters: &str, existing: Vec<Prop>) -> Vec<Prop> {
        let snapshot: devup_mcp_figma::Snapshot = serde_json::from_value(serde_json::json!({
            "fileKey": "test", "version": null, "diagnostics": [], "nodes": {"t": {"id": "t", "type": "TEXT", "fields": {"name": "Copy", "characters": characters}}},
            "roots": ["t"]
        }))
        .unwrap();
        let mut props = existing;
        push_korean_wrap_policy(&snapshot.nodes["t"].typed_view(), None, &mut props);
        props
    }

    fn value_of<'a>(props: &'a [Prop], key: &str) -> Option<&'a str> {
        props.iter().find(|(name, _)| name == key).and_then(|(_, value)| match value {
            PropValue::String(value) => Some(value.as_str()),
            _ => None,
        })
    }

    #[test]
    fn korean_wrapping_text_keeps_word_boundaries_and_pretty_wrap() {
        let props = props_for("진단 결과를 바탕으로 개선을 제안합니다", Vec::new());
        assert_eq!(value_of(&props, "wordBreak"), Some("keep-all"));
        assert_eq!(value_of(&props, "overflowWrap"), Some("break-word"));
        assert_eq!(value_of(&props, "textWrap"), Some("pretty"));
    }

    #[test]
    fn pretty_wrap_never_reenables_wrapping_for_pre_or_nowrap_text() {
        for mode in ["pre", "nowrap"] {
            let props = props_for(
                "편집 설정",
                vec![("whiteSpace".into(), PropValue::String(mode.into()))],
            );
            assert_eq!(value_of(&props, "wordBreak"), Some("keep-all"), "{mode}");
            assert_eq!(value_of(&props, "textWrap"), None, "{mode}");
        }
        let props = props_for(
            "편집  설정",
            vec![("whiteSpace".into(), PropValue::String("pre-wrap".into()))],
        );
        assert_eq!(value_of(&props, "textWrap"), Some("pretty"));
    }

    #[test]
    fn explicit_local_or_theme_text_wrap_is_preserved() {
        let props = props_for(
            "한국어 문장",
            vec![("textWrap".into(), PropValue::String("balance".into()))],
        );
        assert_eq!(value_of(&props, "textWrap"), Some("balance"));
        let snapshot: devup_mcp_figma::Snapshot = serde_json::from_value(serde_json::json!({
            "fileKey": "test", "version": null, "diagnostics": [], "nodes": {"t": {"id": "t", "type": "TEXT", "fields": {"name": "Copy", "characters": "한국어 문장"}}},
            "roots": ["t"]
        }))
        .unwrap();
        let theme = serde_json::json!([{"textWrap": "balance"}]);
        let mut props = Vec::new();
        push_korean_wrap_policy(&snapshot.nodes["t"].typed_view(), Some(&theme), &mut props);
        assert_eq!(value_of(&props, "textWrap"), None);
        assert_eq!(value_of(&props, "wordBreak"), Some("keep-all"));
    }
}
