use serde_json::Value;

#[cfg(test)]
use serde_json::json;

/// Declared JSON types of a property schema, following `$ref`, `anyOf`,
/// `oneOf` and `allOf` into the root `$defs`. Depth-limited so a cyclic
/// schema cannot loop.
fn declared_types(
    root: &serde_json::Map<String, Value>,
    schema: &Value,
    depth: u8,
    out: &mut Vec<String>,
) {
    if depth > 6 {
        return;
    }
    let Some(object) = schema.as_object() else {
        return;
    };
    match object.get("type") {
        Some(Value::String(kind)) => out.push(kind.clone()),
        Some(Value::Array(kinds)) => {
            out.extend(kinds.iter().filter_map(|k| k.as_str().map(str::to_owned)))
        }
        _ => {}
    }
    if !out.iter().any(|t| t == "string")
        && object
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().all(Value::is_string))
    {
        out.push("string".to_owned());
    }
    if let Some(reference) = object.get("$ref").and_then(Value::as_str)
        && let Some(name) = reference.rsplit('/').next()
        && let Some(target) = ["$defs", "definitions"]
            .iter()
            .find_map(|key| root.get(*key).and_then(|defs| defs.get(name)))
    {
        declared_types(root, target, depth + 1, out);
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(Value::Array(branches)) = object.get(key) {
            for branch in branches {
                declared_types(root, branch, depth + 1, out);
            }
        }
    }
}

/// Some MCP clients send a structured argument as its JSON text (`"[\"tsx\"]"`
/// for an array, `"true"` for a boolean, `"60000"` for a number). The schema
/// says what the field is, so a string that parses as exactly that is
/// accepted instead of failing the first call. A field that is declared to
/// accept a string is never touched.
pub(super) fn coerce_stringified_arguments(
    schema: &serde_json::Map<String, Value>,
    arguments: &mut serde_json::Map<String, Value>,
) {
    let Some(Value::Object(properties)) = schema.get("properties") else {
        return;
    };
    for (key, value) in arguments.iter_mut() {
        let Value::String(text) = value else {
            continue;
        };
        let Some(property) = properties.get(key) else {
            continue;
        };
        let mut types = Vec::new();
        declared_types(schema, property, 0, &mut types);
        if types.is_empty() || types.iter().any(|t| t == "string") {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<Value>(text.trim()) else {
            continue;
        };
        let accepted = types.iter().any(|kind| match kind.as_str() {
            "array" => parsed.is_array(),
            "object" => parsed.is_object(),
            "boolean" => parsed.is_boolean(),
            "integer" => parsed.is_i64() || parsed.is_u64(),
            "number" => parsed.is_number(),
            "null" => parsed.is_null(),
            _ => false,
        });
        if accepted {
            *value = parsed;
        }
    }
}

#[cfg(test)]
mod coerce_tests {
    use super::*;

    fn schema() -> serde_json::Map<String, Value> {
        json!({
            "type": "object",
            "properties": {
                "outputs": {"type": "array", "items": {"type": "string"}},
                "refresh": {"type": "boolean"},
                "limit": {"type": ["integer", "null"]},
                "url": {"type": ["string", "null"]},
                "mode": {"anyOf": [{"$ref": "#/$defs/Mode"}, {"type": "null"}]},
                "plan": {"anyOf": [{"$ref": "#/$defs/Plan"}, {"type": "null"}]}
            },
            "$defs": {
                "Mode": {"type": "string", "enum": ["a", "b"]},
                "Plan": {"type": "object"}
            }
        })
        .as_object()
        .unwrap()
        .clone()
    }

    fn run(args: Value) -> Value {
        let mut args = args.as_object().unwrap().clone();
        coerce_stringified_arguments(&schema(), &mut args);
        Value::Object(args)
    }

    #[test]
    fn stringified_structured_values_are_decoded() {
        let out = run(json!({
            "outputs": "[\"tsx\"]",
            "refresh": "true",
            "limit": "20",
            "plan": "{\"a\":1}"
        }));
        assert_eq!(out["outputs"], json!(["tsx"]));
        assert_eq!(out["refresh"], json!(true));
        assert_eq!(out["limit"], json!(20));
        assert_eq!(out["plan"], json!({"a": 1}));
    }

    #[test]
    fn string_fields_and_mismatches_are_left_alone() {
        let out = run(json!({
            "url": "[\"x\"]",
            "mode": "[1]",
            "outputs": "tsx",
            "limit": "abc"
        }));
        assert_eq!(out["url"], json!("[\"x\"]"));
        assert_eq!(out["mode"], json!("[1]"));
        assert_eq!(out["outputs"], json!("tsx"));
        assert_eq!(out["limit"], json!("abc"));
    }
}
