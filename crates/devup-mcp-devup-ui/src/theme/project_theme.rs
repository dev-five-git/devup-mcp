//! Reads an on-disk project `devup.json` — the file an application actually
//! ships, authored by hand or generated once by `devup_figma_to_json` — and
//! exposes the token names and resolved values it actually defines.
//!
//! This is deliberately a *different* type from [`super::VariableSnapshot`]:
//! `VariableSnapshot` is the raw Figma variable/style export this crate
//! projects *into* a `devup.json` string. [`ProjectTheme`] instead *reads
//! back* an already-materialized `devup.json` file so a caller (the
//! `devup_project_context` and `devup_ui_validate` MCP tools) can check
//! whether a `$token` an agent wants to use actually exists in the project,
//! instead of guessing. See `README.md`'s brief for the incident this
//! guards against: three agents independently invented `$gray100`, a
//! 16px bubble radius, and a 36px avatar size that did not exist in the
//! project's real `devup.json`.
//!
//! `devup.json`'s `theme.colors` / `theme.length` / `theme.shadow` are
//! conventionally mode-keyed (`{"default": {"primary": "#000"}, "dark": {...}}`,
//! matching [`super::generate_devup_json`]'s own output), but hand-authored
//! files sometimes flatten a single-mode theme directly to
//! `{"primary": "#000"}`. [`parse_project_theme`] accepts both shapes:
//! second-level values that are themselves JSON objects are treated as a
//! mode name containing tokens; scalar/array second-level values are
//! treated as tokens of an implicit `"default"` mode.

use std::collections::BTreeMap;

use devup_mcp_figma::{DevupError, ErrorCode};
use serde_json::Value;

use super::tokens::normalize_token;

/// Which theme axis a token belongs to. Mirrors `devup.json`'s
/// `theme.{colors,typography,length,shadow}` keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenCategory {
    Colors,
    Typography,
    Length,
    Shadow,
}

impl TokenCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            TokenCategory::Colors => "colors",
            TokenCategory::Typography => "typography",
            TokenCategory::Length => "length",
            TokenCategory::Shadow => "shadow",
        }
    }
}

/// A theme token's resolved value(s) across whichever modes define it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenEntry {
    pub category: TokenCategory,
    /// mode name -> resolved value. Typography tokens (which `devup.json`
    /// never mode-keys) use the single implicit mode `"default"`.
    pub values_by_mode: BTreeMap<String, Value>,
}

/// A project's `devup.json`, as actually read from disk: only the tokens it
/// defines, nothing inferred or assumed.
#[derive(Debug, Clone, Default)]
pub struct ProjectTheme {
    /// mode -> token -> value
    pub colors: BTreeMap<String, BTreeMap<String, Value>>,
    /// token -> value (devup.json never mode-keys typography)
    pub typography: BTreeMap<String, Value>,
    /// mode -> token -> value
    pub length: BTreeMap<String, BTreeMap<String, Value>>,
    /// mode -> token -> value
    pub shadow: BTreeMap<String, BTreeMap<String, Value>>,
}

impl ProjectTheme {
    /// All mode names any category actually defines, sorted and deduplicated.
    pub fn modes(&self) -> Vec<String> {
        let mut modes = self
            .colors
            .keys()
            .chain(self.length.keys())
            .chain(self.shadow.keys())
            .cloned()
            .collect::<Vec<_>>();
        modes.sort();
        modes.dedup();
        modes
    }

    /// A flat catalog of every token this theme defines, keyed by token
    /// name, merged across categories. `devup_ui_validate` uses this to
    /// check whether a referenced `$token` exists anywhere in the theme;
    /// `devup_project_context` uses the per-category maps directly so it
    /// can report which axis (`colors`/`typography`/`length`/`shadow`) a
    /// token belongs to.
    pub fn token_catalog(&self) -> BTreeMap<String, TokenEntry> {
        let mut catalog = BTreeMap::new();
        for (mode, tokens) in &self.colors {
            for (token, value) in tokens {
                catalog
                    .entry(token.clone())
                    .or_insert_with(|| TokenEntry {
                        category: TokenCategory::Colors,
                        values_by_mode: BTreeMap::new(),
                    })
                    .values_by_mode
                    .insert(mode.clone(), value.clone());
            }
        }
        for (token, value) in &self.typography {
            catalog
                .entry(token.clone())
                .or_insert_with(|| TokenEntry {
                    category: TokenCategory::Typography,
                    values_by_mode: BTreeMap::new(),
                })
                .values_by_mode
                .insert("default".to_owned(), value.clone());
        }
        for (mode, tokens) in &self.length {
            for (token, value) in tokens {
                catalog
                    .entry(token.clone())
                    .or_insert_with(|| TokenEntry {
                        category: TokenCategory::Length,
                        values_by_mode: BTreeMap::new(),
                    })
                    .values_by_mode
                    .insert(mode.clone(), value.clone());
            }
        }
        for (mode, tokens) in &self.shadow {
            for (token, value) in tokens {
                catalog
                    .entry(token.clone())
                    .or_insert_with(|| TokenEntry {
                        category: TokenCategory::Shadow,
                        values_by_mode: BTreeMap::new(),
                    })
                    .values_by_mode
                    .insert(mode.clone(), value.clone());
            }
        }
        catalog
    }

    pub fn contains_token(&self, token: &str) -> bool {
        self.colors
            .values()
            .any(|tokens| tokens.contains_key(token))
            || self.typography.contains_key(token)
            || self
                .length
                .values()
                .any(|tokens| tokens.contains_key(token))
            || self
                .shadow
                .values()
                .any(|tokens| tokens.contains_key(token))
    }

    pub fn token_count(&self) -> usize {
        self.token_catalog().len()
    }

    /// Color tokens (any mode) whose resolved value normalizes to the same
    /// hex string as `hex`. Used to suggest an existing token instead of a
    /// hardcoded color.
    pub fn color_tokens_matching_hex(&self, hex: &str) -> Vec<String> {
        let normalized = normalize_hex(hex);
        let mut matches = self
            .colors
            .values()
            .flat_map(|tokens| tokens.iter())
            .filter(|(_, value)| {
                value
                    .as_str()
                    .is_some_and(|candidate| normalize_hex(candidate) == normalized)
            })
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        matches
    }

    /// Length tokens (any mode) whose resolved value equals `px` (e.g.
    /// `"16px"`) exactly as written.
    pub fn length_tokens_matching_px(&self, px: &str) -> Vec<String> {
        let mut matches = self
            .length
            .values()
            .flat_map(|tokens| tokens.iter())
            .filter(|(_, value)| value.as_str() == Some(px))
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        matches
    }
}

fn normalize_hex(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

/// Parses a project's `devup.json` file content (the whole file, i.e. the
/// object with the top-level `theme` key) into a [`ProjectTheme`].
///
/// Never invents or assumes structure: a missing `theme` key, or a missing
/// category under it, simply yields an empty map for that category rather
/// than an error. Malformed JSON is the only parse failure.
pub fn parse_project_theme(source: &str) -> Result<ProjectTheme, DevupError> {
    let root: Value = serde_json::from_str(source).map_err(|error| {
        DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "Failed to parse devup.json as JSON.",
            false,
            serde_json::json!({ "parseError": error.to_string() }),
        )
    })?;
    let theme = root.get("theme").cloned().unwrap_or(Value::Null);
    Ok(ProjectTheme {
        colors: parse_mode_keyed(theme.get("colors")),
        typography: parse_flat(theme.get("typography")),
        length: parse_mode_keyed(theme.get("length")),
        shadow: parse_mode_keyed(theme.get("shadow")),
    })
}

/// Parses a `theme.<category>` value that is conventionally mode-keyed
/// (`{"default": {"token": value}}`) but tolerates a flattened single-mode
/// shape (`{"token": value}`) by treating it as the `"default"` mode.
/// Distinguishes the two shapes per top-level entry: an entry whose value is
/// itself a JSON object is treated as `mode -> tokens`; an entry whose value
/// is a scalar/array is treated as a token of the implicit `"default"` mode.
fn parse_mode_keyed(value: Option<&Value>) -> BTreeMap<String, BTreeMap<String, Value>> {
    let mut result = BTreeMap::<String, BTreeMap<String, Value>>::new();
    let Some(Value::Object(entries)) = value else {
        return result;
    };
    for (key, entry) in entries {
        match entry {
            Value::Object(tokens) => {
                let mode_tokens = result.entry(key.clone()).or_default();
                for (token, token_value) in tokens {
                    mode_tokens.insert(token.clone(), token_value.clone());
                }
            }
            other => {
                result
                    .entry("default".to_owned())
                    .or_default()
                    .insert(key.clone(), other.clone());
            }
        }
    }
    result
}

fn parse_flat(value: Option<&Value>) -> BTreeMap<String, Value> {
    let Some(Value::Object(entries)) = value else {
        return BTreeMap::new();
    };
    entries
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Simple Levenshtein edit distance, used only to suggest the closest
/// existing token names for a `$token` that does not exist. Deliberately
/// unweighted (all edits cost 1): this is a "did you mean" hint, not a
/// scored ranking algorithm.
pub fn edit_distance(left: &str, right: &str) -> usize {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    let mut previous_row = (0..=right.len()).collect::<Vec<_>>();
    let mut current_row = vec![0usize; right.len() + 1];
    for (i, &left_char) in left.iter().enumerate() {
        current_row[0] = i + 1;
        for (j, &right_char) in right.iter().enumerate() {
            let cost = usize::from(left_char != right_char);
            current_row[j + 1] = (current_row[j] + 1)
                .min(previous_row[j + 1] + 1)
                .min(previous_row[j] + cost);
        }
        std::mem::swap(&mut previous_row, &mut current_row);
    }
    previous_row[right.len()]
}

/// Returns up to `limit` token names from `catalog` closest to `query` by
/// edit distance, sorted by distance then name. Empty if `catalog` is empty.
pub fn closest_tokens<'a>(
    query: &str,
    catalog: impl Iterator<Item = &'a String>,
    limit: usize,
) -> Vec<String> {
    let mut scored = catalog
        .map(|token| (edit_distance(query, token), token.clone()))
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, token)| token)
        .collect()
}

/// Confirms [`normalize_token`] stays reachable for callers that need
/// devup.json-style token normalization alongside project-theme reading
/// (`devup_project_context`'s `api`/`db` scopes derive suggested
/// identifiers the same way theme tokens are named).
pub fn normalize_identifier(input: &str) -> String {
    normalize_token(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mode_keyed_colors_and_flat_typography() {
        let source = r##"{
            "theme": {
                "colors": {
                    "default": { "primary": "#111111", "background": "#ffffff" },
                    "dark": { "primary": "#eeeeee", "background": "#000000" }
                },
                "typography": {
                    "body1": { "fontSize": "14px", "lineHeight": "20px" }
                },
                "length": {
                    "default": { "sm": "8px", "md": "16px" }
                },
                "shadow": {
                    "default": { "card": "0 1px 2px rgba(0,0,0,0.1)" }
                }
            }
        }"##;
        let theme = parse_project_theme(source).expect("valid devup.json");
        assert_eq!(
            theme.colors["default"]["primary"],
            Value::String("#111111".to_owned())
        );
        assert_eq!(
            theme.colors["dark"]["primary"],
            Value::String("#eeeeee".to_owned())
        );
        assert!(theme.typography.contains_key("body1"));
        assert_eq!(
            theme.length["default"]["md"],
            Value::String("16px".to_owned())
        );
        assert!(theme.contains_token("primary"));
        assert!(theme.contains_token("md"));
        assert!(!theme.contains_token("gray100"));
    }

    #[test]
    fn tolerates_flattened_single_mode_colors() {
        let source = r##"{ "theme": { "colors": { "primary": "#111111" } } }"##;
        let theme = parse_project_theme(source).expect("valid devup.json");
        assert_eq!(
            theme.colors["default"]["primary"],
            Value::String("#111111".to_owned())
        );
    }

    #[test]
    fn missing_theme_key_yields_empty_categories_not_an_error() {
        let theme = parse_project_theme("{}").expect("empty object is valid JSON");
        assert!(theme.colors.is_empty());
        assert!(theme.typography.is_empty());
        assert_eq!(theme.token_count(), 0);
    }

    #[test]
    fn rejects_malformed_json() {
        let error = parse_project_theme("{ not json").unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    }

    #[test]
    fn suggests_closest_tokens_by_edit_distance() {
        let source = r##"{ "theme": { "colors": { "default": {
            "captionLight": "#999999", "backgroundLight": "#fafafa", "primary": "#111111"
        } } } }"##;
        let theme = parse_project_theme(source).unwrap();
        let catalog = theme.token_catalog();
        let names = catalog.keys().collect::<Vec<_>>();
        let suggestions = closest_tokens("gray100", names.into_iter(), 2);
        assert_eq!(suggestions.len(), 2);
    }

    #[test]
    fn finds_color_tokens_matching_hardcoded_hex() {
        let source = r##"{ "theme": { "colors": { "default": { "primary": "#FF0000" } } } }"##;
        let theme = parse_project_theme(source).unwrap();
        assert_eq!(theme.color_tokens_matching_hex("#ff0000"), vec!["primary"]);
        assert!(theme.color_tokens_matching_hex("#00ff00").is_empty());
    }

    #[test]
    fn finds_length_tokens_matching_hardcoded_px() {
        let source = r##"{ "theme": { "length": { "default": { "md": "16px" } } } }"##;
        let theme = parse_project_theme(source).unwrap();
        assert_eq!(theme.length_tokens_matching_px("16px"), vec!["md"]);
        assert!(theme.length_tokens_matching_px("17px").is_empty());
    }
}
