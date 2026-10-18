

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_prop_lists_are_sorted_for_binary_search() {
        let mut sorted = DEVUP_STYLE_PROPS.to_vec();
        sorted.sort_unstable();
        assert_eq!(DEVUP_STYLE_PROPS, sorted.as_slice());
        let mut colors = DEVUP_COLOR_LIKE_PROPS.to_vec();
        colors.sort_unstable();
        assert_eq!(DEVUP_COLOR_LIKE_PROPS, colors.as_slice());
        let mut lengths = DEVUP_LENGTH_LIKE_PROPS.to_vec();
        lengths.sort_unstable();
        assert_eq!(DEVUP_LENGTH_LIKE_PROPS, lengths.as_slice());
    }

    #[test]
    fn color_and_length_subsets_are_subsets_of_style_props() {
        for prop in DEVUP_COLOR_LIKE_PROPS {
            assert!(is_known_style_prop(prop), "{prop} missing from style props");
        }
        for prop in DEVUP_LENGTH_LIKE_PROPS {
            assert!(is_known_style_prop(prop), "{prop} missing from style props");
        }
    }

    /// A shorthand and the `*Color` spelling of the same property are the
    /// same property, and a caller picks whichever reads better. Covering
    /// one and not the other makes the rule depend on spelling: `bg` and
    /// `stroke` were unchecked while `bgColor` and `strokeColor`'s
    /// counterpart `stroke` were - so the same screen passed or failed on
    /// how its author had typed it.
    #[test]
    fn a_color_prop_and_its_shorthand_are_covered_together() {
        for (shorthand, long_form) in [
            ("bg", "backgroundColor"),
            ("bgColor", "backgroundColor"),
            ("stroke", "strokeColor"),
        ] {
            assert!(
                is_color_like_prop(shorthand),
                "{shorthand} is color-like but is not covered"
            );
            assert!(
                is_color_like_prop(long_form),
                "{long_form} is color-like but is not covered"
            );
        }
    }

    /// The physical border colors were covered and their logical
    /// equivalents were not, which is the same property written the way
    /// CSS writes it for a right-to-left document.
    #[test]
    fn logical_border_colors_are_covered_like_the_physical_ones() {
        for prop in [
            "borderBlockColor",
            "borderBlockStartColor",
            "borderBlockEndColor",
            "borderInlineColor",
            "borderInlineStartColor",
            "borderInlineEndColor",
        ] {
            assert!(is_color_like_prop(prop), "{prop} is not covered");
        }
    }

    /// Having "color" in the name is not what makes a prop color-valued.
    /// These four take keywords, and reporting a hex in them would be
    /// reporting something nobody can write.
    #[test]
    fn keyword_valued_props_named_color_are_not_color_like() {
        for prop in [
            "colorScheme",
            "colorInterpolationFilters",
            "forcedColorAdjust",
            "printColorAdjust",
        ] {
            assert!(
                is_known_style_prop(prop),
                "{prop} should still be a known style prop"
            );
            assert!(!is_color_like_prop(prop), "{prop} is not color-valued");
        }
    }

    #[test]
    fn recognizes_known_and_rejects_unknown_props() {
        assert!(is_known_style_prop("bg"));
        assert!(is_known_style_prop("borderRadius"));
        assert!(!is_known_style_prop("bgg"));
        assert!(is_color_like_prop("bgColor"));
        // `bg` is devup-ui's shorthand for `background` and what the Figma
        // generator writes for a solid fill. Excluding it meant the hardcoded
        // color the generator emits most often was the one color nothing
        // looked at.
        assert!(is_color_like_prop("bg"));
        assert!(is_length_like_prop("w"));
        assert!(is_known_non_style_prop("onClick"));
        assert!(is_known_non_style_prop("data-testid"));
        assert!(is_known_non_style_prop("_hover"));
        assert!(is_known_non_style_prop("as"));
    }
}
