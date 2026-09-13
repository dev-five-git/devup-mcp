//! Capture an original upload through the updated, read-only bridge plugin.
//! Run with: FILE_KEY NODE_ID FILL_INDEX IMAGE_HASH OUTPUT_PATH.
//! The selected bridge port must be free and match the plugin configuration.

use std::{io::Write, time::Duration};

use devup_mcp_figma::{
    AssetRequest, BridgeFigmaClient, BridgeServer, FigmaUpstream, ReadToolCall,
    original_image_from_result,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        args.len() == 5,
        "usage: original_image_probe FILE_KEY NODE_ID FILL_INDEX IMAGE_HASH OUTPUT_PATH"
    );
    let fill_index = args[2].parse::<usize>()?;
    let server = BridgeServer::from_env().ok_or_else(|| anyhow::anyhow!(
        "bridge port unavailable or disabled; stop its current owner or configure the same free port in both plugin and DEVUP_FIGMA_BRIDGE_PORT"
    ))?;
    eprintln!(
        "Waiting up to 60 seconds for the updated plugin on port {}",
        server.port()
    );
    let state = server.state();
    tokio::time::timeout(Duration::from_secs(60), async {
        while !state.has_plugin(&args[0]).await {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("updated bridge plugin did not connect"))?;
    let request = AssetRequest::original_image(&args[1], fill_index, &args[3]);
    let response = BridgeFigmaClient::new(state)
        .call_read_tool(ReadToolCall::asset_export(&args[0], None, request.clone()))
        .await?;
    let original = original_image_from_result(&response, &args[0], None, &request)?;
    // Refuse to overwrite an earlier capture; its byte identity is evidence.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[4])?;
    file.write_all(&original.bytes)?;
    println!(
        "{}",
        serde_json::json!({
            "representation":"original-image-v1", "fileKey":args[0], "nodeId":args[1],
            "fillIndex":fill_index, "imageHash":args[3], "version":null,
            "mimeType":original.mime_type, "width":original.width, "height":original.height,
            "byteLength":original.bytes.len(), "sha256":original.sha256, "outputPath":args[4],
            "note":"Original upload only; no isolated-fill or document-version parity is claimed."
        })
    );
    Ok(())
}
