use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use devup_mcp_figma::{DevupError, Diagnostic, DiagnosticSeverity, ErrorCode, UpstreamResult};

use super::tokens::{normalize_token, style_token, variable_token};
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
                message: format!("Collection for variable '{}' was not found.", variable.name),
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
                        "Alias for variable '{}' could not be resolved safely.",
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
                "The same theme token had conflicting values; applied deterministic precedence: token={token}, mode={mode}"
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

    // A text style is a `typography` entry and an effect style a `shadow`
    // entry, each in the shape devup-ui reads and under the name the plugin's
    // `styleNameToTypography` gives it. A style named for a breakpoint —
    // `desktop/h1`, `3/bodyXlgBold` — is one slot of a responsive entry, and
    // the slots of one name are gathered into an array. The raw style object
    // used to be written as it came, and devup-ui's own plugin refused the
    // file: `Invalid typography property value: Object {"unit": "PERCENT",
    // "value": 120}`.
    let mut typography_slots: BTreeMap<String, [Option<Value>; 6]> = BTreeMap::new();
    let mut shadow_slots: BTreeMap<String, [Option<String>; 6]> = BTreeMap::new();
    let variable_names = variables
        .values()
        .map(|variable| {
            (
                variable.id.as_str(),
                variable_token(
                    &variable.name,
                    variable.code_syntax.get("WEB").map(String::as_str),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
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
        let (level, token) = style_token(&style.name);
        let level = level.min(5);
        match style.style_type.as_str() {
            "TEXT" => {
                let slots = typography_slots.entry(token.clone()).or_default();
                // The first style seen for a slot keeps it, as in the plugin.
                if slots[level].is_none() {
                    slots[level] = Some(typography_value(&style.value, &variable_names));
                }
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
                let Some(shadow) = shadow_value(&style.value) else {
                    continue;
                };
                let slots = shadow_slots.entry(token.clone()).or_default();
                if slots[level].is_none() {
                    slots[level] = Some(shadow);
                }
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
    let typography = typography_slots
        .into_iter()
        .filter_map(|(token, slots)| responsive_entry(slots).map(|entry| (token, entry)))
        .collect::<Map<_, _>>();
    let shadows = shadow_slots
        .into_iter()
        .filter_map(|(token, slots)| {
            responsive_entry(slots.map(|slot| slot.map(Value::String))).map(|entry| (token, entry))
        })
        .collect::<Map<_, _>>();

    // devup-ui takes the first theme written under `colors` as the default —
    // the one in effect without a `data-theme` — so the order of the modes is
    // not cosmetic. Sorted by name, `dark` came before `light`, and a file
    // whose default mode is Light was handed to consumers dark. The
    // collections' default modes go first, then the rest of their modes in
    // the order the collections declare them, as the plugin writes them.
    let mode_order = {
        let mut order: Vec<String> = Vec::new();
        let mut push = |name: String| {
            if !order.contains(&name) {
                order.push(name);
            }
        };
        for collection in collections.values() {
            if let Some(default) = collection
                .modes
                .iter()
                .find(|mode| mode.mode_id == collection.default_mode_id)
            {
                push(normalize_token(&default.name));
            }
        }
        for collection in collections.values() {
            for mode in &collection.modes {
                push(normalize_token(&mode.name));
            }
        }
        order
    };
    let in_mode_order = |modes: BTreeMap<String, BTreeMap<String, Value>>| {
        let mut ordered = Map::new();
        for name in &mode_order {
            if let Some(tokens) = modes.get(name) {
                ordered.insert(name.clone(), json!(tokens));
            }
        }
        for (name, tokens) in modes {
            if !ordered.contains_key(&name) {
                ordered.insert(name, json!(tokens));
            }
        }
        Value::Object(ordered)
    };
    let colors = in_mode_order(colors);
    // Lengths and shadows do not vary by colour theme, but devup-ui keys them
    // by one, so each colour theme gets a copy — the plugin replicates them
    // the same way. A file with no colour themes keeps `default`.
    let theme_names = colors
        .as_object()
        .map(|themes| themes.keys().cloned().collect::<Vec<_>>())
        .filter(|names| !names.is_empty())
        .unwrap_or_else(|| vec!["default".to_owned()]);
    let lengths = {
        let ordered = in_mode_order(lengths);
        let mut by_theme = ordered.as_object().cloned().unwrap_or_default();
        let first = by_theme.values().next().cloned();
        let mut replicated = Map::new();
        for name in &theme_names {
            if let Some(tokens) = by_theme.remove(name) {
                replicated.insert(name.clone(), tokens);
            } else if let Some(first) = &first {
                replicated.insert(name.clone(), first.clone());
            }
        }
        for (name, tokens) in by_theme {
            replicated.insert(name, tokens);
        }
        Value::Object(replicated)
    };
    let shadows = theme_names
        .iter()
        .map(|name| (name.clone(), Value::Object(shadows.clone())))
        .collect::<Map<_, _>>();
    let mut theme = Map::new();
    theme.insert("colors".to_owned(), colors);
    theme.insert("typography".to_owned(), Value::Object(typography));
    theme.insert("length".to_owned(), lengths);
    theme.insert("shadow".to_owned(), Value::Object(shadows));
    let mut root = Map::new();
    root.insert("theme".to_owned(), Value::Object(theme));
    let mut output = serde_json::to_string_pretty(&Value::Object(root)).map_err(|_| {
        DevupError::new(
            ErrorCode::DevupThemeConflict,
            "Failed to serialize devup.json.",
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

/// A text style's value as devup-ui reads it — the plugin's
/// `textStyleToTypography`.
///
/// `fontSize` is in pixels, `lineHeight` a ratio from a percentage (`120%`
/// is `1.2`), pixels as they are, and `normal` when Figma sets it
/// automatically; `letterSpacing` is `em` from a percentage and pixels as
/// they are; the weight is read off the font style's name; and a field the
/// style binds to a variable is that variable's token. Two departures from
/// the plugin, on purpose: `Bold Italic` is `700` and italic, where the
/// plugin reads the weight off the whole style name and gets `400`; and a
/// text case is the CSS `text-transform` value — `uppercase`, not `upper`.
fn typography_value(style: &Value, variable_names: &BTreeMap<&str, String>) -> Value {
    let mut entry = Map::new();
    let font = style.get("fontName");
    let family = font
        .and_then(|font| font.get("family"))
        .and_then(Value::as_str);
    let face = font
        .and_then(|font| font.get("style"))
        .and_then(Value::as_str)
        .unwrap_or("Regular");
    if let Some(family) = family {
        entry.insert("fontFamily".to_owned(), Value::String(family.to_owned()));
    }
    if face.contains("Italic") {
        entry.insert("fontStyle".to_owned(), Value::String("italic".to_owned()));
    }
    entry.insert("fontWeight".to_owned(), Value::from(font_weight(face)));
    if let Some(size) = style.get("fontSize").and_then(Value::as_f64) {
        entry.insert("fontSize".to_owned(), Value::String(format_px(size)));
    }
    match style.get("textDecoration").and_then(Value::as_str) {
        Some("UNDERLINE") => {
            entry.insert(
                "textDecoration".to_owned(),
                Value::String("underline".to_owned()),
            );
        }
        Some("STRIKETHROUGH") => {
            entry.insert(
                "textDecoration".to_owned(),
                Value::String("line-through".to_owned()),
            );
        }
        _ => {}
    }
    let transform = match style.get("textCase").and_then(Value::as_str) {
        Some("UPPER") => Some("uppercase"),
        Some("LOWER") => Some("lowercase"),
        Some("TITLE") => Some("capitalize"),
        _ => None,
    };
    if let Some(transform) = transform {
        entry.insert(
            "textTransform".to_owned(),
            Value::String(transform.to_owned()),
        );
    }
    if let Some(line_height) = style.get("lineHeight") {
        let unit = line_height.get("unit").and_then(Value::as_str);
        let value = line_height.get("value").and_then(Value::as_f64);
        let written = match (unit, value) {
            (Some("AUTO"), _) => Some(Value::String("normal".to_owned())),
            (Some("PERCENT"), Some(percent)) => Some(Value::from((percent / 10.0).round() / 10.0)),
            (Some(_), Some(pixels)) => Some(Value::String(format_px(pixels))),
            _ => None,
        };
        if let Some(written) = written {
            entry.insert("lineHeight".to_owned(), written);
        }
    }
    if let Some(spacing) = style.get("letterSpacing") {
        let unit = spacing.get("unit").and_then(Value::as_str);
        let value = spacing.get("value").and_then(Value::as_f64);
        let written = match (unit, value) {
            (Some("PERCENT"), Some(percent)) => Some(format!("{}em", percent.round() / 100.0)),
            (Some(_), Some(pixels)) => Some(format_px(pixels)),
            _ => None,
        };
        if let Some(written) = written {
            entry.insert("letterSpacing".to_owned(), Value::String(written));
        }
    }
    if let Some(bound) = style.get("boundVariables").and_then(Value::as_object) {
        for field in [
            "fontFamily",
            "fontSize",
            "fontStyle",
            "fontWeight",
            "letterSpacing",
            "lineHeight",
        ] {
            if let Some(token) = bound
                .get(field)
                .and_then(|alias| alias.get("id"))
                .and_then(Value::as_str)
                .and_then(|id| variable_names.get(id))
            {
                entry.insert(field.to_owned(), Value::String(format!("${token}")));
            }
        }
    }
    Value::Object(entry)
}

/// The weight a font style's name means — the plugin's `getFontWeight`,
/// with `Italic` set aside first so `Bold Italic` is still bold.
fn font_weight(face: &str) -> u32 {
    let name = face
        .replace("Italic", "")
        .replace([' ', '-', '_'], "")
        .to_ascii_lowercase();
    match name.as_str() {
        "thin" | "hairline" => 100,
        "extralight" | "ultralight" => 200,
        "light" => 300,
        "" | "normal" | "regular" | "book" => 400,
        "medium" => 500,
        "semibold" | "demibold" => 600,
        "bold" => 700,
        "extrabold" | "ultrabold" => 800,
        "black" | "heavy" => 900,
        other => match other.parse::<u32>() {
            Ok(number) if (1..=9).contains(&number) => number * 100,
            Ok(number) => number,
            Err(_) => 400,
        },
    }
}

/// An effect style's visible shadows as one CSS `box-shadow` — the plugin's
/// `effectStyleToCssShadow`. `None` when it casts none.
fn shadow_value(effects: &Value) -> Option<String> {
    let parts = effects
        .as_array()?
        .iter()
        .filter(|effect| effect.get("visible").and_then(Value::as_bool) != Some(false))
        .filter_map(|effect| {
            let kind = effect.get("type").and_then(Value::as_str)?;
            let inset = match kind {
                "DROP_SHADOW" => "",
                "INNER_SHADOW" => "inset ",
                _ => return None,
            };
            let offset = effect.get("offset");
            let x = offset
                .and_then(|offset| offset.get("x"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let y = offset
                .and_then(|offset| offset.get("y"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let radius = effect.get("radius").and_then(Value::as_f64).unwrap_or(0.0);
            let spread = effect.get("spread").and_then(Value::as_f64).unwrap_or(0.0);
            let color = color_value(effect.get("color")?)?;
            let length = |value: f64| {
                if value == 0.0 {
                    "0".to_owned()
                } else {
                    format_px(value)
                }
            };
            Some(format!(
                "{inset}{} {} {} {} {color}",
                length(x),
                length(y),
                length(radius),
                length(spread)
            ))
        })
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// One entry of `typography` or `shadow` from its six breakpoint slots — the
/// plugin's reduction: one slot filled is the value alone; the first slot
/// empty is filled with the first value there is, since a mobile-first array
/// has to start somewhere; and the trailing empty slots are dropped, which
/// the plugin does only for an array it had to fill.
fn responsive_entry(slots: [Option<Value>; 6]) -> Option<Value> {
    let filled = slots.iter().filter(|slot| slot.is_some()).count();
    if filled == 0 {
        return None;
    }
    if filled == 1 {
        return slots.into_iter().flatten().next();
    }
    let mut values = slots.to_vec();
    if values[0].is_none() {
        values[0] = slots.iter().flatten().next().cloned();
    }
    while values.last().is_some_and(Option::is_none) {
        values.pop();
    }
    Some(Value::Array(
        values
            .into_iter()
            .map(|slot| slot.unwrap_or(Value::Null))
            .collect(),
    ))
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
            "Variable snapshot was not found in the Figma MCP response.",
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
