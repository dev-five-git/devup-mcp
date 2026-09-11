use devup_mcp_devup_ui::ui_validate::validate_devup_ui_tsx;

#[test]
fn r12_intrinsic_as_accepts_the_rendered_elements_attributes() {
    for tsx in [
        r#"<Text as="input" maxLength={50} placeholder="받는 분 이름" type="text" />"#,
        r#"<Text maxLength={500} as={'input'} />"#,
        r#"<Text as="textarea" maxLength={500} wrap="soft" />"#,
        r#"<Box as="form" action="/submit" encType="multipart/form-data" />"#,
        r#"<Flex as="video" playsInline controlsList="nodownload" />"#,
        r#"<Text as="label" htmlFor="name" />"#,
        r#"<Text {...props} as="input" maxLength={50} />"#,
    ] {
        let report = validate_devup_ui_tsx(tsx, None, false);
        assert!(report.ok, "{tsx}: {:?}", report.violations);
    }
}

#[test]
fn r12_as_does_not_disable_unknown_prop_or_leak_to_other_elements() {
    for tsx in [
        r#"<Text maxLength={50} />"#,
        r#"<Text as="span" maxLength={50} />"#,
        r#"<Text as="button" maxLength={50} />"#,
        r#"<Text as="input" maxLenght={50} />"#,
        r#"<Text as={Component} maxLength={50} />"#,
        r#"<Text as={tag} maxLength={50} />"#,
        r#"<Text as="input" {...props} maxLength={50} />"#,
        r#"<Text as="input" as={tag} maxLength={50} />"#,
        r#"<><Text as="input" maxLength={50} /><Text maxLength={50} /></>"#,
        r#"<Box child={<Text as="input" maxLength={50} />} maxLength={50} />"#,
    ] {
        let report = validate_devup_ui_tsx(tsx, None, false);
        assert!(!report.ok, "{tsx}");
        assert!(
            report.violations.iter().any(|v| v.rule == "unknown-prop"),
            "{tsx}"
        );
    }
}
