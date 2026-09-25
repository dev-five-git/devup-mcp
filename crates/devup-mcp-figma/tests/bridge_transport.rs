//! 브리지 전송 계층을 진짜 소켓으로 통과시킨다.
//!
//! Figma 앱 자체는 여기서 띄울 수 없으므로 플러그인 자리에 WebSocket 클라이언트를
//! 세운다. 그 너머(스크립트 실행)는 플러그인 빌드가 책임지고, 여기서는 그 앞의
//! 모든 것 — 등록, 이름·값 매핑, 상관, 봉투 모양, 라우팅 판정 — 을 확인한다.

use devup_mcp_figma::{
    BridgeFigmaClient, BridgeServed, BridgeServer, DevupError, FallbackUpstream, FigmaUpstream,
    PreferredUpstream, ReadToolCall, UpstreamResult,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

const FILE_KEY: &str = "FileKey123";

type Plugin = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// 플러그인 흉내: 붙어서 자기 파일을 알리고, 등록될 때까지 기다린다.
///
/// `file_key` 가 `None` 이면 키 없이 붙는다. 지어낸 경우가 아니라 실제로 일어난다 —
/// 실기기에서 `figma.fileKey` 가 비어 오는 것을 확인했다.
async fn connect_plugin_as(server: &BridgeServer, file_key: Option<&str>) -> Plugin {
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/plugin", server.port()))
        .await
        .expect("bridge accepts a plugin");
    socket
        .send(Message::Text(
            json!({ "kind": "hello", "fileKey": file_key })
                .to_string()
                .into(),
        ))
        .await
        .expect("hello is sent");

    // 등록은 서버 쪽 태스크에서 일어난다. 보낸 직후에는 아직일 수 있다.
    let expected = file_key.unwrap_or_default();
    let state = server.state();
    for _ in 0..200 {
        if state.connected_files().await.iter().any(|k| k == expected) {
            return socket;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("plugin never registered");
}

async fn connect_plugin(server: &BridgeServer) -> Plugin {
    connect_plugin_as(server, Some(FILE_KEY)).await
}

/// 다음 작업 하나를 받아 요청 id 와 함께 돌려준다.
async fn next_job(socket: &mut Plugin) -> Value {
    let message = socket
        .next()
        .await
        .expect("a job arrives")
        .expect("the socket stays open");
    let Message::Text(text) = message else {
        panic!("jobs are sent as text");
    };
    serde_json::from_str(&text).expect("a job is JSON")
}

#[tokio::test]
async fn a_script_read_reaches_the_plugin_and_comes_back_wrapped() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let client = BridgeFigmaClient::new(server.state());

    let call = ReadToolCall::fast_snapshot(FILE_KEY, "1:2");
    let reading = tokio::spawn(async move { client.call_read_tool(call).await });

    let job = next_job(&mut plugin).await;
    assert_eq!(job["kind"], "devup-job");
    // 플러그인이 아는 이름이어야 한다. 코드젠이 파일명에서 만든 것과 같다.
    assert_eq!(job["script"], "fastSnapshot");
    assert_eq!(job["params"]["nodeId"], "1:2");
    // 단일 루트 읽기는 대상 노드 하나를 루트로 넘긴다 — `source` 의 기본값과 같다.
    assert_eq!(job["params"]["rootIds"], json!(["1:2"]));

    let payload = json!({ "fileKey": FILE_KEY, "nodes": [{ "id": "1:2" }] });
    plugin
        .send(Message::Text(
            json!({
                "kind": "devup-result",
                "requestId": job["requestId"],
                "data": payload,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("the result is sent");

    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the bridge answers");

    // 원격 `use_figma` 와 같은 모양이어야 디코더들이 그대로 동작한다.
    let text = result.raw["content"][0]["text"]
        .as_str()
        .expect("the payload rides in content[].text");
    assert_eq!(
        serde_json::from_str::<Value>(text).expect("the carried text is JSON"),
        payload,
    );
}

#[tokio::test]
async fn each_script_read_maps_to_the_name_the_plugin_knows() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let client = BridgeFigmaClient::new(server.state());

    for (call, expected) in [
        (ReadToolCall::section_index(FILE_KEY, "1:2"), "sectionIndex"),
        (ReadToolCall::page_catalog(FILE_KEY), "pageCatalog"),
        (ReadToolCall::fast_theme(FILE_KEY), "fastTheme"),
    ] {
        let client = client.clone();
        let reading = tokio::spawn(async move { client.call_read_tool(call).await });

        let job = next_job(&mut plugin).await;
        assert_eq!(job["script"], expected, "script name for {expected}");

        plugin
            .send(Message::Text(
                json!({
                    "kind": "devup-result",
                    "requestId": job["requestId"],
                    "data": { "ok": true },
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("the result is sent");
        reading
            .await
            .expect("the read task finishes")
            .expect("the bridge answers");
    }
}

#[tokio::test]
async fn a_failing_script_surfaces_its_own_code() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let client = BridgeFigmaClient::new(server.state());

    let call = ReadToolCall::fast_snapshot(FILE_KEY, "9:9");
    let reading = tokio::spawn(async move { client.call_read_tool(call).await });

    let job = next_job(&mut plugin).await;
    plugin
        .send(Message::Text(
            json!({
                "kind": "devup-result",
                "requestId": job["requestId"],
                "error": "DEVUP_NODE_NOT_FOUND",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("the failure is sent");

    let error = reading
        .await
        .expect("the read task finishes")
        .expect_err("a script failure is an error");
    // 원격 경로와 같은 문자열이어야 위쪽 분기가 동일하게 동작한다.
    assert_eq!(error.message, "DEVUP_NODE_NOT_FOUND");
}

#[tokio::test]
async fn routing_is_decided_before_the_call() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let client = BridgeFigmaClient::new(server.state());

    // 플러그인이 붙기 전에는 스크립트 읽기도 맡지 않는다 — 원격이 받아야 한다.
    assert!(
        !client
            .can_serve(&ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
            .await
    );

    let _plugin = connect_plugin(&server).await;
    assert!(
        client
            .can_serve(&ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
            .await
    );

    // 노드가 지정된 메타데이터는 두 경로가 같은 `MetadataDocument` 로 환원되므로
    // 플러그인이 맡는다. 노드 없는 읽기는 최상위 페이지 목록이라는 다른 계약이라
    // 원격이 받아야 한다.
    assert!(
        client
            .can_serve(&ReadToolCall::metadata(FILE_KEY, Some("1:2")))
            .await
    );
    assert!(
        !client
            .can_serve(&ReadToolCall::metadata(FILE_KEY, None))
            .await
    );
    // 다른 파일은 그 파일의 플러그인이 필요하다.
    assert!(
        !client
            .can_serve(&ReadToolCall::fast_snapshot("OtherFile", "1:2"))
            .await
    );
}

/// 로그인을 요구할지 말지는 이 값이 정한다.
///
/// 브리지는 Figma 한도도 자격증명도 쓰지 않으므로, 이 파일을 맡은 플러그인이
/// 있으면 수집은 토큰 없이 성립한다. 판정은 파일 단위여야 한다 — 다른 파일을
/// 열어 둔 플러그인이 붙어 있다고 해서 이 파일을 읽을 수 있는 것은 아니다.
#[tokio::test]
async fn a_plugin_holding_the_file_makes_it_readable_without_credentials() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let client = BridgeFigmaClient::new(server.state());

    assert!(!client.serves_without_credentials(FILE_KEY).await);

    let _plugin = connect_plugin(&server).await;
    assert!(client.serves_without_credentials(FILE_KEY).await);
    assert!(!client.serves_without_credentials("OtherFile").await);
}

/// 키 없이 붙은 플러그인도 혼자면 맡는다.
///
/// `figma.fileKey` 는 늘 오는 값이 아니다. 실기기에서 비어 온 적이 있고, 그때 등록을
/// 건너뛰었더니 플러그인 창은 "연결됨"이라고 하는데 읽기는 한 건도 도착하지 않았다.
/// 연결이 스스로를 멀쩡하다고 보고하는 실패라 원인을 찾기가 가장 어렵다.
#[tokio::test]
async fn a_plugin_without_a_file_key_serves_when_it_is_the_only_one() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let client = BridgeFigmaClient::new(server.state());
    let mut plugin = connect_plugin_as(&server, None).await;

    // 어느 파일을 물어도 이 하나가 답한다. 무엇을 열고 있는지 모르지만, 하나뿐이라
    // 그것이 열려 있는 파일이다.
    assert!(
        client
            .can_serve(&ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
            .await
    );

    let reading = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .call_read_tool(ReadToolCall::page_catalog(FILE_KEY))
                .await
        })
    };
    let job = next_job(&mut plugin).await;
    assert_eq!(job["script"], "pageCatalog");
    plugin
        .send(Message::Text(
            json!({
                "kind": "devup-result",
                "requestId": job["requestId"],
                "data": { "ok": true },
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("the result is sent");
    reading
        .await
        .expect("the read task finishes")
        .expect("a keyless plugin still answers");
}

/// 키 없는 플러그인이 둘이면 아무도 맡지 않는다.
///
/// 어느 쪽이 대상 파일을 열고 있는지 가릴 수 없다. 엉뚱한 문서를 읽어 주느니 요금을
/// 쓰더라도 원격으로 넘기는 편이 낫다.
#[tokio::test]
async fn two_keyless_plugins_are_ambiguous_and_neither_serves() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let client = BridgeFigmaClient::new(server.state());

    let _first = connect_plugin_as(&server, None).await;
    // 다른 파일을 연 플러그인이 하나 더 붙으면 키 없는 쪽은 더는 혼자가 아니다.
    // 둘 다 키가 없는 경우는 `two_keyless_plugins_are_counted_apart` 가 본다.
    let _second = connect_plugin_as(&server, Some("AnotherFile")).await;

    assert!(
        !client
            .can_serve(&ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
            .await
    );
}

/// 플러그인 흉내: `hello` 를 그대로 보내고, 붙은 플러그인이 `count` 개가 될
/// 때까지 기다린다. 키 없는 플러그인이 둘이면 `connected_files` 로는 둘째의
/// 등록을 가릴 수 없어서 수로 기다린다.
async fn attach(server: &BridgeServer, hello: Value, count: usize) -> Plugin {
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/plugin", server.port()))
        .await
        .expect("bridge accepts a plugin");
    socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .expect("hello is sent");
    let state = server.state();
    for _ in 0..200 {
        if state.attached_files().await.len() == count {
            return socket;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("plugin never registered");
}

async fn answer(plugin: &mut Plugin, job: &Value, data: Value) {
    plugin
        .send(Message::Text(
            json!({ "kind": "devup-result", "requestId": job["requestId"], "data": data })
                .to_string()
                .into(),
        ))
        .await
        .expect("the result is sent");
}

/// 플러그인이 보고하는 페이지와 선택은 `status` 가 그대로 보여 주고, url 없는
/// 요청은 그것으로 대상을 고른다. 키를 보고하지 못한 플러그인에는 그 연결만
/// 가리키는 키가 주어지고, 그 키로 보낸 읽기는 그 플러그인에 닿으며, 답에는
/// 파일 키를 지어내지 않은 출처가 실린다.
#[tokio::test]
async fn a_keyless_plugin_reports_where_it_is_and_answers_its_own_connection() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = attach(
        &server,
        json!({
            "kind": "hello", "fileKey": null, "fileName": "Landing",
            "currentPage": { "id": "0:1", "name": "Page 1" },
            "selection": [{ "id": "1:2", "name": "Hero", "type": "FRAME" }],
            "selectionCount": 1,
        }),
        1,
    )
    .await;
    let state = server.state();
    let attached = state.attached_files().await;
    let file = &attached[0];
    assert_eq!(file.file_key, None);
    assert_eq!(file.file_name.as_deref(), Some("Landing"));
    assert_eq!(
        file.context.single_selection().map(|node| node.id.as_str()),
        Some("1:2")
    );
    assert!(devup_mcp_figma::is_bridge_only_key(&file.target_key));

    let client = BridgeFigmaClient::new(state.clone()).with_port(server.port());
    let call = ReadToolCall::fast_snapshot(file.target_key.clone(), "1:2");
    let reading = tokio::spawn(async move { client.call_read_tool(call).await });
    let job = next_job(&mut plugin).await;
    assert_eq!(job["params"]["nodeId"], "1:2");
    answer(&mut plugin, &job, json!({ "fileKey": "", "nodes": [] })).await;
    let result = reading.await.unwrap().expect("the plugin answers");
    let served =
        serde_json::from_value::<BridgeServed>(result.raw["_meta"]["devup/bridge"].clone())
            .expect("a bridge answer names the plugin that gave it");
    assert_eq!(served.reads, 1);
    assert_eq!(served.port, Some(server.port()));
    assert_eq!(
        served.file_key, None,
        "no key is claimed for a keyless plugin"
    );
    assert_eq!(served.file_name.as_deref(), Some("Landing"));
    assert_eq!(served.page_name.as_deref(), Some("Page 1"));

    // A new selection reaches the server without a reconnect.
    plugin
        .send(Message::Text(
            json!({
                "kind": "context",
                "currentPage": { "id": "0:2", "name": "Page 2" },
                "selection": [],
                "selectionCount": 0,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("the context is sent");
    for _ in 0..200 {
        let context = state.attached_files().await[0].context.clone();
        if context.current_page.map(|page| page.name).as_deref() == Some("Page 2") {
            assert_eq!(context.selection, Some(vec![]));
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the context update never arrived");
}

/// 키 없는 플러그인 둘은 두 개로 센다. 한 자리에 묶이면 "정확히 하나"를 셀 수
/// 없고, 먼저 붙은 쪽이 끊길 때 나중 쪽의 등록까지 지워진다. 둘이면 아무 키도
/// 맡지 않지만, 각 연결의 키는 여전히 제 플러그인에만 닿는다.
#[tokio::test]
async fn two_keyless_plugins_are_counted_apart() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let hello = json!({ "kind": "hello", "fileKey": null });
    let first = attach(&server, hello.clone(), 1).await;
    let _second = attach(&server, hello, 2).await;
    let state = server.state();
    let client = BridgeFigmaClient::new(state.clone());
    assert!(
        !client
            .can_serve(&ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
            .await
    );
    let attached = state.attached_files().await;
    assert_eq!(attached.len(), 2);
    for file in &attached {
        assert!(
            client
                .can_serve(&ReadToolCall::fast_snapshot(file.target_key.clone(), "1:2"))
                .await
        );
    }

    drop(first);
    for _ in 0..200 {
        if state.attached_files().await.len() == 1 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the first plugin's disconnect must leave the second attached");
}

/// 원격만 아는 상류. 몇 번 불렸는지만 센다.
struct CountingRemote {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl FigmaUpstream for CountingRemote {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(UpstreamResult { raw: json!({}) })
    }
}

/// 브리지만 읽을 수 있는 키는 원격으로 넘어가지 않는다. Figma 에 없는 키를
/// 물어 봐야 원인과 무관한 거절만 돌아오고, 로그인이 필요 없는 요청에 로그인이
/// 요구된다.
#[tokio::test]
async fn a_bridge_only_key_never_reaches_the_remote_path() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let remote_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let upstream = FallbackUpstream::new(
        BridgeFigmaClient::new(server.state()),
        CountingRemote {
            calls: remote_calls.clone(),
        },
    );
    assert!(upstream.serves_without_credentials("bridge:41").await);
    let error = upstream
        .call_read_tool(ReadToolCall::fast_snapshot("bridge:41", "1:2"))
        .await
        .expect_err("no plugin holds this connection");
    assert_eq!(error.details["stage"], "bridge-routing");
    let error = upstream
        .call_read_tool(ReadToolCall::screenshot("bridge:41", "1:2"))
        .await
        .expect_err("the bridge cannot take a screenshot, and direct has no key");
    assert_eq!(error.details["tool"], "get_screenshot");

    assert_eq!(remote_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    // An ordinary key still falls through when no plugin holds it.
    upstream
        .call_read_tool(ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
        .await
        .expect("the remote path answers");
    assert_eq!(remote_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn original_upload_crosses_the_socket_without_remote_text_truncation() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use devup_mcp_figma::{AssetRequest, original_image_from_result};
    use sha2::{Digest, Sha256};

    let server = BridgeServer::start(0).unwrap();
    let mut plugin = connect_plugin(&server).await;
    let client = BridgeFigmaClient::new(server.state());
    let request = AssetRequest::original_image("n", 2, "hash");
    let call = ReadToolCall::asset_export(FILE_KEY, Some("v1".into()), request.clone());
    let reading = tokio::spawn(async move { client.call_read_tool(call).await });
    let job = next_job(&mut plugin).await;
    assert_eq!(job["script"], "assets");
    assert_eq!(job["params"]["asset"]["transport"], "bridge");
    assert_eq!(job["params"]["asset"]["field"], "$original-image/fills/2");
    let mut bytes = vec![31u8; 1_100_000];
    bytes[..3].copy_from_slice(&[255, 216, 255]);
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    plugin
        .send(Message::Text(
            json!({
                "kind":"devup-result", "requestId":job["requestId"],
                "data":{"kind":"devupOriginalImage", "representation":"original-image-v1",
                    "fileKey":FILE_KEY, "version":"v1", "nodeId":"n", "assetId":request.asset_id,
                    "field":request.field, "imageHash":"hash", "status":"exported",
                    "format":null, "scale":null, "mimeType":"image/jpeg", "width":100, "height":100,
                    "byteLength":bytes.len(), "sha256":hash, "data":STANDARD.encode(&bytes)}
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let result = reading.await.unwrap().unwrap();
    let original = original_image_from_result(&result, FILE_KEY, Some("v1"), &request).unwrap();
    assert_eq!(original.bytes, bytes);
}
