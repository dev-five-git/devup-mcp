use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn output(snapshot: &Snapshot, verified: bool) -> String {
    let result = merge_breakpoints(
        snapshot,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(result.gaps.is_empty(), verified, "{:?}", result.gaps);
    result.tsx.split_whitespace().collect()
}

fn captured_output(snapshot: &Snapshot, ids: &[&str], properties: &[&str]) -> String {
    let result = merge_breakpoints(
        snapshot,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    // These exact source subtrees also contain independently unverified native
    // appearance/image-layer properties. Keep those reported; verify only the
    // alignment/gap constraints this regression changes.
    let relevant_gap = result.gaps.iter().any(|gap| {
        ids.contains(&gap["widthNodeId"].as_str().unwrap_or_default())
            && properties.contains(&gap["property"].as_str().unwrap_or_default())
    });
    assert!(!relevant_gap, "{:?}", result.gaps);
    result.tsx.split_whitespace().collect()
}

fn frames(desktop: Value, mobile: Value) -> Snapshot {
    let node = |id, width, extra: Value| {
        let mut fields = json!({"name":if id=="d" {"desktop"} else {"mobile"}, "width":width,"height":100,
            "layoutMode":"HORIZONTAL","layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "inferredAutoLayout":{"layoutMode":"HORIZONTAL","itemSpacing":0}});
        fields
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({"id":id,"type":"FRAME","fields":fields})
    };
    serde_json::from_value(
        json!({"fileKey":"layout-defaults","roots":["d","m"],"nodes":{
        "d":node("d",1440,desktop),"m":node("m",360,mobile)},"diagnostics":[]}),
    )
    .unwrap()
}

#[test]
fn mobile_center_resets_only_known_desktop_min_defaults() {
    for desktop in [
        json!({"primaryAxisAlignItems":"MIN"}),
        json!({"inferredAutoLayout":{"layoutMode":"HORIZONTAL","primaryAxisAlignItems":"MIN"}}),
    ] {
        let source = output(
            &frames(desktop, json!({"primaryAxisAlignItems":"CENTER"})),
            true,
        );
        assert!(
            source.contains("justifyContent={[\"center\",null,null,null,\"initial\"]}"),
            "{source}"
        );
    }
    let reverse = output(
        &frames(
            json!({"primaryAxisAlignItems":"CENTER"}),
            json!({"primaryAxisAlignItems":"MIN"}),
        ),
        true,
    );
    assert!(
        reverse.contains("justifyContent={[\"initial\",null,null,null,\"center\"]}"),
        "{reverse}"
    );
}

#[test]
fn unknown_main_alignment_cannot_be_reset_and_verified() {
    for desktop in [
        json!({}),
        json!({"primaryAxisAlignItems":null}),
        json!({"primaryAxisAlignItems":{"$unsupported":"mixed"}}),
    ] {
        let source = output(
            &frames(desktop, json!({"primaryAxisAlignItems":"CENTER"})),
            false,
        );
        assert!(!source.contains("justifyContent={["), "{source}");
    }
    let mut snapshot = frames(
        json!({"primaryAxisAlignItems":"MIN"}),
        json!({"primaryAxisAlignItems":"CENTER"}),
    );
    snapshot
        .nodes
        .get_mut("d")
        .unwrap()
        .field_errors
        .insert("primaryAxisAlignItems".into(), "read failed".into());
    let source = output(&snapshot, false);
    assert!(!source.contains("justifyContent={["), "{source}");
}

#[test]
fn captured_home_ctas_retain_mobile_center_and_desktop_start() {
    // Exact CTA subtrees under captured viewport roots; unrelated root children
    // are pruned and direct parent links adapted only to bound this regression.
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/home-product-cta-defaults.json")).unwrap();
    let source = captured_output(
        &snapshot,
        &["51:5685", "51:5704", "51:6663", "51:6684"],
        &["justifyContent"],
    );
    assert_eq!(
        source
            .matches("justifyContent={[\"center\",null,null,null,\"initial\"]}")
            .count(),
        2,
        "{source}"
    );
    if let Ok(path) = std::env::var("DEVUP_CTA_DEFAULT_PROBE_OUTPUT") {
        let result = merge_breakpoints(
            &snapshot,
            &CodegenOptions {
                component_name: Some("CapturedCtas".into()),
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        std::fs::write(path, result.tsx).unwrap();
    }
}

#[test]
fn captured_news_gap_axes_survive_and_about_grid_uses_its_own_source_axes() {
    for (fixture, column, row) in [
        (
            include_str!("fixtures/news-grid-gap-aliases.json"),
            "32px",
            "48px",
        ),
        (
            include_str!("fixtures/about-grid-gap-aliases.json"),
            "40px",
            "48px",
        ),
    ] {
        let snapshot: Snapshot = serde_json::from_str(fixture).unwrap();
        let source = captured_output(
            &snapshot,
            &["73:4686", "73:4766", "41:771", "42:964"],
            &["gap", "columnGap", "rowGap"],
        );
        assert!(
            source.contains(&format!(
                "columnGap={{[\"{}\",null,null,null,\"{column}\"]}}",
                if column == "32px" { "32px" } else { "20px" }
            )) || source.contains(&format!("columnGap=\"{column}\"")),
            "{source}"
        );
        assert!(
            source.contains(&format!(
                "rowGap={{[\"{}\",null,null,null,\"{row}\"]}}",
                if column == "32px" { "32px" } else { "20px" }
            )),
            "{source}"
        );
    }
}

#[test]
fn captured_gap_axes_also_merge_when_wrap_distribution_is_at_mobile() {
    let mut snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/news-grid-gap-aliases.json")).unwrap();
    for (id, mode, wrap, cross) in [
        ("73:4686", "VERTICAL", "NO_WRAP", 0),
        ("73:4766", "HORIZONTAL", "WRAP", 48),
    ] {
        let fields = &mut snapshot.nodes.get_mut(id).unwrap().fields;
        fields.insert("layoutMode".into(), json!(mode));
        fields.insert("layoutWrap".into(), json!(wrap));
        fields.insert("itemSpacing".into(), json!(32));
        fields.insert("counterAxisSpacing".into(), json!(cross));
        fields.get_mut("inferredAutoLayout").unwrap()["layoutMode"] = json!(mode);
        fields.get_mut("inferredAutoLayout").unwrap()["itemSpacing"] = json!(32);
    }
    let source = captured_output(
        &snapshot,
        &["73:4686", "73:4766"],
        &["gap", "columnGap", "rowGap"],
    );
    assert!(source.contains("columnGap=\"32px\""), "{source}");
    assert!(
        source.contains("rowGap={[\"48px\",null,null,null,\"32px\"]}"),
        "{source}"
    );
}

#[test]
fn unknown_gap_reads_do_not_gain_synthetic_verified_axes() {
    for field in ["itemSpacing", "counterAxisSpacing", "inferredAutoLayout"] {
        let mut snapshot: Snapshot =
            serde_json::from_str(include_str!("fixtures/news-grid-gap-aliases.json")).unwrap();
        snapshot
            .nodes
            .get_mut("73:4766")
            .unwrap()
            .field_errors
            .insert(field.into(), "read failed".into());
        let result = merge_breakpoints(
            &snapshot,
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        let source: String = result.tsx.split_whitespace().collect();
        // Without trustworthy per-frame spacing the old shorthand/axis output
        // remains; the normalizer must not assert two known mobile axes.
        assert!(
            source.contains("gap={[\"32px\",null,null,null,\"initial\"]}"),
            "{source}"
        );
        assert!(!source.contains("columnGap=\"32px\""), "{source}");
    }
}
