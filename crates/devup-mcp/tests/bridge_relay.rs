//! Several devup-mcp processes on one machine share the Devup Bridge plugin,
//! and each one's `status` says what it is doing about it.
//!
//! Measured on one machine: eight devup-mcp processes, one of which held the
//! bridge port. The plugin attached to that one, and a session that needed
//! Figma - a different process - reported `listening: false` and could only
//! read through the metered direct path. Its only remedy was to kill another
//! session's process.
//!
//! These drive the tool surface with the bridge on a port picked for the test
//! and a plugin standing in for Figma. Port 1993 is never touched.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, BridgeFigmaClient, BridgeServer, DevupError, ErrorCode, FallbackUpstream,
    FigmaUpstream, ReadToolCall, UpstreamResult,
};
use futures_util::{SinkExt, StreamExt};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Map, Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

/// Generous, so a slow CI runner does not decide the outcome. Anything that
/// takes longer is a defect, not a wait.
const DEADLINE: Duration = Duration::from_secs(30);

/// How long `status` may take to answer about a port someone else holds.
/// Well above the probe's own limits, well below "waits indefinitely".
const STATUS_LIMIT: Duration = Duration::from_secs(15);

struct Auth;

#[async_trait]
impl DevupAuth for Auth {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }

    async fn login(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }

    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

/// The metered path. Nothing here should reach it.
struct Remote(Arc<AtomicUsize>);

#[async_trait]
impl FigmaUpstream for Remote {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(DevupError::new(
            ErrorCode::DevupFigmaDirectUnavailable,
            "the direct path was not expected to be used",
            false,
        ))
    }
}

/// Wired the way production wires a process: its bridge in front of the
/// metered path.
fn upstream(bridge: &BridgeServer, remote_calls: &Arc<AtomicUsize>) -> Arc<dyn FigmaUpstream> {
    Arc::new(FallbackUpstream::new(
        BridgeFigmaClient::new(bridge.state()).with_port(bridge.port()),
        Remote(remote_calls.clone()),
    ))
}

/// A devup-mcp that found the port held. It used to get no bridge at all.
fn second_process_on(port: u16) -> BridgeServer {
    BridgeServer::start(port).expect(
        "a devup-mcp that finds the bridge port held must still get a bridge, through the \
         process that holds it",
    )
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{what} did not happen within {DEADLINE:?}");
}

async fn sees_one_plugin(bridge: &BridgeServer) {
    let state = bridge.state();
    eventually("the plugin to show up in this process", || {
        let state = state.clone();
        async move { state.attached_files().await.len() == 1 }
    })
    .await;
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The Dev Mode case: a plugin that cannot report its file key, with one
/// frame selected.
fn keyless_hello() -> Value {
    json!({
        "kind": "hello",
        "fileKey": null,
        "fileName": "Landing",
        "currentPage": { "id": "0:1", "name": "Page 1" },
        "selection": [{ "id": "1:2", "name": "Synthetic Frame", "type": "FRAME" }],
        "selectionCount": 1,
    })
}

async fn attach(port: u16) -> Socket {
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{port}/plugin"))
        .await
        .expect("the holder accepts a plugin");
    socket
        .send(Message::Text(keyless_hello().to_string().into()))
        .await
        .expect("hello is sent");
    socket
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

/// What the plugin's scripts return for a one-frame file whose key it could
/// not report.
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

/// Answers every job the way the plugin would.
fn serve(mut socket: Socket) {
    tokio::spawn(async move {
        while let Some(Ok(message)) = socket.next().await {
            let Message::Text(text) = message else {
                continue;
            };
            let job: Value = serde_json::from_str(&text).expect("a job is JSON");
            let answer = match script_answer(job["script"].as_str().unwrap_or_default()) {
                Ok(data) => {
                    json!({ "kind": "devup-result", "requestId": job["requestId"], "data": data })
                }
                Err(error) => {
                    json!({ "kind": "devup-result", "requestId": job["requestId"], "error": error })
                }
            };
            if socket
                .send(Message::Text(answer.to_string().into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });
}

/// Calls one tool on a server over `upstream` and returns its structured
/// answer, error or not, polling an export job to completion as a client does.
async fn call(
    upstream: Arc<dyn FigmaUpstream>,
    tool: &str,
    arguments: Value,
) -> anyhow::Result<(bool, Value)> {
    let server = DevupServer::new(Services::new(Arc::new(Auth), upstream));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap_or_default();
    let mut result = client
        .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments))
        .await?;
    for _ in 0..500 {
        let Some(id) = result
            .structured_content
            .as_ref()
            .filter(|value| value["exportJob"]["state"] == "running")
            .and_then(|value| value["exportJob"]["jobId"].as_str())
            .map(str::to_owned)
        else {
            break;
        };
        result = client
            .call_tool(
                CallToolRequestParams::new("devup_figma_export")
                    .with_arguments(json!({ "jobId": id }).as_object().cloned().unwrap()),
            )
            .await?;
    }
    client.cancel().await?;
    task.await??;
    Ok((
        result.is_error == Some(true),
        result.structured_content.unwrap_or_default(),
    ))
}

async fn status(bridge: &BridgeServer) -> anyhow::Result<Value> {
    let remote_calls = Arc::new(AtomicUsize::new(0));
    let (failed, status) = call(
        upstream(bridge, &remote_calls),
        "devup_figma_auth",
        json!({ "action": "status" }),
    )
    .await?;
    assert!(!failed, "{status}");
    Ok(status)
}

/// Whether the answer shows the key the bridge routes a keyless plugin by.
fn shows_a_routing_key(answer: &Value) -> bool {
    let text = answer.to_string();
    text.match_indices("bridge:")
        .any(|(at, _)| !text[at + "bridge:".len()..].starts_with("//"))
}

/// Every process sees the same plugin, is connected through it, and says
/// which part it plays: the one holding the port, and the ones reading
/// through it - naming the holder.
#[tokio::test]
async fn every_process_reports_the_same_plugin_and_who_holds_the_port() -> anyhow::Result<()> {
    let host = BridgeServer::start(0).expect("an ephemeral port is free");
    let port = host.port();
    let relay = second_process_on(port);
    serve(attach(port).await);
    sees_one_plugin(&host).await;
    sees_one_plugin(&relay).await;

    let on_host = status(&host).await?;
    let on_relay = status(&relay).await?;
    for status in [&on_host, &on_relay] {
        assert_eq!(status["connected"], true, "{status}");
        assert_eq!(status["activePath"], "bridge", "{status}");
        assert_eq!(status["paths"]["bridge"]["available"], true, "{status}");
        assert_eq!(status["paths"]["bridge"]["port"], port, "{status}");
        // #76: with the plugin reachable, nothing sends anyone to log in.
        assert!(!status.to_string().contains("disconnected"), "{status}");
        assert_eq!(status["nextAction"]["tool"], "devup_figma_export");
        assert!(status["nextAction"]["arguments"].get("url").is_none());
        assert!(!shows_a_routing_key(status), "{status}");
    }
    assert_eq!(
        on_host["paths"]["bridge"]["attachedFiles"], on_relay["paths"]["bridge"]["attachedFiles"],
        "every process shows the plugin the same way"
    );

    let bridge = &on_host["paths"]["bridge"];
    assert_eq!(bridge["role"], "host", "{bridge}");
    assert_eq!(bridge["listening"], true);

    let bridge = &on_relay["paths"]["bridge"];
    assert_eq!(bridge["role"], "relay", "{bridge}");
    assert_eq!(
        bridge["listening"], false,
        "this process does not hold the port"
    );
    assert_eq!(bridge["host"]["pid"], std::process::id(), "{bridge}");
    assert_eq!(bridge["host"]["version"], env!("CARGO_PKG_VERSION"));
    Ok(())
}

/// The acceptance case from the process that did not get the port: no url,
/// no token, and the export still reads the frame selected in Figma through
/// the bridge - spending nothing on the metered path.
#[tokio::test]
async fn an_export_without_url_on_a_relay_reads_the_selection_through_the_host()
-> anyhow::Result<()> {
    let host = BridgeServer::start(0).expect("an ephemeral port is free");
    let port = host.port();
    let relay = second_process_on(port);
    serve(attach(port).await);
    sees_one_plugin(&relay).await;

    let remote_calls = Arc::new(AtomicUsize::new(0));
    let (failed, output) = call(
        upstream(&relay, &remote_calls),
        "devup_figma_export",
        json!({ "outputs": ["tsx"] }),
    )
    .await?;
    assert!(!failed, "{output}");
    assert!(
        output["tsx"]
            .as_str()
            .is_some_and(|tsx| tsx.contains("SyntheticFrame")),
        "{output}"
    );
    let source = &output["source"];
    assert_eq!(source["kind"], "bridge", "{source}");
    assert_eq!(source["bridgePort"], port);
    assert!(source["fileKey"].is_null(), "{source}");
    assert_eq!(source["fileName"], "Landing");
    assert_eq!(source["nodeId"], "1:2");
    assert!(!shows_a_routing_key(&output), "{output}");
    assert_eq!(remote_calls.load(Ordering::SeqCst), 0);
    drop(host);
    Ok(())
}

/// What a devup-mcp built before the bridge could be shared answers on the
/// port: `/plugin`, and nothing else.
async fn legacy_devup_mcp() -> u16 {
    use axum::{extract::ws::WebSocketUpgrade, routing::any};

    let app = axum::Router::new().route(
        "/plugin",
        any(|upgrade: WebSocketUpgrade| async move {
            upgrade.on_upgrade(|mut socket| async move {
                while let Some(Ok(_)) = socket.recv().await {}
            })
        }),
    );
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("an ephemeral port is free");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await });
    port
}

/// A process that finds `port` held, once its first `status` has come back -
/// which has to be prompt, whoever holds the port.
async fn process_on(port: u16) -> anyhow::Result<BridgeServer> {
    let relay = second_process_on(port);
    let started = Instant::now();
    status(&relay).await?;
    assert!(
        started.elapsed() < STATUS_LIMIT,
        "status took {:?}: it must not wait on the port's holder indefinitely",
        started.elapsed()
    );
    Ok(relay)
}

/// A holder that does not say who it is gets looked up in the operating
/// system after `status` has answered - that takes an external command - so
/// its name arrives a moment later. The status that names it, at `pid`.
async fn status_naming_the_holder(relay: &BridgeServer, pid: &str) -> anyhow::Result<Value> {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let status = status(relay).await?;
        if status.pointer(pid).is_some_and(|pid| !pid.is_null()) || Instant::now() > deadline {
            return Ok(status);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// An older devup-mcp holds the port. It cannot be fixed from here, so the
/// answer is to say so - which process kind, why this one cannot use the
/// plugin, and what would change that - instead of the generic "not
/// listening" that left a session guessing.
#[tokio::test]
async fn a_devup_mcp_from_before_sharing_is_named_with_its_repair() -> anyhow::Result<()> {
    let port = legacy_devup_mcp().await;
    let relay = process_on(port).await?;
    let status = status_naming_the_holder(&relay, "/paths/bridge/host/pid").await?;

    let bridge = &status["paths"]["bridge"];
    assert_eq!(bridge["available"], false, "{bridge}");
    assert_eq!(bridge["role"], "unavailable", "{bridge}");
    assert_eq!(bridge["issue"], "legacy-host", "{bridge}");
    // The holder cannot say who it is, so the operating system is asked
    // which process listens on the port. Here that is this test process.
    assert_eq!(
        bridge["host"]["pid"],
        std::process::id(),
        "the process holding the port is named: {bridge}"
    );
    let reason = bridge["reason"].as_str().expect("a reason");
    assert!(reason.contains(&port.to_string()), "{reason}");
    assert!(reason.contains("restart"), "{reason}");
    assert!(
        reason.contains(&format!("pid {}", std::process::id())),
        "{reason}"
    );

    let options = status["nextAction"]["options"]
        .as_array()
        .expect("ways forward");
    assert_eq!(options[0]["path"], "bridge", "{status}");
    assert!(
        options[0]["action"]
            .as_str()
            .is_some_and(|action| action.contains("restart")),
        "{status}"
    );
    assert_eq!(options[1]["path"], "direct", "{status}");
    Ok(())
}

/// Something that is not devup-mcp holds the port - an unrelated web server,
/// or a program that accepts and never answers. Neither is mistaken for a
/// devup-mcp, and neither makes `status` wait.
#[tokio::test]
async fn another_program_on_the_port_is_told_apart_from_devup_mcp() -> anyhow::Result<()> {
    let web = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let web_port = web.local_addr()?.port();
    tokio::spawn(async move { axum::serve(web, axum::Router::new()).await });

    let silent = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let silent_port = silent.local_addr()?.port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = silent.accept().await {
            held.push(socket);
        }
    });

    for port in [web_port, silent_port] {
        let relay = process_on(port).await?;
        let status = status_naming_the_holder(&relay, "/paths/bridge/holder/pid").await?;
        let bridge = &status["paths"]["bridge"];
        assert_eq!(bridge["available"], false, "{bridge}");
        assert_eq!(bridge["role"], "unavailable", "{bridge}");
        assert_eq!(bridge["issue"], "foreign-program", "{bridge}");
        assert!(bridge["host"].is_null(), "it is not a devup-mcp: {bridge}");
        assert_eq!(
            bridge["holder"]["pid"],
            std::process::id(),
            "the program holding the port is named: {bridge}"
        );
        let reason = bridge["reason"].as_str().expect("a reason");
        assert!(reason.contains("not a devup-mcp"), "{reason}");
    }
    Ok(())
}

/// One server, many calls: the pacer belongs to the server, so the calls that
/// share it have to share a session.
struct Session {
    client: rmcp::service::RunningService<rmcp::RoleClient, ()>,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Session {
    async fn open(upstream: Arc<dyn FigmaUpstream>) -> anyhow::Result<Self> {
        let server = DevupServer::new(Services::new(Arc::new(Auth), upstream));
        let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
        let task = tokio::spawn(async move {
            server.serve(server_transport).await?.waiting().await?;
            anyhow::Ok(())
        });
        let client = ().serve(client_transport).await?;
        Ok(Self { client, task })
    }

    async fn export(&self) -> anyhow::Result<Value> {
        let arguments = json!({ "outputs": ["tsx"], "refresh": true });
        let mut result = self
            .client
            .call_tool(
                CallToolRequestParams::new("devup_figma_export")
                    .with_arguments(arguments.as_object().cloned().unwrap()),
            )
            .await?;
        for _ in 0..500 {
            let Some(id) = result
                .structured_content
                .as_ref()
                .filter(|value| value["exportJob"]["state"] == "running")
                .and_then(|value| value["exportJob"]["jobId"].as_str())
                .map(str::to_owned)
            else {
                break;
            };
            result = self
                .client
                .call_tool(
                    CallToolRequestParams::new("devup_figma_export")
                        .with_arguments(json!({ "jobId": id }).as_object().cloned().unwrap()),
                )
                .await?;
        }
        anyhow::ensure!(result.is_error != Some(true), "{result:?}");
        Ok(result.structured_content.unwrap_or_default())
    }

    async fn close(self) -> anyhow::Result<()> {
        self.client.cancel().await?;
        self.task.await??;
        Ok(())
    }
}

/// Figma meters reads made through its remote MCP, and every read used to be
/// held to that pace - eight a minute - including the ones the plugin
/// answers, which cost nothing. A second export on the same server then sat
/// out most of a minute for an allowance it was not spending. Only reads
/// bound for the metered path are paced now.
#[tokio::test]
async fn reads_through_the_bridge_are_not_held_to_the_metered_pace() -> anyhow::Result<()> {
    let host = BridgeServer::start(0).expect("an ephemeral port is free");
    serve(attach(host.port()).await);
    sees_one_plugin(&host).await;
    let remote_calls = Arc::new(AtomicUsize::new(0));
    let session = Session::open(upstream(&host, &remote_calls)).await?;

    let started = Instant::now();
    // Well past the metered ceiling of eight reads a minute.
    for _ in 0..10 {
        let output = session.export().await?;
        assert_eq!(output["source"]["kind"], "bridge", "{output}");
    }
    let elapsed = started.elapsed();
    session.close().await?;
    assert!(
        elapsed < Duration::from_secs(40),
        "ten bridge exports took {elapsed:?}: they were paced as if metered"
    );
    assert_eq!(remote_calls.load(Ordering::SeqCst), 0);
    Ok(())
}
