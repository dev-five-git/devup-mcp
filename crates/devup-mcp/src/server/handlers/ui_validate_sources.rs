use devup_mcp_devup_ui::ui_validate::SourceFile;
use std::{collections::BTreeSet, io::Read};

pub(super) fn read_paths(root: &str, paths: &[String]) -> Result<Vec<SourceFile>, String> {
    if paths.len() > 512 { return Err("paths exceeds 512 entries".into()); }
    let root = std::fs::canonicalize(root).map_err(|error| format!("projectRoot: {error}"))?;
    let mut pending = paths.iter().map(|path| root.join(path)).collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut files = Vec::new();
    let mut total: usize = 0;
    while let Some(path) = pending.pop() {
        let path = std::fs::canonicalize(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if !path.starts_with(&root) {
            return Err(format!("paths must stay inside projectRoot: {}", path.display()));
        }
        if !visited.insert(path.clone()) {
            continue;
        }
        if path.is_dir() {
            for entry in std::fs::read_dir(&path).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') || ["node_modules", "dist", "build", "target", "df"].contains(&name.as_ref()) {
                    continue;
                }
                pending.push(entry.path());
            }
        } else if matches!(path.extension().and_then(|value| value.to_str()), Some("tsx" | "ts" | "jsx" | "js")) {
            if files.len() == 512 {
                return Err("paths exceeds 512 source files".into());
            }
            let file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
            let mut bytes = Vec::new();
            file.take(u64::try_from(8 * 1024 * 1024 - total + 1).map_err(|error| error.to_string())?)
                .read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            total += bytes.len();
            if total > 8 * 1024 * 1024 {
                return Err("paths exceeds 8 MiB total read bytes".into());
            }
            let relative = path.strip_prefix(&root).map_err(|error| error.to_string())?;
            files.push(SourceFile {
                path: relative.to_string_lossy().replace('\\', "/"),
                content: String::from_utf8(bytes).map_err(|error| format!("{}: {error}", path.display()))?,
            });
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files.is_empty() {
        return Err("paths contains no .tsx, .ts, .jsx or .js sources".into());
    }
    Ok(files)
}

pub(super) fn summarize(result: &mut serde_json::Value, sources: &[( &str, &str)]) {
    use serde_json::{Value, json};
    let mut rows: Vec<Value> = Vec::new();
    let mut groups = std::collections::BTreeMap::new();
    let mut value_groups: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for finding in result["violations"].as_array().into_iter().flatten() {
        let name = finding.get("sourceName").or_else(|| finding.get("path")).or_else(|| finding["context"].get("path"))
            .or_else(|| finding["context"].get("sourceName"));
        if let Some(value) = finding.get("value").and_then(Value::as_str)
            && matches!(finding["rule"].as_str(), Some("hardcoded-length" | "hardcoded-color")) {
            let tokens = finding["tokenMatches"].as_array().map(|matches| matches.iter().map(|m| m["token"].clone()).collect::<Vec<_>>())
                .or_else(|| finding["otherPurposeTokens"].as_array().cloned()).unwrap_or_default();
            let key = format!("{}|{}|{value}|{}", finding["rule"], finding["severity"], json!(tokens));
            let line = finding.get("line").cloned().unwrap_or(Value::Null);
            let prop = finding.get("prop").cloned().unwrap_or(Value::Null);
            let file = name.cloned().unwrap_or(Value::Null);
            if let Some(&index) = value_groups.get(&key) {
                let row: &mut Value = &mut rows[index];
                row["count"] = json!(row["count"].as_u64().unwrap_or(1) + 1);
                if let Some(props) = row["props"].as_array_mut() && !props.contains(&prop) { props.push(prop); }
                let files = row["files"].as_array_mut().expect("files array");
                match files.iter_mut().find(|entry| entry["sourceName"] == file) {
                    Some(entry) => entry["lines"].as_array_mut().expect("lines array").push(line),
                    None => files.push(json!({"sourceName":file,"lines":[line]})),
                }
                continue;
            }
            let kind = if finding["rule"] == "hardcoded-color" { "color" } else { "length" };
            let message = match (finding["severity"].as_str(), tokens.is_empty()) {
                (Some("warning"), false) => format!("hardcoded {kind} {value}; devup.json defines it as {}", tokens.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")),
                (_, false) => format!("hardcoded {kind} {value}; only tokens named for another purpose have this value"),
                _ => format!("hardcoded {kind} {value}; no matching token"),
            };
            let mut row = json!({"rule":finding["rule"],"severity":finding["severity"],"value":value,"message":message,
                "count":1,"props":[prop],"files":[{"sourceName":file,"lines":[line]}]});
            if !tokens.is_empty() { row["tokens"] = json!(tokens); }
            if let Some(suggestion) = finding.get("suggestion").filter(|value| !value.is_null()) { row["suggestion"] = suggestion.clone(); }
            value_groups.insert(key, rows.len());
            rows.push(row);
            continue;
        }
        let key = (name.cloned().unwrap_or(Value::Null).to_string(), finding["rule"].to_string(), finding["message"].to_string());
        if let Some(&index) = groups.get(&key) {
            let row: &mut Value = &mut rows[index];
            row["count"] = json!(row["count"].as_u64().unwrap_or(1) + 1);
            continue;
        }
        let mut row = json!({"rule":finding["rule"],"severity":finding["severity"],"message":finding["message"]});
        if let Some(name) = name { row["sourceName"] = name.clone(); }
        let source = sources.iter().find(|(path, _)| name.and_then(Value::as_str).is_none_or(|name| name == *path));
        if let Some(line) = finding.get("line").or_else(|| finding["context"].get("line")) {
            row["line"] = line.clone();
        } else if let Some((_, source)) = source
            && let Some(start) = finding["byteRange"][0].as_u64().and_then(|value| usize::try_from(value).ok())
            && let Some(prefix) = source.get(..start) {
                row["line"] = json!(prefix.bytes().filter(|byte| *byte == b'\n').count() + 1);
        }
        if let Some(suggestion) = finding.get("suggestion").filter(|value| !value.is_null()) { row["suggestion"] = suggestion.clone(); }
        groups.insert(key, rows.len());
        rows.push(row);
    }
    let total = rows.len();
    rows.truncate(100);
    let risk_count = result["layoutRisks"].as_array().map_or(0, Vec::len);
    if let Some(object) = result.as_object_mut() {
        object.retain(|key, value| matches!(key.as_str(), "ok" | "okReason" | "strict" | "violationCounts" | "themeAvailable" | "tokens" | "nextAction" | "recoveryState" | "recoveryReason" | "nextActionReason" | "skillGap" | "themeNotes" | "pathsRead") || (key == "themeGuardrail" && !value.is_null()));
    }
    result["violations"] = json!(rows);
    result["layoutRiskCount"] = json!(risk_count);
    result["detail"] = json!("summary");
    result["full"] = json!({"how":"repeat with detail: \"full\""});
    if total > 100 { result["violationsTruncated"] = json!({"shown":100,"total":total}); }
}
