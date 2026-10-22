use super::*;

const SCHEMA: &str = "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json";
pub(super) fn api(plan: &Value) -> bool {
    text(&plan["facts"]["parts"])
        .to_ascii_lowercase()
        .contains("api")
}
const COMPLEX_KINDS: [&str; 5] = ["varchar", "char", "numeric", "enum", "custom"];
const SIMPLE_TYPES: [&str; 19] = [
    "small_int",
    "integer",
    "big_int",
    "real",
    "double_precision",
    "text",
    "boolean",
    "date",
    "time",
    "timestamp",
    "timestamptz",
    "interval",
    "bytea",
    "uuid",
    "json",
    "inet",
    "cidr",
    "macaddr",
    "xml",
];
fn unsupported_type(t: &Value) -> Option<String> {
    match t {
        Value::String(s) => (!SIMPLE_TYPES.contains(&s.as_str())).then(|| s.clone()),
        Value::Object(_) => {
            let kind = text(&t["kind"]);
            (!COMPLEX_KINDS.contains(&kind)).then(|| kind.to_owned())
        }
        _ => None,
    }
}
fn ident(s: &str) -> bool {
    !s.is_empty()
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn col(name: &str, typ: Value) -> Value {
    json!({"name":name,"type":typ,"nullable":false})
}
fn entity(name: &str, mut columns: Vec<Value>) -> Value {
    let mut id = col("id", json!("integer"));
    id["primary_key"] = json!({"auto_increment":true});
    columns.insert(0, id);
    json!({"$schema":SCHEMA,"name":name,"columns":columns})
}
fn inferred(plan: &Value) -> Vec<Value> {
    // Figma field labels are evidence, not authoritative database semantics.
    // Every inferred column is reviewed in the ERD round before writing.
    let mut result = Vec::new();
    let screens = array(&plan["facts"]["screens"]);
    let names = screens
        .iter()
        .map(|s| text(&s["name"]).to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    if names.contains("signup") || names.contains("sign up") {
        let mut email = col("email", json!("text"));
        email["unique"] = json!(true);
        let mut columns = vec![
            email,
            col(
                "role",
                json!({"kind":"enum","name":"user_role","values":[{"name":"guest","value":0},{"name":"user","value":10},{"name":"moderator","value":50},{"name":"admin","value":100}]}),
            ),
        ];
        for screen in screens
            .iter()
            .filter(|s| text(&s["name"]).to_lowercase().contains("sign"))
        {
            for field in array(&screen["fields"]) {
                let label = field
                    .as_str()
                    .or_else(|| field["name"].as_str())
                    .unwrap_or_default()
                    .to_lowercase()
                    .replace(' ', "_");
                if ident(&label)
                    && !["email", "role", "id", "password"].contains(&label.as_str())
                    && !columns.iter().any(|c| c["name"] == label)
                {
                    columns.push(col(&label, json!("text")));
                }
            }
        }
        result.push(entity("user", columns));
    }
    if names.contains("post") {
        let mut columns = vec![col("title", json!("text")), col("body", json!("text"))];
        if result.iter().any(|e| e["name"] == "user") {
            columns.push(fk("user_id", "user", "restrict"));
        }
        result.push(entity("post", columns));
    }
    result
}
fn fk(column: &str, table: &str, on_delete: &str) -> Value {
    json!({"name":column,"type":"integer","nullable":on_delete == "set_null","index":true,
        "foreign_key":{"ref_table":table,"ref_columns":["id"],"on_delete":on_delete}})
}
fn scan_models(root: &Path, result: &mut Vec<Value>) -> Result<(), DevupError> {
    let Ok(files) = std::fs::read_dir(root) else {
        return Ok(());
    };
    for file in files {
        let path = file.map_err(|e| invalid(e.to_string()))?.path();
        if path.extension().is_some_and(|e| e == "json") {
            let meta = std::fs::symlink_metadata(&path).map_err(|e| invalid(e.to_string()))?;
            if meta.file_type().is_symlink() || meta.len() > 512 * 1024 {
                return Err(invalid("Unsafe or oversized existing model"));
            }
            let model =
                serde_json::from_slice(&std::fs::read(&path).map_err(|e| invalid(e.to_string()))?)
                    .map_err(|e| invalid(format!("{}: {e}", path.display())))?;
            result.push(model);
        }
    }
    Ok(())
}
pub(super) fn draft(input: &ProjectPlanInput, plan: &mut Value) -> Result<(), DevupError> {
    if !api(plan) {
        return Ok(());
    }
    let root = Path::new(&input.project_root);
    let mut dir = input
        .models_dir
        .clone()
        .or_else(|| plan["modelsDir"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "models".into());
    if input.models_dir.is_none() && plan["modelsDir"].is_null() {
        let config = root.join("vespertide.json");
        if config.is_file() {
            let conf: Value =
                serde_json::from_slice(&std::fs::read(config).map_err(|e| invalid(e.to_string()))?)
                    .map_err(|e| invalid(e.to_string()))?;
            if let Some(d) = conf["modelsDir"].as_str() {
                dir = d.into();
            }
        } else if let Ok(entries) = std::fs::read_dir(root.join("apis")) {
            let dirs = entries
                .filter_map(Result::ok)
                .filter(|e| e.path().join("vespertide.json").is_file())
                .collect::<Vec<_>>();
            if dirs.len() == 1 {
                let conf: Value = serde_json::from_slice(
                    &std::fs::read(dirs[0].path().join("vespertide.json"))
                        .map_err(|e| invalid(e.to_string()))?,
                )
                .map_err(|e| invalid(e.to_string()))?;
                dir = format!(
                    "apis/{}/{}",
                    dirs[0].file_name().to_string_lossy(),
                    conf["modelsDir"].as_str().unwrap_or("models")
                );
                plan["modelsWorkingDirectory"] =
                    json!(format!("apis/{}", dirs[0].file_name().to_string_lossy()));
            } else if dirs.len() > 1 {
                return Err(invalid("Several APIs exist; set modelsDir explicitly"));
            }
        }
    }
    if Path::new(&dir).is_absolute()
        || !Path::new(&dir)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        || dir
            .split(['/', '\\'])
            .any(|p| ["migrations", "src"].contains(&p))
    {
        return Err(invalid(
            "modelsDir must be a relative source-model directory, outside migrations/ and src/",
        ));
    }
    plan["modelsDir"] = json!(dir);
    if plan["modelsWorkingDirectory"].is_null() {
        plan["modelsWorkingDirectory"] = json!(".");
    }
    let mut entities = if !input.entities.is_empty() {
        input.entities.clone()
    } else {
        array(&plan["entities"]).to_vec()
    };
    if entities.is_empty() {
        scan_models(&root.join(&dir), &mut entities)?;
    }
    if entities.is_empty() {
        entities = inferred(plan);
    }
    let relations: Vec<Relation> = if !input.relations.is_empty() {
        input.relations.clone()
    } else {
        serde_json::from_value(plan["relations"].clone()).unwrap_or_default()
    };
    for r in &relations {
        if !ident(&r.from)
            || !ident(&r.to)
            || ![
                "cascade",
                "restrict",
                "set_null",
                "set_default",
                "no_action",
            ]
            .contains(&r.on_delete.as_str())
        {
            return Err(invalid("Invalid relation identifier or delete action"));
        }
        if !entities.iter().any(|e| e["name"] == r.from)
            || !entities.iter().any(|e| e["name"] == r.to)
        {
            return Err(invalid("Relation endpoints must exist"));
        }
        match r.cardinality.as_str() {
            "manyToMany" => {
                let name = r
                    .join_table
                    .clone()
                    .unwrap_or_else(|| format!("{}_{}", r.from, r.to));
                if entities.iter().any(|e| e["name"] == name) {
                    continue;
                }
                let columns = [&r.from, &r.to]
                    .iter()
                    .map(|table| {
                        let mut c = fk(&format!("{table}_id"), table, &r.on_delete);
                        c["primary_key"] = json!(true);
                        c
                    })
                    .collect::<Vec<_>>();
                entities.push(json!({"$schema":SCHEMA,"name":name,"columns":columns}));
            }
            "oneToMany" => {
                let name = r.column.clone().unwrap_or_else(|| format!("{}_id", r.from));
                let columns = entities.iter_mut().find(|e| e["name"] == r.to).unwrap()["columns"]
                    .as_array_mut()
                    .ok_or_else(|| invalid("Entity needs columns"))?;
                let reference = fk(&name, &r.from, &r.on_delete);
                if let Some(c) = columns.iter_mut().find(|c| c["name"] == name) {
                    c["foreign_key"] = reference["foreign_key"].clone();
                    c["index"] = json!(true);
                } else {
                    columns.push(reference);
                }
            }
            _ => return Err(invalid("cardinality must be oneToMany or manyToMany")),
        }
    }
    for e in &mut entities {
        e["$schema"] = json!(SCHEMA);
    }
    validate(&entities)?;
    plan["entities"] = json!(entities);
    plan["relations"] = json!(relations);
    plan["backendCommands"] = json!([
        "vespertide diff",
        "vespertide sql",
        "vespertide revision -m \"confirmed project plan\"",
        "vespertide export --orm seaorm"
    ]);
    plan["backendNotes"] = json!([
        "Before cargo check, make sure the API Cargo.toml depends on serde (derive) and serde_json; exported entities use them.",
        "Add `mod models;` to the API main.rs; export does not wire the generated src/models.",
        "Right after export, cargo clippy -- -D warnings reports never-constructed ModelEx structs until routes use them; that is expected before the route units."
    ]);
    Ok(())
}
pub(super) fn validate(entities: &[Value]) -> Result<(), DevupError> {
    let mut names = std::collections::BTreeSet::new();
    for e in entities {
        let name = text(&e["name"]);
        if !ident(name) || !names.insert(name) || array(&e["columns"]).is_empty() {
            return Err(invalid("Entities need unique snake_case names and columns"));
        }
        let mut columns = std::collections::BTreeSet::new();
        for c in array(&e["columns"]) {
            if !ident(text(&c["name"]))
                || !columns.insert(text(&c["name"]))
                || !c["nullable"].is_boolean()
                || !(c["type"].is_string() || c["type"].is_object())
            {
                return Err(invalid(
                    "Columns need unique snake_case names, type and nullable",
                ));
            }
            if let Some(bad) = unsupported_type(&c["type"]) {
                let hint = if bad == "jsonb" { "; use json" } else { "" };
                return Err(invalid(format!(
                    "Column {}.{} has type {bad}, which vespertide cannot parse{hint}",
                    text(&e["name"]),
                    text(&c["name"])
                )));
            }
            if !c["foreign_key"].is_null() && c["index"].is_null() {
                return Err(invalid("Foreign keys require inline indexes"));
            }
            if c["name"] == "role" {
                let values = array(&c["type"]["values"]);
                if c["type"]["kind"] != "enum"
                    || values.is_empty()
                    || values.iter().any(|v| !v["value"].is_i64())
                    || values.windows(2).any(|w| {
                        w[1]["value"].as_i64().unwrap() - w[0]["value"].as_i64().unwrap() < 2
                    })
                {
                    return Err(invalid("Roles require gapped integer enums"));
                }
            }
        }
        if array(&e["constraints"])
            .iter()
            .any(|c| c["type"] != "check")
        {
            return Err(invalid("Only CHECK constraints belong at table level"));
        }
    }
    for e in entities {
        for c in array(&e["columns"]) {
            let fk = &c["foreign_key"];
            if let Some(table) = fk["ref_table"].as_str() {
                let target = entities
                    .iter()
                    .find(|e| e["name"] == table)
                    .ok_or_else(|| invalid("Foreign key references missing entity"))?;
                for col in array(&fk["ref_columns"]) {
                    if !array(&target["columns"]).iter().any(|c| c["name"] == *col) {
                        return Err(invalid("Foreign key references missing column"));
                    }
                }
                if fk["on_delete"] == "set_null" && c["nullable"] != true {
                    return Err(invalid("set_null requires nullable column"));
                }
            }
        }
    }
    Ok(())
}
pub(super) fn write_models(
    tx: &mut OutputTransaction,
    policy: &OutputPolicy,
    root: &str,
    plan: &Value,
) -> Result<(), DevupError> {
    if !api(plan) {
        return Ok(());
    }
    let entities = array(&plan["entities"]);
    if entities.is_empty() {
        return Err(invalid("API plan requires a complete ERD"));
    }
    validate(entities)?;
    for e in entities {
        let file = format!("{}/{}.json", text(&plan["modelsDir"]), text(&e["name"]));
        let bytes = serde_json::to_vec_pretty(e).map_err(|e| invalid(e.to_string()))?;
        stage(tx, policy, root, &file, &bytes)?;
    }
    Ok(())
}
pub(super) fn mermaid(entities: &[Value]) -> String {
    let mut out = "erDiagram\n".to_owned();
    for e in entities {
        out.push_str(&format!("  {} {{\n", text(&e["name"])));
        for c in array(&e["columns"]) {
            out.push_str(&format!(
                "    {} {}\n",
                c["type"].as_str().or_else(|| c["type"]["kind"].as_str()).unwrap_or("unknown"),
                text(&c["name"])
            ));
        }
        out.push_str("  }\n");
        for c in array(&e["columns"]) {
            let fk = &c["foreign_key"];
            let table = fk["ref_table"]
                .as_str()
                .or_else(|| fk.as_str().and_then(|s| s.split('.').next()));
            if let Some(table) = table {
                out.push_str(&format!(
                    "  {table} ||--o{{ {} : {}\n",
                    text(&e["name"]),
                    text(&c["name"])
                ));
            }
        }
    }
    out
}
