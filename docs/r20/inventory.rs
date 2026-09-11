//! Read-only R20 investigation: replay every supplied snapshot and list evidence.
use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{CollectedPayload, FidelityImpact, Snapshot, UpstreamResult};
use serde_json::{Value, json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = Vec::new();
    for path in std::env::args().skip(1) {
        let value: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        let raw = value.get("snapshot").unwrap_or(&value);
        if raw.get("nodes").is_none() || raw.get("roots").is_none() {
            continue;
        }
        let snapshot: Snapshot = serde_json::from_value(raw.clone())?;
        let payload = serde_json::from_value::<CollectedPayload>(value.clone()).ok();
        let resources = value
            .get("resources")
            .and_then(|v| serde_json::from_value::<UpstreamResult>(v.clone()).ok());
        let mut roots = snapshot.roots.clone();
        for root in &snapshot.roots {
            if let Some(n) = snapshot.nodes.get(root)
                && n.node_type == "SECTION"
            {
                roots.extend(n.typed_view().child_ids().map(str::to_owned));
            }
        }
        roots.sort();
        roots.dedup();
        for root in roots {
            for inline in [false, true] {
                let mut options = CodegenOptions {
                    inline_instances: inline,
                    include_diagnostics: true,
                    ..Default::default()
                };
                if let Some(p) = &payload {
                    options = options.with_payload_tokens(p);
                } else if let Some(r) = &resources {
                    options = options.with_resource_results(Some(r), Some(r));
                }
                match generate_component(&snapshot, &root, &options) {
                    Ok(output) => {
                        let diagnostics: Vec<_> = output.diagnostics.iter().filter(|d| {
                            d.fidelity_impact() == FidelityImpact::Approximated
                                || d.details.as_ref().is_some_and(|v| v["classification"] == "text-auto-size")
                                || d.code == "DEVUP_CODEGEN_LAYOUT_ACCOUNTED_FOR"
                        }).collect();
                        rows.push(json!({"fixture":path,"root":root,"inlineInstances":inline,
                            "nodes":snapshot.nodes.len(),"impacts":output.fidelity_report.impacts,
                            "diagnostics":diagnostics}));
                    }
                    Err(error) => rows.push(json!({"fixture":path,"root":root,"inlineInstances":inline,"error":error.to_string()})),
                }
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
