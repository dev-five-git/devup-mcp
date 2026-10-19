//! Replay a snapshot through production codegen for the browser regression test.
use std::{error::Error, path::Path};

use devup_mcp_devup_ui::codegen::{
    CodegenOptions, generate_component, node_ids::annotate_node_ids,
};
use devup_mcp_figma::Snapshot;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: render_fixture <snapshot.json> <output.tsx>".into());
    }
    let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let options = CodegenOptions {
        component_name: Some("RenderFixture".into()),
        ..Default::default()
    };
    let mut output = generate_component(&snapshot, &snapshot.roots[0], &options)?;
    annotate_node_ids(&mut output, &snapshot.nodes.keys().cloned().collect());
    std::fs::write(Path::new(&args[1]), output.tsx)?;
    Ok(())
}
