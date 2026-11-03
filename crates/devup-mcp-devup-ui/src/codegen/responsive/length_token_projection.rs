fn css_length_slot(width: f64) -> usize {
    [480.0, 768.0, 992.0, 1280.0, 1600.0]
        .iter()
        .filter(|min| width >= **min)
        .count()
}

fn length_property(name: &str) -> bool {
    matches!(
        name,
        "p" | "px"
            | "py"
            | "pt"
            | "pr"
            | "pb"
            | "pl"
            | "gap"
            | "rowGap"
            | "columnGap"
            | "borderRadius"
            | "fontSize"
    )
}

fn known_length<'a>(
    value: &str,
    tokens: &'a BTreeMap<String, super::component::LengthToken>,
) -> Option<&'a Value> {
    let name = value
        .strip_prefix('$')
        .or_else(|| value.strip_prefix("var(--")?.strip_suffix(')'))?;
    tokens
        .values()
        .find(|token| token.token == name)
        .map(|token| &token.value)
}

fn length_token_at(value: &Value, slot: usize) -> Option<&str> {
    match value {
        Value::Array(values) => values
            .iter()
            .take(slot + 1)
            .rev()
            .find(|value| !value.is_null())?
            .as_str(),
        value => value.as_str(),
    }
}

fn resolved_length_value(
    value: Option<String>,
    slot: usize,
    tokens: &BTreeMap<String, super::component::LengthToken>,
) -> Option<String> {
    value.map(|value| {
        known_length(&value, tokens)
            .and_then(|token| length_token_at(token, slot))
            .map_or_else(|| value.clone(), str::to_owned)
    })
}

// JSX token syntax is retained at every responsive slot. A compiler-specific
// extraction caveat is evidence to verify, not permission to emit raw CSS vars.
fn project_nested_length_tokens(
    property: &str,
    merged: &mut Merged,
    node_id: &str,
    context: &mut MergeContext<'_>,
) {
    if !length_property(property) { return; }
    let Merged::Array(slots) = merged else { return; };
    for value in slots.iter_mut().flatten() {
        if let Some(name) = value.strip_prefix("var(--").and_then(|v| v.strip_suffix(')'))
            && known_length(value, context.length_tokens).is_some()
        {
            *value = format!("${name}");
        }
    }
    let equivalent = slots.iter().flatten().filter(|value| value.starts_with('$')).find(|candidate| {
        let Some(theme) = known_length(candidate, context.length_tokens) else { return false; };
        let last = theme.as_array().map_or(6, |values| values.len().max(6));
        (0..last).all(|slot| {
            let Some(captured) = slots.iter().take(slot + 1).rev().flatten().next() else { return false; };
            resolved_length_value(Some(captured.clone()), slot, context.length_tokens).as_deref() == length_token_at(theme, slot)
        })
    }).cloned();
    if let Some(token) = equivalent {
        *merged = Merged::Same(Some(token));
        return;
    }
    if !slots.iter().flatten().any(|value| value.starts_with('$') && known_length(value, context.length_tokens).is_some_and(Value::is_array)) { return; }
    context.diagnostics.push(devup_mcp_figma::Diagnostic {
        code: "DEVUP_CODEGEN_RESPONSIVE_LENGTH_TOKEN_PROJECTED".into(),
        message: "Responsive JSX keeps $token references. Verify computed styles at every theme breakpoint with the installed devup-ui compiler; source syntax alone does not verify CSS extraction.".into(),
        node_id: Some(node_id.to_owned()), property: Some(property.to_owned()),
        severity: Some(devup_mcp_figma::DiagnosticSeverity::Warning),
        fidelity_impact: Some(devup_mcp_figma::FidelityImpact::None),
        details: Some(json!({"output":"responsiveTsx","projectedSlots":slots,"bindingPreserved":true,"runtimeVerified":false,"scope":"Captured tokens are retained in JSX. Nested responsive extraction needs consumer-browser validation."})),
        ..Default::default()
    });
}
