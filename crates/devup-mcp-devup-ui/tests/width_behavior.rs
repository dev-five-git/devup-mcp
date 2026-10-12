//! A flat 360px mobile screen, every element placed by its own coordinates,
//! as a consumer widened it to 100%: a card's background apart from its rows,
//! a badge's background apart from its text, a QR tile left behind on the
//! right, a 1px rule that kept its length, and a notice that stopped being
//! centred. The geometry mirrors the reported screens; the text does not.

use std::collections::BTreeMap;

use devup_mcp_devup_ui::codegen::width::{Anchor, WidthPolicy, width_behavior};
use devup_mcp_devup_ui::codegen::{CodegenOptions, CodegenOutput, generate_component};
use devup_mcp_figma::{Snapshot, SnapshotChunk, merge_chunks};
use serde_json::{Value, json};

const ROOT: &str = "2:0";

fn shape(id: &str, name: &str, x: f64, y: f64, w: f64, h: f64, color: &str) -> Value {
    json!({"id": id, "type": "RECTANGLE", "fields": {
        "name": name, "parentId": ROOT, "childrenIds": [], "visible": true,
        "layoutPositioning": "AUTO", "constraints": {"horizontal": "MIN", "vertical": "MIN"},
        "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
        "x": x, "y": y, "width": w, "height": h,
        "absoluteBoundingBox": {"x": 1000.0 + x, "y": y, "width": w, "height": h},
        "absoluteRenderBounds": {"x": 1000.0 + x, "y": y, "width": w, "height": h},
        "fills": [{"type": "SOLID", "visible": true, "color": color_value(color)}]
    }, "extra": {}, "fieldErrors": {}})
}

fn text(id: &str, characters: &str, x: f64, y: f64, w: f64, h: f64, align: &str) -> Value {
    json!({"id": id, "type": "TEXT", "fields": {
        "name": characters, "parentId": ROOT, "visible": true,
        "layoutPositioning": "AUTO", "constraints": {"horizontal": "MIN", "vertical": "MIN"},
        "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
        "x": x, "y": y, "width": w, "height": h,
        "absoluteBoundingBox": {"x": 1000.0 + x, "y": y, "width": w, "height": h},
        "characters": characters, "styledTextSegments": [{"characters": characters}],
        "textAutoResize": "WIDTH_AND_HEIGHT", "textAlignHorizontal": align,
        "fontSize": (h * 0.78).round(), "lineHeight": {"unit": "PIXELS", "value": h},
        "fills": [{"type": "SOLID", "visible": true, "color": color_value("111827")}]
    }, "extra": {}, "fieldErrors": {}})
}

/// A pen-tool rule: its bounding box has no height, and only the 1px stroke
/// with round caps paints, half a pixel past each end.
fn rule(id: &str, x: f64, y: f64, w: f64) -> Value {
    json!({"id": id, "type": "VECTOR", "fields": {
        "name": "Vector 1", "parentId": ROOT, "childrenIds": [], "visible": true,
        "layoutPositioning": "AUTO", "constraints": {"horizontal": "MIN", "vertical": "MIN"},
        "x": x, "y": y, "width": w, "height": 0.0,
        "absoluteBoundingBox": {"x": 1000.0 + x, "y": y, "width": w, "height": 0.0},
        "absoluteRenderBounds": {"x": 999.5 + x, "y": y - 0.5, "width": w + 1.0, "height": 1.0},
        "fills": [], "strokeWeight": 1.0, "strokeAlign": "CENTER",
        "strokes": [{"type": "SOLID", "visible": true, "color": color_value("E5E7EB")}]
    }, "extra": {}, "fieldErrors": {}})
}

fn icon(id: &str, x: f64, y: f64, size: f64) -> Value {
    json!({"id": id, "type": "VECTOR", "fields": {
        "name": "qr", "parentId": ROOT, "childrenIds": [], "visible": true,
        "layoutPositioning": "AUTO", "constraints": {"horizontal": "MIN", "vertical": "MIN"},
        "x": x, "y": y, "width": size, "height": size,
        "absoluteBoundingBox": {"x": 1000.0 + x, "y": y, "width": size, "height": size},
        "absoluteRenderBounds": {"x": 1000.0 + x, "y": y, "width": size, "height": size},
        "fills": [{"type": "SOLID", "visible": true, "color": color_value("111827")}]
    }, "extra": {}, "fieldErrors": {}})
}

fn color_value(hex: &str) -> Value {
    let channel = |at: usize| f64::from(u8::from_str_radix(&hex[at..at + 2], 16).unwrap()) / 255.0;
    json!({"r": channel(0), "g": channel(2), "b": channel(4)})
}

fn screen() -> Snapshot {
    let children = [
        shape("2:1", "page", 0.0, 0.0, 360.0, 760.0, "F3F4F6"),
        rule("2:2", 0.0, 56.0, 360.0),
        text("2:3", "Today", 20.0, 17.0, 87.0, 22.0, "LEFT"),
        shape("2:4", "card", 20.0, 193.0, 320.0, 200.0, "FFFFFF"),
        text("2:5", "School", 36.0, 209.0, 80.0, 24.0, "LEFT"),
        shape("2:6", "badge", 259.89, 210.0, 64.11, 22.0, "ECFDF3"),
        text("2:7", "20 left", 269.44, 214.0, 45.0, 14.0, "LEFT"),
        rule("2:8", 36.0, 361.0, 288.0),
        shape("2:9", "food card", 20.0, 420.0, 320.0, 80.0, "FFFFFF"),
        shape("2:10", "qr tile", 276.0, 436.0, 48.0, 48.0, "F3F4F6"),
        icon("2:11", 280.0, 440.0, 40.0),
        text(
            "2:12",
            "Staff can help on site",
            55.5,
            710.0,
            249.0,
            20.0,
            "CENTER",
        ),
    ];
    let ids = children
        .iter()
        .map(|child| child["id"].clone())
        .collect::<Vec<_>>();
    let mut nodes = vec![json!({"id": ROOT, "type": "FRAME", "fields": {
        "name": "Screen", "childrenIds": ids, "visible": true,
        "layoutMode": "NONE", "layoutPositioning": "AUTO",
        "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
        "width": 360.0, "height": 760.0, "clipsContent": true,
        "constraints": {"horizontal": "MIN", "vertical": "MIN"},
        "absoluteBoundingBox": {"x": 1000.0, "y": 0.0, "width": 360.0, "height": 760.0},
        "fills": [{"type": "SOLID", "visible": true, "color": color_value("FFFFFF")}],
        "parentId": "0:1", "parentType": "SECTION"
    }, "extra": {}, "fieldErrors": {}})];
    nodes.extend(children);
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "width-case", "version": "1", "rootIds": [ROOT],
        "nodes": nodes, "diagnostics": []
    }))
    .unwrap();
    merge_chunks(vec![chunk]).unwrap()
}

fn generate(policy: Option<WidthPolicy>) -> CodegenOutput {
    generate_component(
        &screen(),
        ROOT,
        &CodegenOptions {
            width_policy: policy,
            asset_names_per_node: true,
            ..CodegenOptions::default()
        },
    )
    .unwrap()
}

fn opening<'a>(output: &'a CodegenOutput, id: &str) -> &'a str {
    let range = output
        .source_map
        .entries
        .iter()
        .find(|entry| entry.node_id.as_deref() == Some(id) && entry.resolution == "node")
        .and_then(|entry| entry.generated_range.as_ref())
        .unwrap_or_else(|| panic!("{id} was not emitted:\n{}", output.tsx));
    output.tsx[range.start..range.end]
        .split_once('>')
        .unwrap()
        .0
}

fn has(opening: &str, prop: &str) -> bool {
    opening
        .split_whitespace()
        .any(|attribute| attribute.trim_end_matches('/') == prop)
}

fn findings_by_node(report: &Value) -> BTreeMap<String, Value> {
    report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["nodeId"].as_str().unwrap().to_owned(),
                finding.clone(),
            )
        })
        .collect()
}

fn ui_policy() -> WidthPolicy {
    WidthPolicy {
        anchors: BTreeMap::from([
            ("2:2".to_owned(), Anchor::Stretch),
            ("2:4".to_owned(), Anchor::Stretch),
            ("2:6".to_owned(), Anchor::Right),
            ("2:8".to_owned(), Anchor::Stretch),
            ("2:9".to_owned(), Anchor::Stretch),
            ("2:10".to_owned(), Anchor::Right),
            ("2:12".to_owned(), Anchor::Center),
        ]),
    }
}

#[test]
fn the_default_export_is_unchanged_and_says_it_holds_only_at_the_drawn_width() {
    let output = generate(None);
    let root = opening(&output, ROOT);
    assert!(has(root, "w=\"360px\""), "{root}");
    assert!(has(opening(&output, "2:6"), "left=\"259.89px\""));
    assert!(has(opening(&output, "2:8"), "maskSize=\"contain\""));

    let report = width_behavior(&screen(), ROOT, &output);
    assert_eq!(report["designWidth"], 360.0);
    assert_eq!(report["root"]["fluid"], false);
    assert_eq!(report["verification"]["runtime"], "not-performed");
    assert!(
        report["exactnessScope"]
            .as_str()
            .unwrap()
            .contains("drawn width only")
    );
    let findings = findings_by_node(&report);

    let badge = &findings["2:6"];
    assert_eq!(badge["kind"], "right-aligned-but-held-left");
    assert_eq!(badge["status"], "consumer-policy-required");
    assert_eq!(
        badge["candidates"][1],
        json!({"anchor": "right", "rightPx": 36.0})
    );
    assert_eq!(
        badge["carries"],
        json!(["2:7"]),
        "the badge text goes with it"
    );
    assert!(!findings.contains_key("2:7"));

    let qr = &findings["2:10"];
    assert_eq!(qr["kind"], "right-aligned-but-held-left");
    assert_eq!(qr["carries"], json!(["2:11"]));

    for (id, line) in [
        ("2:2", true),
        ("2:4", false),
        ("2:8", true),
        ("2:9", false),
        ("2:12", false),
    ] {
        let finding = &findings[id];
        assert_eq!(finding["kind"], "centered-but-held-left", "{id}: {finding}");
        assert_eq!(finding["straightLineMask"] == true, line, "{id}");
    }
    assert_eq!(
        findings["2:8"]["candidates"][2],
        json!({"anchor": "stretch", "leftPx": 35.5, "rightPx": 35.5})
    );
    assert!(
        !findings.contains_key("2:1"),
        "the page background already follows"
    );
    assert!(!findings.contains_key("2:3") && !findings.contains_key("2:5"));
    assert_eq!(report["summary"]["shapeSizeOmitted"], 4);
    assert!(report.get("widthPolicy").is_none());
}

#[test]
fn a_caller_policy_keeps_gutters_anchors_and_containment_at_any_width() {
    let output = generate(Some(ui_policy()));
    let root = opening(&output, ROOT);
    assert!(
        has(root, "w=\"100%\"") && has(root, "h=\"760px\""),
        "{root}"
    );

    let page = opening(&output, "2:1");
    assert!(has(page, "boxSize=\"100%\""), "{page}");

    let card = opening(&output, "2:4");
    assert!(
        has(card, "left=\"20px\"") && has(card, "right=\"20px\""),
        "{card}"
    );
    assert!(!card.contains(" w=") && has(card, "h=\"200px\""), "{card}");

    let badge = opening(&output, "2:6");
    assert!(has(badge, "right=\"36px\""), "{badge}");
    assert!(
        has(badge, "w=\"64.11px\"") && has(badge, "h=\"22px\""),
        "{badge}"
    );
    assert!(!badge.contains("left="), "{badge}");
    let badge_text = opening(&output, "2:7");
    assert!(has(badge_text, "right=\"45.56px\""), "{badge_text}");

    let qr_icon = opening(&output, "2:11");
    assert!(has(qr_icon, "right=\"40px\""), "{qr_icon}");
    assert!(has(opening(&output, "2:10"), "right=\"36px\""));

    let divider = opening(&output, "2:8");
    assert!(
        has(divider, "left=\"35.5px\"") && has(divider, "right=\"35.5px\""),
        "{divider}"
    );
    assert!(!divider.contains(" w="), "{divider}");
    assert!(divider.contains("maskSize=\"100% 100%\""), "{divider}");

    let notice = opening(&output, "2:12");
    assert!(has(notice, "left=\"50%\""), "{notice}");
    assert!(
        notice.contains("transform=\"translateX(-50%)\""),
        "{notice}"
    );

    let card_title = opening(&output, "2:5");
    assert!(has(card_title, "left=\"36px\""), "{card_title}");

    let report = width_behavior(&screen(), ROOT, &output);
    assert_eq!(report["root"]["fluid"], true);
    assert_eq!(report["widthPolicy"]["applied"], true);
    let findings = findings_by_node(&report);
    for id in [
        "2:2", "2:4", "2:6", "2:7", "2:8", "2:9", "2:10", "2:11", "2:12",
    ] {
        assert!(
            !findings.contains_key(id),
            "{id} is decided: {}",
            findings[id]
        );
    }
}

#[test]
fn policy_output_keeps_every_attribute_mapped_and_the_drawn_width_exact() {
    let default = generate(None);
    let policy = generate(Some(ui_policy()));
    let unmapped = |output: &CodegenOutput| {
        output
            .diagnostics
            .iter()
            .filter(|d| d.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED")
            .map(|d| {
                format!(
                    "{:?} {}",
                    d.node_id,
                    d.details.as_ref().unwrap()["generatedProperty"]
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(unmapped(&policy), Vec::<String>::new());
    assert!(unmapped(&default).is_empty());
    for id in ["2:4", "2:6", "2:8", "2:9", "2:10"] {
        for axis in ["width", "height"] {
            let pair = format!("{id}#{axis}");
            assert!(
                !policy.fidelity_report.uncovered_layout.contains(&pair),
                "{pair} uncovered under the policy: {:?}",
                policy.fidelity_report.uncovered_layout
            );
        }
    }
    assert!(
        policy.fidelity_report.uncovered_layout.len()
            < default.fidelity_report.uncovered_layout.len(),
        "policy {:?} default {:?}",
        policy.fidelity_report.uncovered_layout,
        default.fidelity_report.uncovered_layout
    );
}

/// An auto-layout card with a badge pinned ABSOLUTE in its top-right corner,
/// held by a MIN constraint as Figma defaults it.
fn auto_layout_card() -> Snapshot {
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "width-case", "version": "1", "rootIds": ["3:0"],
        "nodes": [
            {"id": "3:0", "type": "FRAME", "fields": {
                "name": "Card", "childrenIds": ["3:1", "3:2"], "visible": true,
                "layoutMode": "VERTICAL", "layoutPositioning": "AUTO",
                "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                "itemSpacing": 0.0, "paddingTop": 16.0, "paddingBottom": 16.0,
                "paddingLeft": 16.0, "paddingRight": 16.0,
                "width": 320.0, "height": 120.0,
                "absoluteBoundingBox": {"x": 0.0, "y": 0.0, "width": 320.0, "height": 120.0},
                "parentId": "0:1", "parentType": "SECTION"
            }, "extra": {}, "fieldErrors": {}},
            {"id": "3:1", "type": "TEXT", "fields": {
                "name": "Title", "parentId": "3:0", "visible": true,
                "layoutPositioning": "AUTO", "layoutSizingHorizontal": "FILL",
                "layoutSizingVertical": "HUG", "x": 16.0, "y": 16.0,
                "width": 288.0, "height": 24.0, "characters": "Title",
                "styledTextSegments": [{"characters": "Title"}], "textAutoResize": "HEIGHT",
                "absoluteBoundingBox": {"x": 16.0, "y": 16.0, "width": 288.0, "height": 24.0}
            }, "extra": {}, "fieldErrors": {}},
            {"id": "3:2", "type": "RECTANGLE", "fields": {
                "name": "badge", "parentId": "3:0", "childrenIds": [], "visible": true,
                "layoutPositioning": "ABSOLUTE",
                "constraints": {"horizontal": "MIN", "vertical": "MIN"},
                "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                "x": 240.0, "y": 16.0, "width": 64.0, "height": 22.0,
                "absoluteBoundingBox": {"x": 240.0, "y": 16.0, "width": 64.0, "height": 22.0},
                "fills": [{"type": "SOLID", "visible": true, "color": color_value("ECFDF3")}]
            }, "extra": {}, "fieldErrors": {}}
        ],
        "diagnostics": []
    }))
    .unwrap();
    merge_chunks(vec![chunk]).unwrap()
}

#[test]
fn an_absolute_node_re_anchored_by_the_caller_is_verified_at_the_drawn_width() {
    let snapshot = auto_layout_card();
    let policy = WidthPolicy {
        anchors: BTreeMap::from([("3:2".to_owned(), Anchor::Right)]),
    };
    let output = generate_component(
        &snapshot,
        "3:0",
        &CodegenOptions {
            width_policy: Some(policy),
            ..CodegenOptions::default()
        },
    )
    .unwrap();
    let badge = opening(&output, "3:2");
    assert!(has(badge, "right=\"16px\""), "{badge}");
    let absolute = output
        .diagnostics
        .iter()
        .find(|d| {
            d.node_id.as_deref() == Some("3:2") && d.code.starts_with("DEVUP_CODEGEN_ABSOLUTE")
        })
        .expect("absolute diagnostic");
    let components = &absolute.details.as_ref().unwrap()["components"];
    assert_eq!(
        components["horizontal"]["state"], "verified",
        "{components}"
    );
    assert_eq!(components["horizontal"]["anchorSource"], "caller");
}

#[test]
fn a_stretch_constraint_is_reported_by_default_and_written_under_any_policy() {
    let mut snapshot = screen();
    snapshot.nodes.get_mut("2:4").unwrap().fields.insert(
        "constraints".into(),
        json!({"horizontal": "STRETCH", "vertical": "MIN"}),
    );
    let default = generate_component(&snapshot, ROOT, &CodegenOptions::default()).unwrap();
    let card = &findings_by_node(&width_behavior(&snapshot, ROOT, &default))["2:4"];
    assert_eq!(card["kind"], "constraint-not-expressed");
    assert_eq!(card["status"], "generator-approximation");

    let policy = generate_component(
        &snapshot,
        ROOT,
        &CodegenOptions {
            width_policy: Some(WidthPolicy::default()),
            ..CodegenOptions::default()
        },
    )
    .unwrap();
    let card = opening(&policy, "2:4");
    assert!(
        has(card, "left=\"20px\"") && has(card, "right=\"20px\""),
        "{card}"
    );
    let badge = opening(&policy, "2:6");
    assert!(
        has(badge, "left=\"259.89px\""),
        "constraint MIN stays: {badge}"
    );
    assert!(!findings_by_node(&width_behavior(&snapshot, ROOT, &policy)).contains_key("2:4"));
}

#[test]
fn a_validator_violation_and_its_source_map_entry_share_an_element_index() {
    let output = generate(None);
    let report = devup_mcp_devup_ui::ui_validate::validate_devup_ui_tsx(&output.tsx, None, false);
    let violation = report
        .violations
        .iter()
        .find(|violation| {
            violation.rule == "hardcoded-color"
                && violation.context.get("attribute") == Some(&json!("bg"))
        })
        .expect("a hardcoded background");
    let element = violation.context["elementIndex"].as_u64().unwrap() as usize;
    let entries = output.source_map.property_entries();
    let linked = entries
        .iter()
        .find(|entry| {
            entry.element_index == Some(element)
                && entry
                    .generated_property
                    .as_deref()
                    .is_some_and(|generated| generated.starts_with("bg="))
        })
        .unwrap_or_else(|| panic!("no entry at element {element}: {entries:#?}"));
    assert_eq!(linked.property.as_deref(), Some("fills"));
}

#[test]
fn a_container_held_by_its_own_constraint_does_not_move_its_contents() {
    let mut snapshot = screen();
    snapshot.nodes.get_mut("2:6").unwrap().fields.insert(
        "constraints".into(),
        json!({"horizontal": "MAX", "vertical": "MIN"}),
    );
    let output = generate_component(&snapshot, ROOT, &CodegenOptions::default()).unwrap();
    assert!(has(opening(&output, "2:6"), "right=\"36px\""));
    assert!(has(opening(&output, "2:7"), "left=\"269.44px\""));
    let findings = findings_by_node(&width_behavior(&snapshot, ROOT, &output));
    let text = &findings["2:7"];
    assert_eq!(text["kind"], "group-anchor-mismatch");
    assert_eq!(text["confidence"], "high");
    assert_eq!(text["drawnInside"], "2:6");
}

/// A 64px loading indicator's dots, drawn right of centre, centred and
/// against the right edge as the live one was. Its host sizes it; no
/// viewport ever widens it, so "where does this dot go on a wider screen"
/// is noise there.
#[test]
fn a_component_sized_root_asks_no_viewport_questions() {
    let dots = [
        shape("2:1", "dot", 44.0, 0.0, 12.0, 12.0, "111827"),
        shape("2:2", "dot", 52.0, 0.0, 12.0, 12.0, "111827"),
        shape("2:3", "dot", 26.0, 0.0, 12.0, 12.0, "111827"),
    ];
    let mut nodes = vec![json!({"id": ROOT, "type": "FRAME", "fields": {
        "name": "Loading", "childrenIds": ["2:1", "2:2", "2:3"], "visible": true,
        "layoutMode": "NONE", "layoutPositioning": "AUTO",
        "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
        "width": 64.0, "height": 12.0,
        "constraints": {"horizontal": "MIN", "vertical": "MIN"},
        "absoluteBoundingBox": {"x": 1000.0, "y": 0.0, "width": 64.0, "height": 12.0},
        "parentId": "0:1", "parentType": "SECTION"
    }, "extra": {}, "fieldErrors": {}})];
    nodes.extend(dots);
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "width-case", "version": "1", "rootIds": [ROOT],
        "nodes": nodes, "diagnostics": []
    }))
    .unwrap();
    let snapshot = merge_chunks(vec![chunk]).unwrap();
    let output = generate_component(&snapshot, ROOT, &CodegenOptions::default()).unwrap();

    let report = width_behavior(&snapshot, ROOT, &output);
    assert_eq!(report["root"]["componentSized"], true);
    assert_eq!(report["summary"]["componentRootSuppressed"], 3, "{report}");
    assert_eq!(report["findings"], json!([]), "{report}");

    let screen_report = width_behavior(&screen(), ROOT, &generate(None));
    assert_eq!(screen_report["root"]["componentSized"], false);
    assert!(
        screen_report["summary"]
            .get("componentRootSuppressed")
            .is_none()
    );
}
