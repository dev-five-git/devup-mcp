//! How the focus of a field and of the box drawn around it fit together;
//! see the `input-focus-*` rules in the parent module documentation.

use super::{Element, Severity, Violation, violation};
use oxc_span::Span;

/// Props that make an element paint a visible box.
const BOX_PROPS: [&str; 11] = [
    "border", "borderWidth", "borderColor", "borderBottom", "borderTop", "borderLeft",
    "borderRight", "bg", "background", "backgroundColor", "boxShadow",
];

/// Whether an element paints a box of its own: a border, background or
/// shadow that is not reset to none, 0 or transparent.
pub(super) fn paints_box(attributes: &crate::effective_attributes::EffectiveAttributes) -> bool {
    BOX_PROPS.iter().any(|name| {
        attributes.contains(name)
            && !attributes.literal(name).is_some_and(|value| {
                matches!(value.trim(), "none" | "0" | "0px" | "transparent" | "unset")
            })
    })
}

/// The `{ ... }` block written after `prop=` in a tag, braces balanced.
fn prop_block<'t>(text: &'t str, prop: &str) -> Option<&'t str> {
    let at = text
        .match_indices(prop)
        .find(|(at, _)| {
            !text[..*at].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                && text[at + prop.len()..].starts_with('=')
        })?
        .0;
    let rest = &text[at + prop.len() + 1..];
    let open = rest.find('{')?;
    let mut depth = 0usize;
    for (index, ch) in rest[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[open..open + index + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether a style text sets `outline` to none or 0.
fn resets_outline(text: &str) -> bool {
    text.match_indices("outline").any(|(at, _)| {
        let rest = text[at + "outline".len()..].trim_start();
        let Some(rest) = rest.strip_prefix([':', '=']) else {
            return false;
        };
        let value = rest
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '{' | '`'));
        value.starts_with("none") || value.starts_with('0')
    })
}

/// Whether a focus block draws something besides removing the outline: a
/// border, ring, background or an outline that is not none.
fn draws_focus(block: &str) -> bool {
    ["border", "boxShadow", "bg", "background", "color"]
        .iter()
        .any(|word| block.contains(word))
        || (block.contains("outline") && !resets_outline(block))
}

/// The pixel width the first border in a style text sets: `1px solid`, `2px`.
fn border_px(text: &str) -> Option<f64> {
    let at = text.find("border")?;
    let rest = &text[at..];
    let end = rest.find([',', '}', '\n']).unwrap_or(rest.len());
    let rest = &rest[..end];
    let px = rest.find("px")?;
    let digits = rest[..px]
        .rsplit(|c: char| !(c.is_ascii_digit() || c == '.'))
        .next()?;
    digits.parse().ok()
}

/// The focus style blocks an element writes.
fn focus_blocks(text: &str) -> Vec<&str> {
    ["_focus", "_focusVisible", "_focusWithin"]
        .iter()
        .filter_map(|prop| prop_block(text, prop))
        .collect()
}

/// A tag with its focus blocks removed: what it looks like at rest.
fn resting(text: &str) -> String {
    let mut rest = text.to_owned();
    for block in focus_blocks(text) {
        rest = rest.replacen(block, "", 1);
    }
    rest
}

/// A finding source alone cannot settle: the runtime audit `input-focus`
/// check answers it in a browser, by click and by Tab.
fn browser_required(mut finding: Violation) -> Violation {
    finding
        .context
        .insert("verification".into(), serde_json::json!("browser-required"));
    finding
        .context
        .insert("browserCheck".into(), serde_json::json!("input-focus"));
    finding
}

const BROWSER_CHECK: &str = "Then confirm in a browser: devup_runtime_audit with an input-focus check focuses every field by click and by Tab and fails an inner outline inside the container, a ring that does not surround the container, focus landing on the container, and a size or position change.";

/// A border wider when focused than at rest, as (rest, focused) pixels.
fn wider_border(rest: &str, blocks: &[&str]) -> Option<(Option<f64>, f64)> {
    let rest = border_px(rest);
    let focused = blocks.iter().find_map(|block| border_px(block))?;
    rest.is_none_or(|rest| focused > rest + f64::EPSILON)
        .then_some((rest, focused))
}

/// How the focus of one field and of the box drawn around it fit together.
pub(super) fn input_focus_findings(source: &str, element: &Element) -> Vec<Violation> {
    let name = &element.name;
    let mut found = Vec::new();
    let own_blocks = focus_blocks(&element.text);
    let outline_removed = resets_outline(&element.text);
    let own_draws = own_blocks.iter().any(|block| draws_focus(block));
    let shift = |span: Span, who: &str, (rest, focused): (Option<f64>, f64)| {
        browser_required(violation(
            source,
            span,
            "input-focus-layout-shift",
            Severity::Info,
            format!(
                "<{who}> widens its border to {focused}px on focus{}; a wider border resizes the field or moves its text and the elements around it.",
                rest.map(|rest| format!(" from {rest}px")).unwrap_or_default()
            ),
            &format!(
                "Keep the resting border width and change only borderColor, or draw the extra width with boxShadow (\"0 0 0 1px $primary\") or outline with outlineOffset, which take no layout space. {BROWSER_CHECK}"
            ),
        ))
    };
    if let Some(container) = &element.container {
        let box_name = &container.name;
        if container.tab_index {
            found.push(violation(
                source,
                container.span,
                "input-focus-moved-to-container",
                Severity::Warning,
                format!(
                    "<{box_name}> wraps a <{name}> field and takes tabIndex itself, so Tab can stop on the box instead of the field the user types in."
                ),
                "Remove tabIndex from the container. Keep keyboard focus on the <input>/<textarea>/<select> and style the container with _focusWithin.",
            ));
        }
        if !container.focus_within {
            found.push(browser_required(violation(
                source,
                element.span,
                "input-focus-container",
                Severity::Info,
                format!(
                    "<{name}> draws no box of its own; the visible field is its container <{box_name}>, which has no _focusWithin. {}",
                    if element.focus {
                        "The field's own focus style draws inside the box instead of around it."
                    } else {
                        "Nothing marks the box when the field is focused; at most the browser's default outline appears inside it."
                    }
                ),
                &format!(
                    "Put the focus style on the container: <{box_name}> _focusWithin={{{{ borderColor: \"$primary\", boxShadow: \"0 0 0 2px $primary\" }}}}, and remove only the field's own outline (_focus={{{{ outline: \"none\" }}}}) once the container shows it. Never move focus to the container. {BROWSER_CHECK}"
                ),
            )));
        } else if !outline_removed {
            found.push(browser_required(violation(
                source,
                element.span,
                "input-focus-duplicate-outline",
                Severity::Info,
                format!(
                    "<{box_name}> marks focus with _focusWithin, but <{name}> inside it keeps the browser's default outline, so a second, smaller ring can appear inside the box (unless a global reset removes it)."
                ),
                &format!(
                    "Add outline: \"none\" to the field (outline=\"none\" or _focus={{{{ outline: \"none\" }}}}): the container's _focusWithin already shows focus. {BROWSER_CHECK}"
                ),
            )));
        }
        let blocks = prop_block(&container.text, "_focusWithin").into_iter().collect::<Vec<_>>();
        if let Some(widths) = wider_border(&resting(&container.text), &blocks) {
            found.push(shift(container.span, box_name, widths));
        }
    } else if let Some(widths) = wider_border(&resting(&element.text), &own_blocks) {
        found.push(shift(element.span, name, widths));
    }
    // The box that shows focus eases into it: a ring that snaps on reads as
    // a flicker, and the generator writes a short ease-out by default.
    let shows = match &element.container {
        Some(box_) if box_.focus_within => Some((box_.span, box_.name.as_str(), box_.text.as_str())),
        None if own_draws => Some((element.span, name.as_str(), element.text.as_str())),
        _ => None,
    };
    if let Some((span, who, text)) = shows
        && !text.contains("transition")
    {
        found.push(browser_required(violation(
            source,
            span,
            "input-state-transition",
            Severity::Info,
            format!(
                "<{who}> changes its border/ring on focus with no transition, so the focus state snaps on and off instead of easing."
            ),
            &format!(
                "Add transition=\"160ms ease-out\" and transitionProperty=\"border-color,box-shadow,background\" (only the props the states change, never layout props) to <{who}>, or the duration and easing the design's prototype gives. {BROWSER_CHECK}"
            ),
        )));
    }
    let replaced = own_draws || element.container.as_ref().is_some_and(|box_| box_.focus_within);
    if outline_removed && !replaced {
        found.push(browser_required(violation(
            source,
            element.span,
            "input-focus-outline-removed",
            Severity::Warning,
            format!(
                "<{name}> removes its focus outline and nothing in this source replaces it, on the field or on a container; a keyboard user cannot see where focus is."
            ),
            &format!(
                "Remove the outline only where a replacement shows in the same focus state: a border/ring on the field (_focusVisible) or on its container (_focusWithin). {BROWSER_CHECK}"
            ),
        )));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::super::*;

    fn rules_of(source: &str) -> Vec<&'static str> {
        interaction_findings("Page.tsx", source)
            .iter()
            .map(|finding| finding.rule)
            .collect()
    }

    fn input_focus_rules(source: &str) -> Vec<&'static str> {
        interaction_findings("Form.tsx", source)
            .into_iter()
            .map(|finding| finding.rule)
            .filter(|rule| rule.starts_with("input-focus"))
            .collect()
    }

    #[test]
    fn a_field_that_is_its_own_box_or_sits_in_a_focus_within_box_passes() {
        // The input is the bordered box and draws its own ring.
        let own = r#"export const F = () => <Box as="input" aria-label="Name" border="1px solid " borderRadius="8px" _hover={{ borderColor: "" }} _focusVisible={{ borderColor: "", boxShadow: "0 0 0 2px ", outline: "none" }} />;"#;
        // An icon, the field and a button inside a box that shows focus.
        let wrapped = r#"export const S = () => <Flex border="1px solid " borderRadius="8px" _focusWithin={{ borderColor: "", boxShadow: "0 0 0 2px " }}>
      <Image src="/icons/search.svg" alt="" />
      <Box as="input" aria-label="Search" border="none" bg="transparent" outline="none" />
      <Box as="button" aria-label="Clear" _hover={{ bg: "" }} _active={{ bg: "" }} _focusVisible={{ outline: "2px solid " }} transition="all 0.2s ease"><Image src="/x.svg" alt="" /></Box>
    </Flex>;"#;
        // A textarea that is its own box, and error and disabled states.
        let states = r#"export const T = () => <>
      <Box as="textarea" aria-label="Message" border="1px solid " _hover={{ borderColor: "" }} _focusVisible={{ borderColor: "", boxShadow: "0 0 0 2px " }} />
      <Box as="input" aria-label="Email" aria-invalid="true" border="1px solid " _hover={{ borderColor: "" }} _focusVisible={{ boxShadow: "0 0 0 2px " }} />
      <Box as="input" aria-label="Locked" disabled border="1px solid " _hover={{ borderColor: "" }} _focusVisible={{ boxShadow: "0 0 0 2px " }} _disabled={{ opacity: 0.5 }} />
    </>;"#;
        for source in [own, wrapped, states] {
            assert_eq!(input_focus_rules(source), Vec::<&str>::new(), "{source}");
            assert!(!rules_of(source).contains(&"focus-style"), "{source}");
        }
        // The container's _focusWithin counts as the field's focus style.
        let coverage = role_coverage("Search.tsx", wrapped);
        assert!(coverage.iter().any(|row| row.role == Role::Input && row.complete == 0 && row.total == 1));
        assert_eq!(
            interaction_coverage("Search.tsx", wrapped).with_states,
            2
        );
    }

    #[test]
    fn a_ring_on_the_inner_field_of_a_box_needs_a_browser_check_not_an_error() {
        let source = r#"export const S = () => <Flex border="1px solid " borderRadius="8px">
      <Image src="/icons/search.svg" alt="" />
      <Box as="input" aria-label="Search" border="none" _focusVisible={{ outline: "2px solid " }} />
    </Flex>;"#;
        let found = interaction_findings("Search.tsx", source);
        let finding = found
            .iter()
            .find(|finding| finding.rule == "input-focus-container")
            .expect("input-focus-container");
        assert_eq!(finding.severity, Severity::Info);
        assert_eq!(finding.context["verification"], "browser-required");
        assert_eq!(finding.context["browserCheck"], "input-focus");
        assert!(finding.message.contains("draws inside the box"), "{}", finding.message);
        assert!(found.iter().all(|finding| finding.severity != Severity::Error));
        // A select with a chevron in a box with no focus style at all.
        let select = r#"export const P = () => <Box border="1px solid "><Box as="select" aria-label="Sort" border="0" /><Image src="/chevron.svg" alt="" /></Box>;"#;
        assert_eq!(input_focus_rules(select), ["input-focus-container"]);
    }

    #[test]
    fn an_inner_default_outline_inside_a_focus_within_box_is_a_duplicate() {
        let source = r#"export const S = () => <Flex border="1px solid " _focusWithin={{ borderColor: "" }}>
      <Box as="input" aria-label="Search" border="none" />
      <Box as="button" _hover={{ bg: "" }} _active={{ bg: "" }} _focusVisible={{ outline: "2px solid " }} transition="all 0.2s ease">Go</Box>
    </Flex>;"#;
        assert_eq!(input_focus_rules(source), ["input-focus-duplicate-outline"]);
    }

    #[test]
    fn a_removed_outline_with_no_replacement_and_a_focus_container_with_tab_index_warn() {
        let removed = r#"export const F = () => <Box as="input" aria-label="Name" border="1px solid " _focus={{ outline: "none" }} />;"#;
        let found = interaction_findings("Form.tsx", removed);
        let finding = found.iter().find(|finding| finding.rule == "input-focus-outline-removed").unwrap();
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(finding.context["verification"], "browser-required");
        // A replacement on the container lets the field drop its outline.
        let replaced = r#"export const F = () => <Flex border="1px solid " _focusWithin={{ boxShadow: "0 0 0 2px " }}><Box as="input" border="none" _focus={{ outline: "none" }} /></Flex>;"#;
        assert_eq!(input_focus_rules(replaced), Vec::<&str>::new());
        let moved = r#"export const F = () => <Flex tabIndex={0} border="1px solid " _focusWithin={{ boxShadow: "0 0 0 2px " }}><Box as="input" border="none" outline="none" /></Flex>;"#;
        assert_eq!(input_focus_rules(moved), ["input-focus-moved-to-container"]);
    }

    #[test]
    fn a_focus_ring_that_snaps_on_is_reported_and_an_eased_one_is_not() {
        let state_rules = |source: &str| {
            interaction_findings("Form.tsx", source)
                .into_iter()
                .filter(|finding| finding.rule == "input-state-transition")
                .collect::<Vec<_>>()
        };
        let snaps = r#"export const S = () => <Flex border="1px solid " _focusWithin={{ borderColor: "" }}><Box as="input" border="none" outline="none" /></Flex>;"#;
        let found = state_rules(snaps);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Info);
        assert_eq!(found[0].context["browserCheck"], "input-focus");
        let own = r#"export const F = () => <Box as="input" aria-label="Name" border="1px solid " _focusVisible={{ boxShadow: "0 0 0 2px " }} />;"#;
        assert_eq!(state_rules(own).len(), 1);
        let eased = r#"export const S = () => <Flex border="1px solid " transition="160ms ease-out" transitionProperty="border-color,box-shadow" _focusWithin={{ borderColor: "" }}><Box as="input" border="none" outline="none" /></Flex>;"#;
        assert!(state_rules(eased).is_empty());
    }

    #[test]
    fn a_wider_focused_border_is_a_layout_shift_to_check() {
        let own = r#"export const T = () => <Box as="textarea" border="1px solid " _focusVisible={{ border: "2px solid " }} />;"#;
        assert_eq!(input_focus_rules(own), ["input-focus-layout-shift"]);
        let wrapped = r#"export const S = () => <Flex border="1px solid " _focusWithin={{ border: "2px solid " }}><Box as="input" border="none" outline="none" /></Flex>;"#;
        assert_eq!(input_focus_rules(wrapped), ["input-focus-layout-shift"]);
        let same = r#"export const S = () => <Flex border="1px solid " _focusWithin={{ border: "1px solid ", boxShadow: "0 0 0 1px " }}><Box as="input" border="none" outline="none" /></Flex>;"#;
        assert_eq!(input_focus_rules(same), Vec::<&str>::new());
    }
}
