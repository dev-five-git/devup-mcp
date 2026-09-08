pub(crate) fn normalize_token(input: &str) -> String {
    let words = input
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let mut output = String::new();
    for (index, word) in words.into_iter().enumerate() {
        let mut characters = word.chars();
        if let Some(first) = characters.next() {
            if index == 0 {
                output.extend(first.to_lowercase());
            } else {
                output.extend(first.to_uppercase());
            }
            output.extend(characters);
        }
    }
    if output.is_empty() {
        "token".to_owned()
    } else if output
        .chars()
        .next()
        .is_some_and(|character| character.is_numeric())
    {
        format!("_{output}")
    } else {
        output
    }
}

pub(crate) fn variable_token(name: &str, web_syntax: Option<&str>) -> String {
    if let Some(web_syntax) = web_syntax.filter(|value| !value.trim().is_empty()) {
        return normalize_token(web_syntax.trim_start_matches('$'));
    }
    normalize_token(name.rsplit('/').next().unwrap_or(name))
}

/// A style's token and the breakpoint it is for, from its name — the
/// plugin's `styleNameToTypography`.
///
/// `desktop/h1`, `tablet/h1` and `mobile/h1` are `h1` at slots 4, 2 and 0,
/// and `3/bodyXlgBold` is `bodyXlgBold` at slot 3: a leading group that is a
/// breakpoint or a number says where the style applies, not what it is
/// called. Any other name is slot 0 as it is, with a group kept — the corpus
/// has `typography/heading` as `typographyHeading`. The same rule names the
/// `typography="…"` a text is given and the key `devup.json` defines it
/// under, so the two agree.
pub(crate) fn style_token(name: &str) -> (usize, String) {
    let lower = name.to_ascii_lowercase();
    for (prefix, level) in [("desktop/", 4), ("tablet/", 2), ("mobile/", 0)] {
        if lower.starts_with(prefix) {
            return (level, normalize_token(&name[prefix.len()..]));
        }
    }
    if let Some((group, rest)) = name.split_once('/')
        && !rest.is_empty()
        && let Ok(level) = group.trim().parse::<usize>()
    {
        return (level, normalize_token(rest));
    }
    (0, normalize_token(name))
}

#[cfg(test)]
mod style_token_tests {
    use super::style_token;

    #[test]
    fn a_breakpoint_or_number_in_front_says_where_not_what() {
        assert_eq!(style_token("desktop/h1"), (4, "h1".to_owned()));
        assert_eq!(style_token("Tablet/H1"), (2, "h1".to_owned()));
        assert_eq!(style_token("mobile/body lg"), (0, "bodyLg".to_owned()));
        assert_eq!(style_token("3/bodyXlgBold"), (3, "bodyXlgBold".to_owned()));
        assert_eq!(style_token("0/buttonSm"), (0, "buttonSm".to_owned()));
        assert_eq!(
            style_token("typography/heading"),
            (0, "typographyHeading".to_owned())
        );
        assert_eq!(style_token("Heading/H1"), (0, "headingH1".to_owned()));
    }
}
