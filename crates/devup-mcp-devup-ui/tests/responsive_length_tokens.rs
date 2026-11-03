use devup_mcp_devup_ui::codegen::{CodegenOptions, LengthToken, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn options(value: Value) -> CodegenOptions {
    CodegenOptions {
        inline_instances: true,
        length_tokens: BTreeMap::from([(
            "space".into(),
            LengthToken {
                token: "containerY".into(),
                value,
            },
        )]),
        variable_tokens: BTreeMap::from([("space".into(), "containerY".into())]),
        ..Default::default()
    }
}

fn snapshot(
    mobile_bound: bool,
    desktop_bound: bool,
    desktop_width: u32,
    desktop_padding: u32,
) -> Snapshot {
    let mut nodes = serde_json::Map::new();
    for (suffix, width, padding, bound) in [
        ("mobile", 360, 40, mobile_bound),
        ("desktop", desktop_width, desktop_padding, desktop_bound),
    ] {
        let id = format!("root-{suffix}");
        let child = format!("text-{suffix}");
        let mut fields = json!({"name":"Screen","width":width,"height":300,"layoutMode":"VERTICAL","paddingTop":0,"paddingLeft":0,"paddingRight":0,"paddingBottom":padding,"childrenIds":[child]});
        if bound {
            fields["boundVariables"] =
                json!({"paddingBottom":{"type":"VARIABLE_ALIAS","id":"space"}})
        }
        nodes.insert(id.clone(), json!({"id":id,"type":"FRAME","fields":fields}));
        nodes.insert(child.clone(),json!({"id":child,"type":"TEXT","fields":{"name":"Label","parentId":id,"characters":"Research","fontSize":16}}));
    }
    serde_json::from_value(json!({"fileKey":"nested-length","roots":["root-mobile","root-desktop"],"nodes":nodes,"diagnostics":[]})).unwrap()
}

#[test]
fn responsive_prop_retains_jsx_token_and_reports_runtime_verification() {
    let output = merge_breakpoints(
        &snapshot(false, true, 1440, 80),
        &options(json!(["40px", null, null, null, "80px"])),
    )
    .unwrap()
    .unwrap();
    assert!(output.verified, "{:?}", output.gaps);
    assert!(output.tsx.contains("$containerY"), "{}", output.tsx);
    assert!(!output.tsx.contains("var(--containerY)"), "{}", output.tsx);
    assert!(output.tsx.contains("pb=\"$containerY\""), "{}", output.tsx);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_RESPONSIVE_LENGTH_TOKEN_PROJECTED")
    );
}

#[test]
fn scalar_responsive_token_and_nested_scalar_token_remain_tokens() {
    for (source, value) in [
        (
            snapshot(true, true, 1440, 80),
            json!(["40px", null, null, null, "80px"]),
        ),
        (snapshot(false, true, 1440, 80), json!("80px")),
    ] {
        let output = merge_breakpoints(&source, &options(value))
            .unwrap()
            .unwrap();
        assert!(output.verified, "{:?}", output.gaps);
        assert!(output.tsx.contains("$containerY"), "{}", output.tsx);
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_RESPONSIVE_LENGTH_TOKEN_PROJECTED")
        );
    }
}

#[test]
fn expansion_keeps_intermediate_and_trailing_theme_breakpoints() {
    let values = json!(["40px", "48px", null, "64px", "80px", "96px"]);
    let from_mobile =
        merge_breakpoints(&snapshot(true, false, 1440, 120), &options(values.clone()))
            .unwrap()
            .unwrap();
    assert!(from_mobile.verified, "{:?}", from_mobile.gaps);
    for value in ["$containerY", "120px"] {
        assert!(from_mobile.tsx.contains(value), "{}", from_mobile.tsx)
    }
    let to_desktop = merge_breakpoints(&snapshot(false, true, 1920, 96), &options(values))
        .unwrap()
        .unwrap();
    assert!(to_desktop.verified, "{:?}", to_desktop.gaps);
    for value in ["$containerY"] {
        assert!(to_desktop.tsx.contains(value), "{}", to_desktop.tsx)
    }
}

#[test]
fn captured_records_bottom_padding_is_eighty_at_desktop_not_token_base_forty() {
    // Original frames/section/container fields only; unrelated children omitted.
    let source: Snapshot = serde_json::from_str(include_str!(
        "fixtures/chondrogen_records_padding_tokens.json"
    ))
    .unwrap();
    let mut opts = options(json!(["40px", null, null, null, "80px"]));
    opts.length_tokens = BTreeMap::from([
        (
            "VariableID:30:632".into(),
            LengthToken {
                token: "containerY".into(),
                value: json!(["40px", null, null, null, "80px"]),
            },
        ),
        (
            "VariableID:30:628".into(),
            LengthToken {
                token: "containerX".into(),
                value: json!(["20px", null, null, null, "40px"]),
            },
        ),
    ]);
    opts.variable_tokens = BTreeMap::from([
        ("VariableID:30:632".into(), "containerY".into()),
        ("VariableID:30:628".into(), "containerX".into()),
    ]);
    let output = merge_breakpoints(&source, &opts).unwrap().unwrap();
    assert!(!output.tsx.contains("var(--containerY)"), "{}", output.tsx);
    assert!(output.tsx.contains("$containerY"), "{}", output.tsx);
    assert!(output.tsx.contains("$containerX"), "{}", output.tsx);
    assert!(
        output
            .contract
            .iter()
            .filter(|p| p["generatedProperty"]
                .as_str()
                .is_some_and(|v| v.starts_with("pb=")))
            .all(|p| p["verified"] == true),
        "{:?}",
        output.gaps
    );
}
