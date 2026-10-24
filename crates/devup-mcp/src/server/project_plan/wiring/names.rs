use super::*;

pub(super) fn camel_case(name: &str) -> String {
    let chars = name.chars().collect::<Vec<_>>();
    let mut result = String::new();
    let mut uppercase = false;
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch == '_' || ch == '-' || ch.is_whitespace() {
            uppercase = !result.is_empty();
            continue;
        }
        let boundary = ch.is_uppercase() && index > 0 &&
            (chars[index - 1].is_lowercase() || chars.get(index + 1).is_some_and(|next| next.is_lowercase()));
        if !result.is_empty() && (uppercase || boundary) { result.extend(ch.to_uppercase()); }
        else { result.extend(ch.to_lowercase()); }
        uppercase = false;
    }
    result
}

pub(super) fn violation(name: &str) -> Option<String> {
    let expected = camel_case(name);
    (name != expected).then(|| format!("casing violation field: expected {expected} got {name}; vespera request/response structs need #[serde(rename_all = \"camelCase\")]") )
}

pub(super) fn schema_violations(doc: &Value, schema: &Value, depth: usize) -> Vec<String> {
    if depth > 16 { return vec![]; }
    let schema = schema::resolve(doc, schema);
    let mut reasons = vec![];
    for (name, property) in schema["properties"].as_object().into_iter().flatten() {
        if let Some(reason) = violation(name) { reasons.push(reason); }
        reasons.extend(schema_violations(doc, property, depth + 1));
    }
    for key in ["items", "additionalProperties"] {
        if schema[key].is_object() { reasons.extend(schema_violations(doc, &schema[key], depth + 1)); }
    }
    for key in ["allOf", "oneOf", "anyOf"] {
        for part in array(&schema[key]) { reasons.extend(schema_violations(doc, part, depth + 1)); }
    }
    reasons
}
