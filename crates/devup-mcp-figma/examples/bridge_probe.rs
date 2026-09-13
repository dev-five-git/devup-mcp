//! 브리지를 실제 Figma 에 붙여 본다.
//!
//! 통합 테스트는 플러그인 자리에 WebSocket 클라이언트를 세우므로 그 너머 —
//! 생성된 스크립트가 진짜 문서를 읽는 부분 — 은 증명하지 못한다. 이 프로브는
//! 기본 포트로 브리지를 띄우고, Figma 에서 플러그인이 붙기를 기다린 뒤,
//! 실제 파일의 페이지 목록을 읽어 낸다.
//!
//! ```text
//! cargo run -p devup-mcp-figma --example bridge_probe
//! ```

use std::time::Duration;

use devup_mcp_figma::{
    BridgeFigmaClient, BridgeServer, DEFAULT_BRIDGE_PORT, FigmaUpstream, ReadToolCall,
};
use serde_json::Value;

#[tokio::main]
async fn main() {
    let server = match BridgeServer::start(DEFAULT_BRIDGE_PORT) {
        Some(server) => server,
        None => {
            eprintln!("PROBE_FAIL: port {DEFAULT_BRIDGE_PORT} is already taken");
            std::process::exit(1);
        }
    };
    println!("listening on ws://127.0.0.1:{}/plugin", server.port());
    println!("waiting for the Devup Bridge plugin in Figma...");

    // 플러그인을 가져오고 파일을 여는 데 시간이 걸리므로 넉넉히 기다린다.
    let state = server.state();
    let mut files: Vec<String> = Vec::new();
    for _ in 0..3000 {
        files = state.connected_files().await;
        if !files.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let Some(file_key) = files.first().cloned() else {
        eprintln!("PROBE_FAIL: no plugin connected within 10 minutes");
        std::process::exit(1);
    };
    println!("plugin connected for file {file_key}");

    // 페이지 목록은 노드 id 를 몰라도 읽을 수 있어 첫 확인에 맞다.
    let client = BridgeFigmaClient::new(state);
    let result = match client
        .call_read_tool(ReadToolCall::page_catalog(&file_key))
        .await
    {
        Ok(result) => result,
        Err(error) => {
            eprintln!("PROBE_FAIL: {:?} / {}", error.code, error.message);
            std::process::exit(1);
        }
    };

    let text = result.raw["content"][0]["text"]
        .as_str()
        .expect("the payload rides in content[].text");
    let payload: Value = serde_json::from_str(text).expect("the carried text is JSON");

    let pages = payload["nodes"].as_array().map_or(0, Vec::len);
    println!("PROBE_OK fileKey={} pages={pages}", payload["fileKey"]);
    for node in payload["nodes"].as_array().into_iter().flatten() {
        println!("  {} {}", node["id"], node["fields"]["name"]);
    }
}
