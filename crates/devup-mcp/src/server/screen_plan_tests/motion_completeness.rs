// The motion the user chose, and eased field states, are completion items:
// status is not done while the sources lack them.

#[test]
fn chosen_motion_and_field_state_easing_must_be_in_the_sources() {
    let fixture = Fixture::new();
    fixture.write("src/components/Search.tsx", r#"export const S = () => <Flex border="1px solid" _focusWithin={{ borderColor: "$primary" }}><Box as="input" outline="none" /></Flex>;"#);
    let mut project = evidence();
    project.root = fixture.0.clone();
    project.sources = vec!["src/components/Search.tsx".into()];
    let item = |plan: &Value, name: &str| {
        site_completeness(plan, &project).into_iter().find(|item| item["item"] == name).unwrap()
    };
    let predicted = json!({"routes": [], "site": {"motion": {"state": "answered", "answer": {"motion": {"source": "predicted"}}}}});
    assert_eq!(item(&predicted, "motion")["state"], "missing");
    let fields = item(&predicted, "field-state-motion");
    assert_eq!(fields["state"], "missing");
    assert!(text(&fields["detail"]).contains("src/components/Search.tsx"));
    let none = json!({"routes": [], "site": {"motion": {"state": "answered", "answer": {"motion": {"source": "none"}}}}});
    assert_eq!(item(&none, "motion")["state"], "not-applicable");
    let unanswered = json!({"routes": [], "site": {"motion": {"state": "asked", "question": "site:motion"}}});
    assert_eq!(item(&unanswered, "motion")["state"], "missing");

    fixture.write("src/components/Search.tsx", r#"const fadeUp = keyframes({ from: { opacity: 0 }, to: { opacity: 1 } });
export const S = () => <Flex border="1px solid" transition="160ms ease-out" transitionProperty="border-color,box-shadow" _focusWithin={{ borderColor: "$primary" }} animationName={fadeUp}><Box as="input" outline="none" /></Flex>;"#);
    assert_eq!(item(&predicted, "motion")["state"], "filled");
    assert_eq!(item(&predicted, "field-state-motion")["state"], "filled");
}
