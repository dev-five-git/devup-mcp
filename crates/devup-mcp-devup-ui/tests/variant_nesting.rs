//! A component set whose props depend on more than one dimension at a time.
//!
//! `fixtures/plugin-answers/button/components.tsx` is what the plugin answered
//! for `devup-Test`'s `Button` set, and the capture it was answered from is
//! `fixtures/local-components/button-set.json`. The capture is not committed,
//! so these skip when it is absent rather than pretending to have checked.
//!
//! The values asserted here are the ones that carry no design token, because
//! an offline replay has no variable table and would render `$primary` as its
//! numeric id.

use std::{fs, path::PathBuf};

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component_set_target};
use devup_mcp_figma::Snapshot;

fn button() -> Option<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let raw = fs::read_to_string(root.join("fixtures/local-components/button-set.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let snapshot: Snapshot = serde_json::from_value(value.get("snapshot")?.clone()).ok()?;
    generate_component_set_target(
        &snapshot,
        "582:2137",
        "Button",
        &CodegenOptions {
            component_name: Some("Button".to_owned()),
            ..CodegenOptions::default()
        },
    )
    .ok()
    .map(|output| output.tsx)
}

/// `px` is decided by `size` and by `varient` together. Writing it as one map
/// is impossible and writing the default's literal loses every other
/// combination, so it is a map inside a map — and `size` is on the outside
/// because that nests less deeply than the transpose would.
#[test]
fn a_prop_two_dimensions_decide_is_written_as_a_map_inside_a_map() {
    let Some(tsx) = button() else {
        eprintln!("no capture; skipping");
        return;
    };
    let expected = r#"px={{
        lg: {
          primary: "24px",
          white: "24px",
          ghost: "10px",
          disabled: "24px",
          error: "24px"
        }[varient],
        md: {
          primary: "16px",
          white: "16px",
          ghost: "12px",
          disabled: "16px",
          error: "16px"
        }[varient],
        sm: {
          primary: "12px",
          white: "12px",
          ghost: "10px",
          disabled: "12px",
          error: "12px"
        }[varient],
        tag: "10px"
      }[size]}"#;
    assert!(tsx.contains(expected), "{tsx}");
}

/// Only one of the four sizes actually varies by `varient`, so the other three
/// stay plain values rather than each growing a map of five identical entries.
#[test]
fn a_branch_whose_options_agree_collapses_to_the_value() {
    let Some(tsx) = button() else {
        eprintln!("no capture; skipping");
        return;
    };
    let expected = r#"gap={{
        lg: "10px",
        md: {
          primary: "10px",
          white: "10px",
          ghost: "8px",
          disabled: "10px",
          error: "10px"
        }[varient],
        sm: "8px"
      }[size]}"#;
    assert!(tsx.contains(expected), "{tsx}");
}

/// An option with no value means one of two things, and they want opposite
/// answers. `tag` collapses because every variant drawn at that size agrees —
/// `ghost` has no `tag` at all, so its absence is not a hole. `border` does not
/// collapse, because the other four variants exist and refuse a border.
#[test]
fn a_combination_never_drawn_is_not_a_hole_but_a_refused_value_is() {
    let Some(tsx) = button() else {
        eprintln!("no capture; skipping");
        return;
    };
    assert!(
        tsx.contains(
            r#"borderRadius={{
        lg: "8px",
        md: "8px",
        sm: "8px",
        tag: "6px"
      }[size]}"#
        ),
        "{tsx}"
    );
    assert!(
        tsx.contains("border={varient === 'white' && "),
        "a value only one variant sets stays a condition: {tsx}"
    );
}

/// A boolean property is a switch, not a choice. It is optional, because
/// leaving it out is how a caller says the child should not be drawn, and the
/// child names it rather than the set describing what it does.
#[test]
fn a_boolean_property_becomes_an_optional_prop_and_a_guard() {
    let Some(tsx) = button() else {
        eprintln!("no capture; skipping");
        return;
    };
    assert!(
        tsx.contains(
            r#"export interface ButtonProps {
  leftIcon?: boolean
  rightIcon?: boolean
  size: 'lg' | 'md' | 'sm' | 'tag'
  varient: 'primary' | 'white' | 'ghost' | 'disabled' | 'error'
}"#
        ),
        "{tsx}"
    );
    assert!(
        tsx.contains(
            r#"export function Button({ leftIcon, rightIcon, size, varient }: ButtonProps)"#
        ),
        "{tsx}"
    );
}

/// Not every variant holds every node — a `tag` button has no icon — so the
/// merged tree, which is built from a variant that does, has to say when the
/// node is drawn or it would draw one at every size.
#[test]
fn a_node_some_variants_do_not_hold_is_guarded_by_the_ones_that_do() {
    let Some(tsx) = button() else {
        eprintln!("no capture; skipping");
        return;
    };
    for property in ["leftIcon", "rightIcon"] {
        assert!(
            tsx.contains(&format!(
                r#"{{{property} && (size === "lg" || size === "md" || size === "sm") && ("#
            )),
            "{property} should be guarded by the sizes that have an icon: {tsx}"
        );
    }
    // And nothing every variant holds should have grown a guard.
    assert_eq!(
        tsx.matches("&& (size ===").count(),
        2,
        "only the two icons are conditional: {tsx}"
    );
}

/// A text node's own words vary like anything else around them.
///
/// `tag` is the case that needs a node to be found by what it is rather than
/// where it sits: a `tag` button has no icons, so its text is the first child
/// where every other size has it third. Addressed by position, the lookup read
/// the icon and `tag` fell out of the map.
#[test]
fn what_a_text_node_says_is_a_variant_too() {
    let Some(tsx) = button() else {
        eprintln!("no capture; skipping");
        return;
    };
    assert!(
        tsx.contains(
            r#"{{
          lg: "buttonLg",
          md: "button",
          sm: "button",
          tag: "Tag"
        }[size]}"#
        ),
        "{tsx}"
    );
}

/// A component set's grid is how Figma arranges its variants on the canvas.
/// Carrying it would place every button at the coordinates of its row in the
/// sheet.
#[test]
fn the_sheet_a_variant_was_laid_out_on_does_not_reach_the_component() {
    let Some(tsx) = button() else {
        eprintln!("no capture; skipping");
        return;
    };
    assert!(!tsx.contains("gridColumn"), "{tsx}");
    assert!(!tsx.contains("gridRow"), "{tsx}");
}
