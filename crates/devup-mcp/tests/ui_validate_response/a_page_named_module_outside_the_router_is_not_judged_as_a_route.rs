

/// `page` is only a route name inside the router directory. A module that
/// happens to be called `page.tsx` elsewhere is not a route and owes the
/// route naming convention nothing.
#[tokio::test]
async fn a_page_named_module_outside_the_router_is_not_judged_as_a_route() -> anyhow::Result<()> {
    let output = structural(
        "export default function Paper(){return <div/>}",
        "src/components/page.tsx",
    )
    .await?;
    let messages: Vec<_> = output["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["message"].as_str())
        .collect();
    assert!(
        !messages.iter().any(|m| m.contains("does not end in Page")),
        "{output}"
    );
    Ok(())
}

/// The route component still has to be named for the route, including when
/// the default export names it rather than declaring it.
#[tokio::test]
async fn bundle_page_default_export_by_identifier_is_still_judged() -> anyhow::Result<()> {
    let output = structural(
        "function Home(){return <div/>}\nexport default Home;",
        "src/app/page.tsx",
    )
    .await?;
    assert!(has_rule(&output, "file-placement"), "{output}");
    Ok(())
}

/// Validating nothing is not a pass. This used to check the empty string
/// and answer `ok: true, okReason: "clean"` about code never supplied.
#[tokio::test]
async fn validate_refuses_a_call_with_nothing_to_check() {
    let error = validate(json!({})).await.unwrap_err().to_string();
    assert!(error.contains("nothing to validate"), "{error}");
    let error = validate(json!({"tsx":"   "}))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("nothing to validate"), "{error}");
}

/// A monorepo root has no `devup.json` of its own. Taking the first one the
/// sorted scan finds validated one package's code against another package's
/// theme and called its real tokens `unknown-token` — an error verdict,
/// suggesting the other package's token as the fix.
#[tokio::test]
async fn monorepo_root_refuses_to_pick_one_of_several_themes() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("devup-mono-theme-{}", std::process::id()));
    let front = root.join("apps").join("front");
    let admin = root.join("apps").join("admin");
    std::fs::create_dir_all(&front)?;
    std::fs::create_dir_all(&admin)?;
    std::fs::write(root.join("package.json"), "{}")?;
    std::fs::write(
        front.join("devup.json"),
        r##"{"theme":{"colors":{"default":{"bg":"#F7F3EC"}}}}"##,
    )?;
    std::fs::write(
        admin.join("devup.json"),
        r##"{"theme":{"colors":{"default":{"adminBg":"#000000"}}}}"##,
    )?;
    let result =
        validate(json!({"tsx":r##"<Box bg="$bg"/>"##,"projectRoot":root.to_string_lossy()})).await;
    std::fs::remove_dir_all(&root)?;
    let out = result?;
    assert!(!has_rule(&out, "unknown-token"), "{out}");
    assert_eq!(out["themeAvailable"], false, "{out}");
    assert_eq!(out["okReason"], "clean-unverified-tokens", "{out}");
    let choices = out["themeGuardrail"]["candidateProjectRoots"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(choices.len(), 2, "{out}");
    assert!(
        choices
            .iter()
            .all(|choice| choice["authority"] == "package-local"),
        "{out}"
    );
    Ok(())
}

/// The token check used to read JSX attributes only, so a theme token used
/// in a global style was never checked and an unknown one passed in silence.
#[tokio::test]
async fn tokens_inside_global_styles_are_checked() -> anyhow::Result<()> {
    let out = validate(json!({
        "tsx": r##"globalCss({ body: { background: "$definitelyNotAToken" } })"##,
        "projectRoot": fixture_project_root(),
    }))
    .await?;
    assert!(has_rule(&out, "unknown-token"), "{out}");
    assert!(out["checkedTokens"].as_u64().unwrap_or(0) >= 1, "{out}");

    // Nested selectors and responsive arrays carry tokens too.
    let out = validate(json!({
        "tsx": r##"css({ _hover: { color: ["$alsoNotAToken"] } })"##,
        "projectRoot": fixture_project_root(),
    }))
    .await?;
    assert!(has_rule(&out, "unknown-token"), "{out}");
    Ok(())
}

/// A violation names the element and attribute it sits in, and with the
/// export's sourceMap the Figma node and field that attribute came from.
#[tokio::test]
async fn violations_link_to_the_source_map_on_element_index() -> anyhow::Result<()> {
    let tsx = r##"<Box w="100%"><Text color="#123456">Hi</Text></Box>"##;
    let out = validate(json!({
        "tsx": tsx,
        "projectRoot": fixture_project_root(),
        "sourceMap": {"version": 2, "entries": [
            {"nodeId": "1:2", "property": "fills", "generatedProperty": "color=\"#123456\"",
             "resolution": "exact", "elementIndex": 1},
            {"nodeId": "1:1", "property": "width", "generatedProperty": "w=\"100%\"",
             "resolution": "raw-fallback", "elementIndex": 0}
        ]},
    }))
    .await?;
    let violation = out["violations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["rule"] == "hardcoded-color")
        .expect("hardcoded colour");
    assert_eq!(violation["elementIndex"], 1, "{violation}");
    assert_eq!(violation["element"], "Text");
    assert_eq!(violation["attribute"], "color");
    assert_eq!(violation["source"][0]["nodeId"], "1:2", "{violation}");
    assert_eq!(violation["source"][0]["property"], "fills");
    assert_eq!(out["sourceMapLink"]["state"], "linked");
    let violations = out["violations"].as_array().unwrap();
    let total = violations.len();
    let linked = violations
        .iter()
        .filter(|v| v.get("source").is_some())
        .count();
    assert_eq!(out["sourceMapLink"]["linkedViolations"], linked, "{out}");
    assert_eq!(
        out["sourceMapLink"]["unlinkedViolations"],
        total - linked,
        "{out}"
    );

    let only_an_unrelated_entry = validate(json!({
        "tsx": tsx,
        "projectRoot": fixture_project_root(),
        "sourceMap": {"entries": [
            {"nodeId": "9:9", "property": "fills", "generatedProperty": "bg=\"#000\"",
             "resolution": "exact", "elementIndex": 7}
        ]},
    }))
    .await?;
    let link = &only_an_unrelated_entry["sourceMapLink"];
    assert_eq!(link["linkedViolations"], 0, "{link}");
    assert_eq!(link["unlinkedViolations"], total, "{link}");
    assert!(link["unlinkedReason"].is_string(), "{link}");

    let stale = validate(json!({
        "tsx": tsx,
        "sourceMap": {"entries": [{"nodeId": "1:2", "generatedProperty": "color=\"#123456\""}]},
    }))
    .await?;
    assert_eq!(stale["sourceMapLink"]["state"], "unavailable", "{stale}");

    let bundled = validate(json!({
        "files": [{"path": "src/components/Badge.tsx",
            "content": format!("export function Badge() {{ return {tsx} }}")}],
        "sourceMap": {"entries": [
            {"nodeId": "1:2", "property": "fills", "generatedProperty": "color=\"#123456\"",
             "resolution": "exact", "elementIndex": 1}
        ]},
    }))
    .await?;
    assert_eq!(
        bundled["sourceMapLink"]["state"], "unavailable",
        "{bundled}"
    );
    assert!(
        bundled["violations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v.get("source").is_none()),
        "{bundled}"
    );
    Ok(())
}

/// A hardcoded value says whether its Figma node bound a variable there:
/// a bound one outranks any token that only shares the value, an unbound
/// one means the design hardcodes it too.
#[tokio::test]
async fn hardcoded_values_report_the_figma_binding_behind_them() -> anyhow::Result<()> {
    let tsx = r##"<Box w="100%"><Text color="#123456">Hi</Text></Box>"##;
    let entry = |variable: Option<&str>| {
        json!({"nodeId": "1:2", "property": "fills", "generatedProperty": "color=\"#123456\"",
            "resolution": "raw-fallback", "elementIndex": 1, "variableId": variable})
    };
    for (variable, state) in [(Some("VariableID:9:345"), "bound"), (None, "unbound")] {
        for detail in ["full", "summary"] {
            let out = validate(json!({
                "tsx": tsx,
                "projectRoot": fixture_project_root(),
                "detail": detail,
                "sourceMap": {"entries": [entry(variable)]},
            }))
            .await?;
            let violation = out["violations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["rule"] == "hardcoded-color")
                .expect("hardcoded colour");
            assert_eq!(violation["designBinding"]["state"], state, "{detail}: {violation}");
            if let Some(variable) = variable {
                assert_eq!(violation["designBinding"]["variableIds"][0], variable);
            }
        }
    }
    Ok(())
}

/// A value the design hardcodes too is info even when a theme token shares
/// it, so strict passes; a bound one stays a warning.
#[tokio::test]
async fn an_unbound_value_matching_a_token_is_info() -> anyhow::Result<()> {
    let tsx = r##"<Box w="100%"><Box p="16px" /></Box>"##;
    let entry = |variable: Option<&str>| {
        json!({"nodeId": "1:2", "property": "paddingLeft", "generatedProperty": "p=\"16px\"",
            "resolution": "raw-fallback", "elementIndex": 1, "variableId": variable})
    };
    let plain = validate(json!({"tsx": tsx, "projectRoot": fixture_project_root(), "strict": true})).await?;
    let finding = |out: &Value| {
        out["violations"].as_array().unwrap().iter()
            .find(|v| v["rule"] == "hardcoded-length").cloned().expect("hardcoded length")
    };
    assert_eq!(finding(&plain)["severity"], "warning", "{plain}");
    for (variable, severity, ok) in [(None, "info", true), (Some("VariableID:1:1"), "warning", false)] {
        for detail in ["full", "summary"] {
            let out = validate(json!({
                "tsx": tsx, "projectRoot": fixture_project_root(), "strict": true, "detail": detail,
                "sourceMap": {"entries": [entry(variable)]},
            }))
            .await?;
            assert_eq!(finding(&out)["severity"], severity, "{detail}: {out}");
            assert_eq!(out["ok"], ok, "{detail}: {out}");
        }
    }
    Ok(())
}

/// Layout risks and what was never rendered are reported beside the rules,
/// and do not move `ok` or `okReason`.
#[tokio::test]
async fn layout_risks_are_reported_without_changing_the_verdict() -> anyhow::Result<()> {
    let tsx = r##"<Box h="760px" pos="relative" w="100%">
  <Box bg="$primaryColor" h="22px" pos="absolute" right="36px" top="210px" w="64px" />
  <Text left="269px" pos="absolute" top="214px" typography="body1">x</Text>
</Box>"##;
    let without_width =
        validate(json!({"tsx": tsx, "projectRoot": fixture_project_root()})).await?;
    let with_width = validate(json!({
        "tsx": tsx, "projectRoot": fixture_project_root(), "referenceWidth": 360
    }))
    .await?;
    assert_eq!(without_width["okReason"], with_width["okReason"]);
    assert_eq!(with_width["ok"], without_width["ok"]);
    assert!(
        without_width["layoutRisks"].as_array().unwrap().is_empty(),
        "{without_width}"
    );
    assert_eq!(
        without_width["verificationScope"]["static"]["groupsWithoutReferenceWidth"],
        1
    );
    let risks = with_width["layoutRisks"].as_array().unwrap();
    assert_eq!(risks.len(), 1, "{with_width}");
    assert_eq!(risks[0]["rule"], "anchor-mismatch");
    assert_eq!(
        with_width["verificationScope"]["runtime"]["state"],
        "not-performed"
    );
    Ok(())
}

/// One theme inside the project is still unambiguous, so the common
/// single-package layout keeps resolving without a guardrail.
#[tokio::test]
async fn a_single_nested_theme_is_still_resolved() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("devup-single-theme-{}", std::process::id()));
    let front = root.join("apps").join("front");
    std::fs::create_dir_all(&front)?;
    std::fs::write(root.join("package.json"), "{}")?;
    std::fs::write(
        front.join("devup.json"),
        r##"{"theme":{"colors":{"default":{"bg":"#F7F3EC"}}}}"##,
    )?;
    let result =
        validate(json!({"tsx":r##"<Box bg="$bg"/>"##,"projectRoot":root.to_string_lossy()})).await;
    std::fs::remove_dir_all(&root)?;
    let out = result?;
    assert_eq!(out["themeAvailable"], true, "{out}");
    assert_eq!(out["okReason"], "clean", "{out}");
    assert!(!has_rule(&out, "unknown-token"), "{out}");
    Ok(())
}
