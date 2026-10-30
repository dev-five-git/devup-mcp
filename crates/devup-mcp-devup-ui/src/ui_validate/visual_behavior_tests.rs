use super::*;

fn count(source: &str, rule: &str) -> usize {
    validate_devup_ui_tsx(source, None, false)
        .violations
        .iter()
        .filter(|finding| finding.rule == rule)
        .count()
}

#[test]
fn link_underline_warns_for_native_polymorphic_and_aliased_links() {
    let source = r#"import RouteLink from 'next/link';
        const View = () => <><a href='/'>A</a><RouteLink href='/'>B</RouteLink>
        <Box as='a'>C</Box><Box as={RouteLink}>D</Box><button>E</button></>;"#;
    assert_eq!(count(source, "link-underline"), 4);
}

#[test]
fn link_underline_accepts_explicit_decoration_but_not_hover_only() {
    let source = r#"const View = () => <><a textDecoration='underline'>A</a>
        <Box as='a' {...{ textDecoration: 'none' }}>B</Box>
        <a _hover={{ textDecoration: 'none' }}>C</a></>;"#;
    assert_eq!(count(source, "link-underline"), 1);
}

#[test]
fn link_underline_accepts_a_bundle_global_reset() {
    let files = [
        SourceFile { path: "src/Reset.ts".into(), content: "import { globalCss as reset } from '@devup-ui/react'; reset({ a: { textDecoration: 'none', color: 'inherit' } });".into() },
        SourceFile { path: "src/components/View.tsx".into(), content: "export const View = () => <a href='/'>Go</a>;".into() },
    ];
    let report = validate_bundle(&files, None, false).unwrap();
    assert!(!report.violations.iter().any(|finding| finding.rule == "link-underline"));
}

#[test]
fn link_underline_does_not_accept_unrelated_or_hover_global_styles() {
    for reset in [
        "const styles = { a: { textDecoration: 'none' } };",
        "globalCss({ button: { textDecoration: 'none' } });",
        "globalCss({ 'a:hover': { textDecoration: 'none' } });",
        "globalCss({ a: { _hover: { textDecoration: 'none' } } });",
        "// globalCss({ a: { textDecoration: 'none' } });",
    ] {
        assert_eq!(count(&format!("{reset}\nconst View = () => <a>Go</a>;"), "link-underline"), 1);
    }
}

#[test]
fn marquee_static_reports_repeated_single_line_text_without_motion() {
    let source = "const View = () => <Text whiteSpace='nowrap'>Research ✦ Repair ✦ Renew ✦ Research</Text>;";
    let report = validate_devup_ui_tsx(source, None, false);
    let finding = report.violations.iter().find(|finding| finding.rule == "marquee-static").unwrap();
    assert_eq!(finding.severity, Severity::Info);
    assert_eq!(finding.context["confidence"], "heuristic");
}

#[test]
fn marquee_static_ignores_short_multiline_and_animated_content() {
    for source in [
        "const View = () => <Text>A / B / C</Text>;",
        "const View = () => <Text>Research ✦ Repair\n✦ Renew ✦ Research</Text>;",
        "const View = () => <Text animation='strip 20s infinite linear'>Research ✦ Repair ✦ Renew ✦ Research</Text>;",
        "const strip = keyframes({ from: { transform: 'translateX(0)' }, to: { transform: 'translateX(-50%)' } }); const View = () => <Text>Research ✦ Repair ✦ Renew ✦ Research</Text>;",
        "const View = () => <Text>https://example.com/a/b/c/d/e</Text>;",
    ] {
        assert_eq!(count(source, "marquee-static"), 0, "{source}");
    }
}
