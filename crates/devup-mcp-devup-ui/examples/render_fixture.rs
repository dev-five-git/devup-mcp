//! Replay a snapshot through production codegen for the browser regression test.
use std::{error::Error, path::Path};

use devup_mcp_devup_ui::codegen::{
    CodegenOptions, generate_component, node_ids::annotate_node_ids,
};
use devup_mcp_figma::Snapshot;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("usage: render_fixture <snapshot.json> <output.tsx>".into());
    }
    let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let mut options = CodegenOptions {
        component_name: Some("RenderFixture".into()),
        inline_instances: true,
        ..Default::default()
    };
    if let Some(index) = args.iter().position(|arg| arg == "--theme") {
        let file = args
            .get(index + 1)
            .ok_or("--theme needs a binding fixture")?;
        let theme: serde_json::Value = serde_json::from_slice(&std::fs::read(file)?)?;
        options.variable_tokens = serde_json::from_value(theme["variableTokens"].clone())?;
        for (id, value) in theme["lengthTokens"]
            .as_object()
            .ok_or("lengthTokens must be an object")?
        {
            options.length_tokens.insert(
                id.clone(),
                devup_mcp_devup_ui::codegen::LengthToken {
                    token: value["token"].as_str().ok_or("token name missing")?.into(),
                    value: value["value"].clone(),
                },
            );
        }
    }
    let clean = args.iter().any(|arg| arg == "--clean");
    if let Some(index) = args.iter().position(|arg| arg == "--proof") {
        let target = args.get(index + 1).ok_or("--proof needs a destination")?;
        let mut proof = String::new();
        for root in &snapshot.roots {
            proof.push_str(&format!(
                "{root}\n{:#?}\n",
                generate_component(&snapshot, root, &options)?
            ));
        }
        if let Some(merged) = devup_mcp_devup_ui::codegen::responsive::merge_confirmed_breakpoints(
            &snapshot, &options,
        )? {
            proof.push_str(&format!("{merged:#?}"));
        }
        std::fs::write(target, proof)?;
    }
    if args.iter().any(|arg| arg == "--responsive") {
        let mut merged = devup_mcp_devup_ui::codegen::responsive::merge_confirmed_breakpoints(
            &snapshot, &options,
        )?
        .ok_or("responsive fixture requires two frames")?;
        if !clean {
            devup_mcp_devup_ui::codegen::node_ids::annotate_merged(&mut merged);
        }
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
    if !clean {
        annotate_node_ids(&mut output, &snapshot.nodes.keys().cloned().collect());
    }
    std::fs::write(Path::new(&args[1]), output.tsx)?;
    Ok(())
}
