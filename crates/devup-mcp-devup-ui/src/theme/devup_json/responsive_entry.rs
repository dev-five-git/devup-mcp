

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

/// Whether a value is a variable snapshot by its own shape, rather than by
/// holding anything. A frame that binds no variable and uses no style still
/// answers with the snapshot's fields, all empty - and that is a theme with
/// nothing in it, not a failure to find one. All three fields have to be
/// there: an empty `VariableSnapshot` deserializes from almost any object,
/// and without this the search would seize on the first one it walked into.
fn names_a_variable_snapshot(value: &Value) -> bool {
    ["collections", "variables", "styles"]
        .into_iter()
        .all(|field| value.get(field).is_some_and(Value::is_array))
}

fn find_variable_snapshot(value: &Value) -> Option<VariableSnapshot> {
    if let Ok(snapshot) = serde_json::from_value::<VariableSnapshot>(value.clone())
        && (!snapshot.collections.is_empty()
            || !snapshot.variables.is_empty()
            || !snapshot.styles.is_empty()
            || names_a_variable_snapshot(value))
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
        // An alias into another collection is read in that collection's mode,
        // which this mode id is not. A collection with one mode has one value
        // to read; with several, which one applies is the consumer's to set.
        let mode = if target.values_by_mode.contains_key(mode) {
            mode
        } else if target.variable_collection_id != variable.variable_collection_id
            && target.values_by_mode.len() == 1
        {
            target.values_by_mode.keys().next()?.as_str()
        } else {
            return None;
        };
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

/// A percentage as CSS writes it, to two decimals: Figma stores `130%` as
/// `129.99999523162842`.
fn format_percent(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}%")
    } else {
        format!("{}%", format!("{rounded:.2}").trim_end_matches('0'))
    }
}

/// The breakpoint slot a variable mode named for one stands for: `mobile`,
/// `tablet` and `desktop` (or `pc`) at 0, 2 and 4, as in front of a style's
/// name, and a number at its own slot.
fn breakpoint_slot(name: &str) -> Option<usize> {
    let name = name.trim().to_ascii_lowercase();
    match name.as_str() {
        "mobile" => Some(0),
        "tablet" => Some(2),
        "desktop" | "pc" => Some(4),
        _ => name.parse::<usize>().ok().filter(|slot| *slot <= 5),
    }
}

/// The slot of each of a collection's modes, in its order, when it has more
/// than one and every one is named for a different breakpoint.
fn breakpoint_slots(collection: &VariableCollection) -> Option<Vec<usize>> {
    let slots = collection
        .modes
        .iter()
        .map(|mode| breakpoint_slot(&mode.name))
        .collect::<Option<Vec<_>>>()?;
    let distinct = slots.iter().collect::<BTreeSet<_>>().len() == slots.len();
    (slots.len() > 1 && distinct).then_some(slots)
}

/// A length token from its breakpoint slots: the value alone when every
/// breakpoint has the same one, the responsive array otherwise.
fn responsive_length(slots: [Option<Value>; 6]) -> Option<Value> {
    let mut filled = slots.iter().flatten();
    let first = filled.next()?.clone();
    if filled.all(|value| *value == first) {
        return Some(first);
    }
    responsive_entry(slots)
}

/// The size a text style's bound font size resolves to at each breakpoint:
/// when every mode of the variable's collection is named for a breakpoint,
/// each resolves, no two share a slot and the sizes differ. Otherwise `None`,
/// and the style keeps the variable's token.
fn font_size_by_breakpoint(
    style: &Value,
    variables: &BTreeMap<&str, &VariableDefinition>,
    collections: &BTreeMap<&str, &VariableCollection>,
) -> Option<Vec<(usize, f64)>> {
    let id = style.pointer("/boundVariables/fontSize/id")?.as_str()?;
    let variable = variables.get(id)?;
    let collection = collections.get(variable.variable_collection_id.as_str())?;
    let sizes = collection
        .modes
        .iter()
        .zip(breakpoint_slots(collection)?)
        .map(|(mode, slot)| {
            let size = resolve_value(variable, &mode.mode_id, variables, &mut HashSet::new())?;
            Some((slot, size.as_f64()?))
        })
        .collect::<Option<Vec<_>>>()?;
    let differ = sizes.iter().any(|(_, size)| *size != sizes[0].1);
    differ.then_some(sizes)
}

/// The theme mode under which the breakpoint lengths are gathered until the
/// themes are known; `normalize_token` never makes a name like it.
const BREAKPOINTS: &str = "@breakpoints";

fn unresolved_alias(variable: &VariableDefinition, mode_id: &str) -> ThemeUnresolvedVariable {
    ThemeUnresolvedVariable {
        variable_id: variable.id.clone(),
        mode_id: Some(mode_id.to_owned()),
        reason: "alias-cycle-or-missing-target".to_owned(),
    }
}

fn alias_diagnostic(variable: &VariableDefinition) -> Diagnostic {
    Diagnostic {
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
    }
}

fn conflict_candidate(candidate: &ProjectedVariable) -> ThemeConflictCandidate {
    ThemeConflictCandidate {
        variable_id: candidate.variable_id.clone(),
        collection_id: candidate.collection_id.clone(),
        collection_name: candidate.collection_name.clone(),
        raw_name: candidate.raw_name.clone(),
        mode_id: candidate.mode_id.clone(),
        source: candidate.source,
        value_hash: value_hash(&candidate.value),
    }
}

fn conflict_diagnostic(kind: &str, mode: &str, token: &str, winner: &str) -> Diagnostic {
    Diagnostic {
        code: "DEVUP_THEME_TOKEN_CONFLICT".to_owned(),
        message: format!(
            "The same theme token had conflicting values; applied deterministic precedence: token={token}, mode={mode}"
        ),
        node_id: None,
        severity: Some(DiagnosticSeverity::Warning),
        property: Some(format!("theme.{kind}.{mode}.{token}")),
        resource_kind: Some("variable".to_owned()),
        resource_id: Some(winner.to_owned()),
        fallback: Some("deterministic-winner".to_owned()),
        recoverable: Some(true),
        ..Diagnostic::default()
    }
}

fn format_px(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}px")
    } else {
        format!("{value}px")
    }
}
