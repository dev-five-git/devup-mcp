/// The concrete URLs a dynamic route is audited at, and where they came from.
pub(super) struct Dynamic {
    pub urls: Vec<String>,
    pub source: &'static str,
    pub why: String,
    pub assumption: Option<String>,
}

/// `[slug]` -> ("slug", false); `[...slug]` and `[[...slug]]` -> ("slug", true).
fn params(pattern: &str) -> Vec<(String, bool)> {
    pattern
        .split('/')
        .filter(|segment| segment.starts_with('[') && segment.ends_with(']'))
        .map(|segment| {
            let inner = segment.trim_matches(|c| c == '[' || c == ']');
            match inner.strip_prefix("...") {
                Some(name) => (name.to_owned(), true),
                None => (inner.to_owned(), false),
            }
        })
        .collect()
}

fn fill(pattern: &str, values: &BTreeMap<String, String>) -> Option<String> {
    let mut out = Vec::new();
    for segment in pattern.split('/') {
        if segment.starts_with('[') && segment.ends_with(']') {
            let inner = segment.trim_matches(|c| c == '[' || c == ']');
            out.push(values.get(inner.trim_start_matches("...")).cloned()?);
        } else {
            out.push(segment.to_owned());
        }
    }
    Some(out.join("/"))
}

/// Every literal value of `key` in `text`: `key: 'a'`, `"key": "a"`, or
/// `key: ['a', 'b']` (joined with `/` for a catch-all).
pub(super) fn values_of(text: &str, key: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(at) = text[from..].find(key) {
        let start = from + at;
        from = start + key.len();
        let before = start.checked_sub(1).map(|i| bytes[i] as char);
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.') {
            continue;
        }
        let mut rest = text[from..].trim_start_matches(['"', '\'']).trim_start();
        let Some(after) = rest.strip_prefix(':') else {
            continue;
        };
        rest = after.trim_start();
        let quoted = |rest: &str| -> Option<(String, usize)> {
            let quote = rest
                .chars()
                .next()
                .filter(|c| matches!(c, '"' | '\'' | '`'))?;
            let end = rest[1..].find(quote)?;
            Some((rest[1..=end].to_owned(), end + 2))
        };
        if let Some((value, _)) = quoted(rest) {
            found.push(value);
        } else if let Some(list) = rest.strip_prefix('[') {
            let end = list.find(']').unwrap_or(list.len());
            let parts = list[..end]
                .split(',')
                .filter_map(|part| quoted(part.trim()).map(|(value, _)| value))
                .collect::<Vec<_>>();
            if !parts.is_empty() {
                found.push(parts.join("/"));
            }
        }
    }
    found.dedup();
    found
}
