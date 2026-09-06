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
use devup_mcp_figma::{Snapshot, UpstreamResult};

fn merged() -> Option<MergedScreen> {
    merged_from("bp-family.json")
}

/// The merge of a capture in `fixtures/local-screens`, or `None` when that
/// capture is not on this machine.
///
/// A capture may carry a `payload` beside its `snapshot` — the variables and
/// styles the export collected with it. They are what the converter names
/// tokens from, and without them a `$gray200` fill is written as the tail of
/// its variable ID and a `typography="h4"` as five font props, which is not
/// what the reference wrote and not what the server would write either. A
/// capture without one is converted as before.
fn merged_from(capture: &str) -> Option<MergedScreen> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/local-screens")
        .join(capture);
    let raw = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let snapshot: Snapshot = serde_json::from_value(value.get("snapshot")?.clone()).ok()?;
    let resource = |name: &str| {
        value
            .get("payload")
            .and_then(|payload| payload.get(name))
            .cloned()
            .map(|raw| UpstreamResult { raw })
    };
    let variables = resource("variables");
    let styles = resource("styles");
    let options =
        CodegenOptions::default().with_resource_results(variables.as_ref(), styles.as_ref());
    merge_breakpoints(&snapshot, &options).ok()?
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

/// Each element of a file with the nesting it sits at.
///
/// Each file indents by its own unit, so nesting is counted rather than
/// measured; both bodies begin two levels in.
fn outline(source: &str, unit: usize) -> Vec<(usize, String)> {
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
        .collect()
}

/// How many times each element appears at each nesting, so that two outlines
/// can be compared without depending on the order they were written in.
fn tally(outline: &[(usize, String)]) -> std::collections::BTreeMap<(usize, String), usize> {
    let mut counts = std::collections::BTreeMap::new();
    for entry in outline {
        *counts.entry(entry.clone()).or_default() += 1;
    }
    counts
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
    let reference = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/plugin-answers/notice/responsive.tsx"),
    )
    .expect("the plugin's answer is committed");

    let ours = outline(&merged.tsx, 2);
    let theirs = outline(&reference, 4);

    // Every element, at the nesting it sits at. Order is compared separately,
    // because the two files order one pair differently on purpose.
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

/// The merge reads widths, not the order they were written in.
///
/// This matters because a screen too large to capture in one call is captured
/// a width at a time and the parts are stitched back together, and whoever
/// stitches them chooses an order — `bp-family` was captured narrowest first,
/// while the Section index lists `about` widest first. `breakpoints` sorts by
/// rank, so neither choice reaches the output; nothing checked that, and the
/// existing tests could not, because the capture they run on is already in
/// ascending order.
#[test]
fn the_order_the_widths_were_stitched_in_does_not_reach_the_output() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/local-screens/bp-family.json");
    let Ok(raw) = fs::read_to_string(path) else {
        eprintln!("no capture; skipping");
        return;
    };
    let mut value: serde_json::Value = serde_json::from_str(&raw).expect("captured screen is json");
    let snapshot: Snapshot =
        serde_json::from_value(value["snapshot"].clone()).expect("captured snapshot");
    let forwards = merge_breakpoints(&snapshot, &CodegenOptions::default())
        .expect("merge")
        .expect("three widths merge");

    let roots = value["snapshot"]["roots"]
        .as_array()
        .expect("roots")
        .iter()
        .rev()
        .cloned()
        .collect::<Vec<_>>();
    value["snapshot"]["roots"] = serde_json::Value::Array(roots);
    let reversed_snapshot: Snapshot =
        serde_json::from_value(value["snapshot"].clone()).expect("reversed snapshot");
    let backwards = merge_breakpoints(&reversed_snapshot, &CodegenOptions::default())
        .expect("merge")
        .expect("three widths merge");

    assert_eq!(
        forwards.slots, backwards.slots,
        "the slots come from the widths"
    );
    assert_eq!(
        forwards.tsx, backwards.tsx,
        "and so does everything drawn from them"
    );
    assert_eq!(forwards.components, backwards.components);
    assert_eq!(forwards.unrepresented, backwards.unrepresented);
}

/// The same comparison for `about`, the screen whose widths go back.
///
/// It runs on nothing today: neither the capture nor the plugin's answer is on
/// disk, so this skips. It is written now because the alternative was reading
/// the two files side by side once and calling that checked — and a screen
/// that costs more reads than a day's allowance holds is one nobody will want
/// to check twice by hand. Drop `about-family.json` beside the other captures
/// and `about/responsive.tsx` beside the other answers, and the suite decides
/// it instead.
///
/// No difference is allowed for in advance. `notice` has one, and it is
/// written down there because it was understood first; here there is no
/// evidence of any, so anything that turns up should be looked at rather than
/// waved through.
#[test]
fn the_about_screen_matches_the_answer_when_both_are_present() {
    let Some(merged) = merged_from("about-family.json") else {
        eprintln!("no about capture; skipping");
        return;
    };
    let Ok(reference) = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/plugin-answers/about/responsive.tsx"),
    ) else {
        eprintln!("no about answer; skipping");
        return;
    };

    // 360 / 992 / 1920, the widths read off the frames themselves.
    assert_eq!(merged.slots, vec![0, 2, 4], "about is drawn at 0, 2 and 4");

    let ours = tally(&outline(&merged.tsx, 2));
    let theirs = tally(&outline(&reference, 4));

    let mut only_ours = Vec::new();
    let mut only_theirs = Vec::new();
    for (entry, count) in &ours {
        let held = theirs.get(entry).copied().unwrap_or_default();
        if *count > held {
            only_ours.push((entry.clone(), count - held));
        }
    }
    for (entry, count) in &theirs {
        let held = ours.get(entry).copied().unwrap_or_default();
        if *count > held {
            only_theirs.push((entry.clone(), count - held));
        }
    }

    assert!(
        only_ours.is_empty() && only_theirs.is_empty(),
        "the two outlines differ.\n  drawn here and not in the answer: {only_ours:?}\n  \
         in the answer and not here: {only_theirs:?}"
    );
}

/// Every element the screen draws, as tags, ignoring the ones only mentioned
/// inside a `{/* … */}` comment — those are deliberately not rendered and so
/// deliberately not imported.
fn rendered_tags(tsx: &str) -> std::collections::BTreeSet<String> {
    let mut uncommented = String::with_capacity(tsx.len());
    let mut rest = tsx;
    while let Some(start) = rest.find("{/*") {
        uncommented.push_str(&rest[..start]);
        match rest[start..].find("*/}") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    uncommented.push_str(rest);

    let mut tags = std::collections::BTreeSet::new();
    let bytes = uncommented.as_bytes();
    for (index, _) in uncommented.match_indices('<') {
        let name = uncommented[index + 1..]
            .chars()
            .take_while(|character| character.is_alphanumeric() || *character == '_')
            .collect::<String>();
        let starts_upper = name.chars().next().is_some_and(char::is_uppercase);
        // `</Box>` closes what `<Box` already counted.
        let closing = bytes.get(index + 1) == Some(&b'/');
        if starts_upper && !closing {
            tags.insert(name);
        }
    }
    tags
}

/// The module is what `devup_figma_export` hands back as `responsiveTsx`, and
/// it is assembled from the merge rather than emitted by it — imports on one
/// side, the tree on the other. Nothing here checked that the two agree on a
/// real screen: the merge's own tests stop at `tsx`, and the export test that
/// covers this wiring runs against a stub. An element drawn but not imported
/// is a file that does not compile, and it would only be discovered after a
/// capture had been paid for.
#[test]
fn every_element_the_module_draws_is_one_it_imports() {
    let Some(merged) = merged() else {
        eprintln!("no capture; skipping");
        return;
    };

    let primitives = merged.primitives();
    let components = merged.referenced_components();

    // The two lists are the one set of components, split and not overlapping.
    let declared = primitives
        .iter()
        .chain(components.iter())
        .map(|name| (*name).to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        declared.len(),
        primitives.len() + components.len(),
        "a name must be a primitive or a component of this design, not both"
    );

    let drawn = rendered_tags(&merged.tsx);
    assert!(!drawn.is_empty(), "the screen draws something");
    let unimported = drawn.difference(&declared).collect::<Vec<_>>();
    assert!(
        unimported.is_empty(),
        "drawn but not imported: {unimported:?}"
    );

    let module = merged.module("AboutPage");

    // Imports first, then one default export holding the tree.
    let export = module
        .find("export default function AboutPage()")
        .expect("a default export named after the screen");
    for line in module[..export]
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        assert!(
            line.starts_with("import "),
            "only imports may precede the export: {line}"
        );
    }
    assert_eq!(
        module.matches("export default function").count(),
        1,
        "one screen, one default export"
    );
    assert!(
        module.contains(&merged.tsx),
        "the module must carry the merged tree unchanged"
    );

    // Each component is imported once, from its own file.
    let imports = module[..export]
        .lines()
        .filter(|line| line.starts_with("import "))
        .collect::<Vec<_>>();
    let unique = imports.iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), imports.len(), "no import written twice");
    for name in &components {
        assert!(
            module.contains(&format!("import {{ {name} }} from '@/components/{name}'")),
            "{name} must be imported from its own file"
        );
    }
    if !primitives.is_empty() {
        assert!(
            module.contains("from '@devup-ui/react'"),
            "primitives come from devup-ui"
        );
    }
}
