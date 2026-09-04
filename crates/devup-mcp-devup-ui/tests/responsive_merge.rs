//! Writing one prop across several widths as a devup-ui responsive array.
//!
//! Every expectation here is transcribed from a plugin answer kept under
//! `fixtures/plugin-answers/`, so a reader can check each row against the file
//! it names rather than against this test's opinion.

use devup_mcp_devup_ui::codegen::responsive::{
    Drawn::{self, Absent, Set, Unset},
    Merged, SLOTS, merge_slots, slot_of_width,
};

fn slots(prop: &str, values: [Drawn<'_>; SLOTS]) -> Vec<Option<String>> {
    match merge_slots(prop, &values) {
        Merged::Array(slots) => slots,
        Merged::Same(value) => panic!("{prop}: expected an array, got the value {value:?}"),
    }
}

fn array(values: &[Option<&str>]) -> Vec<Option<String>> {
    values
        .iter()
        .map(|value| value.map(str::to_owned))
        .collect()
}

/// One prop of one element, as the three widths draw it, and the array the
/// reference writes for it.
struct Case<'a> {
    prop: &'a str,
    mobile: Drawn<'a>,
    tablet: Drawn<'a>,
    desktop: Drawn<'a>,
    expected: &'a [Option<&'a str>],
}

/// `popup/responsive.tsx`. Its three widths sit at slots 0, 1 and 4 — slot 1,
/// not slot 2, which is what makes this screen different from `notice`.
#[test]
fn the_popup_screen_is_reproduced_prop_for_prop() {
    let case = |prop, mobile, tablet, desktop, expected| Case {
        prop,
        mobile,
        tablet,
        desktop,
        expected,
    };
    let cases = [
        // Every width differs.
        case(
            "py",
            Set("211.5px"),
            Set("279.5px"),
            Set("287.5px"),
            &[
                Some("211.5px"),
                Some("279.5px"),
                None,
                None,
                Some("287.5px"),
            ],
        ),
        case(
            "gap",
            Set("36px"),
            Set("40px"),
            Set("50px"),
            &[Some("36px"), Some("40px"), None, None, Some("50px")],
        ),
        // Tablet and desktop agree, so the array stops after slot 1.
        case(
            "p",
            Set("30px"),
            Set("40px"),
            Set("40px"),
            &[Some("30px"), Some("40px")],
        ),
        case(
            "gap",
            Set("20px"),
            Set("30px"),
            Set("30px"),
            &[Some("20px"), Some("30px")],
        ),
        case(
            "gap",
            Set("12px"),
            Set("16px"),
            Set("16px"),
            &[Some("12px"), Some("16px")],
        ),
        case(
            "boxSize",
            Set("20px"),
            Set("24px"),
            Set("24px"),
            &[Some("20px"), Some("24px")],
        ),
        case(
            "px",
            Set("30px"),
            Set("40px"),
            Set("40px"),
            &[Some("30px"), Some("40px")],
        ),
        case(
            "py",
            Set("12px"),
            Set("16px"),
            Set("16px"),
            &[Some("12px"), Some("16px")],
        ),
        case(
            "p",
            Set("16px"),
            Set("20px"),
            Set("20px"),
            &[Some("16px"), Some("20px")],
        ),
        // Mobile and tablet agree, so nothing is written until slot 4.
        case(
            "flexDir",
            Set("column"),
            Set("column"),
            Set("row"),
            &[Some("column"), None, None, None, Some("row")],
        ),
        case(
            "boxSize",
            Set("16px"),
            Set("16px"),
            Set("19px"),
            &[Some("16px"), None, None, None, Some("19px")],
        ),
        // A prop that stops. It cannot simply be dropped, because the narrower
        // width's value would be inherited, so the reference clears it.
        case(
            "pl",
            Set("36.5px"),
            Unset,
            Unset,
            &[Some("36.5px"), Some("initial")],
        ),
        case(
            "pr",
            Set("35.5px"),
            Unset,
            Unset,
            &[Some("35.5px"), Some("initial")],
        ),
        case(
            "w",
            Set("220px"),
            Unset,
            Unset,
            &[Some("220px"), Some("initial")],
        ),
        // A prop that only the middle width sets. Slot 0 stays `null` because
        // nothing is in effect yet to clear, and slot 4 spends an `"initial"`
        // because by then something is.
        case(
            "px",
            Unset,
            Set("184px"),
            Unset,
            &[None, Some("184px"), None, None, Some("initial")],
        ),
    ];

    for Case {
        prop,
        mobile,
        tablet,
        desktop,
        expected,
    } in cases
    {
        assert_eq!(
            slots(prop, [mobile, tablet, Absent, Absent, desktop]),
            array(expected),
            "{prop}: {mobile:?} / {tablet:?} / {desktop:?}"
        );
    }
}

/// `notice/responsive.tsx`, whose widths sit at slots 0, 2 and 4. Only
/// `display` varies there, and these are the four arrays it writes.
#[test]
fn the_notice_screen_display_toggles_are_reproduced() {
    let cases = [
        // Shown from tablet up: the desktop banner and the desktop section.
        (
            [Set("none"), Absent, Set("flex"), Absent, Set("flex")],
            vec![Some("none"), None, Some("flex")],
        ),
        // Hidden from tablet up, on an element that needs no display to show.
        (
            [Unset, Absent, Set("none"), Absent, Set("none")],
            vec![None, None, Some("none")],
        ),
        // Hidden from tablet up, on one that states its own.
        (
            [Set("flex"), Absent, Set("none"), Absent, Set("none")],
            vec![Some("flex"), None, Some("none")],
        ),
    ];
    for (widths, expected) in cases {
        assert_eq!(slots("display", widths), array(&expected), "{widths:?}");
    }
}

/// Widths that agree need no array, and that is most props on both screens —
/// `alignItems="center"`, `gap="16px"`, `bg="$cardBg"` are written plainly.
#[test]
fn widths_that_agree_are_left_alone() {
    assert_eq!(
        merge_slots(
            "gap",
            &[Set("16px"), Set("16px"), Absent, Absent, Set("16px")]
        ),
        Merged::Same(Some("16px".to_owned()))
    );
    assert_eq!(
        merge_slots("gap", &[Unset, Unset, Absent, Absent, Unset]),
        Merged::Same(None)
    );
    assert_eq!(merge_slots("gap", &[Absent; SLOTS]), Merged::Same(None));
}

/// A single width is a screen, not a screen that changes.
#[test]
fn one_width_never_produces_an_array() {
    assert_eq!(
        merge_slots("p", &[Absent, Absent, Absent, Absent, Set("40px")]),
        Merged::Same(Some("40px".to_owned()))
    );
}

/// Only layout, spacing and position props are cleared when a wider width
/// stops setting them. `bg` is not one, so it is left to inherit — a limit of
/// the reference this repo follows rather than a choice made here.
#[test]
fn a_prop_outside_the_cleared_set_is_left_to_inherit() {
    let dropped = [Set("$cardBg"), Unset, Absent, Absent, Unset];
    assert_eq!(
        merge_slots("bg", &dropped),
        Merged::Same(Some("$cardBg".to_owned())),
        "bg inherits rather than resetting"
    );
    assert_eq!(
        slots("px", dropped),
        array(&[Some("$cardBg"), Some("initial")]),
        "px is in the cleared set and does reset"
    );
}

/// A first value that is simply what the prop already is need not be written.
/// `flexDir="row"` and `alignItems="flex-start"` are the two this hits in
/// practice.
#[test]
fn a_leading_default_is_dropped() {
    assert_eq!(
        slots(
            "flexDir",
            [Set("row"), Absent, Absent, Absent, Set("column")]
        ),
        array(&[None, None, None, None, Some("column")])
    );
    assert_eq!(
        slots(
            "alignItems",
            [Set("flex-start"), Absent, Absent, Absent, Set("center")]
        ),
        array(&[None, None, None, None, Some("center")])
    );
    // The same value on a prop with no registered default stays put.
    assert_eq!(
        slots(
            "justify",
            [Set("flex-start"), Absent, Absent, Absent, Set("center")]
        ),
        array(&[Some("flex-start"), None, None, None, Some("center")])
    );
}

/// The frame's width decides its slot, and the plugin's own unit test pins
/// these five.
#[test]
fn a_width_lands_in_the_slot_its_size_falls_into() {
    for (width, slot) in [(320, 0), (768, 1), (991, 2), (1280, 3), (1600, 4)] {
        assert_eq!(slot_of_width(width), slot, "{width}px");
    }
    // The two screens this repo keeps.
    assert_eq!([360, 992, 1920].map(slot_of_width), [0, 2, 4], "notice");
}
