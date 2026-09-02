use std::{process::Stdio, time::Duration};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdout, Command},
    time::timeout,
};

async fn response(reader: &mut BufReader<ChildStdout>, id: u64) -> anyhow::Result<Value> {
    loop {
        let mut line = String::new();
        timeout(Duration::from_secs(10), reader.read_line(&mut line)).await??;
        anyhow::ensure!(!line.is_empty(), "stdio server closed before response {id}");
        let value: Value = serde_json::from_str(&line)?;
        if value["id"] == id {
            return Ok(value);
        }
    }
}

async fn send(stdin: &mut tokio::process::ChildStdin, value: Value) -> anyhow::Result<()> {
    stdin.write_all(value.to_string().as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

#[tokio::test]
async fn fresh_binary_initializes_lists_tools_and_reports_auth_status() -> anyhow::Result<()> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_devup-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut stderr = child.stderr.take().unwrap();

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "devup-mcp-smoke", "version": "1"}
            }
        }),
    )
    .await?;
    let initialized = response(&mut stdout, 1).await?;
    assert_eq!(initialized["result"]["serverInfo"]["name"], "devup-mcp");

    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await?;
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    )
    .await?;
    let tools = response(&mut stdout, 2).await?;
    let names = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"devup_figma_auth"));
    assert!(names.contains(&"devup_figma_explore"));

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "devup_figma_auth", "arguments": {"action": "status"}}
        }),
    )
    .await?;
    let auth = response(&mut stdout, 3).await?;
    assert!(auth.get("result").is_some() || auth.get("error").is_some());

    drop(stdin);
    let status = timeout(Duration::from_secs(10), child.wait()).await??;
    assert!(status.success());
    let mut stderr_bytes = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut stderr_bytes).await?;
    let stderr = String::from_utf8_lossy(&stderr_bytes).to_ascii_lowercase();
    for forbidden in ["access_token", "refresh_token", "client_secret"] {
        assert!(!stderr.contains(forbidden));
    }
    Ok(())
}
