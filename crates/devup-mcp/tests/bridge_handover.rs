//! Real devup-mcp processes sharing one bridge port, and passing it on.
//!
//! Stdio servers built from this target run on a port picked for the test -
//! never 1993, which belongs to whatever sessions this machine runs. Each has
//! a `HOME` of its own, the way MCP clients hand their servers different ones
//! (some point it at a sandbox), and they still have to find each other. The
//! Windows user profile, which the system sets per user, is one scratch
//! directory they share, so the relay secret kept there is the test's own; on
//! Unix the secret follows the user id rather than the environment. A stand-in
//! plugin attaches the way the real one does: it names its window with a
//! session id, and after its socket closes it waits two seconds and connects
//! again (`RETRY_MS` in plugin/src/ui.ts).
//!
//! Only url-less reads are made. They go to the attached plugin or nowhere, so
//! nothing here can reach the metered direct path or need a credential store -
//! and nothing is held to the metered pace, which only that path's reads are.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
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
    /// A devup-mcp the way an MCP client starts one: `home` is its own,
    /// `profile` the user's.
    async fn spawn(port: u16, home: &Path, profile: &Path) -> anyhow::Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_devup-mcp"))
            .current_dir(home)
            .env("DEVUP_FIGMA_BRIDGE_PORT", port.to_string())
            .env("HOME", home)
            .env("USERPROFILE", profile)
            .env("DEVUP_MCP_NO_UPDATE_CHECK", "1")
            .env("DEVUP_MCP_SKILLS_OFFLINE", "1")
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
    /// plugin - through its own port or through the process holding it. A
    /// search has no `refresh`, so only a process's first one is sure to reach
    /// the plugin: the plugin keeps its key across reconnects, and a later one
    /// may be answered from this process's cache.
    async fn searches_through_the_bridge(&mut self) -> anyhow::Result<bool> {
        let (failed, output) = self
            .tool("devup_figma_search", json!({ "query": "syntheticframe" }))
            .await?;
        Ok(!failed
            && output["source"]["kind"] == "bridge"
            && output["matches"][0]["nodeId"] == "1:2")
    }

    /// A url-less export that goes all the way to the plugin, whatever this
    /// process has cached.
    async fn export(&mut self) -> anyhow::Result<(bool, Value)> {
        self.tool(
            "devup_figma_export",
            json!({ "outputs": ["tsx"], "refresh": true }),
        )
        .await
    }

    /// Whether this process reads the attached plugin right now.
    async fn reads_through_the_bridge(&mut self) -> anyhow::Result<bool> {
        let (failed, output) = self.export().await?;
        Ok(!failed && output["source"]["kind"] == "bridge")
    }

    async fn exports_through_the_bridge(&mut self) -> anyhow::Result<Value> {
        let (failed, output) = self.export().await?;
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

/// The plugin as plugin/src/ui.ts behaves: connect, say hello naming its
/// window, answer jobs, and after the socket closes wait `RETRY_MS` before
/// connecting again. Keyless, as in Dev Mode.
#[derive(Clone)]
struct StandIn {
    /// When each connection opened.
    attached: Arc<Mutex<Vec<Instant>>>,
    /// Leave the next job unanswered - a read the plugin is still working on.
    hold_next: Arc<AtomicBool>,
    held: Arc<AtomicBool>,
}

impl StandIn {
    fn attach(port: u16) -> Self {
        let plugin = Self {
            attached: Arc::default(),
            hold_next: Arc::default(),
            held: Arc::default(),
        };
        // One plugin window: the same name on every connection it makes.
        let session = format!("{:032x}", rand::random::<u128>());
        let this = plugin.clone();
        tokio::spawn(async move {
            loop {
                // The real plugin has to say `localhost` (Figma refuses 127.0.0.1
                // in `allowedDomains`), and a browser tries both address families
                // at once. A plain connect tries ::1 first, and Windows spends two
                // seconds being refused there before trying 127.0.0.1 - time the
                // plugin never loses, so it is left out of what this measures.
                if let Ok((mut socket, _)) =
                    connect_async(format!("ws://127.0.0.1:{port}/plugin")).await
                {
                    this.attached.lock().unwrap().push(Instant::now());
                    let hello = json!({
                        "kind": "hello", "sessionId": session, "fileKey": null,
                        "fileName": "Landing",
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
                            if job["kind"] != "devup-job" {
                                continue;
                            }
                            if this.hold_next.swap(false, Ordering::SeqCst) {
                                this.held.store(true, Ordering::SeqCst);
                                continue;
                            }
                            let answer =
                                match script_answer(job["script"].as_str().unwrap_or_default()) {
                                    Ok(data) => json!({
                                        "kind": "devup-result", "requestId": job["requestId"],
                                        "data": data
                                    }),
                                    Err(error) => json!({
                                        "kind": "devup-result", "requestId": job["requestId"],
                                        "error": error
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
        plugin
    }

    /// Keep the next read waiting, as a slow script does.
    fn hold_next_read(&self) {
        self.hold_next.store(true, Ordering::SeqCst);
    }

    /// Until the held read has arrived.
    async fn until_holding(&self) -> anyhow::Result<()> {
        let started = Instant::now();
        while !self.held.load(Ordering::SeqCst) {
            anyhow::ensure!(
                started.elapsed() < DEADLINE,
                "no read reached the plugin within {DEADLINE:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }
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
            if !server.reads_through_the_bridge().await? {
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
    let profile = scratch_home();
    let homes = [scratch_home(), scratch_home(), scratch_home()];
    let port = free_port();

    let mut first = Server::spawn(port, &homes[0], &profile).await?;
    until_listening(port).await?;
    let mut second = Server::spawn(port, &homes[1], &profile).await?;
    let mut third = Server::spawn(port, &homes[2], &profile).await?;
    let plugin = StandIn::attach(port);

    for (name, server) in [
        ("the port's holder", &mut first),
        ("the second process", &mut second),
        ("the third process", &mut third),
    ] {
        until_all_read_through_the_bridge(name, &mut [&mut *server]).await?;
        assert!(
            server.searches_through_the_bridge().await?,
            "{name} searches through the bridge"
        );
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
    let attached_before = plugin.attached.lock().unwrap().len();
    let ended = Instant::now();
    first.child.kill().await?;

    let rebound = until_listening(port).await?;
    let read_again = until_all_read_through_the_bridge(
        "the two remaining processes",
        &mut [&mut second, &mut third],
    )
    .await?;
    let reattached = plugin
        .attached
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
    for directory in homes.iter().chain([&profile]) {
        let _ = std::fs::remove_dir_all(directory);
    }
    Ok(())
}

/// A collection is under way through the port's holder when that session
/// ends, with the plugin still working on one of its reads. That read is
/// sent again once the plugin is back - here through the process that took
/// the port over - and the export finishes. It used to fail on the spot, and
/// the caller had to start the collection over.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_collection_under_way_when_the_holder_exits_finishes_through_the_next_holder()
-> anyhow::Result<()> {
    let profile = scratch_home();
    let homes = [scratch_home(), scratch_home()];
    let port = free_port();

    let mut holder = Server::spawn(port, &homes[0], &profile).await?;
    until_listening(port).await?;
    let mut reader = Server::spawn(port, &homes[1], &profile).await?;
    let plugin = StandIn::attach(port);
    until_all_read_through_the_bridge("the reading process", &mut [&mut reader]).await?;

    plugin.hold_next_read();
    let export = tokio::spawn(async move {
        let output = reader.exports_through_the_bridge().await;
        (reader, output)
    });
    plugin.until_holding().await?;
    let ended = Instant::now();
    holder.child.kill().await?;

    let (reader, output) = timeout(DEADLINE, export).await??;
    let output = output?;
    let finished = ended.elapsed();
    assert_eq!(output["source"]["kind"], "bridge", "{output}");
    assert!(
        output["tsx"]
            .as_str()
            .is_some_and(|tsx| tsx.contains("SyntheticFrame")),
        "{output}"
    );
    eprintln!(
        "collection across a handover measured: the export whose read was in flight when the \
         port's holder was killed finished {finished:?} later, through the process that took \
         the port over (the stand-in plugin retries every {PLUGIN_RETRY:?})"
    );

    drop(reader);
    for directory in homes.iter().chain([&profile]) {
        let _ = std::fs::remove_dir_all(directory);
    }
    Ok(())
}
