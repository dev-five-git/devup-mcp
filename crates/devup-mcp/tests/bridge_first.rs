//! The bridge is the path to reach for first, and every answer an agent reads
//! before its first Figma call has to say so.
//!
//! A real session met a Devup Bridge plugin attached and serving, was told
//! `status: "disconnected"` because that word described only the OAuth path,
//! and asked its user to log in. When it pressed on, `devup_figma_export`
//! without a url was refused with no next step, so it invented
//! `/design/bridge/bridge` - and the answer then reported `source.kind:
//! "direct"` and echoed the invented key as the file's.
//!
//! These tests drive the tool surface with a real bridge socket and a plugin
//! standing in for Figma, across the three states that have to be told apart:
//! bridge only, direct only, and neither.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
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
use tokio::{net::TcpStream, sync::Mutex};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

struct Auth {
    status: AuthStatus,
    logins: AtomicUsize,
}

impl Auth {
    fn new(status: AuthStatus) -> Arc<Self> {
        Arc::new(Self {
            status,
            logins: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl DevupAuth for Auth {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(self.status)
    }

    async fn login(&self) -> Result<AuthStatus, DevupError> {
        self.logins.fetch_add(1, Ordering::SeqCst);
        Ok(AuthStatus::Connected)
    }

    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

/// The metered path behind the bridge. Every read that reaches it is counted,
/// and none should while a plugin serves.
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

/// A bridge listening on an ephemeral port in front of the metered path, and
/// a server that reads through both, as production wires them.
struct Setup {
    bridge: BridgeServer,
    remote_calls: Arc<AtomicUsize>,
    upstream: Arc<dyn FigmaUpstream>,
}

fn setup() -> Setup {
    let bridge = BridgeServer::start(0).expect("an ephemeral port is free");
    let remote_calls = Arc::new(AtomicUsize::new(0));
    let upstream = Arc::new(FallbackUpstream::new(
        BridgeFigmaClient::new(bridge.state()).with_port(bridge.port()),
        Remote(remote_calls.clone()),
    ));
    Setup {
        bridge,
        remote_calls,
        upstream,
    }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// What the plugin says when it attaches: a file it could not report the key
/// of - the Dev Mode case the session ran into - with one frame selected.
fn keyless_hello(file_name: &str) -> Value {
    json!({
        "kind": "hello",
        "fileKey": null,
        "fileName": file_name,
        "currentPage": { "id": "0:1", "name": "Page 1" },
        "selection": [{ "id": "1:2", "name": "Synthetic Frame", "type": "FRAME" }],
        "selectionCount": 1,
    })
}

async fn attach(bridge: &BridgeServer, hello: Value) -> Socket {
    let expected = bridge.state().attached_files().await.len() + 1;
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/plugin", bridge.port()))
        .await
        .expect("the bridge accepts a plugin");
    socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .expect("hello is sent");
    for _ in 0..200 {
        if bridge.state().attached_files().await.len() == expected {
            return socket;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the plugin never registered");
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

/// What the plugin's scripts would return for a one-frame file whose key the
/// plugin could not report - every `fileKey` empty, as `figma.fileKey` was.
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

/// Answers every job the way the plugin would, and keeps what it was asked.
fn serve(mut socket: Socket) -> Arc<Mutex<Vec<Value>>> {
    let jobs = Arc::new(Mutex::new(Vec::new()));
    let log = jobs.clone();
    tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let job: Value = serde_json::from_str(&text).expect("a job is JSON");
            let answer = match script_answer(job["script"].as_str().unwrap_or_default()) {
                Ok(data) => {
                    json!({ "kind": "devup-result", "requestId": job["requestId"], "data": data })
                }
                Err(error) => {
                    json!({ "kind": "devup-result", "requestId": job["requestId"], "error": error })
                }
            };
            log.lock().await.push(job);
            if socket
                .send(Message::Text(answer.to_string().into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    jobs
}

/// Calls one tool and returns its structured answer, error or not, polling an
/// export job to completion the way a client does.
async fn call(
    auth: Arc<Auth>,
    upstream: Arc<dyn FigmaUpstream>,
    tool: &str,
    arguments: Value,
) -> anyhow::Result<(bool, Value)> {
    let server = DevupServer::new(Services::new(auth, upstream));
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

/// Whether the answer shows the key the bridge routes a keyless plugin by.
/// It names a connection, not a file, and must never be offered as the file's.
fn shows_a_routing_key(answer: &Value) -> bool {
    let text = answer.to_string();
    text.match_indices("bridge:")
        .any(|(at, _)| !text[at + "bridge:".len()..].starts_with("//"))
}

#[tokio::test]
async fn status_with_only_the_bridge_is_connected_through_it() -> anyhow::Result<()> {
    let setup = setup();
    let _plugin = attach(&setup.bridge, keyless_hello("Landing")).await;

    let (failed, status) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_auth",
        json!({ "action": "status" }),
    )
    .await?;
    assert!(!failed, "{status}");
    assert_eq!(status["connected"], true);
    assert_eq!(status["status"], "connected");
    assert_eq!(status["activePath"], "bridge");
    assert!(
        !status.to_string().contains("disconnected"),
        "nothing may say disconnected while the bridge serves: {status}"
    );
    let file = &status["paths"]["bridge"]["attachedFiles"][0];
    assert_eq!(file["fileName"], "Landing");
    assert!(file["fileKey"].is_null());
    assert_eq!(file["currentPage"]["name"], "Page 1");
    assert_eq!(file["selection"][0]["id"], "1:2");
    assert_eq!(status["nextAction"]["tool"], "devup_figma_export");
    assert!(status["nextAction"]["arguments"].get("url").is_none());
    assert!(!shows_a_routing_key(&status), "{status}");
    Ok(())
}

#[tokio::test]
async fn status_with_only_the_direct_path_is_connected_through_it() -> anyhow::Result<()> {
    let setup = setup();
    let (failed, status) = call(
        Auth::new(AuthStatus::Connected),
        setup.upstream.clone(),
        "devup_figma_auth",
        json!({ "action": "status" }),
    )
    .await?;
    assert!(!failed, "{status}");
    assert_eq!(status["connected"], true);
    assert_eq!(status["activePath"], "direct");
    assert_eq!(status["paths"]["bridge"]["listening"], true);
    assert_eq!(status["paths"]["bridge"]["available"], false);
    assert_eq!(status["nextAction"]["requiredArguments"], json!(["url"]));
    Ok(())
}

#[tokio::test]
async fn status_with_neither_path_offers_the_plugin_then_login() -> anyhow::Result<()> {
    let setup = setup();
    let (failed, status) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_auth",
        json!({ "action": "status" }),
    )
    .await?;
    assert!(!failed, "{status}");
    assert_eq!(status["connected"], false);
    assert_eq!(status["status"], "disconnected");
    assert!(status["activePath"].is_null());
    let options = status["nextAction"]["options"].as_array().unwrap();
    assert_eq!(options[0]["path"], "bridge");
    assert_eq!(options[1]["path"], "direct");
    assert_eq!(options[1]["arguments"]["action"], "login");
    Ok(())
}

/// The acceptance case: one plugin attached, no token, and no url. The export
/// reads the plugin's file and the frame selected in Figma, says the bridge
/// served it, and claims no file key the plugin did not report.
#[tokio::test]
async fn an_export_without_url_reads_the_selection_through_the_bridge() -> anyhow::Result<()> {
    let setup = setup();
    let jobs = serve(attach(&setup.bridge, keyless_hello("Landing")).await);

    let (failed, output) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
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
    assert_eq!(source["kind"], "bridge");
    assert_eq!(source["bridgePort"], setup.bridge.port());
    assert!(source["fileKey"].is_null(), "{source}");
    assert!(source["fileKeyReason"].is_string(), "{source}");
    assert!(
        source.get("requestedFileKey").is_none(),
        "nothing was requested by key: {source}"
    );
    assert_eq!(source["fileName"], "Landing");
    assert_eq!(source["pageName"], "Page 1");
    assert_eq!(source["nodeId"], "1:2");
    assert!(source["bridgeReads"].as_u64().unwrap_or_default() > 0);
    assert!(!shows_a_routing_key(&output), "{output}");

    assert_eq!(setup.remote_calls.load(Ordering::SeqCst), 0);
    let jobs = jobs.lock().await;
    assert!(!jobs.is_empty());
    assert!(
        jobs.iter()
            .filter(|job| job["script"] != "pageCatalog")
            .all(|job| job["params"]["nodeId"] == "1:2"),
        "every read targets the selected frame: {jobs:?}"
    );
    Ok(())
}

/// A key a caller supplied is served by a keyless plugin while it is alone -
/// that is how a real Figma link keeps working in Dev Mode - but the plugin
/// cannot vouch for it, so it is reported as requested, never as the file's.
#[tokio::test]
async fn a_requested_key_the_plugin_cannot_confirm_is_not_claimed() -> anyhow::Result<()> {
    let setup = setup();
    let _jobs = serve(attach(&setup.bridge, keyless_hello("Landing")).await);

    let (failed, output) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_export",
        json!({
            "url": "https://www.figma.com/design/bridge/bridge?node-id=1-2",
            "outputs": ["tsx"],
        }),
    )
    .await?;
    assert!(!failed, "{output}");
    let source = &output["source"];
    assert_eq!(source["kind"], "bridge");
    assert!(source["fileKey"].is_null(), "{source}");
    assert_eq!(source["requestedFileKey"], "bridge");
    Ok(())
}

#[tokio::test]
async fn an_export_without_url_and_no_path_names_both_ways_forward() -> anyhow::Result<()> {
    let setup = setup();
    let (failed, answer) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_export",
        json!({ "outputs": ["tsx"] }),
    )
    .await?;
    assert!(failed, "{answer}");
    let error = &answer["error"];
    assert_eq!(error["code"], "DEVUP_FIGMA_HANDOFF_INVALID");
    let options = error["details"]["nextAction"]["options"]
        .as_array()
        .unwrap();
    assert_eq!(options[0]["path"], "bridge");
    assert_eq!(options[0]["then"]["tool"], "devup_figma_export");
    assert_eq!(options[1]["path"], "direct");
    assert_eq!(options[1]["tool"], "devup_figma_auth");
    assert_eq!(options[1]["arguments"]["action"], "login");
    assert_eq!(options[1]["then"]["requiredArguments"], json!(["url"]));
    assert_eq!(setup.remote_calls.load(Ordering::SeqCst), 0);
    Ok(())
}

/// Signed in but no plugin: a link is all the call is missing, so no login is
/// suggested - only the link, and the plugin as the cheaper way.
#[tokio::test]
async fn an_export_without_url_on_the_direct_path_asks_for_the_link() -> anyhow::Result<()> {
    let setup = setup();
    let (failed, answer) = call(
        Auth::new(AuthStatus::Connected),
        setup.upstream.clone(),
        "devup_figma_export",
        json!({ "outputs": ["tsx"] }),
    )
    .await?;
    assert!(failed, "{answer}");
    let options = answer["error"]["details"]["nextAction"]["options"]
        .as_array()
        .unwrap();
    let direct = options
        .iter()
        .find(|option| option["path"] == "direct")
        .expect("a direct option");
    assert_eq!(direct["tool"], "devup_figma_export");
    assert_eq!(direct["requiredArguments"], json!(["url"]));
    assert!(!answer.to_string().contains("\"login\""), "{answer}");
    Ok(())
}

#[tokio::test]
async fn an_export_without_url_between_two_plugins_lists_both() -> anyhow::Result<()> {
    let setup = setup();
    let _first = attach(&setup.bridge, keyless_hello("Landing")).await;
    let _second = attach(
        &setup.bridge,
        json!({ "kind": "hello", "fileKey": "OtherFile123", "fileName": "Other",
                "currentPage": { "id": "0:1", "name": "Page 1" },
                "selection": [{ "id": "7:8", "name": "Card", "type": "FRAME" }],
                "selectionCount": 1 }),
    )
    .await;

    let (failed, answer) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_export",
        json!({ "outputs": ["tsx"] }),
    )
    .await?;
    assert!(failed, "{answer}");
    let options = answer["error"]["details"]["nextAction"]["options"]
        .as_array()
        .unwrap();
    assert_eq!(options.len(), 2);
    assert!(
        options[0].get("arguments").is_none(),
        "a keyless file has no link to offer"
    );
    assert_eq!(
        options[1]["arguments"]["url"],
        "https://www.figma.com/design/OtherFile123/devup?node-id=7-8"
    );
    assert!(!shows_a_routing_key(&answer), "{answer}");
    Ok(())
}

/// Search without url reads the whole file the plugin has open, and every
/// link it hands back routes to the same place - the bridge placeholder, not
/// a Figma URL around a key no Figma file has.
#[tokio::test]
async fn a_search_without_url_links_back_through_the_bridge() -> anyhow::Result<()> {
    let setup = setup();
    let _jobs = serve(attach(&setup.bridge, keyless_hello("Landing")).await);

    let (failed, output) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_search",
        json!({ "query": "syntheticframe" }),
    )
    .await?;
    assert!(!failed, "{output}");
    assert_eq!(output["matches"][0]["nodeId"], "1:2");
    assert_eq!(
        output["matches"][0]["canonicalUrl"],
        "figma-bridge://current?node-id=1-2"
    );
    assert_eq!(output["source"]["kind"], "bridge");
    assert!(!shows_a_routing_key(&output), "{output}");
    assert_eq!(setup.remote_calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn an_explore_without_url_anchors_on_the_selection() -> anyhow::Result<()> {
    let setup = setup();
    let jobs = serve(attach(&setup.bridge, keyless_hello("Landing")).await);

    // The simulated plugin does not run the explore script; what matters here
    // is which node the read was addressed to.
    let _ = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_explore",
        json!({}),
    )
    .await?;
    let jobs = jobs.lock().await;
    let explore = jobs
        .iter()
        .find(|job| job["script"] == "explore")
        .expect("the explore read went to the plugin");
    assert_eq!(explore["params"]["nodeId"], "1:2");
    assert_eq!(setup.remote_calls.load(Ordering::SeqCst), 0);
    Ok(())
}

/// Login is not refused while a plugin is attached - the bridge cannot serve
/// every read - but the answer says it was probably not needed.
#[tokio::test]
async fn login_with_a_plugin_attached_warns_and_still_logs_in() -> anyhow::Result<()> {
    let setup = setup();
    let _plugin = attach(&setup.bridge, keyless_hello("Landing")).await;
    let auth = Auth::new(AuthStatus::Disconnected);

    let (failed, answer) = call(
        auth.clone(),
        setup.upstream.clone(),
        "devup_figma_auth",
        json!({ "action": "login" }),
    )
    .await?;
    assert!(!failed, "{answer}");
    assert_eq!(auth.logins.load(Ordering::SeqCst), 1);
    assert!(answer["warning"].is_string(), "{answer}");
    assert_eq!(answer["activePath"], "bridge");
    Ok(())
}
