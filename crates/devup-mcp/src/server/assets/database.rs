//! PostgreSQL scalar-text URL adapter: guarded SQL artifacts, never a live connection.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    key: Value,
    url: String,
}

fn identifier(value: &str) -> Result<String, DevupError> {
    if value.is_empty()
        || !value
            .chars()
            .enumerate()
            .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
    {
        return Err(invalid(
            "PostgreSQL asset adapter identifiers must be simple schema/table/column identifiers",
        ));
    }
    Ok(format!("\"{value}\""))
}
fn literal(value: &str) -> Result<String, DevupError> {
    if value.contains('\0') || value.contains("$devup_asset$") {
        return Err(invalid(
            "SQL migration values contain an unsupported NUL or block delimiter",
        ));
    }
    Ok(format!("'{}'", value.replace('\'', "''")))
}
fn rows(bytes: &[u8]) -> Result<Vec<(String, String)>, DevupError> {
    let rows: Vec<Row> = serde_json::from_slice(bytes).map_err(|e| {
        invalid(format!(
            "Database reference snapshot must be an array of {{key,url}} rows: {e}"
        ))
    })?;
    if rows.len() > MAX_FILES {
        return Err(invalid(
            "Database URL snapshots exceed the row bound; split the inventory",
        ));
    }
    let mut keys = BTreeSet::new();
    rows.into_iter()
        .map(|row| {
            let key = match row.key {
                Value::String(key) => key,
                Value::Number(key) => key.to_string(),
                _ => {
                    return Err(invalid(
                        "Database reference keys must be strings or numbers",
                    ));
                }
            };
            if key.len() > 512 || row.url.len() > 4096 {
                return Err(invalid(
                    "Database reference key or URL exceeds the supported bound",
                ));
            }
            if !keys.insert(key.clone()) {
                return Err(invalid(
                    "Database reference snapshot contains duplicate row keys",
                ));
            }
            Ok((key, row.url))
        })
        .collect()
}
pub(super) fn inventory(
    descriptor: &DatabaseReferences,
    bytes: &[u8],
) -> Result<Value, DevupError> {
    for name in [
        &descriptor.schema,
        &descriptor.table,
        &descriptor.key_column,
        &descriptor.url_column,
    ] {
        identifier(name)?;
    }
    let rows = rows(bytes)?;
    Ok(
        json!({"snapshotPath":descriptor.snapshot_path,"sha256":hash(bytes),"adapter":"postgresql-scalar-text-keyed-v1","table":format!("{}.{}",descriptor.schema,descriptor.table),"rowCount":rows.len(),"references":rows.iter().map(|(key,url)|json!({"key":key,"url":url})).collect::<Vec<_>>(),"snapshotFreshnessVerified":false,"liveDatabaseVerified":false,"unsupported":["JSON/array/rich-text fields","computed or dynamically generated URLs","multi-database atomic commit"]}),
    )
}

fn guarded(
    table: &str,
    key_column: &str,
    url_column: &str,
    key: &str,
    from: &str,
    to: &str,
) -> Result<String, DevupError> {
    let key = literal(key)?;
    let from = literal(from)?;
    let to = literal(to)?;
    Ok(format!(
        "DO $devup_asset$\nDECLARE changed integer;\nBEGIN\n  IF (SELECT count(*) FROM {table} WHERE {key_column}::text = {key}) <> 1 THEN\n    RAISE EXCEPTION 'Asset URL migration key is missing or not unique';\n  END IF;\n  IF EXISTS (SELECT 1 FROM {table} WHERE {key_column}::text = {key} AND {url_column} = {to}) THEN\n    RETURN;\n  END IF;\n  UPDATE {table} SET {url_column} = {to}\n    WHERE {key_column}::text = {key} AND {url_column} = {from};\n  GET DIAGNOSTICS changed = ROW_COUNT;\n  IF changed <> 1 THEN\n    RAISE EXCEPTION 'Asset URL changed since inventory; migration refused';\n  END IF;\nEND\n$devup_asset$;\n"
    ))
}

type Scripts = (BTreeMap<String, Vec<u8>>, Vec<Value>);
pub(super) fn scripts(
    workspace: &Workspace<'_>,
    descriptors: &[DatabaseReferences],
    urls: &BTreeMap<String, String>,
) -> Result<Scripts, DevupError> {
    let mut forward = String::from(
        "-- Generated from declared snapshots. Review and run against PostgreSQL separately.\n-- No live database state was verified by devup_assets.\nBEGIN;\nSET LOCAL standard_conforming_strings = on;\n",
    );
    let mut reverse = forward.clone();
    let mut verify = String::from(
        "-- Read-only expected URL checks; each row must return exactly one match.\nBEGIN READ ONLY;\nSET LOCAL standard_conforming_strings = on;\n",
    );
    let mut changes = Vec::new();
    let mut seen = BTreeSet::new();
    for descriptor in descriptors {
        let table = format!(
            "{}.{}",
            identifier(&descriptor.schema)?,
            identifier(&descriptor.table)?
        );
        let key_column = identifier(&descriptor.key_column)?;
        let url_column = identifier(&descriptor.url_column)?;
        let bytes = workspace.read(&descriptor.snapshot_path)?;
        for (key, from) in rows(&bytes)? {
            let Some(to) = urls.get(&from) else {
                continue;
            };
            if !seen.insert((
                table.clone(),
                key_column.clone(),
                url_column.clone(),
                key.clone(),
            )) {
                return Err(invalid("Overlapping database URL adapter rows"));
            }
            forward.push_str(&guarded(&table, &key_column, &url_column, &key, &from, to)?);
            reverse.push_str(&guarded(&table, &key_column, &url_column, &key, to, &from)?);
            verify.push_str(&format!("SELECT {} AS row_key, count(*) = 1 AS expected_url_present FROM {table} WHERE {key_column}::text = {} AND {url_column} = {};\n",literal(&key)?,literal(&key)?,literal(to)?));
            changes.push(json!({"snapshotPath":descriptor.snapshot_path,"schema":descriptor.schema,"table":descriptor.table,"keyColumn":descriptor.key_column,"urlColumn":descriptor.url_column,"key":key,"from":from,"to":to,"liveVerified":false}));
        }
    }
    if changes.is_empty() {
        return Ok((BTreeMap::new(), changes));
    }
    forward.push_str("COMMIT;\n");
    reverse.push_str("COMMIT;\n");
    verify.push_str("COMMIT;\n");
    Ok((
        BTreeMap::from([
            ("migration.sql".into(), forward.into_bytes()),
            ("rollback.sql".into(), reverse.into_bytes()),
            ("verify.sql".into(), verify.into_bytes()),
        ]),
        changes,
    ))
}
