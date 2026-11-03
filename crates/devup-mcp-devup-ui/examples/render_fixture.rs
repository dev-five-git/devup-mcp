//! Replay a snapshot through production codegen for the browser regression test.
use std::{error::Error, path::Path};

use devup_mcp_devup_ui::codegen::{
    CodegenOptions, generate_component, node_ids::annotate_node_ids,
};
use devup_mcp_figma::Snapshot;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 && !(args.len() == 3 && args[2] == "--responsive") {
        return Err("usage: render_fixture <snapshot.json> <output.tsx>".into());
    }
    let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let options = CodegenOptions {
        component_name: Some("RenderFixture".into()),
        inline_instances: true,
        ..Default::default()
    };
    if args.len() == 3 {
        let mut merged = devup_mcp_devup_ui::codegen::responsive::merge_confirmed_breakpoints(
            &snapshot, &options,
        )?
        .ok_or("responsive fixture requires two frames")?;
        devup_mcp_devup_ui::codegen::node_ids::annotate_merged(&mut merged);
        let imports = merged
            .components
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!(
            "import {{ {imports} }} from '@devup-ui/react';\nexport function RenderFixture() {{ return ({}); }}\n",
            merged.tsx
        );
        std::fs::write(Path::new(&args[1]), source)?;
        return Ok(());
    }
    let mut output = generate_component(&snapshot, &snapshot.roots[0], &options)?;
    annotate_node_ids(&mut output, &snapshot.nodes.keys().cloned().collect());
    std::fs::write(Path::new(&args[1]), output.tsx)?;
    Ok(())
}
