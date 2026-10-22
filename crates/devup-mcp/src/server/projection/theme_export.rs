use super::*;
use devup_mcp_devup_ui::theme::{ThemeOutput, TokenUsage};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeExportOptions {
    pub mode: String,
    pub project_root: Option<String>,
}

pub fn validate(
    mode: &str,
    outputs: &[String],
    project_root: Option<&str>,
) -> Result<(), DevupError> {
    if !matches!(mode, "full" | "used-only" | "missing-only") {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "devupJsonMode must be full, used-only or missing-only.",
            false,
        ));
    }
    let file_wide = mode == "missing-only" && outputs.iter().any(|o| o == "devupJson");
    if mode != "full"
        && !file_wide
        && (!outputs.iter().any(|o| o == "devupJson")
            || !outputs
                .iter()
                .any(|o| matches!(o.as_str(), "tsx" | "componentTsx" | "responsiveTsx")))
    {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "used-only/missing-only requires devupJson and a generated code output (tsx, componentTsx or responsiveTsx).",
            false,
        ));
    }
    if mode == "missing-only" && project_root.is_none() {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "missing-only requires projectRoot with an existing devup.json.",
            false,
        ));
    }
    Ok(())
}

fn usage(result: &Map<String, Value>) -> Result<(TokenUsage, usize), DevupError> {
    fn add(
        result: &Map<String, Value>,
        usage: &mut TokenUsage,
        count: &mut usize,
    ) -> Result<(), DevupError> {
        for key in ["tsx", "componentTsx", "responsiveTsx"] {
            if let Some(source) = result.get(key).and_then(Value::as_str) {
                usage.add_tsx(source)?;
                *count += 1;
            }
        }
        if let Some(frames) = result.get("frames").and_then(Value::as_array) {
            for frame in frames.iter().filter_map(Value::as_object) {
                add(frame, usage, count)?;
            }
        }
        Ok(())
    }
    let mut usage = TokenUsage::default();
    let mut count = 0;
    add(result, &mut usage, &mut count)?;
    Ok((usage, count))
}

/// Every token name a generated devup.json defines.
fn all_tokens(json: &str) -> TokenUsage {
    let mut usage = TokenUsage::default();
    let Ok(source) = serde_json::from_str::<Value>(json) else {
        return usage;
    };
    if let Some(typography) = source.pointer("/theme/typography").and_then(Value::as_object) {
        usage.typography.extend(typography.keys().cloned());
    }
    for category in ["colors", "length", "shadow"] {
        if let Some(modes) = source["theme"][category].as_object() {
            for values in modes.values().filter_map(Value::as_object) {
                usage.tokens.extend(values.keys().cloned());
            }
        }
    }
    usage
}

pub fn generate(
    snapshot: &devup_mcp_devup_ui::theme::VariableSnapshot,
    scope: devup_mcp_devup_ui::theme::ThemeScope,
    options: &ThemeExportOptions,
    result: &Map<String, Value>,
) -> Result<ThemeOutput, DevupError> {
    if matches!(options.mode.as_str(), "used-only" | "missing-only") {
        let (usage, count) = usage(result)?;
        if count == 0 && options.mode == "missing-only" {
            return generate_devup_json(snapshot, scope);
        }
        if count == 0 {
            return Err(DevupError::new(
                ErrorCode::DevupCodegenFailed,
                "No generated code is available to establish used theme references.",
                false,
            ));
        }
        devup_mcp_devup_ui::theme::generate_used_devup_json(snapshot, &usage)
    } else {
        generate_devup_json(snapshot, scope)
    }
}

pub fn prepare(
    output: &mut ThemeOutput,
    options: &ThemeExportOptions,
    result: &mut Map<String, Value>,
    payload: &CollectedPayload,
    strict: bool,
) -> Result<(), DevupError> {
    for skipped in &mut output.skipped_styles {
        skipped.node_ids = payload
            .snapshot
            .nodes
            .values()
            .filter(|node| {
                let view = node.typed_view();
                view.string("textStyleId") == Some(skipped.style_id.as_str())
                    || view
                        .value("styledTextSegments")
                        .and_then(Value::as_array)
                        .is_some_and(|segments| {
                            segments
                                .iter()
                                .any(|segment| segment["textStyleId"] == skipped.style_id)
                        })
            })
            .map(|node| node.id.clone())
            .collect();
        for diagnostic in output
            .diagnostics
            .iter_mut()
            .filter(|d| d.resource_id.as_ref() == Some(&skipped.style_id))
        {
            diagnostic.node_id = skipped.node_ids.first().cloned();
            diagnostic.message = format!(
                "Text style '{}' ({}) used by nodes [{}] was skipped: {}",
                skipped.name,
                skipped.style_id,
                skipped.node_ids.join(", "),
                skipped.reason
            );
            if let Some(details) = &mut diagnostic.details {
                details["nodeIds"] = json!(skipped.node_ids);
            }
        }
    }
    if strict && let Some(skipped) = output.skipped_styles.first() {
        return Err(DevupError::with_details(
            ErrorCode::DevupThemeConflict,
            format!(
                "Text style '{}' ({}) used by nodes [{}] is not representable: {}",
                skipped.name,
                skipped.style_id,
                skipped.node_ids.join(", "),
                skipped.reason
            ),
            false,
            json!({"skippedStyles":output.skipped_styles}),
        ));
    }
    let (mut usage, mut code_count) = usage(result)?;
    if code_count == 0 && options.mode == "missing-only" {
        usage = all_tokens(&output.json);
        code_count = 1;
    } else if options.mode != "full" && !options.mode.is_empty() {
        if code_count == 0 {
            return Err(DevupError::new(
                ErrorCode::DevupCodegenFailed,
                "No code output was generated; used-only/missing-only cannot infer successful token usage.",
                false,
            ));
        }
        devup_mcp_devup_ui::theme::restrict_devup_json(output, &usage)?;
    }
    result.insert(
        "devupJsonMode".into(),
        json!(if options.mode.is_empty() {
            "full"
        } else {
            &options.mode
        }),
    );
    result.insert("skippedStyles".into(), json!(output.skipped_styles));
    if let Some(root) = options.project_root.as_deref() {
        let lookup = super::super::project_context::theme_for_validation(Some(root))?;
        if let Some(existing) = lookup.theme {
            if code_count > 0 {
                let diff =
                    devup_mcp_devup_ui::theme::missing_token_diff(&output.json, &usage, &existing)?;
                if options.mode == "missing-only" {
                    output.json = format!(
                        "{}\n",
                        serde_json::to_string_pretty(&diff["additions"]).expect("JSON serializes")
                    );
                    let additions = &diff["additions"];
                    output.source_map.entries.retain(|entry| {
                        entry
                            .json_pointer
                            .as_deref()
                            .is_some_and(|pointer| additions.pointer(pointer).is_some())
                    });
                    output.counts.variables = output
                        .source_map
                        .entries
                        .iter()
                        .filter_map(|entry| entry.variable_id.as_ref())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len();
                    output.counts.styles = output
                        .source_map
                        .entries
                        .iter()
                        .filter_map(|entry| entry.style_id.as_ref())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len();
                }
                result.insert("themeDiff".into(), diff);
            } else {
                result.insert("themeDiff".into(), json!({"state":"not-checked","reason":"No generated code output was requested."}));
            }
        } else if options.mode == "missing-only" {
            return Err(DevupError::with_details(
                ErrorCode::DevupInvalidInput,
                "No unambiguous existing devup.json was available for missing-only.",
                false,
                json!({"guardrail":lookup.guardrail}),
            ));
        } else {
            result.insert(
                "themeDiff".into(),
                json!({"state":"unavailable","guardrail":lookup.guardrail}),
            );
        }
    }
    if code_count > 0 {
        let unresolved = if let Some(tokens) = result
            .get("themeDiff")
            .and_then(|diff| diff.get("unresolvedTokens"))
        {
            tokens.clone()
        } else {
            devup_mcp_devup_ui::theme::missing_token_diff(
                &output.json,
                &usage,
                &Default::default(),
            )?["unresolvedTokens"]
                .clone()
        };
        if unresolved
            .as_array()
            .is_some_and(|tokens| !tokens.is_empty())
        {
            output.completeness = devup_mcp_devup_ui::theme::Completeness::ResolvedValuesOnly;
            if strict {
                return Err(DevupError::with_details(
                    ErrorCode::DevupThemeConflict,
                    "Generated code references tokens without a collected or existing definition.",
                    false,
                    json!({"unresolvedUsedTokens":unresolved}),
                ));
            }
        }
        result.insert("unresolvedUsedTokens".into(), unresolved);
    }
    Ok(())
}
