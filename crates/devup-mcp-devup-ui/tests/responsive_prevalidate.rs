//! Deciding a breakpoint family without collecting one.
//!
//! `responsiveTsx` was refused inside projection, which runs only after the
//! collection it needs. Two frames of one Braillify Studio screen spent 77
//! seconds and ten Figma reads to be told the snapshot had no mergeable
//! breakpoint frames — a verdict their names and widths settle on their own,
//! and a Section index carries both for every candidate it lists.
//!
//! So the rule has to be answerable twice, and the two answers have to agree.

use devup_mcp_devup_ui::codegen::responsive::{breakpoint_family_is_possible, breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn snapshot(frames: &[(&str, f64)]) -> Snapshot {
    let nodes = frames
        .iter()
        .enumerate()
        .map(|(position, (name, width))| {
            let id = format!("1:{position}");
            (
                id.clone(),
                json!({
                    "id": id,
                    "type": "FRAME",
                    "fields": {
                        "name": name,
                        "width": width,
                        "height": 812.0,
                        "layoutMode": "VERTICAL",
                        "childrenIds": [],
                    },
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::from_value(json!({
        "fileKey": "qFil2x88O3JOIzWNuXEE0M",
        "version": null,
        "roots": (0..frames.len()).map(|i| format!("1:{i}")).collect::<Vec<_>>(),
        "nodes": nodes,
        "diagnostics": [],
    }))
    .expect("fixture snapshot")
}

/// Every selection the two can disagree about, and the assertion that they do
/// not. This is what keeps the cheap answer honest: a rule changed on one
/// side and not the other fails here, in milliseconds, instead of in an
/// export that collected first and refused afterwards.
#[test]
fn the_predicate_agrees_with_discovery() {
    let selections: [Vec<(&str, f64)>; 8] = [
        // The reported case: one screen, two widths, a name the plugin does
        // not use.
        vec![("AUTH-01", 1920.0), ("AUTH-01", 375.0)],
        // The plugin's own family.
        vec![("mobile", 360.0), ("tablet", 992.0), ("desktop", 1920.0)],
        vec![("mobile", 360.0), ("desktop", 1920.0)],
        // One width is a screen, not a screen that changes.
        vec![("mobile", 360.0)],
        vec![("AUTH-01", 1920.0)],
        // One name twice is still one width.
        vec![("mobile", 360.0), ("mobile", 375.0)],
        // Unrelated screens.
        vec![("AUTH-01", 1920.0), ("AUTH-02", 375.0)],
        vec![],
    ];
    for frames in selections {
        assert_eq!(
            breakpoint_family_is_possible(&frames),
            !breakpoints(&snapshot(&frames)).is_empty(),
            "the two rules disagree about {frames:?}"
        );
    }
}

/// The predicate reads names and widths only, so a Section index answers it
/// as well as a snapshot does — which is the whole point of having it.
#[test]
fn the_plugin_family_is_recognized_from_names_and_widths_alone() {
    assert!(breakpoint_family_is_possible(&[
        ("mobile", 360.0),
        ("desktop", 1920.0)
    ]));
    assert!(!breakpoint_family_is_possible(&[("desktop", 1920.0)]));
}
