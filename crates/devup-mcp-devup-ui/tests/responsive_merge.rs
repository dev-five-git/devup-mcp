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

/// `about/responsive.tsx`, whose widths sit at slots 0, 2 and 4 as `notice`'s
/// do. What it adds is the case neither earlier screen has: a width that goes
/// back. `notice` only ever toggles one way, so three slots always suffice
/// there; `popup` only ever moves a number forward. Here a region is shown at
/// tablet and hidden again at desktop, and several values return at desktop to
/// what mobile said. Slot 4 cannot be left off in either case — omitting it
/// would inherit the tablet value — so these are the arrays that need all five.
#[test]
fn the_about_screen_needs_every_slot() {
    let case = |prop, mobile, tablet, desktop, expected| Case {
        prop,
        mobile,
        tablet,
        desktop,
        expected,
    };
    let cases = [
        // Shown at tablet only. The closing "none" is not redundant with the
        // opening one: without it the tablet "flex" would carry into desktop.
        case(
            "display",
            Set("none"),
            Set("flex"),
            Set("none"),
            &[Some("none"), None, Some("flex"), None, Some("none")],
        ),
        // Shown at desktop only. Here the middle width repeats slot 0, so it
        // drops out and the array is sparse rather than full.
        case(
            "display",
            Set("none"),
            Set("none"),
            Set("flex"),
            &[Some("none"), None, None, None, Some("flex")],
        ),
        // A value that returns. Mobile and desktop agree, but the agreement is
        // across a tablet that differs, so slot 4 has to restate it.
        case(
            "w",
            Set("770px"),
            Set("778px"),
            Set("770px"),
            &[Some("770px"), None, Some("778px"), None, Some("770px")],
        ),
        case(
            "py",
            Set("120px"),
            Set("100px"),
            Set("120px"),
            &[Some("120px"), None, Some("100px"), None, Some("120px")],
        ),
        case(
            "gap",
            Set("60px"),
            Set("40px"),
            Set("60px"),
            &[Some("60px"), None, Some("40px"), None, Some("60px")],
        ),
        // Set at tablet only, on a prop that is neither spacing nor layout.
        // `textAlign` is in the cleared set, so desktop spends an "initial"
        // rather than inheriting "right".
        case(
            "textAlign",
            Unset,
            Set("right"),
            Unset,
            &[None, None, Some("right"), None, Some("initial")],
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
            slots(prop, [mobile, Absent, tablet, Absent, desktop]),
            array(expected),
            "{prop}: {mobile:?} / {tablet:?} / {desktop:?}"
        );
    }
}

/// A node the design hides at every width is not dropped — the plugin's
/// `getVisibilityProps` turns `visible: false` into `display: 'none'`, and
/// merging three widths that all say so collapses back to the plain literal
/// `about/responsive.tsx` opens with.
#[test]
fn a_node_hidden_at_every_width_stays_a_literal() {
    assert_eq!(
        merge_slots(
            "display",
            &[Set("none"), Absent, Set("none"), Absent, Set("none")]
        ),
        Merged::Same(Some("none".to_owned()))
    );
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
    // The screens this repo has an answer for. `about` is drawn at the same
    // three widths as `notice`, measured off the file as 360x7240, 992x5619
    // and 1920x4757 — which is what puts its arrays in slots 0, 2 and 4 rather
    // than anywhere else, and is a reading of the frames rather than of their
    // names. `popup` is the one that lands differently, on 0, 1 and 4.
    assert_eq!(
        [360, 992, 1920].map(slot_of_width),
        [0, 2, 4],
        "notice and about"
    );
    assert_eq!([390, 768].map(slot_of_width), [0, 1], "popup");
}
#[test]
fn r2_visibility_restores_middle_breakpoint_between_hidden_assets() {
    use devup_mcp_devup_ui::codegen::responsive::Drawn::{Absent, Set, Unset};
    assert_eq!(
        merge_slots(
            "visibility",
            &[Set("hidden"), Absent, Unset, Absent, Set("hidden")]
        ),
        Merged::Array(vec![
            Some("hidden".into()),
            None,
            Some("initial".into()),
            None,
            Some("hidden".into())
        ])
    );
}
