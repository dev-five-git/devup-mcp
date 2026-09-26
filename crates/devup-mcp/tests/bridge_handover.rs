//! Real devup-mcp processes sharing one bridge port, and passing it on.
//!
//! Three stdio servers built from this target run on a port picked for the
//! test - never 1993, which belongs to whatever sessions this machine runs -
//! with their home directory pointed at a scratch one, so whatever they keep
//! per user is the test's own. A stand-in plugin attaches the way the real one
//! does: after its socket closes it waits two seconds and connects again
//! (`RETRY_MS` in plugin/src/ui.ts).
//!
//! Only url-less reads are made. They go to the attached plugin or nowhere, so
//! nothing here can reach the metered direct path or need a credential store.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Generous on purpose: a slow runner must not decide the outcome.
const DEADLINE: Duration = Duration::from_secs(30);

/// The real plugin's reconnect interval (plugin/src/ui.ts `RETRY_MS`).
const PLUGIN_RETRY: Duration = Duration::from_secs(2);

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Server {
    async fn spawn(port: u16, home: &Path) -> anyhow::Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_devup-mcp"))
            .current_dir(home)
            .env("DEVUP_FIGMA_BRIDGE_PORT", port.to_string())
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("DEVUP_MCP_NO_UPDATE_CHECK", "1")
            .env("DEVUP_MCP_SKILLS_OFFLINE", "1")
            // Every read is paced to Figma's metered allowance, bridge reads
            // included, which would make this test wait out a minute between
            // exports. It measures relaying, not pacing.
            .env("DEVUP_FIGMA_CALLS_PER_MINUTE", "1000")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        let mut server = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        server
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "devup-mcp-bridge-handover", "version": "1" }
                }),
            )
            .await?;
        server
            .send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await?;
        Ok(server)
    }

    async fn send(&mut self, value: Value) -> anyhow::Result<()> {
        self.stdin.write_all(value.to_string().as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;
        loop {
            let mut line = String::new();
            timeout(DEADLINE, self.stdout.read_line(&mut line)).await??;
            anyhow::ensure!(!line.is_empty(), "the server closed before answering {id}");
            let value: Value = serde_json::from_str(&line)?;
            if value["id"] == id {
                return Ok(value);
            }
        }
    }

    /// Calls a tool and returns `(isError, structuredContent)`, following an
    /// export job to completion as a client does.
    async fn tool(&mut self, name: &str, arguments: Value) -> anyhow::Result<(bool, Value)> {
        let mut response = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        for _ in 0..500 {
            let content = &response["result"]["structuredContent"];
            let Some(job) = content["exportJob"]["jobId"]
                .as_str()
                .filter(|_| content["exportJob"]["state"] == "running")
                .map(str::to_owned)
            else {
                break;
            };
            response = self
                .request(
                    "tools/call",
                    json!({ "name": "devup_figma_export", "arguments": { "jobId": job } }),
                )
                .await?;
        }
        let result = &response["result"];
        Ok((
            result["isError"] == true,
            result["structuredContent"].clone(),
        ))
    }

    /// A url-less search, answered only if this process reads the attached
    /// plugin - through its own port or through the process holding it.
    async fn searches_through_the_bridge(&mut self) -> anyhow::Result<bool> {
        let (failed, output) = self
            .tool("devup_figma_search", json!({ "query": "syntheticframe" }))
            .await?;
        Ok(!failed
            && output["source"]["kind"] == "bridge"
            && output["matches"][0]["nodeId"] == "1:2")
    }

    async fn exports_through_the_bridge(&mut self) -> anyhow::Result<Value> {
        let (failed, output) = self
            .tool("devup_figma_export", json!({ "outputs": ["tsx"] }))
            .await?;
        anyhow::ensure!(!failed, "{output}");
        Ok(output)
    }
}

fn scratch_home() -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "devup-bridge-handover-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&home).expect("a scratch home");
    home
}

/// A port nothing listens on right now, and never the plugin's own.
fn free_port() -> u16 {
    loop {
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .and_then(|listener| listener.local_addr())
            .expect("an ephemeral port")
            .port();
        if port != 1993 {
            return port;
        }
    }
}

async fn listening(port: u16) -> bool {
    tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_ok()
}

fn frame_node() -> Value {
    json!({
        "id": "1:2", "type": "FRAME",
        "fields": {
            "name": "Synthetic Frame", "parentId": "0:1", "childrenIds": [],
            "layoutMode": "VERTICAL",
            "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
            "width": 320, "height": 240
        },
        "extra": {}, "fieldErrors": {}
    })
}

fn script_answer(script: &str) -> Result<Value, &'static str> {
    let page = json!({
        "id": "0:1", "type": "PAGE",
        "fields": { "name": "Page 1", "parentId": null, "childrenIds": ["1:2"] },
        "extra": {}, "fieldErrors": {}
    });
    match script {
        "pageCatalog" => Ok(json!({
            "fileKey": "", "version": null, "rootIds": ["0:1"], "nodes": [page], "diagnostics": []
        })),
        "search" => Ok(json!({
            "fileKey": "", "version": null, "rootIds": ["0:1"],
            "nodes": [page, frame_node()], "diagnostics": []
        })),
        "metadata" => Ok(json!({
            "fileKey": "", "version": null, "rootId": "1:2",
            "nodes": [{
                "id": "1:2", "type": "FRAME", "name": "Synthetic Frame",
                "childrenIds": [], "descendantCount": 0
            }]
        })),
        "fastSnapshot" | "snapshot" => Ok(json!({
            "fileKey": "", "version": null, "rootIds": ["1:2"], "nodes": [frame_node()]
        })),
        _ => Err("DEVUP_TEST_SCRIPT_NOT_SIMULATED"),
    }
}

/// The plugin as plugin/src/ui.ts behaves: connect, say hello, answer jobs,
/// and after the socket closes wait `RETRY_MS` before connecting again. Every
/// time a connection opens is recorded.
fn stand_in_plugin(port: u16) -> Arc<Mutex<Vec<Instant>>> {
    let attached = Arc::new(Mutex::new(Vec::new()));
    let log = attached.clone();
    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) =
                connect_async(format!("ws://localhost:{port}/plugin")).await
            {
                log.lock().unwrap().push(Instant::now());
                let hello = json!({
                    "kind": "hello", "fileKey": null, "fileName": "Landing",
                    "currentPage": { "id": "0:1", "name": "Page 1" },
                    "selection": [{ "id": "1:2", "name": "Synthetic Frame", "type": "FRAME" }],
                    "selectionCount": 1,
                });
                if socket
                    .send(Message::Text(hello.to_string().into()))
                    .await
                    .is_ok()
                {
                    while let Some(Ok(message)) = socket.next().await {
                        let Message::Text(text) = message else {
                            continue;
                        };
                        let Ok(job) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        let answer = match script_answer(job["script"].as_str().unwrap_or_default())
                        {
                            Ok(data) => json!({
                                "kind": "devup-result", "requestId": job["requestId"], "data": data
                            }),
                            Err(error) => json!({
                                "kind": "devup-result", "requestId": job["requestId"], "error": error
                            }),
                        };
                        if socket
                            .send(Message::Text(answer.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            tokio::time::sleep(PLUGIN_RETRY).await;
        }
    });
    attached
}

/// How long until the port accepts connections again.
async fn until_listening(port: u16) -> anyhow::Result<Duration> {
    let started = Instant::now();
    while started.elapsed() < DEADLINE {
        if listening(port).await {
            return Ok(started.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("nothing listened on port {port} within {DEADLINE:?}")
}

/// How long until every one of `servers` answers a url-less read through the
/// bridge.
async fn until_all_read_through_the_bridge(
    what: &str,
    servers: &mut [&mut Server],
) -> anyhow::Result<Duration> {
    let started = Instant::now();
    'retry: while started.elapsed() < DEADLINE {
        for server in servers.iter_mut() {
            if !server.searches_through_the_bridge().await? {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue 'retry;
            }
        }
        return Ok(started.elapsed());
    }
    anyhow::bail!("{what} did not read through the bridge within {DEADLINE:?}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_process_reads_through_the_bridge_and_the_port_passes_on_when_its_holder_exits()
-> anyhow::Result<()> {
    let home = scratch_home();
    let port = free_port();

    let mut first = Server::spawn(port, &home).await?;
    until_listening(port).await?;
    let mut second = Server::spawn(port, &home).await?;
    let mut third = Server::spawn(port, &home).await?;
    let plugin = stand_in_plugin(port);

    for (name, server) in [
        ("the port's holder", &mut first),
        ("the second process", &mut second),
        ("the third process", &mut third),
    ] {
        until_all_read_through_the_bridge(name, &mut [&mut *server]).await?;
        let output = server.exports_through_the_bridge().await?;
        assert_eq!(output["source"]["kind"], "bridge", "{name}: {output}");
        assert_eq!(output["source"]["bridgePort"], port, "{name}: {output}");
        assert!(
            output["tsx"]
                .as_str()
                .is_some_and(|tsx| tsx.contains("SyntheticFrame")),
            "{name}: {output}"
        );
    }

    // The session that held the port ends.
    let attached_before = plugin.lock().unwrap().len();
    let ended = Instant::now();
    first.child.kill().await?;

    let rebound = until_listening(port).await?;
    let read_again = until_all_read_through_the_bridge(
        "the two remaining processes",
        &mut [&mut second, &mut third],
    )
    .await?;
    let reattached = plugin
        .lock()
        .unwrap()
        .get(attached_before)
        .map(|at| at.duration_since(ended))
        .expect("the plugin attached again");

    // One of the two holds the port now; the other reads through it. The OS
    // lets only one of them bind it, and both read.
    assert!(
        std::net::TcpListener::bind(("127.0.0.1", port)).is_err(),
        "the port is held again"
    );
    for server in [&mut second, &mut third] {
        let output = server.exports_through_the_bridge().await?;
        assert_eq!(output["source"]["kind"], "bridge", "{output}");
    }

    eprintln!(
        "bridge handover measured: port bound again {rebound:?} after its holder was killed; \
         the stand-in plugin (retrying every {PLUGIN_RETRY:?}) re-attached after {reattached:?}; \
         both remaining processes read through the bridge after {:?}",
        rebound + read_again
    );
    assert!(
        rebound + read_again < DEADLINE,
        "the handover must finish within {DEADLINE:?}"
    );

    drop((second, third));
    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}
