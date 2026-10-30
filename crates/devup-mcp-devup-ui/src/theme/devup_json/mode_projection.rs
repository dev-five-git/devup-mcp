#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeModeDropped {
    pub collection_id: String,
    pub mode_id: String,
    pub mode_name: String,
    pub reason: String,
    pub mapped_to: Option<String>,
}

fn mode_meaning(name: &str) -> String {
    let normalized = normalize_token(name);
    let lower = normalized.to_ascii_lowercase();
    if lower.ends_with("dark") {
        "dark".into()
    } else if lower.ends_with("light") {
        "light".into()
    } else {
        normalized
    }
}

fn mode_projection(
    snapshot: &VariableSnapshot,
    candidates: &[VariableCandidate<'_>],
) -> (BTreeMap<(String, String), String>, Vec<ThemeModeDropped>) {
    let remote_ids = snapshot
        .used_remote_variables
        .iter()
        .map(|variable| variable.id.as_str())
        .collect::<BTreeSet<_>>();
    let variables = snapshot
        .variables
        .iter()
        .chain(&snapshot.used_remote_variables)
        .map(|variable| (variable.id.as_str(), variable))
        .collect();
    let mut reference_snapshot = snapshot.clone();
    reference_snapshot.used_variable_ids.extend(
        candidates
            .iter()
            .filter(|candidate| !remote_ids.contains(candidate.variable.id.as_str()))
            .map(|candidate| candidate.variable.id.clone()),
    );
    let referenced = selected_variable_ids(&reference_snapshot, ThemeScope::Node, &variables)
        .unwrap_or_default();
    let eligible = |collection: &VariableCollection| {
        let local = candidates.iter().any(|candidate| {
            candidate.variable.variable_collection_id == collection.id
                && !remote_ids.contains(candidate.variable.id.as_str())
        });
        local
            || candidates.iter().any(|candidate| {
                candidate.variable.variable_collection_id == collection.id
                    && referenced.contains(&candidate.variable.id)
            })
    };
    let default = if snapshot
        .collections
        .iter()
        .filter(|collection| eligible(collection))
        .flat_map(|collection| &collection.modes)
        .any(|mode| mode_meaning(&mode.name) == "light")
    {
        "light"
    } else {
        "default"
    };
    let mut mapping = BTreeMap::new();
    let mut dropped = Vec::new();
    for collection in &snapshot.collections {
        for mode in &collection.modes {
            let name = mode_meaning(&mode.name);
            let single_default = collection.modes.len() == 1
                && mode.mode_id == collection.default_mode_id
                && matches!(
                    name.to_ascii_lowercase().as_str(),
                    "mode1" | "value" | "default"
                );
            let (target, reason) = if !candidates
                .iter()
                .any(|candidate| candidate.variable.variable_collection_id == collection.id)
            {
                (None, Some("no-defined-color-values"))
            } else if !eligible(collection) {
                (None, Some("unreferenced-remote-collection"))
            } else if single_default {
                (
                    Some(default.to_owned()),
                    (name != default).then_some("single-default-mode"),
                )
            } else {
                (
                    Some(name.clone()),
                    (name != normalize_token(&mode.name)).then_some("merged-by-mode-meaning"),
                )
            };
            if let Some(reason) = reason {
                dropped.push(ThemeModeDropped {
                    collection_id: collection.id.clone(),
                    mode_id: mode.mode_id.clone(),
                    mode_name: mode.name.clone(),
                    reason: reason.into(),
                    mapped_to: target.clone(),
                });
            }
            if let Some(target) = target {
                mapping.insert((collection.id.clone(), mode.mode_id.clone()), target);
            }
        }
    }
    (mapping, dropped)
}

fn record_empty_color_modes(
    snapshot: &VariableSnapshot,
    mapping: &BTreeMap<(String, String), String>,
    color_modes: &BTreeSet<(String, String)>,
) -> Vec<ThemeModeDropped> {
    snapshot
        .collections
        .iter()
        .flat_map(|collection| {
            collection.modes.iter().filter_map(|mode| {
                let key = (collection.id.clone(), mode.mode_id.clone());
                (mapping.contains_key(&key) && !color_modes.contains(&key)).then(|| {
                    ThemeModeDropped {
                        collection_id: collection.id.clone(),
                        mode_id: mode.mode_id.clone(),
                        mode_name: mode.name.clone(),
                        reason: "no-defined-color-values".into(),
                        mapped_to: None,
                    }
                })
            })
        })
        .collect()
}

fn color_schemes(theme_names: &[String]) -> Value {
    Value::Object(
        theme_names
            .iter()
            .map(|name| {
                (
                    name.clone(),
                    json!(if name.to_ascii_lowercase().contains("dark") {
                        "dark"
                    } else {
                        "light"
                    }),
                )
            })
            .collect(),
    )
}
