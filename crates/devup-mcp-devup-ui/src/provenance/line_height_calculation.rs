use super::SourceMap;

impl SourceMap {
    /// Explain unit conversions the generator performs, so a reader can
    /// reproduce `lineHeight="26px"` from the raw `{unit: PERCENT, value: 160}`.
    pub(crate) fn describe_conversions(&mut self, snapshot: &devup_mcp_figma::Snapshot) {
        for entry in &mut self.entries {
            if let (Some(id), Some(generated)) = (
                entry.node_id.as_deref(),
                entry.generated_property.as_deref(),
            ) && let Some(calculation) =
                crate::codegen::absolute_border_origin_calculation(snapshot, id, generated)
            {
                entry.calculation = Some(calculation);
            }
            if entry.property.as_deref() != Some("lineHeight") {
                continue;
            }
            let Some(node) = entry
                .node_id
                .as_deref()
                .and_then(|id| snapshot.nodes.get(id))
            else {
                continue;
            };
            let view = node.typed_view();
            let Some(line_height) = view.value("lineHeight") else {
                continue;
            };
            if line_height["unit"] != "PERCENT" {
                continue;
            }
            let percent =
                (line_height["value"].as_f64().unwrap_or_default() * 100.0).round() / 100.0;
            let bound = view
                .value("boundVariables")
                .and_then(|b| b.get("fontSize"))
                .is_some();
            entry.calculation = Some(
                if entry
                    .generated_property
                    .as_deref()
                    .is_some_and(|prop| prop.starts_with("lineHeight=\"round("))
                {
                    format!(
                        "round(nearest, {}em, 1px) rounds the effective CSS fontSize per mode; source percentage {percent}% is not frozen to captured fontSize",
                        percent / 100.0
                    )
                } else {
                    match (
                        view.value("fontSize").and_then(serde_json::Value::as_f64),
                        bound,
                    ) {
                        (Some(size), false) => format!(
                            "round(fontSize * percent / 100) = round({size} * {percent} / 100) = {}px",
                            (size * percent / 100.0).round()
                        ),
                        _ => format!(
                            "{percent}% kept as a percentage because fontSize is variable-bound or unknown"
                        ),
                    }
                },
            );
        }
    }
}
