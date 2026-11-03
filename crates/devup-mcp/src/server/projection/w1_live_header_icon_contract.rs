fn assert_r4_live_header_icon_contract(snapshot: &devup_mcp_figma::Snapshot) {
    let probe = devup_mcp_devup_ui::codegen::generate_component(
        snapshot,
        "3997:46582",
        &devup_mcp_devup_ui::codegen::CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    // A 50px vertically FILL header wrapper must retain its fixed 32px icon,
    // not export the whole wrapper as a scalable 50px picture. The child's
    // two independently represented dimensions add two covered obligations.
    let icon = "I3997:46589;65:3063";
    for field in ["width", "height"] {
        assert!(
            probe.source_map.entries.iter().any(|entry| {
                entry.node_id.as_deref() == Some(icon)
                    && entry.property.as_deref() == Some(field)
                    && entry.generated_range.is_some()
            }),
            "The live icon must retain its own {field} source mapping"
        );
    }
}
