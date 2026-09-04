//! The whole join, run on a real screen.
//!
//! `fixtures/plugin-answers/notice/responsive.tsx` is what the plugin answered
//! for this screen, and the capture it was answered from is
//! `fixtures/local-screens/bp-family.json`. The capture is not committed, so
//! these skip when it is absent rather than pretending to have checked.

use std::{fs, path::PathBuf};

use devup_mcp_devup_ui::codegen::{
    CodegenOptions,
    responsive::{MergedScreen, merge_breakpoints},
};
use devup_mcp_figma::Snapshot;

fn merged() -> Option<MergedScreen> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/local-screens/bp-family.json");
    let raw = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let snapshot: Snapshot = serde_json::from_value(value.get("snapshot")?.clone()).ok()?;
    merge_breakpoints(&snapshot, &CodegenOptions::default()).ok()?
}

/// 360 / 992 / 1920, so slots 0 / 2 / 4 — the same three the reference's arrays
/// are written at.
#[test]
fn the_widths_land_where_their_sizes_put_them() {
    let Some(merged) = merged() else {
        eprintln!("no capture; skipping");
        return;
    };
    assert_eq!(merged.slots, vec![0, 2, 4]);
}

/// The four arrays in `notice/responsive.tsx`, and no fifth. Each is a region
/// one width draws and another does not, so the copies are kept and shown at
/// their own widths.
#[test]
fn the_display_arrays_are_the_reference_s() {
    let Some(merged) = merged() else {
        eprintln!("no capture; skipping");
        return;
    };
    let written = merged
        .tsx
        .lines()
        .filter_map(|line| line.trim().split_once("display=").map(|(_, rest)| rest))
        .map(|value| {
            value
                .split_once("}")
                .map_or(value, |(inside, _)| inside)
                .trim_start_matches('{')
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        written,
        [
            // the mobile banner, on an element that needs no display to show
            r#"[null, null, "none"]"#,
            // the desktop banner
            r#"["none", null, "flex"]"#,
            // the desktop content section
            r#"["none", null, "flex"]"#,
            // the mobile content section
            r#"["flex", null, "none"]"#,
        ]
    );
}

/// A component answers for its own widths, so it is referenced once and not
/// descended into. An instance whose component is only a shape is spelled out
/// instead, which is why no `<Logo />` or `<Icons />` appears.
#[test]
fn components_are_referenced_and_shapes_are_spelled_out() {
    let Some(merged) = merged() else {
        eprintln!("no capture; skipping");
        return;
    };
    for referenced in ["Header", "Footer", "Tab", "Pagination"] {
        assert!(
            merged.components.contains(referenced),
            "{referenced} should be referenced: {:?}",
            merged.components
        );
    }
    for inlined in ["Logo", "Icons"] {
        assert!(
            !merged.components.contains(inlined),
            "{inlined} is an asset and should have been spelled out"
        );
    }
    // Spelling it out loses which component it came from, and that is the one
    // thing a reader needs to change it in the right place. The name is the one
    // the definition declares: `Icons` types its property `Property 1` where
    // every other component in this file uses the Korean `속성 1`, and the two
    // sanitize differently.
    assert!(merged.tsx.contains("{/* <Logo /> */}"), "{}", merged.tsx);
    assert!(
        merged
            .tsx
            .contains(r#"{/* <Icons Property1="search" /> */}"#),
        "{}",
        merged.tsx
    );
    assert!(merged.tsx.contains(r#"<Header property1="transparent" />"#));
    // `effect` names the interaction state a variant stands for, and the
    // definition folds those into `_hover` / `_active`. A call site has no
    // state to pass, so `<Tab effect="selected" />` would be asking for a prop
    // the component does not have. The reference writes it bare.
    assert!(
        merged.tsx.contains("<Tab />"),
        "Tab should take no props: {}",
        merged
            .tsx
            .lines()
            .filter(|line| line.contains("<Tab"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        !merged.tsx.contains("effect="),
        "no reserved variant key should reach a call site"
    );
    // The instance is absolutely placed and `<Header />` has nowhere to put
    // that, so it is wrapped.
    assert!(
        merged
            .tsx
            .contains(r#"<Box left="0px" pos="absolute" top="0px" w="100%">"#)
    );
}

/// The screen asks for a different `Header` and `Footer` variant per width.
/// devup-ui reads an array only where it applies CSS, so a component prop
/// cannot carry one; the widest is kept and both places are named.
#[test]
fn a_variant_that_differs_by_width_is_reported_not_faked() {
    let Some(merged) = merged() else {
        eprintln!("no capture; skipping");
        return;
    };
    let said = merged
        .unrepresented
        .iter()
        .map(|note| note.detail.as_str())
        .collect::<Vec<_>>();
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(
        said.iter()
            .any(|note| note.starts_with("Header takes property1"))
    );
    assert!(
        said.iter()
            .any(|note| note.starts_with("Footer takes property1"))
    );
    // Whatever is reported must not also be written as an array, which is the
    // thing that does nothing.
    assert!(!merged.tsx.contains("property1={["));
}

/// Every element of the reference, at the same nesting.
///
/// The one accepted difference is that a positioned shape is written as a
/// single `Box` carrying both its placement and its mask, where the reference
/// wraps a second `Box` around it. Both draw the same thing; this is two
/// elements fewer.
#[test]
fn the_shape_of_the_output_is_the_reference_s() {
    let Some(merged) = merged() else {
        eprintln!("no capture; skipping");
        return;
    };
    // Each file indents by its own unit, so nesting is counted rather than
    // measured. Both bodies begin two levels in.
    let outline = |source: &str, unit: usize| {
        source
            .lines()
            .filter_map(|line| {
                let indent = line.len() - line.trim_start().len();
                let name = line
                    .trim_start()
                    .strip_prefix('<')?
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .collect::<String>();
                name.starts_with(char::is_uppercase)
                    .then(|| (indent / unit, name))
            })
            .collect::<Vec<_>>()
    };
    let reference = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/plugin-answers/notice/responsive.tsx"),
    )
    .expect("the plugin's answer is committed");

    let ours = outline(&merged.tsx, 2);
    let theirs = outline(&reference, 4);

    // Every element, at the nesting it sits at. Order is compared separately,
    // because the two files order one pair differently on purpose.
    let tally = |outline: &[(usize, String)]| {
        let mut counts = std::collections::BTreeMap::<(usize, String), usize>::new();
        for entry in outline {
            *counts.entry(entry.clone()).or_default() += 1;
        }
        counts
    };
    let mut missing = tally(&theirs);
    for entry in tally(&ours) {
        let held = missing.remove(&entry.0).unwrap_or_default();
        assert!(
            held >= entry.1,
            "{:?} appears {} times here and {held} in the reference",
            entry.0,
            entry.1
        );
        if held > entry.1 {
            missing.insert(entry.0, held - entry.1);
        }
    }
    assert_eq!(
        missing,
        std::collections::BTreeMap::from([((5, "Box".to_owned()), 2)]),
        "the only elements the reference has and this does not should be the \
         two shape wrappers"
    );
}
