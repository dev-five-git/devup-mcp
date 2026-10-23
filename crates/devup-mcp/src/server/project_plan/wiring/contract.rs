use super::*;

fn suffix_prefix(actual: &str, expected: &str) -> Option<String> {
    let actual = actual.trim_matches('/').split('/').collect::<Vec<_>>();
    let expected = expected.trim_matches('/').split('/').collect::<Vec<_>>();
    if actual.len() < expected.len() { return None; }
    let offset = actual.len() - expected.len();
    actual[offset..].iter().zip(expected).all(|(left, right)| left == &right || (left.starts_with('{') && left.ends_with('}') && right.starts_with('{') && right.ends_with('}')))
        .then(|| if offset == 0 {String::new()} else {format!("/{}", actual[..offset].join("/"))})
}

fn candidates<'a>(doc: &'a Value, contract: &Value) -> Vec<(&'a str, &'a Value, String)> {
    let method = text(&contract["method"]).to_lowercase();
    let mut found = doc["paths"].as_object().into_iter().flatten().filter_map(|(path, item)| {
        let prefix = suffix_prefix(path, text(&contract["path"]))?;
        item[&method].is_object().then_some((path.as_str(), item, prefix))
    }).collect::<Vec<_>>();
    if found.iter().any(|(_, _, prefix)| prefix.is_empty()) { found.retain(|(_, _, prefix)| prefix.is_empty()); }
    found
}

pub(super) fn report(plan: &Value, path: Option<&Path>) -> (Vec<Value>, Vec<Value>) {
    let Some(path) = path else {return (vec![], vec![])};
    let doc = std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).unwrap_or(Value::Null);
    let contracts = array(&plan["endpointContracts"]);
    let prefixes = contracts.iter().map(|contract| candidates(&doc, contract).into_iter().map(|(_, _, prefix)| prefix).collect::<std::collections::BTreeSet<_>>()).filter(|prefixes| !prefixes.is_empty()).reduce(|left, right| left.intersection(&right).cloned().collect()).unwrap_or_default();
    let (mut missing, mut matched) = (vec![], vec![]);
    for contract in contracts {
        let found = candidates(&doc, contract).into_iter().filter(|(_, _, prefix)| prefix.is_empty() || prefixes.len() == 1 && prefixes.contains(prefix)).collect::<Vec<_>>();
        let mut reasons = vec![];
        let mut field_matches = vec![];
        let mut path_match = Value::Null;
        if let [(path, item, prefix)] = found.as_slice() {
            let operation = &item[text(&contract["method"]).to_lowercase()];
            let request = schema::fields(&doc, schema::body_schema(&doc, &operation["requestBody"]), false, 0);
            let mut response = BTreeMap::<String, schema::Field>::new();
            for (status, body) in operation["responses"].as_object().into_iter().flatten().filter(|(status, _)| status.starts_with('2')) {
                let _ = status;
                for (name, field) in schema::fields(&doc, schema::body_schema(&doc, body), contract["operation"] == "list", 0) {
                    let entry = response.entry(name).or_default();
                    entry.schemas.extend(field.schemas);
                    entry.required |= field.required;
                }
            }
            for (key, fields) in [("requestFields", request), ("responseFields", response)] {
                for field in array(&contract[key]) {
                    let name = text(&field["name"]);
                    if let Some((spelling, actual)) = fields.get_key_value(name).or_else(|| fields.iter().find(|(spelling, _)| draft::normalized(spelling) == draft::normalized(name))) {
                        field_matches.push(json!({"kind":key,"name":name,"openapiName":spelling}));
                        reasons.extend(schema::compare(&doc, field, actual));
                    } else { reasons.push(format!("{key} missing {name}")); }
                }
            }
            let queries = array(&item["parameters"]).iter().chain(array(&operation["parameters"])).map(|parameter| schema::resolve(&doc, parameter)).filter(|parameter| parameter["in"] == "query").collect::<Vec<_>>();
            for query in array(&contract["queryParams"]) {
                let name = text(&query["name"]);
                if let Some(actual) = queries.iter().find(|actual| draft::normalized(text(&actual["name"])) == draft::normalized(name) || array(&query["aliases"]).contains(&actual["name"])) {
                    field_matches.push(json!({"kind":"queryParams","name":name,"openapiName":actual["name"]}));
                } else { reasons.push(format!("queryParams missing {name}")); }
            }
            path_match = json!({"feature":contract["feature"],"method":contract["method"],"path":contract["path"],"openapiPath":path,"prefix":prefix,"fields":field_matches});
        } else { reasons.push("HTTP method/path absent or ambiguous in openapi.json".to_owned()); }
        if !reasons.is_empty() { missing.push(json!({"feature":contract["feature"],"method":contract["method"],"path":contract["path"],"reason":reasons.join("; ")})); }
        if !path_match.is_null() { matched.push(path_match); }
    }
    (missing, matched)
}
