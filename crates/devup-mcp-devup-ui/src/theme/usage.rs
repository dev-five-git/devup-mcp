//! A minimal theme is a projection of actual generated references, including
//! dependencies of typography tokens. Existing project names always win.
use std::collections::BTreeSet;

use devup_mcp_figma::{DevupError, ErrorCode};
use oxc_allocator::Allocator;
use oxc_ast::ast::{JSXAttribute, JSXAttributeName, StringLiteral};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde::Serialize;
use serde_json::{Value, json};

use super::{Completeness, ProjectTheme, ThemeOutput, ThemeScope, VariableSnapshot};

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub tokens: BTreeSet<String>,
    pub typography: BTreeSet<String>,
}

impl TokenUsage {
    pub fn add_tsx(&mut self, source: &str) -> Result<(), DevupError> {
        struct References<'u> {
            usage: &'u mut TokenUsage,
            property: Option<String>,
        }
        impl<'a> Visit<'a> for References<'_> {
            fn visit_jsx_attribute(&mut self, attribute: &JSXAttribute<'a>) {
                let previous = self.property.take();
                self.property = match &attribute.name {
                    JSXAttributeName::Identifier(name) => Some(name.name.to_string()),
                    _ => None,
                };
                walk::walk_jsx_attribute(self, attribute);
                self.property = previous;
            }
            fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
                if self.property.as_deref() == Some("typography") {
                    self.usage.typography.insert(literal.value.to_string());
                } else if self.property.is_some() {
                    dollar_references(literal.value.as_str(), &mut self.usage.tokens);
                }
            }
        }
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
        if !parsed.diagnostics.is_empty() {
            return Err(DevupError::new(
                ErrorCode::DevupCodegenFailed,
                "Cannot determine used theme tokens from invalid generated TSX.",
                false,
            ));
        }
        References {
            usage: self,
            property: None,
        }
        .visit_program(&parsed.program);
        Ok(())
    }
}

fn dollar_references(text: &str, tokens: &mut BTreeSet<String>) {
    for tail in text.split('$').skip(1) {
        let token: String = tail
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
            .collect();
        if !token.is_empty() {
            tokens.insert(token);
        }
    }
}

fn dependencies(value: &Value, tokens: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => dollar_references(text, tokens),
        Value::Array(items) => items.iter().for_each(|item| dependencies(item, tokens)),
        Value::Object(items) => items.values().for_each(|item| dependencies(item, tokens)),
        _ => {}
    }
}

pub fn generate_used_devup_json(
    snapshot: &VariableSnapshot,
    usage: &TokenUsage,
) -> Result<ThemeOutput, DevupError> {
    let mut selected = snapshot.clone();
    selected.styles.retain(|style| {
        usage
            .typography
            .contains(&super::tokens::style_token(&style.name).1)
            || usage
                .tokens
                .contains(&super::tokens::style_token(&style.name).1)
    });
    let variables = snapshot
        .variables
        .iter()
        .chain(&snapshot.used_remote_variables)
        .collect::<Vec<_>>();
    let mut ids = variables
        .iter()
        .filter(|variable| {
            usage.tokens.contains(&super::tokens::variable_token(
                &variable.name,
                variable.code_syntax.get("WEB").map(String::as_str),
            ))
        })
        .map(|variable| variable.id.clone())
        .collect::<BTreeSet<_>>();
    fn binding_ids(value: &Value, ids: &mut BTreeSet<String>) {
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            ids.insert(id.into());
        }
        match value {
            Value::Object(items) => items.values().for_each(|item| binding_ids(item, ids)),
            Value::Array(items) => items.iter().for_each(|item| binding_ids(item, ids)),
            _ => {}
        }
    }
    for style in &selected.styles {
        if let Some(bound) = style.value.get("boundVariables") {
            binding_ids(bound, &mut ids);
        }
    }
    loop {
        let before = ids.len();
        for variable in &variables {
            if ids.contains(&variable.id) {
                for value in variable
                    .values_by_mode
                    .values()
                    .filter(|value| value["type"] == "VARIABLE_ALIAS")
                {
                    binding_ids(value, &mut ids);
                }
            }
        }
        if before == ids.len() {
            break;
        }
    }
    selected
        .variables
        .retain(|variable| ids.contains(&variable.id));
    selected
        .used_remote_variables
        .retain(|variable| ids.contains(&variable.id));
    let mut output = super::generate_devup_json(&selected, ThemeScope::File)?;
    restrict_devup_json(&mut output, usage)?;
    Ok(output)
}

fn selected_theme(source: &Value, usage: &TokenUsage, existing: Option<&ProjectTheme>) -> Value {
    let mut selected = source.clone();
    let mut needed = usage.tokens.clone();
    if let Some(typography) = source
        .pointer("/theme/typography")
        .and_then(Value::as_object)
    {
        for (token, value) in typography {
            if usage.typography.contains(token)
                && existing.is_none_or(|theme| !theme.typography.contains_key(token))
            {
                dependencies(value, &mut needed);
            }
        }
    }
    loop {
        let before = needed.len();
        for category in ["colors", "length", "shadow"] {
            if let Some(modes) = source["theme"][category].as_object() {
                for values in modes.values().filter_map(Value::as_object) {
                    for (token, value) in values {
                        if needed.contains(token)
                            && existing.is_none_or(|theme| !theme.contains_token(token))
                        {
                            dependencies(value, &mut needed);
                        }
                    }
                }
            }
        }
        if before == needed.len() {
            break;
        }
    }
    if let Some(tokens) = selected["theme"]["typography"].as_object_mut() {
        tokens.retain(|name, _| usage.typography.contains(name));
    }
    for category in ["colors", "length", "shadow"] {
        if let Some(modes) = selected["theme"][category].as_object_mut() {
            for values in modes.values_mut().filter_map(Value::as_object_mut) {
                values.retain(|name, _| needed.contains(name));
            }
        }
    }
    selected
}

pub fn restrict_devup_json(output: &mut ThemeOutput, usage: &TokenUsage) -> Result<(), DevupError> {
    let source: Value = serde_json::from_str(&output.json).map_err(|error| {
        DevupError::new(
            ErrorCode::DevupThemeConflict,
            format!("Cannot read generated theme: {error}"),
            false,
        )
    })?;
    let selected = selected_theme(&source, usage, None);
    output.source_map.entries.retain(|entry| {
        entry
            .json_pointer
            .as_deref()
            .is_some_and(|pointer| selected.pointer(pointer).is_some())
    });
    output.counts.variables = output
        .source_map
        .entries
        .iter()
        .filter_map(|entry| entry.variable_id.as_ref())
        .collect::<BTreeSet<_>>()
        .len();
    output.counts.styles = output
        .source_map
        .entries
        .iter()
        .filter_map(|entry| entry.style_id.as_ref())
        .collect::<BTreeSet<_>>()
        .len();
    output.completeness = Completeness::UsedTokens;
    if !output.skipped_styles.is_empty() || !output.unresolved_variables.is_empty() {
        output.completeness = Completeness::ResolvedValuesOnly;
    }
    output.json = format!(
        "{}\n",
        serde_json::to_string_pretty(&selected).expect("JSON value serializes")
    );
    Ok(())
}

/// An additive proposal only: values of every existing token are preserved,
/// including existing names whose Figma value differs. No project file is written.
pub fn missing_token_diff(
    source: &str,
    usage: &TokenUsage,
    existing: &ProjectTheme,
) -> Result<Value, DevupError> {
    let source: Value = serde_json::from_str(source).map_err(|error| {
        DevupError::new(
            ErrorCode::DevupThemeConflict,
            format!("Cannot read generated theme: {error}"),
            false,
        )
    })?;
    let mut additions = selected_theme(&source, usage, Some(existing));
    let mut missing = Vec::new();
    if let Some(tokens) = additions["theme"]["typography"].as_object_mut() {
        tokens.retain(|token, _| !existing.typography.contains_key(token));
        missing.extend(
            tokens
                .keys()
                .map(|name| json!({"category":"typography","token":name})),
        );
    }
    for (category, defined) in [
        ("colors", &existing.colors),
        ("length", &existing.length),
        ("shadow", &existing.shadow),
    ] {
        let mut names = BTreeSet::new();
        if let Some(modes) = additions["theme"][category].as_object_mut() {
            for values in modes.values_mut().filter_map(Value::as_object_mut) {
                values
                    .retain(|token, _| !defined.values().any(|tokens| tokens.contains_key(token)));
                names.extend(values.keys().cloned());
            }
        }
        missing.extend(
            names
                .into_iter()
                .map(|name| json!({"category":category,"token":name})),
        );
    }
    let generated =
        super::parse_project_theme(&serde_json::to_string(&source).expect("JSON serializes"))?;
    let mut needed = usage.tokens.clone();
    dependencies(&additions, &mut needed);
    let unresolved = needed.iter().filter(|token| !existing.contains_token(token) && !generated.contains_token(token))
        .map(|name| json!({"token":format!("${name}"),"reason":"no-collected-definition"}))
        .chain(usage.typography.iter().filter(|token| !existing.typography.contains_key(*token) && !generated.typography.contains_key(*token))
            .map(|name| json!({"token":name,"category":"typography","reason":"no-collected-definition"}))).collect::<Vec<_>>();
    let conflicts = usage.typography.iter().filter_map(|token| {
        let before = existing.typography.get(token)?;
        let proposed = generated.typography.get(token)?;
        (before != proposed).then(|| json!({"category":"typography","token":token,"existingValue":before,"figmaValue":proposed,
            "resolution":"existing-project-value-preserved","fidelity":"unverified"}))
    }).collect::<Vec<_>>();
    Ok(
        json!({"state":if unresolved.is_empty() {"complete"} else {"partial"},
        "additions":additions,"missingTokens":missing,"unresolvedTokens":unresolved,
        "existingNamesPreserved":true,"existingTypographyConflicts":conflicts,"visualFidelity":"unverified","writesProjectTheme":false}),
    )
}
