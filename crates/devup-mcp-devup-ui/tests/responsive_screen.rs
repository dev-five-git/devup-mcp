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
    let folded = folded(&merged.tsx);
    let written = folded
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

/// A responsive array written one slot to a line, folded back onto one, so a
/// test can say what it expects in a line.
fn folded(tsx: &str) -> String {
    let mut out = String::with_capacity(tsx.len());
    let mut items: Option<Vec<String>> = None;
    for line in tsx.lines() {
        let trimmed = line.trim();
        match &mut items {
            Some(collected) => {
                if let Some(rest) = trimmed.strip_prefix("]}") {
                    out.push_str(&collected.join(", "));
                    out.push_str("]}");
                    out.push_str(rest);
                    out.push('\n');
                    items = None;
                } else {
                    collected.push(trimmed.trim_end_matches(',').to_owned());
                }
            }
            None => {
                if trimmed.ends_with("={[") {
                    out.push_str(line);
                    items = Some(Vec::new());
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
    }
    out
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

/// The slots come from the widths; which width stands in for a missing one
/// comes from the order.
///
/// A width that does not draw a node is given a hidden copy of the node from
/// the first width that does — first in the Section's layer order, which is
/// the order the roots are listed in and the order the plugin walks. Every
/// value of that copy lands in the array, not only its `display`, so the
/// order reaches the output and is meant to: the about hero picture is
/// `w={["770px", null, "778px", null, "770px"]}` because desktop comes first
/// in that Section. What the order must not touch is where each width lands
/// and what is drawn.
#[test]
fn the_order_of_the_widths_decides_which_one_stands_in_for_a_missing_one() {
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
        outline(&forwards.tsx, 2),
        outline(&backwards.tsx, 2),
        "and so does what is drawn"
    );
    assert_eq!(forwards.components, backwards.components);
    // A note names the node it was raised on, which is the first width's; what
    // it says does not depend on the order.
    let details = |merged: &MergedScreen| {
        merged
            .unrepresented
            .iter()
            .map(|note| note.detail.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(details(&forwards), details(&backwards));
    assert_ne!(
        forwards.tsx, backwards.tsx,
        "the width that stands in for a missing one comes from the order"
    );

    // The about capture makes the rule concrete. Its roots are desktop,
    // tablet, mobile, so the hidden mobile copy of the hero carries desktop's
    // values; read backwards, tablet stands in and its 778px is the base.
    let Some(about) = merged_from("about-family.json") else {
        return;
    };
    assert!(
        folded(&about.tsx).contains(r#"w={["770px", null, "778px", null, "770px"]}"#),
        "{}",
        about.tsx
    );
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/local-screens/about-family.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).expect("about")).expect("json");
    let roots = value["snapshot"]["roots"]
        .as_array()
        .expect("roots")
        .iter()
        .rev()
        .cloned()
        .collect::<Vec<_>>();
    value["snapshot"]["roots"] = serde_json::Value::Array(roots);
    let snapshot: Snapshot = serde_json::from_value(value["snapshot"].clone()).expect("snapshot");
    let backwards = merge_breakpoints(&snapshot, &CodegenOptions::default())
        .expect("merge")
        .expect("three widths merge");
    assert!(
        folded(&backwards.tsx).contains(r#"w={["778px", null, null, null, "770px"]}"#),
        "{}",
        backwards.tsx
    );
}

/// The `about` screen against the plugin's answer, line for line.
///
/// It runs on nothing until `about-family.json` sits beside the other captures
/// and `about/responsive.tsx` beside the other answers; a screen that costs
/// more reads than a day's allowance holds is one nobody will want to check
/// twice by hand, so the suite decides it instead.
///
/// Both files are read the same way — indentation dropped, a responsive array
/// folded onto one line — and then every line the answer has that this does
/// not, and the reverse, must be one written down in [`ABOUT_DIFFERS_ON_PURPOSE`]
/// with its reason. Anything else is looked at rather than waved through.
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
    let shape_differs = ours
        .iter()
        .chain(theirs.iter())
        .any(|(entry, _)| ours.get(entry) != theirs.get(entry));
    assert!(
        !shape_differs,
        "the two outlines differ:\n  ours {ours:?}\n  theirs {theirs:?}"
    );

    let ours = comparable_lines(&merged.module("AboutPage"));
    let theirs = comparable_lines(&reference);
    let mut only_ours = ours.clone();
    for line in &theirs {
        if let Some(index) = only_ours.iter().position(|other| other == line) {
            only_ours.remove(index);
        }
    }
    let mut only_theirs = theirs.clone();
    for line in &ours {
        if let Some(index) = only_theirs.iter().position(|other| other == line) {
            only_theirs.remove(index);
        }
    }
    let unexplained = |lines: &[String]| {
        lines
            .iter()
            .filter(|line| {
                !ABOUT_DIFFERS_ON_PURPOSE
                    .iter()
                    .any(|(known, _)| known == line)
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    let ours_unexplained = unexplained(&only_ours);
    let theirs_unexplained = unexplained(&only_theirs);
    assert!(
        ours_unexplained.is_empty() && theirs_unexplained.is_empty(),
        "lines not accounted for.\n  written here and not in the answer:\n    {}\n  in the answer and not here:\n    {}",
        ours_unexplained.join("\n    "),
        theirs_unexplained.join("\n    ")
    );
}

/// The lines on which `about` is written differently here on purpose, each
/// with the reason. A line is listed once whichever side it is on; the test
/// above only asks that every unmatched line is one of these.
const ABOUT_DIFFERS_ON_PURPOSE: &[(&str, &str)] = &[
    // A soft return (U+2028) is a line break in Figma and is written as one;
    // the answer has a space where each of them was.
    (
        "{\"  \"}우리는 단순한 진단 도구를 만드는 것이 아니라, 당사자가 스스로를 이해하고 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 만들고 있습니다",
        "soft return",
    ),
    (
        "{\"  \"}우리는 단순한 진단 도구를 만드는 것이 아니라,<br />당사자가 스스로를 이해하고 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 만들고 있습니다",
        "soft return",
    ),
    (
        "저는 성인 ADHD 당사자이자 정신건강간호사입니다. 진단을 받기까지 수년이 걸렸고, 그 과정에서 수많은 좌절과 시행착오를 겪었습니다. 그 경험을 통해 알게 된 것이 있습니다.",
        "soft return",
    ),
    (
        "저는 성인 ADHD 당사자이자 정신건강간호사입니다.<br />진단을 받기까지 수년이 걸렸고, 그 과정에서 수많은 좌절과 시행착오를 겪었습니다.<br />그 경험을 통해 알게 된 것이 있습니다.",
        "soft return",
    ),
    (
        "우리는 단순한 검사 도구를 만드는 것이 아닙니다. 퍼즐핏은 연구 기반의 자가검진, 맞춤형 보고서, 코칭과 커뮤니티를 통해  <br />당사자가 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 구축하고 있습니다.",
        "soft return",
    ),
    (
        "우리는 단순한 검사 도구를 만드는 것이 아닙니다.<br />퍼즐핏은 연구 기반의 자가검진, 맞춤형 보고서, 코칭과 커뮤니티를 통해  <br />당사자가 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 구축하고 있습니다.",
        "soft return",
    ),
    (
        "우리는 단순한 검사 도구를 만드는 것이 아닙니다. 퍼즐핏은 연구 기반의 자가검진, 맞춤형 보고서, <br />코칭과 커뮤니티를 통해 당사자가 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 구축하고 <br />있습니다.",
        "soft return",
    ),
    (
        "우리는 단순한 검사 도구를 만드는 것이 아닙니다.<br />퍼즐핏은 연구 기반의 자가검진, 맞춤형 보고서, <br />코칭과 커뮤니티를 통해 당사자가 사회 속에서 더 잘 기능할 수 있도록 돕는 시스템을 구축하고 <br />있습니다.",
        "soft return",
    ),
    (
        "을 통해 당사자가 사회 속에서 더 잘 기능하도록 돕는 통합 솔루션입니다.{\"  \"}",
        "soft return",
    ),
    (
        "을 통해<br />당사자가 사회 속에서 더 잘 기능하도록 돕는 통합 솔루션입니다.{\"  \"}",
        "soft return",
    ),
    // The hero's coloured span differs by a trailing space between tablet
    // and desktop. The plugin resolves the two by taking the longer rendered
    // string and turning the newlines *of the JSX* into `<br />`, which puts
    // a break before and after the span that no width drew. This keeps the
    // span as an element and takes the first width's text.
    (
        "<Text color=\"$secondary\"><br />  성인 ADHD,{\"  \"}<br /></Text>우리는 다르게 봅니다.",
        "plugin rewrites rendered JSX as text",
    ),
    ("<Text color=\"$secondary\">", "the span kept as an element"),
    ("성인 ADHD,{\" \"}", "the span kept as an element"),
    ("</Text>", "the span kept as an element"),
    ("우리는 다르게 봅니다.", "the span kept as an element"),
    // A positioned mask icon keeps its height; the plugin writes the width
    // alone, and a mask with no height draws nothing.
    ("w=\"465px\"", "mask icon height"),
    ("boxSize=\"465px\"", "mask icon height"),
];

/// A file's lines as they are compared: indentation and blank lines dropped,
/// a responsive array folded onto one line, and an image fill's file masked —
/// the plugin writes every one as its fixed `/icons/image.png`, and this
/// writes the file the export actually produces.
fn comparable_lines(source: &str) -> Vec<String> {
    folded(source)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut line = line.replace("url(/icons/image.png)", "url(IMAGEFILL)");
            while let Some(start) = line
                .find("url('/images/")
                .or_else(|| line.find("url(/images/"))
            {
                let Some(end) = line[start..].find(')') else {
                    break;
                };
                line.replace_range(start..start + end + 1, "url(IMAGEFILL)");
            }
            line
        })
        .collect()
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
