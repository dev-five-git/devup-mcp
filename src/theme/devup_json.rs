use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::figma::{DevupError, Diagnostic, ErrorCode, UpstreamResult};

use super::tokens::{normalize_token, variable_token};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeScope {
    Node,
    Page,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Completeness {
    FullLocalPlusUsedRemote,
    UsedTokens,
    ResolvedValuesOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeCounts {
    pub collections: usize,
    pub variables: usize,
    pub styles: usize,
    pub modes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableMode {
    pub mode_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableCollection {
    pub id: String,
    pub name: String,
    pub default_mode_id: String,
    pub modes: Vec<VariableMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableDefinition {
    pub id: String,
    pub name: String,
    pub resolved_type: String,
    pub variable_collection_id: String,
    #[serde(default)]
    pub code_syntax: BTreeMap<String, String>,
    #[serde(default)]
    pub values_by_mode: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableStyle {
    pub id: String,
    pub name: String,
    pub style_type: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableSnapshot {
    #[serde(default)]
    pub collections: Vec<VariableCollection>,
    #[serde(default)]
    pub variables: Vec<VariableDefinition>,
    #[serde(default)]
    pub styles: Vec<VariableStyle>,
    #[serde(default)]
    pub used_remote_variables: Vec<VariableDefinition>,
    #[serde(default)]
    pub local_complete: bool,
    #[serde(default)]
    pub used_remote_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeOutput {
    pub json: String,
    pub counts: ThemeCounts,
    pub completeness: Completeness,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn generate_devup_json(
    snapshot: &VariableSnapshot,
    _scope: ThemeScope,
) -> Result<ThemeOutput, DevupError> {
    let mut diagnostics = Vec::new();
    let collections = snapshot
        .collections
        .iter()
        .map(|collection| (collection.id.as_str(), collection))
        .collect::<HashMap<_, _>>();
    let variables = snapshot
        .variables
        .iter()
        .chain(snapshot.used_remote_variables.iter())
        .map(|variable| (variable.id.as_str(), variable))
        .collect::<HashMap<_, _>>();
    let mut colors: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    let mut lengths: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();

    for variable in variables.values() {
        let Some(collection) = collections.get(variable.variable_collection_id.as_str()) else {
            diagnostics.push(Diagnostic {
                code: "DEVUP_THEME_COLLECTION_MISSING".to_owned(),
                message: format!("변수 '{}'의 collection을 찾지 못했습니다.", variable.name),
                node_id: None,
            });
            continue;
        };
        let token = variable_token(
            &variable.name,
            variable.code_syntax.get("WEB").map(String::as_str),
        );
        for mode in &collection.modes {
            let mut visiting = HashSet::new();
            let Some(value) = resolve_value(variable, &mode.mode_id, &variables, &mut visiting)
            else {
                diagnostics.push(Diagnostic {
                    code: "DEVUP_THEME_ALIAS_CYCLE".to_owned(),
                    message: format!(
                        "변수 '{}'의 alias를 안전하게 해석하지 못했습니다.",
                        variable.name
                    ),
                    node_id: None,
                });
                continue;
            };
            let mode_name = normalize_token(&mode.name);
            match variable.resolved_type.as_str() {
                "COLOR" => {
                    if let Some(color) = color_value(value) {
                        colors
                            .entry(mode_name)
                            .or_default()
                            .insert(token.clone(), Value::String(color));
                    }
                }
                "FLOAT" => {
                    if let Some(number) = value.as_f64() {
                        lengths
                            .entry(mode_name)
                            .or_default()
                            .insert(token.clone(), Value::String(format_px(number)));
                    }
                }
                _ => {}
            }
        }
    }

    let mut typography = BTreeMap::new();
    let mut shadows = BTreeMap::new();
    for style in &snapshot.styles {
        let token = normalize_token(&style.name);
        match style.style_type.as_str() {
            "TEXT" => {
                typography.insert(token, style.value.clone());
            }
            "EFFECT" => {
                shadows.insert(token, style.value.clone());
            }
            _ => {}
        }
    }

    let mut theme = Map::new();
    theme.insert("colors".to_owned(), json!(colors));
    theme.insert("typography".to_owned(), json!(typography));
    theme.insert("length".to_owned(), json!(lengths));
    theme.insert("shadow".to_owned(), json!({ "default": shadows }));
    let mut root = Map::new();
    root.insert("theme".to_owned(), Value::Object(theme));
    let mut output = serde_json::to_string_pretty(&Value::Object(root)).map_err(|_| {
        DevupError::new(
            ErrorCode::DevupThemeConflict,
            "devup.json을 직렬화하지 못했습니다.",
            false,
        )
    })?;
    output.push('\n');

    let completeness = if snapshot.local_complete && snapshot.used_remote_complete {
        Completeness::FullLocalPlusUsedRemote
    } else if !snapshot.variables.is_empty() || !snapshot.used_remote_variables.is_empty() {
        Completeness::UsedTokens
    } else {
        Completeness::ResolvedValuesOnly
    };
    Ok(ThemeOutput {
        json: output,
        counts: ThemeCounts {
            collections: snapshot.collections.len(),
            variables: snapshot.variables.len() + snapshot.used_remote_variables.len(),
            styles: snapshot.styles.len(),
            modes: snapshot
                .collections
                .iter()
                .map(|collection| collection.modes.len())
                .sum(),
        },
        completeness,
        diagnostics,
    })
}

pub fn variable_snapshot_from_result(
    result: &UpstreamResult,
) -> Result<VariableSnapshot, DevupError> {
    find_variable_snapshot(&result.raw).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupThemeConflict,
            "Figma MCP 응답에서 변수 snapshot을 찾지 못했습니다.",
            false,
        )
    })
}

fn find_variable_snapshot(value: &Value) -> Option<VariableSnapshot> {
    if let Ok(snapshot) = serde_json::from_value::<VariableSnapshot>(value.clone())
        && (!snapshot.collections.is_empty()
            || !snapshot.variables.is_empty()
            || !snapshot.styles.is_empty())
    {
        return Some(snapshot);
    }
    match value {
        Value::Object(object) => {
            if let Some(Value::String(text)) = object.get("text")
                && let Ok(value) = serde_json::from_str::<Value>(text)
                && let Some(snapshot) = find_variable_snapshot(&value)
            {
                return Some(snapshot);
            }
            object.values().find_map(find_variable_snapshot)
        }
        Value::Array(values) => values.iter().find_map(find_variable_snapshot),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| find_variable_snapshot(&value)),
        _ => None,
    }
}

fn resolve_value<'a>(
    variable: &'a VariableDefinition,
    mode: &str,
    variables: &'a HashMap<&str, &VariableDefinition>,
    visiting: &mut HashSet<String>,
) -> Option<&'a Value> {
    if !visiting.insert(variable.id.clone()) {
        return None;
    }
    let value = variable.values_by_mode.get(mode)?;
    if value.get("type").and_then(Value::as_str) == Some("VARIABLE_ALIAS") {
        let alias = value.get("id").and_then(Value::as_str)?;
        let target = variables.get(alias)?;
        resolve_value(target, mode, variables, visiting)
    } else {
        Some(value)
    }
}

fn color_value(value: &Value) -> Option<String> {
    let channel = |name: &str| -> Option<u8> {
        Some((value.get(name)?.as_f64()?.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    let alpha = value.get("a").and_then(Value::as_f64).unwrap_or(1.0);
    if alpha < 1.0 {
        Some(format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            channel("r")?,
            channel("g")?,
            channel("b")?,
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
        ))
    } else {
        Some(format!(
            "#{:02x}{:02x}{:02x}",
            channel("r")?,
            channel("g")?,
            channel("b")?
        ))
    }
}

fn format_px(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}px")
    } else {
        format!("{value}px")
    }
}
