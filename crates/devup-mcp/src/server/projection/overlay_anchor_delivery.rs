fn attach_overlay_anchor_delivery(
    snapshot: &devup_mcp_figma::Snapshot,
    roots: &[String],
    tsx: &str,
    output: &str,
    result: &mut Map<String, Value>,
) {
    let rows = devup_mcp_devup_ui::codegen::overlay_anchor_metadata(snapshot, roots, tsx);
    for row in rows {
        let mut scoped = row;
        if let Some(object) = scoped.as_object_mut() {
            object.insert("output".into(), json!(output));
        }
        let delivered = result.entry("overlayAnchors").or_insert_with(|| json!([]));
        if let Some(delivered) = delivered.as_array_mut()
            && !delivered.contains(&scoped)
        {
            delivered.push(scoped);
        }
    }
}

#[cfg(test)]
mod overlay_anchor_delivery_tests {
    use super::*;
    use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};

    fn snapshot() -> devup_mcp_figma::Snapshot {
        serde_json::from_str(include_str!(
            "../../../../devup-mcp-devup-ui/tests/fixtures/chondrogen_dropdown_anchor_nodes.json"
        )).expect("captured Contact dropdown nodes")
    }

    #[test]
    fn ordinary_delivery_exposes_real_anchor_proof_without_duplicate_comments() {
        let snapshot = snapshot();
        let roots = vec![snapshot.roots[0].clone()];
        let output = generate_component(
            &snapshot, &roots[0],
            &CodegenOptions { inline_instances: true, ..Default::default() },
        ).unwrap();
        let comment_count = output.tsx.matches("// @devup-overlay-anchor ").count();
        assert_eq!(comment_count, 0);
        let mut result = Map::new();
        attach_overlay_anchor_delivery(&snapshot, &roots, &output.tsx, "tsx", &mut result);
        let rows = result["overlayAnchors"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["projected"], true);
        assert_eq!(rows[0]["runtimeVerified"], false);
        assert_eq!(rows[0]["output"], "tsx");
        assert_eq!(output.tsx.matches("// @devup-overlay-anchor ").count(), comment_count);
    }

    #[test]
    fn responsive_delivery_keeps_each_captured_widths_anchor_in_the_written_module() {
        let snapshot = snapshot();
        let merged = devup_mcp_devup_ui::codegen::responsive::merge_confirmed_breakpoints(
            &snapshot,
            &CodegenOptions { inline_instances: true, ..Default::default() },
        ).unwrap().unwrap();
        let module = merged.module("ContactPage");
        let mut result = Map::new();
        attach_overlay_anchor_delivery(
            &snapshot, &snapshot.roots, &module, "responsiveTsx", &mut result,
        );
        let rows = result["overlayAnchors"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row["projected"] == true && row["runtimeVerified"] == false));
        assert_eq!(module.matches("// @devup-overlay-anchor ").count(), 0);
        assert!(devup_mcp_devup_ui::validation::validate_tsx(&module).is_ok());
    }
}
