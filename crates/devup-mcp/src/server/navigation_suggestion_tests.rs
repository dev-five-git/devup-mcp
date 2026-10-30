use super::*;

#[test]
fn nested_korean_location_uses_any_route_segment() {
    let result = suggestions(&json!([{"nodeId":"location","text":"오시는 길"}]), &json!({"routes":[{"route":"/company/location"}]}));
    assert_eq!(result[0]["suggestions"][0]["route"], "/company/location");
    assert_eq!(result[0]["targetResolved"], false);
}

#[test]
fn existing_links_precede_confirmed_names_and_concepts() {
    let result = suggestions(&json!([{"nodeId":"location","text":"오시는 길"}]), &json!({
        "routes":[{"route":"/company/location"}],
        "codeLinks":[{"label":"오시는길","href":"/visit","evidence":"src/components/Header.tsx:7"}],
        "confirmedRoutes":[{"name":"오시는 길","route":"/directions"}]
    }));
    assert_eq!(result[0]["suggestions"][0]["route"], "/visit");
    assert_eq!(result[0]["suggestions"][0]["evidence"], "src/components/Header.tsx:7");
    assert_eq!(result[0]["targetResolved"], true);
}

#[test]
fn confirmed_names_are_suggestions_not_resolved_targets() {
    let result = suggestions(&json!([{"nodeId":"location","text":"오시는 길"}]), &json!({"confirmedRoutes":[{"name":"오시는길","route":"/directions"}]}));
    assert_eq!(result[0]["suggestions"][0]["route"], "/directions");
    assert_eq!(result[0]["targetResolved"], false);
}
