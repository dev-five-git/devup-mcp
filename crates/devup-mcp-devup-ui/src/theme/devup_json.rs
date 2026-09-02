use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use devup_mcp_figma::{DevupError, Diagnostic, DiagnosticSeverity, ErrorCode, UpstreamResult};

use super::tokens::{normalize_token, variable_token};
use crate::provenance::{ProvenanceEntry, SourceMap, json_pointer_segment};

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
    pub used_variable_ids: Vec<String>,
    #[serde(default)]
    pub used_style_ids: Vec<String>,
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
    pub conflicts: Vec<ThemeConflict>,
    pub unresolved_variables: Vec<ThemeUnresolvedVariable>,
    pub source_map: SourceMap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeUnresolvedVariable {
    pub variable_id: String,
    pub mode_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeVariableSource {
    Local,
    UsedRemote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeConflictCandidate {
    pub variable_id: String,
    pub collection_id: String,
    pub collection_name: String,
    pub raw_name: String,
    pub mode_id: String,
    pub source: ThemeVariableSource,
    pub value_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeConflict {
    pub token: String,
    pub mode: String,
    pub kind: String,
    pub winner_variable_id: String,
    pub candidates: Vec<ThemeConflictCandidate>,
}

#[derive(Debug, Clone, Copy)]
struct VariableCandidate<'a> {
    variable: &'a VariableDefinition,
    source: ThemeVariableSource,
}

#[derive(Debug, Clone)]
struct ProjectedVariable {
    variable_id: String,
    collection_id: String,
    collection_name: String,
    raw_name: String,
    mode_id: String,
    source: ThemeVariableSource,
    value: Value,
    resolution: String,
}

pub fn generate_devup_json(
    snapshot: &VariableSnapshot,
    scope: ThemeScope,
) -> Result<ThemeOutput, DevupError> {
    let mut diagnostics = Vec::new();
    let mut conflicts = Vec::new();
    let mut unresolved_variables = Vec::new();
    let mut source_entries = Vec::new();
    let collections = snapshot
        .collections
        .iter()
        .map(|collection| (collection.id.as_str(), collection))
        .collect::<BTreeMap<_, _>>();
    let mut variable_candidates = snapshot
        .variables
        .iter()
        .map(|variable| VariableCandidate {
            variable,
            source: ThemeVariableSource::Local,
        })
        .chain(
            snapshot
                .used_remote_variables
                .iter()
                .map(|variable| VariableCandidate {
                    variable,
                    source: ThemeVariableSource::UsedRemote,
                }),
        )
        .collect::<Vec<_>>();
    variable_candidates.sort_by(|left, right| {
        let left_collection = collections
            .get(left.variable.variable_collection_id.as_str())
            .map(|collection| collection.name.as_str())
            .unwrap_or_default();
        let right_collection = collections
            .get(right.variable.variable_collection_id.as_str())
            .map(|collection| collection.name.as_str())
            .unwrap_or_default();
        let left_explicit = has_web_syntax(left.variable);
        let right_explicit = has_web_syntax(right.variable);
        right_explicit
            .cmp(&left_explicit)
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left_collection.cmp(right_collection))
            .then_with(|| left.variable.name.cmp(&right.variable.name))
            .then_with(|| left.variable.id.cmp(&right.variable.id))
    });
    let mut variables = BTreeMap::new();
    for candidate in &variable_candidates {
        variables
            .entry(candidate.variable.id.as_str())
            .or_insert(candidate.variable);
    }
    let selected_variable_ids = selected_variable_ids(snapshot, scope, &variables);
    if let Some(selected) = &selected_variable_ids {
        variable_candidates.retain(|candidate| selected.contains(&candidate.variable.id));
    }
    let variable_count = variable_candidates.len();
    let mut projected = BTreeMap::<(String, String, String), Vec<ProjectedVariable>>::new();
    let mut colors: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    let mut lengths: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();

    for candidate in variable_candidates {
        let variable = candidate.variable;
        let Some(collection) = collections.get(variable.variable_collection_id.as_str()) else {
            unresolved_variables.push(ThemeUnresolvedVariable {
                variable_id: variable.id.clone(),
                mode_id: None,
                reason: "collection-missing".to_owned(),
            });
            diagnostics.push(Diagnostic {
                code: "DEVUP_THEME_COLLECTION_MISSING".to_owned(),
                message: format!("변수 '{}'의 collection을 찾지 못했습니다.", variable.name),
                node_id: None,
                severity: Some(DiagnosticSeverity::Warning),
                resource_kind: Some("variable".to_owned()),
                resource_id: Some(variable.id.clone()),
                recoverable: Some(true),
                ..Diagnostic::default()
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
                unresolved_variables.push(ThemeUnresolvedVariable {
                    variable_id: variable.id.clone(),
                    mode_id: Some(mode.mode_id.clone()),
                    reason: "alias-cycle-or-missing-target".to_owned(),
                });
                diagnostics.push(Diagnostic {
                    code: "DEVUP_THEME_ALIAS_CYCLE".to_owned(),
                    message: format!(
                        "변수 '{}'의 alias를 안전하게 해석하지 못했습니다.",
                        variable.name
                    ),
                    node_id: None,
                    severity: Some(DiagnosticSeverity::Warning),
                    resource_kind: Some("variable".to_owned()),
                    resource_id: Some(variable.id.clone()),
                    recoverable: Some(true),
                    ..Diagnostic::default()
                });
                continue;
            };
            let mode_name = normalize_token(&mode.name);
            let projection = match variable.resolved_type.as_str() {
                "COLOR" => color_value(value).map(|color| ("color", Value::String(color))),
                "FLOAT" => value
                    .as_f64()
                    .map(|number| ("length", Value::String(format_px(number)))),
                _ => None,
            };
            if let Some((kind, value)) = projection {
                projected
                    .entry((kind.to_owned(), mode_name, token.clone()))
                    .or_default()
                    .push(ProjectedVariable {
                        variable_id: variable.id.clone(),
                        collection_id: variable.variable_collection_id.clone(),
                        collection_name: collection.name.clone(),
                        raw_name: variable.name.clone(),
                        mode_id: mode.mode_id.clone(),
                        source: candidate.source,
                        value,
                        resolution: if variable
                            .values_by_mode
                            .get(&mode.mode_id)
                            .and_then(|value| value.get("type"))
                            .and_then(Value::as_str)
                            == Some("VARIABLE_ALIAS")
                        {
                            "alias".to_owned()
                        } else {
                            "variable".to_owned()
                        },
                    });
            }
        }
    }

    for ((kind, mode, token), candidates) in projected {
        let Some(winner) = candidates.first() else {
            continue;
        };
        match kind.as_str() {
            "color" => {
                colors
                    .entry(mode.clone())
                    .or_default()
                    .insert(token.clone(), winner.value.clone());
            }
            "length" => {
                lengths
                    .entry(mode.clone())
                    .or_default()
                    .insert(token.clone(), winner.value.clone());
            }
            _ => continue,
        }
        let category = if kind == "color" { "colors" } else { "length" };
        source_entries.push(ProvenanceEntry {
            generated_range: None,
            json_pointer: Some(format!(
                "/theme/{}/{}/{}",
                category,
                json_pointer_segment(&mode),
                json_pointer_segment(&token)
            )),
            node_id: None,
            property: None,
            variable_id: Some(winner.variable_id.clone()),
            style_id: None,
            asset_id: None,
            resolution: winner.resolution.clone(),
        });
        if candidates
            .iter()
            .skip(1)
            .all(|candidate| candidate.value == winner.value)
        {
            continue;
        }
        let conflict_candidates = candidates
            .iter()
            .map(|candidate| ThemeConflictCandidate {
                variable_id: candidate.variable_id.clone(),
                collection_id: candidate.collection_id.clone(),
                collection_name: candidate.collection_name.clone(),
                raw_name: candidate.raw_name.clone(),
                mode_id: candidate.mode_id.clone(),
                source: candidate.source,
                value_hash: value_hash(&candidate.value),
            })
            .collect::<Vec<_>>();
        conflicts.push(ThemeConflict {
            token: token.clone(),
            mode: mode.clone(),
            kind: kind.clone(),
            winner_variable_id: winner.variable_id.clone(),
            candidates: conflict_candidates,
        });
        diagnostics.push(Diagnostic {
            code: "DEVUP_THEME_TOKEN_CONFLICT".to_owned(),
            message: format!(
                "동일한 theme token에 서로 다른 값이 있어 결정적 우선순위를 적용했습니다: token={token}, mode={mode}"
            ),
            node_id: None,
            severity: Some(DiagnosticSeverity::Warning),
            property: Some(format!("theme.{kind}.{mode}.{token}")),
            resource_kind: Some("variable".to_owned()),
            resource_id: Some(winner.variable_id.clone()),
            fallback: Some("deterministic-winner".to_owned()),
            recoverable: Some(true),
            ..Diagnostic::default()
        });
    }

    let mut typography = BTreeMap::new();
    let mut shadows = BTreeMap::new();
    let selected_style_ids = (scope != ThemeScope::File && !snapshot.used_style_ids.is_empty())
        .then(|| snapshot.used_style_ids.iter().collect::<BTreeSet<_>>());
    let styles = snapshot
        .styles
        .iter()
        .filter(|style| {
            selected_style_ids
                .as_ref()
                .is_none_or(|selected| selected.contains(&style.id))
        })
        .collect::<Vec<_>>();
    for style in &styles {
        let token = normalize_token(&style.name);
        match style.style_type.as_str() {
            "TEXT" => {
                typography.insert(token.clone(), style.value.clone());
                source_entries.push(ProvenanceEntry {
                    generated_range: None,
                    json_pointer: Some(format!(
                        "/theme/typography/{}",
                        json_pointer_segment(&token)
                    )),
                    node_id: None,
                    property: None,
                    variable_id: None,
                    style_id: Some(style.id.clone()),
                    asset_id: None,
                    resolution: "style".to_owned(),
                });
            }
            "EFFECT" => {
                shadows.insert(token.clone(), style.value.clone());
                source_entries.push(ProvenanceEntry {
                    generated_range: None,
                    json_pointer: Some(format!(
                        "/theme/shadow/default/{}",
                        json_pointer_segment(&token)
                    )),
                    node_id: None,
                    property: None,
                    variable_id: None,
                    style_id: Some(style.id.clone()),
                    asset_id: None,
                    resolution: "style".to_owned(),
                });
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

    let completeness =
        if scope == ThemeScope::File && snapshot.local_complete && snapshot.used_remote_complete {
            Completeness::FullLocalPlusUsedRemote
        } else if variable_count > 0 || !styles.is_empty() {
            Completeness::UsedTokens
        } else {
            Completeness::ResolvedValuesOnly
        };
    unresolved_variables.sort_by(|left, right| {
        left.variable_id
            .cmp(&right.variable_id)
            .then_with(|| left.mode_id.cmp(&right.mode_id))
            .then_with(|| left.reason.cmp(&right.reason))
    });
    source_entries.sort_by(|left, right| left.json_pointer.cmp(&right.json_pointer));
    Ok(ThemeOutput {
        json: output,
        counts: ThemeCounts {
            collections: snapshot.collections.len(),
            variables: variable_count,
            styles: styles.len(),
            modes: snapshot
                .collections
                .iter()
                .map(|collection| collection.modes.len())
                .sum(),
        },
        completeness,
        diagnostics,
        conflicts,
        unresolved_variables,
        source_map: SourceMap {
            version: 1,
            entries: source_entries,
        },
    })
}

fn has_web_syntax(variable: &VariableDefinition) -> bool {
    variable
        .code_syntax
        .get("WEB")
        .is_some_and(|syntax| !syntax.trim().is_empty())
}

fn selected_variable_ids(
    snapshot: &VariableSnapshot,
    scope: ThemeScope,
    variables: &BTreeMap<&str, &VariableDefinition>,
) -> Option<BTreeSet<String>> {
    if scope == ThemeScope::File || snapshot.used_variable_ids.is_empty() {
        return None;
    }
    let mut selected = snapshot
        .used_variable_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut pending = selected.iter().cloned().collect::<Vec<_>>();
    while let Some(variable_id) = pending.pop() {
        let Some(variable) = variables.get(variable_id.as_str()) else {
            continue;
        };
        for value in variable.values_by_mode.values() {
            if value.get("type").and_then(Value::as_str) == Some("VARIABLE_ALIAS")
                && let Some(alias_id) = value.get("id").and_then(Value::as_str)
                && selected.insert(alias_id.to_owned())
            {
                pending.push(alias_id.to_owned());
            }
        }
    }
    Some(selected)
}

fn value_hash(value: &Value) -> String {
    Sha256::digest(serde_json::to_vec(value).unwrap_or_default())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
    variables: &'a BTreeMap<&str, &VariableDefinition>,
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
