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
