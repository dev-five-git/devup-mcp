use super::*;

pub(super) fn resolve<'a>(doc: &'a Value, value: &'a Value) -> &'a Value {
    let mut current = value;
    for _ in 0..16 {
        let Some(reference) = current["$ref"].as_str().and_then(|reference| reference.strip_prefix('#')) else { break };
        let Some(target) = doc.pointer(reference) else { break };
        current = target;
    }
    current
}

pub(super) fn body_schema<'a>(doc: &'a Value, body: &'a Value) -> &'a Value {
    let content = &resolve(doc, body)["content"];
    ["application/json", "multipart/form-data", "application/x-www-form-urlencoded"].into_iter()
        .find_map(|media| content.get(media).map(|value| &value["schema"]))
        .unwrap_or(&Value::Null)
}

#[derive(Default)]
pub(super) struct Field {
    pub schemas: Vec<Value>,
    pub required: bool,
}

pub(super) fn fields(doc: &Value, schema: &Value, list: bool, depth: usize) -> BTreeMap<String, Field> {
    if depth > 16 { return BTreeMap::new(); }
    let schema = resolve(doc, schema);
    if schema["type"] == "array" { return fields(doc, &schema["items"], false, depth + 1); }
    let mut result = BTreeMap::<String, Field>::new();
    for (name, property) in schema["properties"].as_object().into_iter().flatten() {
        let target = resolve(doc, property);
        if list && ["items", "data", "results"].contains(&name.as_str()) {
            let wrapped = fields(doc, target, true, depth + 1);
            if !wrapped.is_empty() { result.extend(wrapped); continue; }
        }
        result.insert(name.clone(), Field {schemas:vec![target.clone()], required:array(&schema["required"]).contains(&json!(name))});
    }
    for key in ["allOf", "oneOf", "anyOf"] {
        for part in array(&schema[key]) {
            for (name, field) in fields(doc, part, list, depth + 1) {
                let entry = result.entry(name).or_default();
                entry.schemas.extend(field.schemas);
                entry.required |= field.required;
            }
        }
    }
    for name in array(&schema["required"]).iter().filter_map(Value::as_str) {
        if let Some(field) = result.get_mut(name) { field.required = true; }
    }
    result
}

fn variants(doc: &Value, schema: &Value, depth: usize) -> Vec<(String, String)> {
    if depth > 16 { return vec![]; }
    let schema = resolve(doc, schema);
    let mut result = vec![];
    let types = schema["type"].as_str().map(|kind| vec![kind]).unwrap_or_else(|| array(&schema["type"]).iter().filter_map(Value::as_str).collect());
    for kind in types { result.push((kind.to_owned(), text(&schema["format"]).to_owned())); }
    if schema["nullable"] == true { result.push(("null".into(), String::new())); }
    for key in ["allOf", "oneOf", "anyOf"] {
        for part in array(&schema[key]) { result.extend(variants(doc, part, depth + 1)); }
    }
    result
}

fn expected(field: &Value) -> (&str, &str) {
    let kind = field["type"].as_str().or_else(|| field["type"]["kind"].as_str()).unwrap_or_default();
    match kind {
        "integer" | "bigint" | "big_int" | "small_int" | "smallint" => ("integer", ""),
        "float" | "real" | "double_precision" | "decimal" | "numeric" => ("number", ""),
        "boolean" | "bool" => ("boolean", ""),
        "date" => ("string", "date"),
        "timestamp" | "timestamptz" | "datetime" => ("string", "date-time"),
        "json" => ("object", ""),
        "enum" => if array(&field["type"]["values"]).iter().any(|value| value["value"].is_number()) { ("integer", "") } else { ("string", "") },
        "text" | "varchar" | "char" | "uuid" | "string" => ("string", ""),
        other if other.starts_with("varchar(") => ("string", ""),
        _ => ("", ""),
    }
}

pub(super) fn compare(doc: &Value, field: &Value, actual: &Field) -> Vec<String> {
    let name = text(&field["name"]);
    let (kind, format) = expected(field);
    let types = actual.schemas.iter().flat_map(|schema| variants(doc, schema, 0)).collect::<Vec<_>>();
    let mut reasons = vec![];
    if !kind.is_empty() && !types.is_empty() && !types.iter().any(|(found, found_format)| found == kind && (format.is_empty() || format == found_format)) {
        let display = |kind: &str, format: &str| if format.is_empty() {kind.to_owned()} else {format!("{kind}({format})")};
        reasons.push(format!("type mismatch {name}: expected {} got {}", display(kind, format), types.iter().map(|(kind, format)| display(kind, format)).collect::<Vec<_>>().join("|")));
    }
    if let Some(nullable) = field["nullable"].as_bool()
        && !types.is_empty() && nullable != types.iter().any(|(kind, _)| kind == "null") {
        reasons.push(format!("nullable mismatch {name}: expected {nullable}"));
    }
    if let Some(required) = field["required"].as_bool() && required != actual.required {
        reasons.push(format!("required mismatch {name}: expected {required} got {}", actual.required));
    }
    reasons
}
