//! The `Button` component set against the plugin's definition of it.
//!
//! `fixtures/plugin-answers/button/components.tsx` is what the plugin wrote
//! for `devup-Test`'s `582:2137`, and `fixtures/local-components/button-set.json`
//! is the capture of the set with the variables it binds. The capture is not
//! committed, so this skips when it is absent rather than pretending to have
//! checked.
//!
//! `variant_nesting.rs` pins the shape of the maps; this compares the whole
//! definition line for line, with each difference tied to what the capture
//! says about the file.

use std::{fs, path::PathBuf};

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component_set_target};
use devup_mcp_figma::{Snapshot, UpstreamResult};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn ours() -> Option<String> {
    let raw = fs::read_to_string(fixtures().join("local-components/button-set.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let snapshot: Snapshot = serde_json::from_value(value.get("snapshot")?.clone()).ok()?;
    let resource = |name: &str| {
        value
            .get("payload")
            .and_then(|payload| payload.get(name))
            .cloned()
            .map(|raw| UpstreamResult { raw })
    };
    let variables = resource("variables")?;
    let styles = resource("styles");
    let options = CodegenOptions {
        component_name: Some("Button".to_owned()),
        ..CodegenOptions::default()
    }
    .with_resource_results(Some(&variables), styles.as_ref());
    Some(
        generate_component_set_target(&snapshot, "582:2137", "Button", &options)
            .ok()?
            .tsx,
    )
}

/// Brace depth of a piece of a line, outside its strings.
fn balance(piece: &str) -> i32 {
    let mut depth = 0;
    let mut quoted = false;
    for character in piece.chars() {
        match character {
            '"' => quoted = !quoted,
            '{' | '[' if !quoted => depth += 1,
            '}' | ']' if !quoted => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// A file's lines as they are compared: indentation and blank lines dropped,
/// an expression that spans several lines — a variant map, a pseudo-selector
/// object, a text map — folded onto one with its spacing normalised, and the
/// module around the definition left out. A JSX element or a function body
/// is not an expression: its opener is a `>` after props or a `{` after `)`,
/// and it is left alone.
fn comparable_lines(source: &str) -> Vec<String> {
    let opens = |piece: &str| {
        ["={{", "={[", ": {", ": [", "{{", "={"]
            .iter()
            .any(|opener| piece.ends_with(opener))
            && balance(piece) > 0
    };
    let normalise = |folded: &str| {
        let squeezed = folded.split_whitespace().collect::<Vec<_>>().join(" ");
        squeezed
            .replace(" ,", ",")
            .replace("{ ", "{")
            .replace(" }", "}")
            .replace("[ ", "[")
            .replace(" ]", "]")
    };
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    for raw in source.lines() {
        let piece = raw.trim();
        if piece.is_empty() {
            continue;
        }
        if current.is_empty() {
            if opens(piece) {
                depth = balance(piece);
                current.push_str(piece);
            } else {
                out.push(normalise(piece));
            }
            continue;
        }
        current.push(' ');
        current.push_str(piece);
        depth += balance(piece);
        if depth <= 0 {
            out.push(normalise(&current));
            current.clear();
        }
    }
    if !current.is_empty() {
        out.push(normalise(&current));
    }
    out
}

/// The lines on which the definition is written differently on purpose,
/// each with what the capture says.
///
/// - **The icons' colour.** The answer gives both icons `bg="$gray400"` at
///   every variant. The capture gives the icon instances a white fill on
///   `primary` and `error`, `text` on `white` and `ghost`, and `gray400` on
///   `disabled` only — the same map as the label's `color`, which the answer
///   itself writes per variant. `$gray400` is the icon component's own
///   colour, not the instances', and the map is the file.
/// - **The right icon's aspect ratio.** The `Arrow` instances carry
///   `targetAspectRatio` 20×20 at `md`, `sm` and `lg`/`ghost`, and none at
///   the other `lg`s; the answer writes nothing for it. The left icon is
///   512×512 everywhere and both write `aspectRatio="1"`.
/// - **The order of a map's keys.** Keys follow the order the set declares
///   its options in, `primary, white, ghost, disabled, error`, as the
///   `varient` type in the interface does; the plugin's follow the layer
///   order of the set's children, `primary, disabled, white, ghost, error`,
///   which is the order a `Map` happened to be filled in.
/// - **A hover value equal to the resting one.** `gap` on hover is `10px`
///   on every `md` variant, and only `ghost` rests at `8px`; the hover block
///   says it where hovering changes it, the answer on every `md`.
/// - **`disabled` on hover and active.** The capture holds 49 components,
///   `disabled` at four sizes and only at `effect=default`; there is no
///   disabled hover or active in the file, and the answer's entries for them
///   come from another state of it.
/// - **The icons' files** are named after the instances' own layers,
///   `MypageIcon` and `Arrow`; the plugin names them after the variant
///   components, which every set with a `user` variant would share.
const DIFFERS_ON_PURPOSE: &[(&str, &str)] = &[
    // the icons' colour
    (
        "bg=\"$gray400\"",
        "the icon instances are coloured per variant",
    ),
    (
        "bg={{primary: \"#FFF\", white: \"$text\", ghost: \"$text\", disabled: \"$gray400\", error: \"#FFF\"}[varient]}",
        "the icon instances are coloured per variant",
    ),
    // the right icon's aspect ratio
    (
        "aspectRatio={{lg: varient === 'ghost' && \"1\", md: \"1\", sm: \"1\"}[size]}",
        "the Arrow instances carry a targetAspectRatio where the file has one",
    ),
    // key order, and the hover and active blocks
    (
        "bg={{primary: \"$primary\", disabled: \"$gray200\", white: \"$innerBg\", error: \"$error\"}[varient]}",
        "keys in the set's option order",
    ),
    (
        "bg={{primary: \"$primary\", white: \"$innerBg\", disabled: \"$gray200\", error: \"$error\"}[varient]}",
        "keys in the set's option order",
    ),
    (
        "gap={{lg: \"10px\", md: {primary: \"10px\", disabled: \"10px\", white: \"10px\", ghost: \"8px\", error: \"10px\"}[varient], sm: \"8px\"}[size]}",
        "keys in the set's option order",
    ),
    (
        "gap={{lg: \"10px\", md: {primary: \"10px\", white: \"10px\", ghost: \"8px\", disabled: \"10px\", error: \"10px\"}[varient], sm: \"8px\"}[size]}",
        "keys in the set's option order",
    ),
    (
        "px={{lg: {primary: \"24px\", disabled: \"24px\", white: \"24px\", ghost: \"10px\", error: \"24px\"}[varient], md: {primary: \"16px\", disabled: \"16px\", white: \"16px\", ghost: \"12px\", error: \"16px\"}[varient], sm: {primary: \"12px\", disabled: \"12px\", white: \"12px\", ghost: \"10px\", error: \"12px\"}[varient], tag: \"10px\"}[size]}",
        "keys in the set's option order",
    ),
    (
        "px={{lg: {primary: \"24px\", white: \"24px\", ghost: \"10px\", disabled: \"24px\", error: \"24px\"}[varient], md: {primary: \"16px\", white: \"16px\", ghost: \"12px\", disabled: \"16px\", error: \"16px\"}[varient], sm: {primary: \"12px\", white: \"12px\", ghost: \"10px\", disabled: \"12px\", error: \"12px\"}[varient], tag: \"10px\"}[size]}",
        "keys in the set's option order",
    ),
    (
        "color={{primary: \"#FFF\", disabled: \"$gray400\", white: \"$text\", ghost: \"$text\", error: \"#FFF\"}[varient]}",
        "keys in the set's option order",
    ),
    (
        "color={{primary: \"#FFF\", white: \"$text\", ghost: \"$text\", disabled: \"$gray400\", error: \"#FFF\"}[varient]}",
        "keys in the set's option order",
    ),
    (
        "boxSize={{lg: {primary: \"20px\", disabled: \"20px\", white: \"20px\", ghost: \"16px\", error: \"20px\"}[varient], md: \"16px\", sm: \"12px\"}[size]}",
        "keys in the set's option order",
    ),
    (
        "boxSize={{lg: {primary: \"20px\", white: \"20px\", ghost: \"16px\", disabled: \"20px\", error: \"20px\"}[varient], md: \"16px\", sm: \"12px\"}[size]}",
        "keys in the set's option order",
    ),
    (
        "boxSize={{lg: {primary: \"20px\", disabled: \"20px\", white: \"20px\", ghost: \"18px\", error: \"20px\"}[varient], md: \"16px\", sm: \"14px\"}[size]}",
        "keys in the set's option order",
    ),
    (
        "boxSize={{lg: {primary: \"20px\", white: \"20px\", ghost: \"18px\", disabled: \"20px\", error: \"20px\"}[varient], md: \"16px\", sm: \"14px\"}[size]}",
        "keys in the set's option order",
    ),
    (
        "_active={{\"bg\": {primary: \"$primaryDarkest\", disabled: \"$errorDarkest\", white: \"$gray200\", ghost: \"$gray200\", error: \"$errorDarkest\"}[varient], \"borderRadius\": varient === 'disabled' && \"6px\", \"px\": varient === 'disabled' && \"10px\"}}",
        "no disabled active in the file; keys in the set's option order",
    ),
    (
        "_active={{\"bg\": {primary: \"$primaryDarkest\", white: \"$gray200\", ghost: \"$gray200\", error: \"$errorDarkest\"}[varient]}}",
        "no disabled active in the file; keys in the set's option order",
    ),
    (
        "_hover={{\"bg\": {primary: \"$primaryDark\", disabled: \"$errorDark\", white: \"$gray100\", ghost: \"$gray100\", error: \"$errorDark\"}[varient], \"borderRadius\": varient === 'disabled' && \"6px\", \"px\": varient === 'disabled' && \"10px\", \"gap\": size === 'md' && \"10px\"}}",
        "no disabled hover in the file; a hover value is said where it changes; keys in the set's option order",
    ),
    (
        "_hover={{\"bg\": {primary: \"$primaryDark\", white: \"$gray100\", ghost: \"$gray100\", error: \"$errorDark\"}[varient], \"gap\": {md: varient === 'ghost' && \"10px\"}[size]}}",
        "no disabled hover in the file; a hover value is said where it changes; keys in the set's option order",
    ),
    // the icons' files
    (
        "maskImage=\"url('/icons/속성 1=user.svg')\"",
        "an asset is named after its instance",
    ),
    (
        "maskImage=\"url(/icons/MypageIcon.svg)\"",
        "an asset is named after its instance",
    ),
    (
        "maskImage=\"url('/icons/속성 1=right.svg')\"",
        "an asset is named after its instance",
    ),
    (
        "maskImage=\"url(/icons/Arrow.svg)\"",
        "an asset is named after its instance",
    ),
];

#[test]
fn the_button_definition_is_the_plugin_s() {
    let Some(tsx) = ours() else {
        eprintln!("no button capture; skipping");
        return;
    };
    let Ok(answer) = fs::read_to_string(fixtures().join("plugin-answers/button/components.tsx"))
    else {
        eprintln!("no button answer; skipping");
        return;
    };
    let ours = comparable_lines(&tsx);
    let theirs = comparable_lines(&answer);
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
            .filter(|line| !line.starts_with("import "))
            .filter(|line| !DIFFERS_ON_PURPOSE.iter().any(|(known, _)| known == line))
            .cloned()
            .collect::<Vec<_>>()
    };
    let ours_unexplained = unexplained(&only_ours);
    let theirs_unexplained = unexplained(&only_theirs);
    assert!(
        ours_unexplained.is_empty() && theirs_unexplained.is_empty(),
        "button: lines not accounted for.\n  written here and not in the answer:\n    {}\n  in the answer and not here:\n    {}",
        ours_unexplained.join("\n    "),
        theirs_unexplained.join("\n    ")
    );
    // Every explanation is for a line that is actually there; a stale entry
    // would hide a line that came to differ later.
    for (known, why) in DIFFERS_ON_PURPOSE {
        assert!(
            only_ours.iter().any(|line| line == known)
                || only_theirs.iter().any(|line| line == known),
            "explained but not different: {known} ({why})"
        );
    }
}
