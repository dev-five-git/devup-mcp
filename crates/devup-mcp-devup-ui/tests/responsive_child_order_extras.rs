use devup_mcp_devup_ui::codegen::{
    CodegenOptions,
    responsive::{MergedScreen, merge_confirmed_breakpoints},
};
use devup_mcp_figma::Snapshot;
use serde_json::json;
fn fixture() -> Snapshot {
    serde_json::from_str(include_str!(
        "fixtures/vision-timeline-width-specific-spacers.json"
    ))
    .unwrap()
}
fn render(source: &Snapshot) -> MergedScreen {
    merge_confirmed_breakpoints(
        source,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap()
}
const DESKTOP: [&str; 3] = ["44:2843", "44:2845", "44:2847"];
#[test]
fn actual_vision_keeps_body_dot_spacer_desktop_and_dot_body_mobile() {
    let source = fixture();
    let output = render(&source);
    for parent in DESKTOP {
        let body = format!("I{parent};44:1985");
        let dot = format!("I{parent};44:1983");
        let spacer = format!("I{parent};44:2036");
        assert_eq!(
            source.nodes[parent].fields["childrenIds"],
            json!([body, dot, spacer])
        );
        for (id, index) in [(body, 0), (dot, 1), (spacer.clone(), 2)] {
            assert!(
                output
                    .contract
                    .iter()
                    .any(|entry| entry["generatedProperty"]
                        .as_str()
                        .is_some_and(|p| p.starts_with("order="))
                        && entry["derivedSource"]
                            .as_array()
                            .is_some_and(|sources| sources
                                .iter()
                                .any(|s| s["childNodeId"] == id && s["sourceIndex"] == index))),
                "Missing captured order for {id}: {:?}",
                output.contract
            );
        }
        let extra = output
            .elements
            .iter()
            .find(|e| {
                e.ids
                    .iter()
                    .any(|(width, id)| *width == 1440 && id == &spacer)
            })
            .unwrap();
        assert!(extra.hidden_at.contains(&360));
    }
    if let Ok(file) = std::env::var("DEVUP_VISION_ORDER_PROBE") {
        std::fs::write(&file, output.module("VisionOrderProbe")).unwrap();
        let elements = output
            .elements
            .iter()
            .enumerate()
            .map(|(index, e)| json!({"index":index,"ids":e.ids,"hiddenAt":e.hidden_at}))
            .collect::<Vec<_>>();
        std::fs::write(
            format!("{file}.elements.json"),
            serde_json::to_vec(&elements).unwrap(),
        )
        .unwrap();
    }
}
#[test]
fn unknown_declared_extra_or_parent_layout_does_not_guess_order() {
    for unknown in ["extra", "layout"] {
        let mut source = fixture();
        for parent in DESKTOP {
            if unknown == "extra" {
                source.nodes.remove(&format!("I{parent};44:2036"));
            } else {
                source
                    .nodes
                    .get_mut(parent)
                    .unwrap()
                    .fields
                    .remove("layoutMode");
            }
        }
        assert!(!render(&source).tsx.contains("order="), "{unknown}");
    }
}
#[test]
fn possible_unmatched_counterpart_is_ambiguous_not_a_proven_extra() {
    let mut source = fixture();
    for parent in DESKTOP {
        let name = source.nodes[&format!("I{parent};44:1985")].fields["name"].clone();
        source
            .nodes
            .get_mut(&format!("I{parent};44:2036"))
            .unwrap()
            .fields
            .insert("name".into(), name);
    }
    assert!(!render(&source).tsx.contains("order="));
}
#[test]
fn width_specific_extra_alone_does_not_require_order() {
    let mut source = fixture();
    for parent in DESKTOP {
        source.nodes.get_mut(parent).unwrap().fields.insert(
            "childrenIds".into(),
            json!([
                format!("I{parent};44:2036"),
                format!("I{parent};44:1983"),
                format!("I{parent};44:1985")
            ]),
        );
    }
    assert!(!render(&source).tsx.contains("order="));
}
