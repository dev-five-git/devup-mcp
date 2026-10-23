use super::*;

pub(super) fn normalized(name: &str) -> String {
    name.chars().filter(|ch| !matches!(ch, '_' | '-')).flat_map(char::to_lowercase).collect()
}

fn resource(name: &str) -> String {
    let name = name.replace('_', "-");
    let word = name.rsplit('-').next().unwrap_or(&name);
    if matches!(word, "news" | "series" | "species") || (word.ends_with('s') && !word.ends_with("ss") && !word.ends_with("us")) {
        name
    } else if word.ends_with('y') && word.chars().rev().nth(1).is_some_and(|ch| !"aeiou".contains(ch)) {
        format!("{}ies", name.strip_suffix('y').unwrap_or(&name))
    } else if ["s", "x", "z", "ch", "sh"].iter().any(|suffix| word.ends_with(suffix)) {
        format!("{name}es")
    } else { format!("{name}s") }
}

fn contains(evidence: &str, words: &[&str]) -> bool {
    words.iter().any(|word| evidence.contains(word))
}

fn structure(node: &Value, counts: &mut BTreeMap<String, u64>, labels: &mut Vec<String>) {
    if let Some(name) = node.as_str() {
        *counts.entry(name.to_owned()).or_default() += 1;
        if contains(&normalized(name), &["input", "textfield", "textarea", "select", "checkbox", "radio"]) { labels.push(name.to_owned()); }
        return;
    }
    let name = node["componentName"].as_str().or_else(|| node["name"].as_str()).unwrap_or_default();
    if !name.is_empty() {
        *counts.entry(name.to_owned()).or_default() += node["count"].as_u64().or_else(|| node["instanceCount"].as_u64()).unwrap_or(1);
    }
    let kind = format!("{} {} {}", name, text(&node["type"]), text(&node["nodeType"]));
    if contains(&normalized(&kind), &["input", "textfield", "textarea", "select", "checkbox", "radio"]) {
        labels.push(node["label"].as_str().or_else(|| node["fieldName"].as_str()).unwrap_or(name).to_owned());
    }
    for key in ["parts", "children", "nodes", "instances"] {
        for child in array(&node[key]) { structure(child, counts, labels); }
    }
    for key in ["repeatedInstances", "instanceCounts"] {
        for (name, count) in node[key].as_object().into_iter().flatten() {
            *counts.entry(name.clone()).or_default() += count.as_u64().unwrap_or(0);
        }
    }
}

fn rules(column: &Value) -> Value {
    let name = text(&column["name"]).to_lowercase();
    let mut rules = json!({"required":column["nullable"] == false && column["default"].is_null()});
    if let Some(length) = column["type"]["length"].as_u64().or_else(|| column["length"].as_u64()) {
        rules["maxLength"] = json!(length);
    } else if let Some(length) = text(&column["type"]).strip_prefix("varchar(").and_then(|value| value.strip_suffix(')')).and_then(|value| value.parse::<u64>().ok()) {
        rules["maxLength"] = json!(length);
    }
    for (format, words) in [("email", vec!["email"]), ("phone", vec!["phone", "tel"]), ("url", vec!["url", "link"])] {
        if contains(&name, &words) { rules["format"] = json!(format); break; }
    }
    if matches!(text(&column["type"]), "boolean" | "bool") && contains(&name, &["agree", "consent", "privacy", "동의"]) {
        rules["const"] = json!(true);
    }
    if column["type"]["kind"] == "enum" { rules["enum"] = column["type"]["values"].clone(); }
    rules
}

fn queries(entity: &Value, evidence: &str) -> Vec<Value> {
    let mut queries = vec![];
    if contains(evidence, &["pagination", "페이지", "page number"]) {
        queries.extend([json!({"name":"page","type":"integer"}), json!({"name":"pageSize","type":"integer","aliases":["size"]})]);
    }
    if contains(evidence, &["sort", "정렬"]) {
        let keys = array(&entity["columns"]).iter().filter(|column| {
            let kind = column["type"].as_str().or_else(|| column["type"]["kind"].as_str()).unwrap_or_default();
            contains(kind, &["int", "float", "real", "double", "decimal", "numeric", "date", "time"]) || text(&column["name"]).contains("title")
        }).map(|column| column["name"].clone()).collect::<Vec<_>>();
        queries.push(json!({"name":"sort","type":"string","allowedKeys":keys}));
    }
    if contains(evidence, &["search", "검색"]) { queries.push(json!({"name":"q","type":"string"})); }
    if contains(evidence, &["filter", "category", "필터", "카테고리", "탭"]) {
        for column in array(&entity["columns"]) {
            let name = text(&column["name"]);
            if column["type"]["kind"] == "enum" || name.ends_with("_id") || contains(name, &["category", "카테고리"]) {
                queries.push(json!({"name":name,"type":column["type"],"values":column["type"]["values"]}));
            }
        }
    }
    queries
}

pub(in crate::server::project_plan) fn draft(plan: &mut Value) {
    if !erd::api(plan) { return; }
    let (mut mappings, mut contracts) = (vec![], vec![]);
    for feature in array(&plan["features"]) {
        let screens = units::feature_screens(plan, feature);
        let evidence = format!("{} {} {} {}", feature["scope"], feature["screens"], feature["acceptance"], json!(screens)).to_lowercase();
        let mut driven = BTreeMap::<&str, Vec<String>>::new();
        for (op, words) in [
            ("list", vec!["list", "table", "search", "filter", "pagination", "목록", "검색", "표"]),
            ("get", vec!["detail", "상세"]),
            ("create", vec!["form", "create", "signup", "input", "textarea", "폼", "등록", "문의"]),
            ("update", vec!["edit", "update", "수정"]),
            ("delete", vec!["delete", "remove", "삭제", "admin", "cms", "관리"]),
        ] {
            for word in words { if evidence.contains(word) { driven.entry(op).or_default().push(format!("keyword: {word}")); } }
        }
        let (mut counts, mut labels) = (BTreeMap::new(), vec![]);
        for screen in &screens {
            structure(screen, &mut counts, &mut labels);
            for key in ["url", "route", "routePath", "proposedRoute"] {
                if text(&screen[key]).split('/').any(|segment| segment.starts_with('[') && segment.ends_with(']')) {
                    driven.entry("get").or_default().push(format!("dynamic route: {}", text(&screen[key])));
                }
            }
        }
        for route in array(&feature["screens"]).iter().filter_map(Value::as_str) {
            if route.split('/').any(|segment| segment.starts_with('[') && segment.ends_with(']')) { driven.entry("get").or_default().push(format!("dynamic route: {route}")); }
        }
        for (name, count) in counts.iter().filter(|(_, count)| **count >= 3) {
            driven.entry("list").or_default().push(format!("repeated component: {name} ({count})"));
        }
        if !labels.is_empty() { driven.entry("create").or_default().push(format!("form controls: {}", labels.join(", "))); }
        let entities = units::feature_entities(plan, feature);
        if driven.is_empty() && entities.is_empty() { continue; }
        if driven.is_empty() { driven.insert("list", vec!["entity mapping".into()]); }
        let operations = driven.keys().copied().collect::<Vec<_>>();
        let columns = array(&plan["entities"]).iter().filter(|entity| entities.contains(&text(&entity["name"]).to_owned())).flat_map(|entity| array(&entity["columns"])).collect::<Vec<_>>();
        let form_fields = labels.iter().map(|label| {
            let column = columns.iter().find(|column| normalized(text(&column["name"])) == normalized(label));
            json!({"label":label,"column":column.map(|column| column["name"].clone()),"status":if column.is_some() {"matched"} else {"unmatched"}})
        }).collect::<Vec<_>>();
        mappings.push(json!({"feature":feature["id"],"screens":feature["screens"],"candidateEntities":entities,"requiredOperations":operations,"operationEvidence":driven,"formFields":form_fields,"status":"draft"}));
        for name in entities {
            let Some(entity) = array(&plan["entities"]).iter().find(|entity| entity["name"] == name) else {continue};
            for op in &operations {
                let method = match *op {"create" => "POST", "update" => "PATCH", "delete" => "DELETE", _ => "GET"};
                let resource = resource(&name);
                let path = if matches!(*op, "get" | "update" | "delete") {format!("/{resource}/{{id}}") } else {format!("/{resource}")};
                let fields = array(&entity["columns"]).iter().map(|column| json!({"name":column["name"],"type":column["type"],"nullable":column["nullable"],"required":column["nullable"] == false})).collect::<Vec<_>>();
                let request_fields = if matches!(*op, "create" | "update") {
                    array(&entity["columns"]).iter().filter(|column| column["primary_key"].is_null() && column["default"].is_null()).map(|column| json!({"name":column["name"],"type":column["type"],"nullable":column["nullable"],"required":*op == "create" && column["nullable"] == false,"rules":rules(column)})).collect::<Vec<_>>()
                } else {vec![]};
                contracts.push(json!({"feature":feature["id"],"entity":name,"operation":op,"method":method,"path":path,"queryParams":if *op == "list" {queries(entity, &evidence)} else {vec![]},
                    "operationEvidence":driven[op],"formFields":form_fields,"requestFields":request_fields,"responseFields":if *op == "delete" {vec![]} else {fields},"status":"draft","pathEvidence":"entity-name convention; confirm API path before implementation"}));
            }
        }
    }
    plan["dataMappings"] = json!(mappings);
    plan["endpointContracts"] = json!(contracts);
}
