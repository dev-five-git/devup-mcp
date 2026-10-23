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
        "explore" => {
            let mut frame = frame_node();
            frame["fields"]["x"] = json!(0);
            frame["fields"]["y"] = json!(0);
            frame["fields"]["height"] = json!(600);
            frame["fields"]["textPreview"] = json!("Featured catalog");
            let mut page = page;
            page["fields"]["x"] = json!(0);
            page["fields"]["y"] = json!(0);
            page["fields"]["width"] = json!(2000);
            page["fields"]["height"] = json!(1000);
            Ok(
                json!({"fileKey":"","version":null,"rootIds":["0:1"],"nodes":[page,frame],"diagnostics":[]}),
            )
        }
        "bridgeContext" => Ok(
            json!({"kind":"devup-bridge-context","currentPage":{"id":"0:1","name":"Page 1"},"selection":[{"id":"1:2","name":"Synthetic Frame","type":"FRAME"}],"selectionCount":1,"observedAt":1}),
        ),
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
        "referencePng" => Ok(json!({
            "kind": "devupReferencePng", "nodeId": "1:2", "mimeType": "image/png", "byteLength": 68,
            "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
            "note": REFERENCE_NOTE
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
            if job["kind"] == "server-info" {
                continue;
            }
            let data = if job["script"] == "measureNodes" {
                Ok(
                    json!({"kind":"devup-measure","schemaVersion":1,"coordinateSpace":"figma-logical",
                    "nodes":job["params"]["rootIds"].as_array().unwrap().iter().map(|id|json!({"nodeId":id,"status":"measured","width":167,"height":94,
                        "padding":{"top":8,"right":8,"bottom":8,"left":8},"gap":12,"layoutMode":"HORIZONTAL","align":{"primary":"CENTER","counter":"MIN"}})).collect::<Vec<_>>()}),
                )
            } else {
                script_answer(job["script"].as_str().unwrap_or_default())
            };
            let answer = match data {
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

fn serve_with_context(mut socket: Socket, context: Option<Value>) -> Arc<Mutex<Vec<Value>>> {
    let jobs = Arc::new(Mutex::new(Vec::new()));
    let log = jobs.clone();
    tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let job: Value = serde_json::from_str(&text).unwrap();
            if job["kind"] == "server-info" {
                continue;
            }
            let data = if job["script"] == "bridgeContext" {
                context.clone().ok_or("DEVUP_BRIDGE_UNKNOWN_SCRIPT")
            } else {
                script_answer(job["script"].as_str().unwrap_or_default())
            };
            let answer = match data {
                Ok(data) => json!({"kind":"devup-result","requestId":job["requestId"],"data":data}),
                Err(error) => {
                    json!({"kind":"devup-result","requestId":job["requestId"],"error":error})
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

#[tokio::test]
async fn operation_refresh_overrides_stale_events_and_empty_current_never_exports_history()
-> anyhow::Result<()> {
    for empty in [false, true] {
        let setup = setup();
        let mut hello = keyless_hello("Synthetic");
        hello["selection"] = json!([{"id":"7:8","name":"Stale","type":"FRAME"}]);
        hello["currentPage"] = json!({"id":"0:9","name":"Stale page"});
        let current = json!({"kind":"devup-bridge-context","currentPage":{"id":"0:1","name":"Current"},
            "selection":if empty {json!([])} else {json!([{"id":"1:2","name":"Fresh","type":"FRAME"}])},
            "selectionCount":if empty {0} else {1},"lastKnownSelection":[{"id":"1:2","name":"Fresh","type":"FRAME"}],"lastKnownPage":{"id":"0:1","name":"Current"},"observedAt":2});
        let jobs = serve_with_context(attach(&setup.bridge, hello).await, Some(current));
        let (failed, value) = call(
            Auth::new(AuthStatus::Disconnected),
            setup.upstream.clone(),
            "devup_figma_export",
            json!({"outputs":["outline"],"screenshotObservations":{"labels":["Featured catalog"],"aspectRatio":0.5333333333333333}}),
        )
        .await?;
        assert_eq!(failed, empty, "{value}");
        if empty {
            let details = &value["error"]["details"];
            assert_eq!(details["selectionFreshness"]["state"], "current");
            assert_eq!(details["file"]["selectionCount"], 0);
            assert_eq!(
                details["selectionFreshness"]["lastKnown"]["selection"][0]["id"],
                "7:8"
            );
            assert_eq!(
                details["exploreSuggestion"]["screenshotMatch"],
                "unverified"
            );
            assert_eq!(
                details["exploreSuggestion"]["screenshotGuidance"]["source"],
                "caller-declared-screenshot-observations"
            );
            assert_eq!(
                details["exploreSuggestion"]["candidates"][0]["node"]["nodeId"],
                "1:2"
            );
            assert_eq!(
                details["exploreSuggestion"]["candidates"][0]["observationEvidence"]["visionVerified"],
                false
            );
            assert!(
                !jobs
                    .lock()
                    .await
                    .iter()
                    .any(|job| matches!(job["script"].as_str(), Some("snapshot" | "fastSnapshot")))
            );
        } else {
            assert_eq!(value["outline"]["nodes"][0]["nodeId"], "1:2");
        }
    }
    Ok(())
}

#[tokio::test]
async fn an_older_plugin_cannot_promote_last_delivered_selection_to_current() -> anyhow::Result<()>
{
    let setup = setup();
    let jobs = serve_with_context(
        attach(&setup.bridge, keyless_hello("Synthetic")).await,
        None,
    );
    let (failed, value) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_export",
        json!({"outputs":["outline"]}),
    )
    .await?;
    assert!(failed, "{value}");
    assert_eq!(
        value["error"]["details"]["selectionFreshness"]["state"],
        "refresh-failed"
    );
    assert_eq!(
        value["error"]["details"]["selectionFreshness"]["current"],
        Value::Null
    );
    assert_eq!(
        value["error"]["details"]["nextAction"]["requiredArguments"],
        json!(["url"])
    );
    assert!(
        !value["error"]["details"]["nextAction"]["arguments"]["url"]
            .as_str()
            .unwrap()
            .contains("1-2")
    );
    assert_eq!(jobs.lock().await.len(), 1);
    assert_eq!(setup.remote_calls.load(Ordering::SeqCst), 0);
    Ok(())
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
    assert_eq!(source["viewerPageName"], "Page 1");
    assert_eq!(source["nodeId"], "1:2");
    assert!(source["bridgeReads"].as_u64().unwrap_or_default() > 0);
    assert!(!shows_a_routing_key(&output), "{output}");

    assert_eq!(setup.remote_calls.load(Ordering::SeqCst), 0);
    let jobs = jobs.lock().await;
    assert!(!jobs.is_empty());
    assert!(
        jobs.iter()
            .filter(|job| !matches!(
                job["script"].as_str(),
                Some("pageCatalog" | "bridgeContext")
            ))
            .filter(|job| job["script"] != "bridgeContext")
            .all(|job| job["params"]["nodeId"] == "1:2"),
        "every read targets the selected frame: {jobs:?}"
    );
    Ok(())
}

const REFERENCE_NOTE: &str = "Drawn as the canvas shows this area, because what lies under the node is not one colour (FRAME \"Hero\" is painted with IMAGE). INSTANCE \"Header\" reaches into it and may show here; get_screenshot leaves other layers out.";

/// The reference image is the plugin's export too, so a screen and its
/// referencePng spend nothing on the metered path. What the plugin says about
/// its picture reaches the answer once, beside it, where resource delivery
/// keeps it too.
#[tokio::test]
async fn a_reference_png_comes_from_the_plugin() -> anyhow::Result<()> {
    let setup = setup();
    let jobs = serve(attach(&setup.bridge, keyless_hello("Landing")).await);

    let (failed, output) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_export",
        json!({ "frameIds": ["1:2"], "outputs": ["tsx", "referencePng"] }),
    )
    .await?;
    assert!(!failed, "{output}");
    assert_eq!(output["referencePng"]["mimeType"], "image/png", "{output}");
    assert_eq!(output["referencePngNote"], REFERENCE_NOTE);
    assert!(output["referencePng"].get("note").is_none(), "{output}");
    assert_eq!(output["cache"]["capabilities"]["referencePng"], true);
    assert_eq!(output["artifactId"], output["cache"]["artifactId"]);
    assert_eq!(output["outputResults"]["referencePng"]["state"], "produced");
    assert_eq!(
        output["source"]["bridgeReads"],
        output["collection"]["figmaToolCalls"]
    );

    let (failed, delivered) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_export",
        json!({ "outputs": ["referencePng"], "delivery": "resource" }),
    )
    .await?;
    assert!(!failed, "{delivered}");
    assert!(delivered.get("referencePng").is_none(), "{delivered}");
    assert_eq!(delivered["referencePngNote"], REFERENCE_NOTE);

    assert_eq!(setup.remote_calls.load(Ordering::SeqCst), 0);
    let jobs = jobs.lock().await;
    assert!(
        jobs.iter()
            .any(|job| job["script"] == "referencePng" && job["params"]["nodeId"] == "1:2"),
        "{jobs:?}"
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

/// Login is not refused while a plugin is attached - the direct path re-reads
/// what the plugin answers short - but the answer says it was probably not
/// needed.
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

#[tokio::test]
async fn node_measurements_are_lightweight_retained_and_cannot_be_reused_as_design_exports()
-> anyhow::Result<()> {
    let setup = setup();
    let jobs = serve(attach(&setup.bridge, keyless_hello("Synthetic")).await);
    let server = DevupServer::new(Services::new(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
    ));
    let (a, b) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(a).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(b).await?;
    let measured=client.call_tool(CallToolRequestParams::new("devup_figma_measure").with_arguments(json!({"nodeIds":["I1:2;3:4","1:2"],"calibration":{"nodeId":"1:2","screenWidthPx":334,"screenHeightPx":188}}).as_object().unwrap().clone())).await?;
    assert_ne!(measured.is_error, Some(true), "{measured:?}");
    let value = measured.structured_content.unwrap();
    assert_eq!(value["nodes"][0]["nodeId"], "I1:2;3:4");
    assert_eq!(value["nodes"][0]["width"], 167);
    assert_eq!(value["source"]["fileKey"], Value::Null);
    assert_eq!(value["cache"]["capabilities"]["kind"], "measurement");
    assert_eq!(value["calibration"]["uniformScale"], 2.0);
    assert!(!shows_a_routing_key(&value), "{value}");
    let id = value["cache"]["artifactId"].as_str().unwrap();
    let refused = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export").with_arguments(
                json!({"artifactId":id,"outputs":["tsx"]})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(refused.is_error, Some(true));
    let planned=client.call_tool(CallToolRequestParams::new("devup_runtime_audit").with_arguments(json!({"action":"plan","plan":{"baseUrl":"http://127.0.0.1:4173","allowedOrigins":["http://127.0.0.1:4173"],"viewports":[{"width":390,"height":844}],"screens":[{"id":"page","routePath":"/","checks":[{"kind":"dimensions","subject":{"testId":"card"},"measure":"box","tolerancePx":0.5,"figmaDimensions":[{"viewportWidth":390,"artifactId":id,"nodeId":"1:2"}]}]}]}}).as_object().unwrap().clone())).await?;
    assert_ne!(planned.is_error, Some(true), "{planned:?}");
    assert!(
        planned
            .structured_content
            .unwrap()
            .to_string()
            .contains("167")
    );
    let invalid = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_measure").with_arguments(
                json!({"nodeIds":["1:2;alert(1)"]})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(invalid.is_error, Some(true));
    let jobs = jobs.lock().await;
    assert_eq!(
        jobs.iter()
            .filter(|j| j["script"] == "measureNodes")
            .count(),
        1
    );
    assert!(
        jobs.iter()
            .all(|j| matches!(j["script"].as_str(), Some("measureNodes" | "bridgeContext")))
    );
    assert_eq!(setup.remote_calls.load(Ordering::SeqCst), 0);
    drop(jobs);
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn bounded_outline_and_path_selection_use_the_same_artifact_and_resource_evidence()
-> anyhow::Result<()> {
    let setup = setup();
    let jobs = serve(attach(&setup.bridge, keyless_hello("Synthetic")).await);
    let server = DevupServer::new(Services::new(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
    ));
    let (a, b) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(a).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(b).await?;
    let exported=client.call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(json!({"url":"figma-bridge://current?node-id=1-2","outputs":["outline"],"responseLimit":1}).as_object().unwrap().clone())).await?;
    assert_ne!(exported.is_error, Some(true), "{exported:?}");
    let value = exported.structured_content.unwrap();
    let id = value["cache"]["artifactId"].as_str().unwrap();
    assert_eq!(value["outline"]["nodes"][0]["layout"]["width"], 320.0);
    assert!(value.get("tsx").is_none());
    assert_eq!(value["deliverable"]["kind"], "outline-data");
    assert!(
        value["selectionEvidence"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("devup://artifact/")
    );
    assert!(value.to_string().len() < 32000);
    let reads = jobs.lock().await.len();
    let args = json!({"artifactId":id,"outputs":["outline"],"path":"nodes[name=\"Synthetic Frame\"].layout"});
    let selected = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await?;
    assert_ne!(selected.is_error, Some(true), "{selected:?}");
    let selected = selected.structured_content.unwrap();
    assert_eq!(selected["cache"]["artifactId"], id);
    assert_eq!(selected["cache"]["cacheHit"], true);
    assert_eq!(
        selected["selected"]["selections"][0]["nodeIds"],
        json!(["1:2"])
    );
    assert_eq!(
        selected["selected"]["selections"][0]["values"][0]["width"],
        320.0
    );
    assert_eq!(selected["deliverable"]["kind"], "selected-data");
    assert!(
        selected["selectionEvidence"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("devup://artifact/")
    );
    assert!(
        selected.to_string().len() < 32000,
        "compact response: {}",
        selected.to_string().len()
    );
    let again = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await?
        .structured_content
        .unwrap();
    assert_eq!(
        again["selectionEvidence"]["uri"],
        selected["selectionEvidence"]["uri"]
    );
    assert_eq!(jobs.lock().await.len(), reads, "reprojection called Figma");
    let invalid = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export").with_arguments(
                json!({"artifactId":id,"outputs":["outline"],"path":"nodes.filter(x)"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(invalid.is_error, Some(true));
    assert_eq!(jobs.lock().await.len(), reads);
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn measurement_batches_stay_bounded_and_keep_request_order() -> anyhow::Result<()> {
    let setup = setup();
    let jobs = serve(attach(&setup.bridge, keyless_hello("Synthetic")).await);
    let ids = (1..=25).map(|n| format!("8:{n}")).collect::<Vec<_>>();
    let (failed, result) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_measure",
        json!({"nodeIds":ids}),
    )
    .await?;
    assert!(!failed, "{result}");
    assert_eq!(
        result["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["nodeId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ids.iter().map(String::as_str).collect::<Vec<_>>()
    );
    let jobs = jobs.lock().await;
    let reads = jobs
        .iter()
        .filter(|j| j["script"] == "measureNodes")
        .collect::<Vec<_>>();
    assert_eq!(reads.len(), 3);
    assert!(
        reads
            .iter()
            .all(|j| j["params"]["rootIds"].as_array().unwrap().len() <= 10)
    );
    Ok(())
}
