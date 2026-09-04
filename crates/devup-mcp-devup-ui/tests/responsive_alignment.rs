//! The same screen at three widths, lined up.

use std::{fs, path::PathBuf};

use devup_mcp_devup_ui::codegen::responsive::{DivergenceReason, breakpoints, divergences};
use devup_mcp_figma::Snapshot;

fn capture(name: &str) -> Option<Snapshot> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/local-screens")
        .join(name);
    let raw = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    serde_json::from_value(value.get("snapshot").cloned().unwrap_or(value)).ok()
}

/// One width is a screen, not a screen that changes.
#[test]
fn a_single_width_has_nothing_to_line_up() {
    let Some(snapshot) = capture("bp-desktop.json") else {
        eprintln!("no capture; skipping");
        return;
    };
    assert!(breakpoints(&snapshot).is_empty());
}

/// Narrowest first, because that is the order the arrays are written in.
#[test]
fn widths_are_ordered_the_way_the_array_is() {
    let Some(snapshot) = capture("bp-family.json") else {
        eprintln!("no capture; skipping");
        return;
    };
    let found = breakpoints(&snapshot);
    let names = found
        .iter()
        .map(|breakpoint| {
            snapshot.nodes[&breakpoint.node_id]
                .typed_view()
                .name()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["mobile", "tablet", "desktop"]);
}

/// The reference keeps two of this screen's four children twice — the banner
/// and the content section — each shown at its own widths, and merges the rest.
/// Those are the places the widths part company, and they are what shows up
/// here: the banner at the top level, and three shapes inside the section.
///
/// Nothing from the Header or Footer appears. Both are instances, both hold a
/// different variant per width, and both are meant to: descending into them
/// reported six differences that were the components doing their job.
#[test]
fn the_places_the_widths_part_company_are_named_and_no_others() {
    let Some(snapshot) = capture("bp-family.json") else {
        eprintln!("no capture; skipping");
        return;
    };
    let found = divergences(&snapshot, &breakpoints(&snapshot));
    let name_of = |id: &String| {
        snapshot.nodes[id]
            .typed_view()
            .name()
            .unwrap_or_default()
            .to_owned()
    };

    assert_eq!(found.len(), 4, "{found:?}");

    let banner = &found[0];
    assert_eq!(banner.path, vec![0]);
    assert_eq!(banner.reason, DivergenceReason::ChildCount);
    assert_eq!(name_of(&banner.node_id), "main banner");

    // The other three sit under the section, which is the second region the
    // reference keeps twice.
    assert!(
        found[1..].iter().all(|divergence| divergence.path[0] == 2),
        "{found:?}"
    );

    // The header is child 1 and the footer child 3; neither is walked into.
    assert!(
        !found
            .iter()
            .any(|divergence| matches!(divergence.path.first(), Some(1) | Some(3))),
        "an instance was descended into: {found:?}"
    );
}
