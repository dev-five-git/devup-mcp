// A field component's focus state belongs on its container.

/// Whether a component set draws a text field, search box, textarea or
/// select by its Figma name. Its frame is the visual container; the focusable
/// control is the native element the implementation renders inside it.
pub(crate) fn is_field_component(name: &str) -> bool {
    let name = name.to_lowercase().replace(['-', '_', ' '], "");
    ["input", "textfield", "textarea", "searchbar", "searchbox", "searchfield", "select", "combobox", "dropdown", "입력", "검색"]
        .iter()
        .any(|word| name.contains(word))
}

/// A field's state change (focus ring, invalid border, disabled fill) eases
/// in instead of snapping. Figma prototype reactions on the field win; this is
/// the default when the design names none: short, ease-out, and only on the
/// props the states change, so layout never animates.
pub(crate) const FIELD_STATE_TRANSITION: &str = "160ms ease-out";

fn field_transition(
    set_name: &str,
    designed: Option<(String, String)>,
    props: &BTreeSet<String>,
) -> Option<(String, String)> {
    if designed.is_some() || props.is_empty() || !is_field_component(set_name) {
        return designed;
    }
    Some((FIELD_STATE_TRANSITION.to_owned(), css_properties(props)))
}

/// A field container never receives focus itself, so a captured `focus`
/// state on it would never apply: it moves to `_focusWithin`, which matches
/// while the control inside it is focused. A root already rendered as the
/// native control keeps `_focus`. Returns whether anything moved.
fn field_focus_within<V>(
    set_name: &str,
    root: &Tree,
    selectors: &mut BTreeMap<String, BTreeMap<String, V>>,
) -> Option<()> {
    let native = root.props.get("as").is_some_and(|tag| {
        ["input", "textarea", "select"].iter().any(|native| tag.contains(native))
    });
    if native || !is_field_component(set_name) {
        return None;
    }
    let focus = selectors.remove("_focus")?;
    selectors.entry("_focusWithin".to_owned()).or_default().extend(focus);
    Some(())
}
