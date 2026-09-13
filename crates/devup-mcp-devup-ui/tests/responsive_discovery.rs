//! Which roots count as the same screen drawn at several widths.
//!
//! The plugin names a breakpoint family `mobile` / `tablet` / `desktop`, and
//! discovery read those names. A file that does not use them — Braillify
//! Studio draws `AUTH-01` at 1920 and again at 375, both directly under the
//! `로그인 화면` Section — had no family at all, and asking for `responsiveTsx`
//! over the two frames answered "no mergeable breakpoint frames".
//!
//! The widths are already placed by size: `slot_of_width` is documented "a
//! width is placed by how wide it is, not by what its frame is called". These
//! fix discovery to agree with that, without letting unrelated frames merge.

use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::json;

/// A snapshot whose roots are the given `(id, name, width)` frames.
fn snapshot(frames: &[(&str, &str, f64)]) -> Snapshot {
    let nodes = frames
        .iter()
        .map(|(id, name, width)| {
            (
                (*id).to_owned(),
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
        "roots": frames.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
        "nodes": nodes,
        "diagnostics": [],
    }))
    .expect("fixture snapshot")
}

fn merged_slots(frames: &[(&str, &str, f64)]) -> Option<Vec<usize>> {
    merge_breakpoints(&snapshot(frames), &CodegenOptions::default())
        .expect("merge must not error")
        .map(|merged| merged.slots)
}

/// The reported case: one screen, two widths, a name the plugin does not use.
#[test]
fn one_name_at_two_widths_is_a_breakpoint_family() {
    let slots = merged_slots(&[
        ("777:510", "AUTH-01", 1920.0),
        ("789:1425", "AUTH-01", 375.0),
    ])
    .expect("1920 and 375 of one screen are a family");
    assert_eq!(slots, vec![0, 4], "375 is slot 0 and 1920 is slot 4");
}

/// The plugin's own naming keeps working and still ranks by width.
#[test]
fn the_plugin_breakpoint_names_still_merge() {
    let slots = merged_slots(&[
        ("1:1", "mobile", 360.0),
        ("1:2", "tablet", 992.0),
        ("1:3", "desktop", 1920.0),
    ])
    .expect("the plugin family merges as before");
    assert_eq!(slots, vec![0, 2, 4]);
}

/// Two frames of one name at one width are not two widths. Braillify Studio
/// draws `AUTH-01` at 375 twice — the light and the dark rendering — and
/// folding one onto the other would claim a breakpoint the design never drew.
#[test]
fn one_name_twice_at_one_width_is_not_a_family() {
    assert_eq!(
        merged_slots(&[
            ("789:1425", "AUTH-01", 375.0),
            ("789:1491", "AUTH-01", 375.0)
        ]),
        None,
        "same width, so there is no second breakpoint"
    );
}

/// Different screens are never folded together, however their widths differ.
/// `AUTH-01` and `AUTH-02` are two designs, not one design at two sizes.
#[test]
fn different_names_are_never_merged_on_width_alone() {
    assert_eq!(
        merged_slots(&[
            ("777:510", "AUTH-01", 1920.0),
            ("789:1540", "AUTH-02", 375.0)
        ]),
        None,
        "unrelated screens must not merge"
    );
}

/// A single root is a screen, not a screen that changes.
#[test]
fn a_lone_frame_is_not_a_family() {
    assert_eq!(merged_slots(&[("777:510", "AUTH-01", 1920.0)]), None);
}
