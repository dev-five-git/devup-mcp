//! Re-execute only typed aggregate SELECTs against capability-read, hashed
//! SQLite snapshots deserialized read-only in memory. Never open a live DB.
use super::{output::OutputPolicy, source_sync::digest};
use devup_mcp_figma::{DevupError, ErrorCode};
use rusqlite::{
    Connection,
    types::{Value as SqlValue, ValueRef},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    path::Path,
};
const PHASES: [&str; 4] = ["baseline", "mutated", "empty", "error"];
#[path = "data_verification/inputs.rs"]
mod inputs;
pub use inputs::DataBindingInput;
pub(super) fn validate_config(config: &DataBindingInput) -> Result<(), DevupError> {
    inputs::validate(config)
}
fn invalid(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message.into(), false)
}
fn read(policy: &OutputPolicy, path: &Path, max: usize) -> Result<Vec<u8>, DevupError> {
    policy.resolve(&path.to_string_lossy())?.read_bounded(max)
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, DevupError> {
    v[key]
        .as_str()
        .ok_or_else(|| invalid(format!("Missing {key}.")))
}
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            if let (Some(a), Some(b)) = (a.as_i64(), b.as_i64()) {
                return a == b;
            }
            let (Some(left), Some(right)) = (a.as_f64(), b.as_f64()) else {
                return false;
            };
            if a.is_i64() || a.is_u64() || b.is_i64() || b.is_u64() {
                left.abs() <= 9_007_199_254_740_992.0
                    && right.abs() <= 9_007_199_254_740_992.0
                    && left == right
            } else {
                (left - right).abs() <= 4.0 * f64::EPSILON * left.abs().max(right.abs()).max(1.0)
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_value(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|other| same_value(v, other)))
        }
        _ => a == b,
    }
}
fn identifier(value: &str) -> Result<String, DevupError> {
    if value.is_empty()
        || !value
            .chars()
            .enumerate()
            .all(|(i, c)| c.is_ascii_alphabetic() || c == '_' || (i > 0 && c.is_ascii_digit()))
    {
        return Err(invalid(
            "DB identifiers must be explicit ASCII identifiers.",
        ));
    }
    Ok(format!("\"{value}\""))
}
fn relative(root: &Path, path: &str) -> Result<std::path::PathBuf, DevupError> {
    if Path::new(path).is_absolute() || path.split(['/', '\\']).any(|p| matches!(p, ".." | "")) {
        return Err(invalid(
            "Evidence paths must remain relative to their declared root.",
        ));
    }
    Ok(root.join(path))
}
fn query(db: &Value) -> Result<(String, Vec<SqlValue>, Vec<Value>), DevupError> {
    let field = identifier(text(db, "field")?)?;
    let table = identifier(text(db, "table")?)?;
    let kind = text(db, "aggregate")?;
    let select = match kind {
        "count" => format!("COUNT({field})"),
        "sum" => format!("COALESCE(SUM({field}),0)"),
        "first" => field.clone(),
        _ => {
            return Err(invalid(
                "Supported typed aggregates are count, sum and first.",
            ));
        }
    };
    let mut params = Vec::new();
    let mut proof_params = Vec::new();
    let mut filters = Vec::new();
    let where_values = db["where"]
        .as_object()
        .into_iter()
        .flatten()
        .collect::<BTreeMap<_, _>>();
    if where_values.len() > 20 {
        return Err(invalid("At most twenty explicit DB filters."));
    }
    for (key, value) in where_values {
        filters.push(format!("{} = ?", identifier(key)?));
        let parameter = match value {
            Value::Null => SqlValue::Null,
            Value::Bool(v) => SqlValue::Integer(i64::from(*v)),
            Value::String(v) => SqlValue::Text(v.clone()),
            Value::Number(v) => {
                if let Some(v) = v.as_i64() {
                    SqlValue::Integer(v)
                } else {
                    SqlValue::Real(v.as_f64().ok_or_else(|| invalid("Invalid DB parameter."))?)
                }
            }
            _ => return Err(invalid("DB filters must be scalar values.")),
        };
        proof_params.push(if let Value::Bool(v) = value {
            json!(i64::from(*v))
        } else {
            value.clone()
        });
        params.push(parameter);
    }
    Ok((
        format!(
            "SELECT {select} AS value FROM {table}{}{}",
            if filters.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", filters.join(" AND "))
            },
            if kind == "first" {
                format!(" ORDER BY {field} LIMIT 1")
            } else {
                String::new()
            }
        ),
        params,
        proof_params,
    ))
}
fn sqlite_rows(bytes: &[u8], db: &Value) -> Result<(String, Vec<Value>, Value), DevupError> {
    if !bytes.starts_with(b"SQLite format 3\0") {
        return Err(invalid("DB proof is not a SQLite snapshot."));
    }
    let mut connection = Connection::open_in_memory().map_err(|e| invalid(e.to_string()))?;
    connection
        .deserialize_read_exact(rusqlite::MAIN_DB, Cursor::new(bytes), bytes.len(), true)
        .map_err(|e| invalid(e.to_string()))?;
    let schema_sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?",
            [text(db, "table")?],
            |row| row.get(0),
        )
        .map_err(|_| {
            invalid("DB proof aggregate must target an existing ordinary table, not a view.")
        })?;
    if schema_sql.to_ascii_lowercase().contains("virtual") {
        return Err(invalid(
            "Virtual SQLite tables are outside this bounded proof adapter.",
        ));
    }
    let (sql, params, proof_params) = query(db)?;
    let mut statement = connection
        .prepare(&sql)
        .map_err(|e| invalid(e.to_string()))?;
    if !statement.readonly() {
        return Err(invalid("DB proof query must be read-only."));
    }
    let mut cursor = statement
        .query(rusqlite::params_from_iter(params))
        .map_err(|e| invalid(e.to_string()))?;
    let mut rows = Vec::new();
    while let Some(row) = cursor.next().map_err(|e| invalid(e.to_string()))? {
        if rows.len() >= 2 {
            return Err(invalid("Aggregate proof returned more than one row."));
        }
        let value = match row.get_ref(0).map_err(|e| invalid(e.to_string()))? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(v) => json!(v),
            ValueRef::Real(v) => json!(v),
            ValueRef::Text(v) => json!(std::str::from_utf8(v).map_err(|e| invalid(e.to_string()))?),
            ValueRef::Blob(_) => {
                return Err(invalid("Binary DB fields are not presentation bindings."));
            }
        };
        rows.push(json!({"value":value}));
    }
    Ok((sql, proof_params, json!(rows)))
}
fn operation<'a>(
    spec: &'a Value,
    id: &str,
    method: &str,
    actual: &str,
) -> Result<(&'a str, &'a Value), DevupError> {
    let actual = actual
        .split('?')
        .next()
        .unwrap_or_default()
        .split('/')
        .collect::<Vec<_>>();
    let mut matches = Vec::new();
    for (path, methods) in spec["paths"].as_object().into_iter().flatten() {
        let parts = path.split('/').collect::<Vec<_>>();
        let op = &methods[method.to_ascii_lowercase()];
        if op["operationId"] == id
            && parts.len() == actual.len()
            && parts
                .iter()
                .zip(&actual)
                .all(|(a, b)| (a.starts_with('{') && a.ends_with('}')) || a == b)
        {
            matches.push((path.as_str(), op));
        }
    }
    if matches.len() != 1 {
        return Err(invalid(
            "Existing OpenAPI operation/path/method anchor is unresolved or ambiguous.",
        ));
    }
    Ok(matches[0])
}
fn field_exists(spec: &Value, schema: &Value, pointer: &str) -> bool {
    fn deref<'a>(spec: &'a Value, mut value: &'a Value) -> Option<&'a Value> {
        for _ in 0..20 {
            if let Some(reference) = value["$ref"].as_str() {
                value = spec.pointer(reference.strip_prefix('#')?)?;
            } else {
                return Some(value);
            }
        }
        None
    }
    if !pointer.starts_with('/') {
        return false;
    }
    let Some(mut current) = deref(spec, schema) else {
        return false;
    };
    for key in pointer.split('/').skip(1) {
        let key = key.replace("~1", "/").replace("~0", "~");
        current = if key.parse::<usize>().is_ok() {
            &current["items"]
        } else {
            &current["properties"][key]
        };
        let Some(next) = deref(spec, current) else {
            return false;
        };
        current = next;
        if current.is_null() {
            return false;
        }
    }
    true
}
fn anchors(
    policy: &OutputPolicy,
    config: &Value,
    binding: &Value,
    receipt: &Value,
) -> Result<Value, DevupError> {
    let root = Path::new(text(config, "projectRoot")?);
    let openapi = read(
        policy,
        &relative(root, text(config, "openapiPath")?)?,
        2 * 1024 * 1024,
    )?;
    let spec: Value = serde_json::from_slice(&openapi).map_err(|e| invalid(e.to_string()))?;
    let db = &binding["db"];
    if receipt["operationId"] != binding["operationId"]
        || receipt["table"] != db["table"]
        || receipt["field"] != db["field"]
        || receipt["aggregate"] != db["aggregate"]
    {
        return Err(invalid(
            "Receipt binding metadata differs from the planned operation/table/field/aggregate.",
        ));
    }
    let model = read(
        policy,
        &relative(root, text(db, "modelPath")?)?,
        2 * 1024 * 1024,
    )?;
    let schema: Value = serde_json::from_slice(&model).map_err(|e| invalid(e.to_string()))?;
    let columns = schema["columns"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|column| column["name"].as_str())
        .collect::<BTreeSet<_>>();
    if schema["name"] != db["table"]
        || !columns.contains(text(db, "field")?)
        || db["where"]
            .as_object()
            .into_iter()
            .flatten()
            .any(|(field, _)| !columns.contains(field.as_str()))
    {
        return Err(invalid(
            "Existing model table/field anchors do not match the binding.",
        ));
    }
    let (path, op) = operation(
        &spec,
        text(binding, "operationId")?,
        "GET",
        text(&binding["api"], "path")?,
    )?;
    if !field_exists(
        &spec,
        &op["responses"]["200"]["content"]["application/json"]["schema"],
        text(&binding["api"], "pointer")?,
    ) {
        return Err(invalid(
            "Bound API field is absent from the actual response schema.",
        ));
    }
    for phase in binding["phases"].as_array().into_iter().flatten() {
        operation(
            &spec,
            text(&phase["mutation"], "operationId")?,
            text(&phase["mutation"], "method")?,
            text(&phase["mutation"], "path")?,
        )?;
    }
    let expected = json!({"openapiPath":config["openapiPath"],"openapiSha256":digest(&openapi),"dbModelPath":db["modelPath"],"dbSchemaSha256":digest(&model),"matchedPath":path,"matchedMethod":"get"});
    if receipt["sourceAnchor"] != expected {
        return Err(invalid(
            "Source anchor hashes differ from actual OpenAPI/model files.",
        ));
    }
    Ok(expected)
}

pub(super) fn verify_data_evidence(
    policy: &OutputPolicy,
    path: &str,
    expected_source_digest: &str,
    plan_path: &str,
) -> Result<Value, DevupError> {
    let receipt_path = Path::new(path);
    let bytes = read(policy, receipt_path, 8 * 1024 * 1024)?;
    let receipt: Value = serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    let plan_bytes = read(policy, Path::new(plan_path), 1024 * 1024)?;
    let plan: Value = serde_json::from_slice(&plan_bytes).map_err(|e| invalid(e.to_string()))?;
    let typed_plan: super::runtime_audit::PlanInput = serde_json::from_value(plan["plan"].clone())
        .map_err(|e| invalid(format!("Data proof plan schema: {e}")))?;
    super::runtime_audit::validate_plan(&typed_plan)?;
    if plan["planId"] != super::runtime_audit::plan_id(&typed_plan)? {
        return Err(invalid(
            "Data proof plan ID does not match its normalized contents.",
        ));
    }
    if receipt["kind"] != "devup-data-binding-receipt"
        || receipt["schemaVersion"] != 1
        || receipt["sourceDigest"] != expected_source_digest
        || receipt["planId"] != plan["planId"]
        || receipt["planSha256"] != digest(&plan_bytes)
        || plan["kind"] != "devup-runtime-audit-plan"
        || plan["plan"]["sourceDigest"] != expected_source_digest
    {
        return Err(invalid(
            "Data receipt must bind the exact runtime plan and current source digest.",
        ));
    }
    let mut planned = BTreeMap::new();
    for screen in plan["plan"]["screens"].as_array().into_iter().flatten() {
        for check in screen["checks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["kind"] == "data-binding")
        {
            let config = &check["dataBinding"];
            for binding in config["bindings"].as_array().into_iter().flatten() {
                let id = text(binding, "id")?.to_owned();
                if planned.insert(id, (config, binding)).is_some() {
                    return Err(invalid("Data binding IDs must be unique in the plan."));
                }
            }
        }
    }
    if planned.is_empty() || planned.len() > 20 {
        return Err(invalid(
            "The bound plan needs 1..20 explicit data bindings.",
        ));
    }
    let supplied = receipt["bindings"]
        .as_array()
        .ok_or_else(|| invalid("Receipt bindings missing."))?;
    if supplied.len() != planned.len() {
        return Err(invalid("Data receipt omitted or added planned bindings."));
    }
    let mut statuses = Vec::new();
    let mut covered = Vec::new();
    let mut source_roots = BTreeMap::new();
    for (id, (config, binding)) in planned {
        let matching = supplied
            .iter()
            .filter(|row| row["id"] == id)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(invalid(
                "Receipt binding IDs must match the plan exactly once.",
            ));
        }
        let observed = matching[0];
        let mut reasons = Vec::new();
        let source_root = text(config, "projectRoot")?;
        let source_tree_matches =
            *source_roots
                .entry(source_root.to_owned())
                .or_insert_with(|| {
                    super::completion::source_digest(Path::new(source_root), policy)
                        .is_ok_and(|(digest, _)| digest == expected_source_digest)
                });
        let source_matched = source_tree_matches
            && match anchors(policy, config, binding, observed) {
                Ok(_) => true,
                Err(e) => {
                    reasons.push(e.message);
                    false
                }
            };
        if !source_tree_matches {
            reasons.push("Data source tree changed since the audit.".into());
        }
        let planned_phases = binding["phases"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let actual_phases = observed["phases"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let complete = planned_phases.len() == 4
            && actual_phases.len() == 4
            && PHASES.iter().enumerate().all(|(i, name)| {
                planned_phases[i]["phase"] == *name && actual_phases[i]["phase"] == *name
            });
        let mut phase_results = Vec::new();
        let mut real_values = Vec::new();
        let mut real_rows = Vec::new();
        if complete {
            for (planned_phase, phase) in planned_phases.iter().zip(actual_phases) {
                let result = verify_phase(
                    policy,
                    receipt_path.parent().unwrap_or(Path::new(".")),
                    binding,
                    planned_phase,
                    phase,
                );
                match result {
                    Ok(value) => {
                        real_values.push(value["dbValue"].clone());
                        real_rows.push(value["dbRows"].clone());
                        phase_results.push(value);
                    }
                    Err(e) => {
                        reasons.push(e.message);
                        phase_results.push(json!({"phase":phase["phase"],"verified":false,"apiVerified":false,"dbVerified":false,"rendered":false}));
                    }
                }
            }
        } else {
            reasons
                .push("All four ordered baseline/mutated/empty/error phases are required.".into());
        }
        let db_verified = complete && phase_results.iter().all(|p| p["dbVerified"] == true);
        let api_verified = complete && phase_results.iter().all(|p| p["apiVerified"] == true);
        let rendered = complete && phase_results.iter().all(|p| p["rendered"] == true);
        let changed = real_rows.len() == 4 && real_rows[0] != real_rows[1];
        let empty = real_values.len() == 4
            && if binding["db"]["aggregate"] == "first" {
                real_rows[2] == json!([])
            } else {
                real_values[2] == 0
            };
        let verified =
            source_matched && db_verified && api_verified && rendered && changed && empty;
        if verified {
            covered.push(id.clone());
        }
        statuses.push(json!({"id":id,"sourceMatched":source_matched,"apiVerified":api_verified,"dbVerified":db_verified,"rendered":rendered,"changed":changed,"empty":empty,"phases":phase_results,"verified":verified,"reasons":reasons}));
    }
    let source_matched = statuses
        .iter()
        .all(|binding| binding["sourceMatched"] == true);
    let api_verified = statuses
        .iter()
        .all(|binding| binding["apiVerified"] == true);
    let db_verified = statuses.iter().all(|binding| binding["dbVerified"] == true);
    Ok(
        json!({"verified":statuses.iter().all(|b|b["verified"]==true),"sourceMatched":source_matched,"apiVerified":api_verified,"dbVerified":db_verified,"bindings":statuses,"coveredBindingIds":covered,"receiptHash":digest(&bytes),"sourceDigest":expected_source_digest,"method":"Actual OpenAPI/model anchors rehashed; typed SELECT aggregates reexecuted on hash-bound SQLite bytes deserialized read-only in memory. API and browser observations remain executor-observed.","databaseReexecuted":db_verified}),
    )
}

fn verify_phase(
    policy: &OutputPolicy,
    directory: &Path,
    binding: &Value,
    planned: &Value,
    phase: &Value,
) -> Result<Value, DevupError> {
    let snapshot = &phase["db"]["snapshot"];
    let bytes = read(
        policy,
        &relative(directory, text(snapshot, "path")?)?,
        16 * 1024 * 1024,
    )?;
    if snapshot["sha256"] != digest(&bytes) {
        return Err(invalid("DB snapshot hash mismatch."));
    }
    let (sql, params, rows) = sqlite_rows(&bytes, &binding["db"])?;
    if phase["db"]["adapter"] != "node-sqlite-readonly"
        || phase["db"]["query"] != sql
        || phase["db"]["params"] != json!(params)
        || !same_value(&phase["db"]["rows"], &rows)
    {
        return Err(invalid(
            "Declared DB observation differs from the actual snapshot SELECT.",
        ));
    }
    let value = rows[0]["value"].clone();
    let error = planned["phase"] == "error";
    let status = phase["api"]["status"].as_u64().unwrap_or(0);
    let pointer = text(&binding["api"], "pointer")?;
    let api_verified = phase["api"]["pointer"] == pointer
        && if error {
            (400..=599).contains(&status)
        } else {
            status == 200
                && phase["api"]["body"]
                    .pointer(pointer)
                    .is_some_and(|api_value| same_value(api_value, &value))
        };
    let mutation = phase["mutation"]["operationId"] == planned["mutation"]["operationId"]
        && phase["mutation"]["status"]
            .as_u64()
            .is_some_and(|s| (200..300).contains(&s))
        && phase["mutation"]["isolationNonce"]
            .as_str()
            .is_some_and(|n| n.len() >= 24 && n.len() <= 128);
    let views = phase["views"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut surfaces = BTreeSet::new();
    let mut rendered = true;
    for view in binding["views"].as_array().into_iter().flatten() {
        let surface = text(view, "surface")?;
        surfaces.insert(surface);
        let expected = if error {
            text(view, "errorText")?.to_owned()
        } else if planned["phase"] == "empty" {
            text(view, "emptyText")?.to_owned()
        } else {
            text(view, "format")?.replace(
                "{value}",
                &value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned),
            )
        };
        let matches = views
            .iter()
            .filter(|observed| {
                observed["surface"] == surface
                    && observed["route"] == view["route"]
                    && observed["selector"] == view["selector"]
            })
            .collect::<Vec<_>>();
        rendered &= matches.len() == 1
            && matches[0]["text"].as_str().is_some_and(|text| {
                text.split_whitespace().collect::<Vec<_>>().join(" ")
                    == expected.split_whitespace().collect::<Vec<_>>().join(" ")
            });
    }
    rendered &= ["landing", "list", "detail"]
        .iter()
        .all(|name| surfaces.contains(name));
    Ok(
        json!({"phase":planned["phase"],"dbVerified":true,"dbRows":rows,"dbValue":value,"apiVerified":api_verified&&mutation,"rendered":rendered,"verified":api_verified&&mutation&&rendered}),
    )
}

#[cfg(test)]
#[path = "data_verification/tests.rs"]
mod tests;
