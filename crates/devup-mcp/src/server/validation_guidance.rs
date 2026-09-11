//! Automatic correction is narrower than source editability: never guess a
//! token, a render element, or the intended value of a runtime expression.
use devup_mcp_devup_ui::{
    theme::ProjectTheme,
    ui_validate::{Severity, UiValidation, validate_devup_ui_tsx},
};
use serde_json::{Value, json};

use super::UiValidateInput;

pub(super) fn guidance(
    input: &UiValidateInput,
    report: &UiValidation,
    theme: Option<&ProjectTheme>,
) -> Value {
    if let Some(tsx) = corrected_tsx(input, report, theme) {
        let mut arguments = json!(input);
        arguments["tsx"] = json!(tsx);
        return json!({
            "recoveryState":"available",
            "recoveryReason":"Every blocking warning has one exact matching theme token in every mode; the corrected source passes validation with the original strictness.",
            "nextActionReason":"Validate the corrected source, then apply its token replacements to your file.",
            "nextAction":{"tool":"devup_ui_validate","arguments":arguments}
        });
    }
    let reasons: std::collections::BTreeSet<_> = report.violations.iter()
        .filter(|v| v.severity == Severity::Error || (input.strict && v.severity == Severity::Warning))
        .map(|v| match v.rule {
            "unknown-prop" => "unknown-prop: inspect the prop spelling and intended render element; changing as or deleting a prop could change behavior",
            "unknown-token" => "unknown-token: choose an intended token from the actual theme; closest-token suggestions do not establish intent",
            "invalid-syntax" => "invalid-syntax: repair the source syntax at the reported byteRange",
            "runtime-value" => "runtime-value: provide a statically analyzable utility value; the validator cannot evaluate runtime expressions",
            "hardcoded-color" | "hardcoded-length" => "strict warning: choose among the matching theme tokens; no unambiguous validated replacement is available",
            _ => "inspect the blocking source finding and its suggestion",
        }).collect();
    json!({
        "recoveryState":"manual-fix-required",
        "recoveryReason":format!("Automatic source correction is unavailable. {}. This does not mean the source is unfixable or that the tool failed to execute.", reasons.into_iter().collect::<Vec<_>>().join("; ")),
        "nextAction":null,
        "nextActionReason":"Edit the source using the blocking violations' byteRange, rule and suggestion, then validate again with the same projectRoot and strict setting. Repeating unchanged arguments cannot resolve these findings."
    })
}

fn corrected_tsx(
    input: &UiValidateInput,
    report: &UiValidation,
    theme: Option<&ProjectTheme>,
) -> Option<String> {
    let theme = theme?;
    if report.ok
        || report
            .violations
            .iter()
            .any(|v| v.severity == Severity::Error)
    {
        return None;
    }
    let mut edits = Vec::new();
    for finding in report
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Warning)
    {
        let [start, end] = finding.byte_range;
        let literal = input.tsx.get(start..end)?;
        // These findings point to a complete JSX string literal. Escaped/entity
        // spellings that cannot be matched exactly remain a caller decision.
        let value = literal
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| {
                literal
                    .strip_prefix('\'')
                    .and_then(|s| s.strip_suffix('\''))
            })?;
        let matches = match finding.rule {
            "hardcoded-color" => theme.color_tokens_matching_hex(value),
            "hardcoded-length" => theme.length_tokens_matching_px(value),
            _ => return None,
        };
        let [token] = matches.as_slice() else {
            return None;
        };
        // Validation success alone cannot prove that a theme switch preserves
        // a literal. Only a unique token matching every mode is auto-correctable.
        if !finding
            .context
            .get("tokenMatches")
            .and_then(Value::as_array)
            .is_some_and(|entries| {
                entries
                    .iter()
                    .any(|e| e["token"] == format!("${token}") && e["allModesMatch"] == true)
            })
        {
            return None;
        }

        edits.push((
            start,
            end,
            serde_json::to_string(&format!("${token}")).ok()?,
        ));
    }
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|edit| edit.0);
    if edits.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return None;
    }
    let mut tsx = input.tsx.clone();
    for (start, end, replacement) in edits.into_iter().rev() {
        tsx.replace_range(start..end, &replacement);
    }
    validate_devup_ui_tsx(&tsx, Some(theme), input.strict)
        .ok
        .then_some(tsx)
}
