#[test]
fn stale_plugin_warning_survives_collection_without_affecting_fidelity() {
    for hash in [Some(devup_mcp_figma::PLUGIN_SCRIPTS_HASH), Some("old-build"), None] {
        let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
        request.resource_scope = ResourceScope::Used;
        let mut collector = CollectorSession::new(request);
        let CollectorStep::Call(call) = collector.advance().unwrap() else {
            panic!("fast snapshot call expected")
        };
        let mut result = fast_envelope_result();
        result.raw["_meta"] = json!({"devup/bridge": {
            "reads": 1, "scriptsHash": hash, "protocol": devup_mcp_figma::PLUGIN_PROTOCOL
        }});
        collector.accept(&call.id, result).unwrap();
        let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
            panic!("fast snapshot should complete")
        };
        let snapshot = merge_chunks(parts.snapshot_chunks).unwrap();
        let warnings = snapshot.diagnostics.iter()
            .filter(|issue| issue.code == "DEVUP_BRIDGE_PLUGIN_STALE")
            .collect::<Vec<_>>();
        if hash == Some(devup_mcp_figma::PLUGIN_SCRIPTS_HASH) {
            assert!(warnings.is_empty());
        } else {
            assert_eq!(warnings.len(), 1);
            assert_eq!(warnings[0].fidelity_impact(), devup_mcp_figma::FidelityImpact::None);
            assert_eq!(warnings[0].details.as_ref().unwrap()["pluginStale"], true);
        }
    }
}
