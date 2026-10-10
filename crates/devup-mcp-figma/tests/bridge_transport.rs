//! 브리지 전송 계층을 진짜 소켓으로 통과시킨다.
//!
//! Figma 앱 자체는 여기서 띄울 수 없으므로 플러그인 자리에 WebSocket 클라이언트를
//! 세운다. 그 너머(스크립트 실행)는 플러그인 빌드가 책임지고, 여기서는 그 앞의
//! 모든 것 — 등록, 이름·값 매핑, 상관, 봉투 모양, 라우팅 판정 — 을 확인한다.

use devup_mcp_figma::{
    BridgeFigmaClient, BridgeServed, BridgeServer, DevupError, ErrorCode, FallbackUpstream,
    FigmaUpstream, PreferredUpstream, ReadToolCall, UpstreamResult,
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

    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the plugin answered; its failure is the answer");
    // 원격 `use_figma` 와 같은 모양이어야 위쪽 분기가 동일하게 동작한다 — 없는
    // 노드는 호출자 오류로, Figma 런타임의 말은 플러그인 실행 오류로.
    assert_eq!(result.raw["isError"], true);
    assert_eq!(
        result.raw["content"][0]["text"],
        "Error: DEVUP_NODE_NOT_FOUND"
    );
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

    // 메타데이터는 노드가 있든 없든 플러그인이 맡는다. 노드가 있으면 두 경로가 같은
    // `MetadataDocument` 로 환원되고, 없으면 최상위 페이지 목록을 공식 `get_metadata`
    // 와 같은 글로 쓴다.
    assert!(
        client
            .can_serve(&ReadToolCall::metadata(FILE_KEY, Some("1:2")))
            .await
    );
    assert!(
        client
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

/// 키 없는 플러그인이 보낸 빠른 봉투는 물은 파일의 것으로 디코딩된다.
///
/// 빠른 스냅샷과 테마 봉투는 키를 `source`·`snapshot` 안에 싣는다. 그 자리가 빈
/// 채로 디코더에 가면 대상 불일치로 빠른 경로를 버리는데, 실기기에서 화면마다 그랬다.
#[tokio::test]
async fn a_keyless_plugins_fast_envelopes_decode_as_the_file_asked_for() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let client = BridgeFigmaClient::new(server.state());
    let mut plugin = connect_plugin_as(&server, None).await;
    let target = devup_mcp_figma::FigmaTarget::parse(&format!(
        "https://www.figma.com/design/{FILE_KEY}/x?node-id=1-1"
    ))
    .expect("a design link");

    let reading = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .call_read_tool(ReadToolCall::fast_snapshot(FILE_KEY, "1:1"))
                .await
        })
    };
    let job = next_job(&mut plugin).await;
    let envelope = json!({
        "kind": "devupFastSnapshotEnvelope", "schemaVersion": 1,
        "source": { "fileKey": "", "rootId": "1:1" },
        "snapshot": { "fileKey": "", "version": null, "rootIds": ["1:1"],
            "nodes": [{ "id": "1:1", "type": "FRAME", "fields": { "childrenIds": [] } }],
            "diagnostics": [] },
        "resources": { "collections": [], "variables": [], "styles": [],
            "usedRemoteVariables": [], "localComplete": false, "usedRemoteComplete": true,
            "unresolved": [] },
        "integrity": { "nodeCount": 1, "variableRefCount": 0, "styleRefCount": 0, "utf8Bytes": 0 }
    });
    answer(&mut plugin, &job, envelope).await;
    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the bridge answers");
    let decoded = devup_mcp_figma::decode_fast_snapshot(&result, &target)
        .expect("a keyless plugin's envelope is the asked-for file's");
    assert_eq!(decoded.snapshot.root_ids, ["1:1"]);

    let reading = tokio::spawn(async move {
        client
            .call_read_tool(ReadToolCall::fast_theme(FILE_KEY))
            .await
    });
    let job = next_job(&mut plugin).await;
    let theme = json!({
        "kind": "devupFastThemeEnvelope", "schemaVersion": 1,
        "source": { "fileKey": "", "version": null },
        "resources": { "collections": [], "variables": [], "styles": [],
            "usedRemoteVariables": [], "usedVariableIds": [], "usedStyleIds": [],
            "localComplete": true, "usedRemoteComplete": true, "unresolved": [] },
        "integrity": { "collectionCount": 0, "variableCount": 0, "styleCount": 0,
            "unresolvedCount": 0, "utf8Bytes": 0 }
    });
    answer(&mut plugin, &job, theme).await;
    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the bridge answers");
    devup_mcp_figma::decode_fast_theme(&result, FILE_KEY)
        .expect("a keyless plugin's theme envelope is the asked-for file's");
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
        .call_read_tool(ReadToolCall::metadata("bridge:41", None))
        .await
        .expect_err("no plugin holds this connection to list its pages");
    assert_eq!(error.details["tool"], "get_metadata");

    assert_eq!(remote_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    // An ordinary key still falls through when no plugin holds it.
    upstream
        .call_read_tool(ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
        .await
        .expect("the remote path answers");
    assert_eq!(remote_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// referencePng 도 플러그인이 맡는다. 플러그인은 노드를 PNG 로 export 하고, 브리지는
/// 그것을 공식 `get_screenshot` 처럼 이미지 블록 하나로 돌려준다 — 수집기가 두 경로를
/// 같은 디코더로 받는다. 플러그인이 그 그림에 단 말은 `_meta` 에 실려 수집기로 간다.
#[tokio::test]
async fn a_reference_png_is_exported_by_the_plugin_and_comes_back_as_one_image() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let client = BridgeFigmaClient::new(server.state());
    let call = ReadToolCall::screenshot(FILE_KEY, "1:2");
    assert!(client.can_serve(&call).await);
    let reading = tokio::spawn(async move { client.call_read_tool(call).await });

    let job = next_job(&mut plugin).await;
    assert_eq!(job["script"], "referencePng");
    assert_eq!(job["params"]["nodeId"], "1:2");
    let note = "INSTANCE \"Header\" reaches into it and may show here";
    answer(
        &mut plugin,
        &job,
        json!({ "kind": "devupReferencePng", "nodeId": "1:2", "mimeType": "image/png",
            "byteLength": 4, "data": "iVBORw==", "note": note }),
    )
    .await;

    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the bridge answers");
    assert_eq!(result.raw["isError"], false);
    assert_eq!(
        result.raw["content"],
        json!([{ "type": "image", "data": "iVBORw==", "mimeType": "image/png" }])
    );
    assert!(result.raw["_meta"]["devup/bridge"].is_object());
    assert_eq!(result.raw["_meta"]["devup/referencePngNote"], note);
}

fn png_base64(width: u32, height: u32, rgba: Vec<u8>) -> String {
    use base64::Engine as _;
    let pixels = image::RgbaImage::from_raw(width, height, rgba).expect("pixels fill the image");
    let mut encoded = Vec::new();
    pixels
        .write_with_encoder(image::codecs::png::PngEncoder::new(&mut encoded))
        .expect("a PNG encodes");
    base64::engine::general_purpose::STANDARD.encode(encoded)
}

/// `get_screenshot` 은 프레임의 반투명한 자리를 캔버스로 채운 불투명한 PNG 를 준다.
/// 플러그인의 export 는 알파를 그대로 두므로, 브리지가 플러그인이 알려 준 페이지 배경
/// 위로 덮는다. 처음부터 불투명한 export 는 다시 인코딩하지 않는다.
#[tokio::test]
async fn a_translucent_reference_png_is_flattened_onto_the_page_background() {
    use base64::Engine as _;
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let read = |plugin_answer: Value| {
        let client = BridgeFigmaClient::new(server.state());
        (
            tokio::spawn(async move {
                client
                    .call_read_tool(ReadToolCall::screenshot(FILE_KEY, "1:2"))
                    .await
            }),
            plugin_answer,
        )
    };
    let answer_with = |data: String| {
        json!({ "kind": "devupReferencePng", "nodeId": "1:2", "mimeType": "image/png",
            "byteLength": 0, "data": data, "backdrop": [30, 30, 30] })
    };

    let (reading, plugin_answer) = read(answer_with(png_base64(
        2,
        1,
        vec![0, 0, 0, 178, 255, 255, 255, 255],
    )));
    let job = next_job(&mut plugin).await;
    answer(&mut plugin, &job, plugin_answer).await;
    let result = reading.await.expect("the read finishes").expect("answered");
    let data = result.raw["content"][0]["data"].as_str().expect("an image");
    let decoded = image::load_from_memory(
        &base64::engine::general_purpose::STANDARD
            .decode(data)
            .expect("base64"),
    )
    .expect("a PNG")
    .into_rgba8();
    assert_eq!(decoded.into_raw(), vec![9, 9, 9, 255, 255, 255, 255, 255]);

    let opaque = png_base64(1, 1, vec![10, 20, 30, 255]);
    let (reading, plugin_answer) = read(answer_with(opaque.clone()));
    let job = next_job(&mut plugin).await;
    answer(&mut plugin, &job, plugin_answer).await;
    let result = reading.await.expect("the read finishes").expect("answered");
    assert_eq!(result.raw["content"][0]["data"], opaque);
}

/// 플러그인이 시도하지 않았거나 답 하나에 담지 못한 읽기 — 이 devup-mcp 보다 먼저
/// 빌드돼 스크립트를 모르거나, PNG 가 답 하나보다 크다 — 는 원격이 읽을 수 있는 키면
/// 원격이 대신 답하고, 플러그인만 아는 키면 그 답이 할 일과 함께 올라간다.
#[tokio::test]
async fn a_plugin_older_than_the_server_leaves_a_keyed_read_to_the_remote_path() {
    let older = "DEVUP_BRIDGE_UNKNOWN_SCRIPT: referencePng (아는 스크립트: assets, snapshot)";
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let remote_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let upstream = std::sync::Arc::new(FallbackUpstream::new(
        BridgeFigmaClient::new(server.state()),
        CountingRemote {
            calls: remote_calls.clone(),
        },
    ));

    for (asked, refusal) in [
        older,
        "DEVUP_REFERENCE_PNG_TOO_LARGE 9000000 bytes, over the 8388608 one answer carries",
    ]
    .into_iter()
    .enumerate()
    {
        let reading = {
            let upstream = upstream.clone();
            tokio::spawn(async move {
                upstream
                    .call_read_tool(ReadToolCall::screenshot(FILE_KEY, "1:2"))
                    .await
            })
        };
        let job = next_job(&mut plugin).await;
        plugin
            .send(Message::Text(
                json!({ "kind": "devup-result", "requestId": job["requestId"], "error": refusal })
                    .to_string()
                    .into(),
            ))
            .await
            .expect("the refusal is sent");
        reading
            .await
            .expect("the read task finishes")
            .expect("the remote path answers in the plugin's place");
        assert_eq!(
            remote_calls.load(std::sync::atomic::Ordering::SeqCst),
            asked + 1,
            "{refusal}"
        );
    }
    remote_calls.store(1, std::sync::atomic::Ordering::SeqCst);

    let keyless = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut lone = connect_plugin_as(&keyless, None).await;
    let target = keyless.state().attached_files().await[0].target_key.clone();
    let upstream = std::sync::Arc::new(FallbackUpstream::new(
        BridgeFigmaClient::new(keyless.state()),
        CountingRemote {
            calls: remote_calls.clone(),
        },
    ));
    let reading = tokio::spawn(async move {
        upstream
            .call_read_tool(ReadToolCall::screenshot(target, "1:2"))
            .await
    });
    let job = next_job(&mut lone).await;
    lone.send(Message::Text(
        json!({ "kind": "devup-result", "requestId": job["requestId"], "error": older })
            .to_string()
            .into(),
    ))
    .await
    .expect("the refusal is sent");
    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the plugin's refusal is its answer");
    assert_eq!(result.raw["isError"], true);
    assert_eq!(remote_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

struct SignedOutRemote;

#[async_trait::async_trait]
impl FigmaUpstream for SignedOutRemote {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(Vec::new())
    }

    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        Err(DevupError::new(
            ErrorCode::DevupAuthRequired,
            "Figma login is required.",
            false,
        ))
    }

    async fn has_credentials(&self) -> bool {
        false
    }
}

/// 브리지 답 하나에 실리지 않는 참조 PNG 는 원격에 묻는다. 원격이 로그인 없이 거절하면
/// 답은 로그인 요구가 아니라 플러그인의 거절이다.
#[tokio::test]
async fn a_signed_out_remote_leaves_the_plugins_refusal_standing() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let upstream = FallbackUpstream::new(BridgeFigmaClient::new(server.state()), SignedOutRemote);
    let reading = tokio::spawn(async move {
        upstream
            .call_read_tool(ReadToolCall::screenshot(FILE_KEY, "1:2"))
            .await
    });
    let job = next_job(&mut plugin).await;
    plugin
        .send(Message::Text(
            json!({ "kind": "devup-result", "requestId": job["requestId"],
                "error": "DEVUP_REFERENCE_PNG_TOO_LARGE 9000000 bytes, over the 8388608 one answer carries" })
            .to_string()
            .into(),
        ))
        .await
        .expect("the refusal is sent");
    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the plugin's refusal is the answer");
    assert_eq!(result.raw["isError"], true);
    assert!(
        result.raw["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("DEVUP_REFERENCE_PNG_TOO_LARGE")),
        "{}",
        result.raw
    );
}

/// 노드 없는 metadata 읽기 — 파일의 최상위 페이지 목록 — 도 플러그인이 맡는다.
/// 플러그인의 페이지 목록을 공식 `get_metadata` 가 쓰는 글로 옮겨, 수집기가 두 경로의
/// 답을 같은 파서로 받는다.
#[tokio::test]
async fn a_files_page_list_comes_from_the_plugins_page_catalog() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let client = BridgeFigmaClient::new(server.state());
    let reading = tokio::spawn(async move {
        client
            .call_read_tool(ReadToolCall::metadata(FILE_KEY, None))
            .await
    });
    let job = next_job(&mut plugin).await;
    assert_eq!(job["script"], "pageCatalog");
    answer(
        &mut plugin,
        &job,
        json!({ "fileKey": FILE_KEY, "nodes": [
            { "id": "0:1", "type": "PAGE", "fields": { "name": "Cover" } },
            { "id": "5:2", "type": "PAGE", "fields": { "name": "Screens" } },
        ]}),
    )
    .await;
    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the bridge answers");
    assert_eq!(
        result.raw["content"][0]["text"],
        "Top-level pages of the document:\n- 0:1: Cover\n- 5:2: Screens\n"
    );
    assert!(result.raw["_meta"]["devup/bridge"].is_object());
}

/// JSON 을 플러그인이 붙들어 둘 때처럼 ASCII 로 쓴다 — 그 밖의 글자는 `\uXXXX`.
fn ascii_json(value: &Value) -> String {
    let mut text = String::new();
    for unit in serde_json::to_string(value)
        .expect("JSON encodes")
        .encode_utf16()
    {
        match u8::try_from(unit) {
            Ok(byte) if byte.is_ascii() => text.push(char::from(byte)),
            _ => text.push_str(&format!("\\u{unit:04x}")),
        }
    }
    text
}

/// 브리지 답 하나에 들어가지 않는 참조 PNG 답은 플러그인이 붙들어 두고 조각으로
/// 내준다. 브리지는 조각을 이어 받아 하나로 되돌리고, 그 답을 평소처럼 감싼다 — 노트의
/// 한글 레이어 이름까지 그대로.
#[tokio::test]
async fn a_held_reference_answer_is_read_back_in_pieces() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let client = BridgeFigmaClient::new(server.state());
    let opaque = png_base64(1, 1, vec![10, 20, 30, 255]);
    let note = "TEXT \"공지 제목\" reaches into it and may show here";
    let whole = ascii_json(&json!({ "kind": "devupReferencePng", "nodeId": "1:2",
        "mimeType": "image/png", "byteLength": 0, "data": opaque, "note": note }));
    let reading = tokio::spawn(async move {
        client
            .call_read_tool(ReadToolCall::screenshot(FILE_KEY, "1:2"))
            .await
    });

    let job = next_job(&mut plugin).await;
    assert_eq!(job["script"], "referencePng");
    answer(
        &mut plugin,
        &job,
        json!({ "kind": "devupReferencePngChunked", "nodeId": "1:2", "id": "1:2@1",
            "length": whole.len() }),
    )
    .await;
    let mut offset = 0;
    while offset < whole.len() {
        let job = next_job(&mut plugin).await;
        assert_eq!(job["script"], "referencePngChunk");
        assert_eq!(job["params"]["largeValue"]["id"], "1:2@1");
        assert_eq!(job["params"]["largeValue"]["offset"], offset);
        let end = (offset + 40).min(whole.len());
        answer(
            &mut plugin,
            &job,
            json!({ "kind": "devupReferencePngChunk", "id": "1:2@1", "offset": offset,
                "nextOffset": end, "complete": end == whole.len(), "text": &whole[offset..end] }),
        )
        .await;
        offset = end;
    }

    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the bridge answers");
    assert_eq!(result.raw["content"][0]["data"], opaque);
    assert_eq!(result.raw["_meta"]["devup/referencePngNote"], note);
}

/// 다른 레이어가 노드에 닿으면 플러그인은 노드만 export 하고 조상들을 설명한다.
/// 브리지가 그 나머지를 그려 `get_screenshot` 이 그리는 그림을 만든다 — 캔버스 export 는
/// 그리지 못할 때를 위해 함께 올 뿐 쓰이지 않는다.
#[tokio::test]
async fn an_overlapped_node_is_composed_over_its_ancestors() {
    use base64::Engine as _;
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let client = BridgeFigmaClient::new(server.state());
    let reading = tokio::spawn(async move {
        client
            .call_read_tool(ReadToolCall::screenshot(FILE_KEY, "1:2"))
            .await
    });
    let job = next_job(&mut plugin).await;
    answer(
        &mut plugin,
        &job,
        json!({ "kind": "devupReferencePng", "nodeId": "1:2", "mimeType": "image/png",
            "byteLength": 0, "data": png_base64(2, 1, vec![0, 0, 0, 255, 0, 0, 0, 255]),
            "backdrop": [30, 30, 30], "reaching": "TEXT \"Caption\"",
            "compose": {
                "area": { "x": 0, "y": 0, "width": 2, "height": 1 }, "scale": 1,
                "width": 2, "height": 1, "page": [30, 30, 30],
                "layers": [{ "transform": [[1, 0, 0], [0, 1, 0]], "width": 2, "height": 1,
                    "fills": [{ "type": "SOLID", "color": { "r": 1, "g": 0, "b": 0 } }] }],
                "node": { "picture": { "data": png_base64(1, 1, vec![255, 255, 255, 255]),
                    "area": { "x": 1, "y": 0, "width": 1, "height": 1 } } },
                "images": {}
            } }),
    )
    .await;
    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the bridge answers");
    let data = result.raw["content"][0]["data"].as_str().expect("an image");
    let decoded = image::load_from_memory(
        &base64::engine::general_purpose::STANDARD
            .decode(data)
            .expect("base64"),
    )
    .expect("a PNG")
    .into_rgba8();
    assert_eq!(decoded.into_raw(), vec![255, 0, 0, 255, 255, 255, 255, 255]);
    assert!(result.raw["_meta"].get("devup/referencePngNote").is_none());
    assert!(
        result.raw["_meta"]
            .get("devup/referencePngApproximate")
            .is_none()
    );
}

/// 원격 경로로 다시 읽으려 한 횟수를 센다. `room` 이 거짓이면 한도 자리가 없다고 답한다.
struct CountingMeter {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    room: bool,
}

#[async_trait::async_trait]
impl devup_mcp_figma::Meter for CountingMeter {
    async fn acquire_within(&self, _budget: std::time::Duration) -> bool {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.room
    }
}

fn meter(
    calls: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
    room: bool,
) -> std::sync::Arc<CountingMeter> {
    std::sync::Arc::new(CountingMeter {
        calls: calls.clone(),
        room,
    })
}

/// 정해 둔 답을 내는 원격. 몇 번 불렸는지 센다.
struct FixedRemote {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    raw: Value,
}

#[async_trait::async_trait]
impl FigmaUpstream for FixedRemote {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(UpstreamResult {
            raw: self.raw.clone(),
        })
    }
}

struct Counts {
    remote: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    metered: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Counts {
    fn new() -> Self {
        Self {
            remote: std::sync::Arc::default(),
            metered: std::sync::Arc::default(),
        }
    }

    fn get(&self) -> (usize, usize) {
        use std::sync::atomic::Ordering::SeqCst;
        (self.remote.load(SeqCst), self.metered.load(SeqCst))
    }
}

/// 플러그인에 `call` 을 보내 `plugin_answer` 로 답하게 하고, 그 뒤의 결과를 돌려준다.
async fn through_fallback(
    upstream: FallbackUpstream<BridgeFigmaClient, impl FigmaUpstream + 'static>,
    plugin: &mut Plugin,
    call: ReadToolCall,
    plugin_answer: Value,
) -> UpstreamResult {
    let reading = tokio::spawn(async move { upstream.call_read_tool(call).await });
    let job = next_job(plugin).await;
    let mut message = plugin_answer;
    message["kind"] = json!("devup-result");
    message["requestId"] = job["requestId"].clone();
    plugin
        .send(Message::Text(message.to_string().into()))
        .await
        .expect("the answer is sent");
    reading
        .await
        .expect("the read task finishes")
        .expect("an answer comes back")
}

fn fixed_fallback(
    server: &BridgeServer,
    counts: &Counts,
    raw: Value,
) -> FallbackUpstream<BridgeFigmaClient, FixedRemote> {
    FallbackUpstream::new(
        BridgeFigmaClient::new(server.state()),
        FixedRemote {
            calls: counts.remote.clone(),
            raw,
        },
    )
    .with_meter(meter(&counts.metered, true))
}

const UNREACHABLE: &str = "Unable to establish connection to Figma after 10 seconds. Please check your internet connection.";

/// 파일이 화면에 보이지 않으면 Figma 는 불러오지 않은 데이터를 플러그인에게 주지
/// 않는다. 그 말이 섞인 플러그인의 답은 원격 경로가 다시 읽는다 — 원격은 그 상태와
/// 무관하다. 한도를 쓰는 읽기라 속도에 맞춰 보내고, 왜 다시 읽었는지 결과에 남긴다.
#[tokio::test]
async fn a_read_figma_withheld_out_of_sight_is_read_again_on_the_direct_path() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let short = json!({ "data": { "fileKey": FILE_KEY, "resources": { "unresolved": [
        { "id": "VariableID:1:2", "reason": "notFoundOrUnavailable", "error": UNREACHABLE }] } } });

    let counts = Counts::new();
    let direct = json!({ "content": [{ "type": "text", "text": "{\"fileKey\":\"FileKey123\"}" }] });
    let result = through_fallback(
        fixed_fallback(&server, &counts, direct.clone()),
        &mut plugin,
        ReadToolCall::fast_snapshot(FILE_KEY, "1:2"),
        short.clone(),
    )
    .await;
    assert_eq!(result.raw["content"], direct["content"]);
    assert_eq!(
        result.raw["_meta"]["devup/directFallback"],
        "file-out-of-sight"
    );
    assert_eq!(counts.get(), (1, 1));

    // 원격의 답이 공식 MCP 의 20 KB 에서 잘렸으면 디코딩되지 않으니 플러그인의 답을 쓴다.
    let counts = Counts::new();
    let truncated = json!({ "content": [{ "type": "text",
        "text": "{\"fileKey\":\"FileKey123\",\"nodes\":[ // truncated to 20kb" }] });
    let result = through_fallback(
        fixed_fallback(&server, &counts, truncated),
        &mut plugin,
        ReadToolCall::fast_snapshot(FILE_KEY, "1:2"),
        short.clone(),
    )
    .await;
    assert!(
        result.raw["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("notFoundOrUnavailable")),
        "{}",
        result.raw
    );
    assert_eq!(counts.get(), (1, 1));

    // 한도에 걸렸다는 문장은 `isError` 없이도 온다. 그것도 쓸 수 없는 답이니 플러그인의 답을 쓴다.
    let counts = Counts::new();
    let refusal = json!({ "content": [{ "type": "text",
        "text": "You've reached the tool call limit for your plan. Try again later." }] });
    let result = through_fallback(
        fixed_fallback(&server, &counts, refusal),
        &mut plugin,
        ReadToolCall::fast_snapshot(FILE_KEY, "1:2"),
        short.clone(),
    )
    .await;
    assert!(
        result.raw["_meta"]["devup/bridge"].is_object(),
        "{}",
        result.raw
    );
    assert_eq!(counts.get(), (1, 1));

    // 정해진 시간 안에 한도 자리가 나지 않으면 기다리지 않고 플러그인의 답을 쓴다 — 이미 받은
    // 답을 읽기 시간 초과로 잃지 않는다.
    let counts = Counts::new();
    let upstream = FallbackUpstream::new(
        BridgeFigmaClient::new(server.state()),
        FixedRemote {
            calls: counts.remote.clone(),
            raw: direct.clone(),
        },
    )
    .with_meter(meter(&counts.metered, false));
    let result = through_fallback(
        upstream,
        &mut plugin,
        ReadToolCall::fast_snapshot(FILE_KEY, "1:2"),
        short.clone(),
    )
    .await;
    assert!(
        result.raw["_meta"]["devup/bridge"].is_object(),
        "{}",
        result.raw
    );
    assert_eq!(counts.get(), (0, 1));

    // 디자인의 글에 같은 문장이 있다고 다시 읽지 않는다. 오류를 싣는 자리에 있을 때만이다.
    let counts = Counts::new();
    let quoted = json!({ "data": { "fileKey": FILE_KEY, "nodes": [{ "id": "1:2", "type": "TEXT",
        "fields": { "characters": UNREACHABLE } }] } });
    let result = through_fallback(
        fixed_fallback(&server, &counts, direct.clone()),
        &mut plugin,
        ReadToolCall::fast_snapshot(FILE_KEY, "1:2"),
        quoted,
    )
    .await;
    assert!(
        result.raw["_meta"]["devup/bridge"].is_object(),
        "{}",
        result.raw
    );
    assert_eq!(counts.get(), (0, 0));

    // 로그인이 없으면 원격은 시도도 되지 않는다. 속도에 맞춰 기다리지 않고 플러그인의 답을 쓴다.
    let metered = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let upstream = FallbackUpstream::new(BridgeFigmaClient::new(server.state()), SignedOutRemote)
        .with_meter(meter(&metered, true));
    let result = through_fallback(
        upstream,
        &mut plugin,
        ReadToolCall::fast_snapshot(FILE_KEY, "1:2"),
        short,
    )
    .await;
    assert!(result.raw["_meta"]["devup/bridge"].is_object());
    assert_eq!(metered.load(std::sync::atomic::Ordering::SeqCst), 0);
}

/// `wait` 만큼 기다려야 자리가 나는 한도. `CallPacer` 처럼, 주어진 시간 안에 자리가 나지
/// 않으면 기다리지 않고 거절한다.
struct PacedMeter {
    wait: std::time::Duration,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl devup_mcp_figma::Meter for PacedMeter {
    async fn acquire_within(&self, budget: std::time::Duration) -> bool {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.wait > budget {
            return false;
        }
        tokio::time::sleep(self.wait).await;
        true
    }
}

/// `delay` 뒤에 `get_screenshot` 과 다 같게 그리지 못한 그림으로 답하는 플러그인.
struct SlowPlugin {
    delay: std::time::Duration,
}

#[async_trait::async_trait]
impl FigmaUpstream for SlowPlugin {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        tokio::time::sleep(self.delay).await;
        Ok(UpstreamResult {
            raw: json!({ "content": [{ "type": "image", "data": "cGx1Z2lu", "mimeType": "image/png" }],
                "_meta": { "devup/referencePngApproximate": "an ancestor's stroke" } }),
        })
    }
}

#[async_trait::async_trait]
impl PreferredUpstream for SlowPlugin {
    async fn can_serve(&self, _call: &ReadToolCall) -> bool {
        true
    }

    fn is_live(&self) -> bool {
        true
    }
}

/// 로그인을 확인하는 데 `sign_in` 이 걸리는 원격. 늘 정해 둔 그림으로 답한다.
struct SlowSignIn {
    sign_in: std::time::Duration,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl FigmaUpstream for SlowSignIn {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(UpstreamResult {
            raw: json!({ "content": [{ "type": "image", "data": "ZGlyZWN0", "mimeType": "image/png" }] }),
        })
    }

    async fn has_credentials(&self) -> bool {
        tokio::time::sleep(self.sign_in).await;
        true
    }
}

/// 원격으로 다시 읽는 일 — 로그인 확인, 한도 자리, 원격의 답 — 은 읽기 하나의 시간
/// (`READ_TIME_LIMIT`)에서 플러그인이 쓰고 남은 만큼만 한다. 60초 걸려 온 플러그인의
/// 그림은, 35초를 기다려야 자리가 나는 한도나 35초 걸리는 로그인 확인 때문에 읽기의 시간
/// 제한에서 잃지 않는다. 시간이 남았으면 그대로 원격이 다시 읽는다.
#[tokio::test(start_paused = true)]
async fn re_reading_on_the_direct_path_fits_in_what_the_plugin_left_of_the_read() {
    for (plugin_took, meter_wait, sign_in, counted) in [
        (60, 35, 0, (0, 1)),
        (60, 0, 35, (0, 0)),
        (10, 35, 0, (1, 1)),
    ] {
        let counts = Counts::new();
        let upstream = FallbackUpstream::new(
            SlowPlugin {
                delay: std::time::Duration::from_secs(plugin_took),
            },
            SlowSignIn {
                sign_in: std::time::Duration::from_secs(sign_in),
                calls: counts.remote.clone(),
            },
        )
        .with_meter(std::sync::Arc::new(PacedMeter {
            wait: std::time::Duration::from_secs(meter_wait),
            calls: counts.metered.clone(),
        }));
        let result = tokio::time::timeout(
            devup_mcp_figma::READ_TIME_LIMIT,
            upstream.call_read_tool(ReadToolCall::fast_snapshot(FILE_KEY, "1:2")),
        )
        .await
        .expect("answered within the read's time")
        .expect("an answer");
        let expected = if counted.0 == 0 {
            "cGx1Z2lu"
        } else {
            "ZGlyZWN0"
        };
        let case = format!("plugin {plugin_took} s, meter {meter_wait} s, sign-in {sign_in} s");
        assert_eq!(result.raw["content"][0]["data"], expected, "{case}");
        assert_eq!(counts.get(), counted, "{case}");
    }
}

/// 플러그인이 `get_screenshot` 과 같은 그림을 그리지 못하면 — 여기서는 그릴 수 없는
/// 칠 — 캔버스 그림에 이유를 달고, 원격이 열려 있으면 원격의 `get_screenshot` 이 대신
/// 답한다. 열려 있지 않으면 캔버스 그림과 그 이유가 답이다.
#[tokio::test]
async fn a_picture_the_plugin_cannot_compose_goes_to_get_screenshot() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let canvas = png_base64(1, 1, vec![10, 20, 30, 255]);
    let uncomposable = json!({ "data": { "kind": "devupReferencePng", "nodeId": "1:2",
        "mimeType": "image/png", "byteLength": 0, "data": canvas, "backdrop": [30, 30, 30],
        "reaching": "TEXT \"Caption\"",
        "compose": { "area": { "x": 0, "y": 0 }, "width": 1, "height": 1, "page": [30, 30, 30],
            "layers": [{ "transform": [[1, 0, 0], [0, 1, 0]], "width": 1, "height": 1,
                "fills": [{ "type": "VIDEO" }] }],
            "node": { "picture": { "data": canvas, "area": { "x": 0, "y": 0 } } } } } });

    let counts = Counts::new();
    let shot = json!({ "content": [{ "type": "image", "data": "aWdub3JlZA==", "mimeType": "image/png" }] });
    let result = through_fallback(
        fixed_fallback(&server, &counts, shot.clone()),
        &mut plugin,
        ReadToolCall::screenshot(FILE_KEY, "1:2"),
        uncomposable.clone(),
    )
    .await;
    assert_eq!(result.raw["content"], shot["content"]);
    assert_eq!(
        result.raw["_meta"]["devup/directFallback"],
        "reference-png-approximate"
    );
    assert_eq!(counts.get(), (1, 1));

    let upstream = FallbackUpstream::new(BridgeFigmaClient::new(server.state()), SignedOutRemote);
    let result = through_fallback(
        upstream,
        &mut plugin,
        ReadToolCall::screenshot(FILE_KEY, "1:2"),
        uncomposable,
    )
    .await;
    assert_eq!(result.raw["content"][0]["data"], canvas);
    let note = result.raw["_meta"]["devup/referencePngNote"]
        .as_str()
        .expect("the canvas picture says why");
    assert!(
        note.contains("VIDEO") && note.contains("TEXT \"Caption\""),
        "{note}"
    );
}

/// 플러그인이 답하지 못해 원격이 읽은 자산의 이어 읽기는, 플러그인이 다시 export 한
/// 바이트가 원격의 해시와 다르면 원격이 답한다. 그 바이트는 원격의 렌더러에서만 나온다.
#[tokio::test]
async fn a_value_the_direct_path_produced_is_read_on_from_it() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let counts = Counts::new();
    let fragment = json!({ "content": [{ "type": "text", "text": "{\"kind\":\"devupLargeValueFragment\"}" }] });
    let call = ReadToolCall::large_value(
        FILE_KEY,
        devup_mcp_figma::LargeValueReadOptions {
            node_id: "1:2".to_owned(),
            field: "$export/png/1".to_owned(),
            offset: 8192,
            max_chunk_bytes: 8192,
            byte_length: 20_000,
            sha256: "0".repeat(64),
            version: None,
        },
    );
    let result = through_fallback(
        fixed_fallback(&server, &counts, fragment.clone()),
        &mut plugin,
        call,
        json!({ "error": "DEVUP_LARGE_VALUE_CHANGED" }),
    )
    .await;
    assert_eq!(result.raw["content"], fragment["content"]);
    assert_eq!(
        result.raw["_meta"]["devup/directFallback"],
        "value-from-direct-path"
    );
    assert_eq!(counts.get(), (1, 1));
}

/// 플러그인이 큰 답들을 붙들고 있어 바쁘다고 하면 브리지는 잠시 뒤 다시 묻는다. 붙들어 둔
/// 답이 읽기 전에 사라졌으면(플러그인이 다시 실행되는 등) 한 번 다시 받는다. 어느 쪽도 실패가
/// 아니다.
#[tokio::test]
async fn a_busy_plugin_or_a_vanished_answer_is_asked_again() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let opaque = png_base64(1, 1, vec![10, 20, 30, 255]);
    let picture = json!({ "kind": "devupReferencePng", "nodeId": "1:2", "mimeType": "image/png",
        "byteLength": 0, "data": opaque });
    let refuse = |job: &Value, error: &str| {
        Message::Text(
            json!({ "kind": "devup-result", "requestId": job["requestId"], "error": error })
                .to_string()
                .into(),
        )
    };

    let client = BridgeFigmaClient::new(server.state());
    let reading = tokio::spawn(async move {
        client
            .call_read_tool(ReadToolCall::screenshot(FILE_KEY, "1:2"))
            .await
    });
    let job = next_job(&mut plugin).await;
    plugin
        .send(refuse(
            &job,
            "DEVUP_REFERENCE_PNG_BUSY 201326592 characters are held for other reads",
        ))
        .await
        .expect("busy is sent");
    let job = next_job(&mut plugin).await;
    assert_eq!(job["script"], "referencePng");
    answer(&mut plugin, &job, picture.clone()).await;
    let result = reading.await.expect("the read finishes").expect("answered");
    assert_eq!(result.raw["content"][0]["data"], opaque);

    let client = BridgeFigmaClient::new(server.state());
    let reading = tokio::spawn(async move {
        client
            .call_read_tool(ReadToolCall::screenshot(FILE_KEY, "1:2"))
            .await
    });
    let job = next_job(&mut plugin).await;
    answer(
        &mut plugin,
        &job,
        json!({ "kind": "devupReferencePngChunked", "nodeId": "1:2", "id": "1:2@1", "length": 10 }),
    )
    .await;
    let job = next_job(&mut plugin).await;
    assert_eq!(job["script"], "referencePngChunk");
    plugin
        .send(refuse(&job, "DEVUP_REFERENCE_PNG_ANSWER_GONE 1:2@1"))
        .await
        .expect("gone is sent");
    let job = next_job(&mut plugin).await;
    assert_eq!(job["script"], "referencePng");
    answer(&mut plugin, &job, picture).await;
    let result = reading.await.expect("the read finishes").expect("answered");
    assert_eq!(result.raw["content"][0]["data"], opaque);
}

/// 플러그인이 답하지 못한 읽기(예전 플러그인이 모르는 스크립트)를 원격으로 넘기려는데 정해진
/// 시간 안에 한도 자리가 나지 않으면, 한도가 찬 것으로 알려 수집이 기다렸다 다시 하게 한다.
#[tokio::test]
async fn a_read_the_plugin_cannot_answer_waits_for_room_rather_than_fail() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = connect_plugin(&server).await;
    let counts = Counts::new();
    let upstream = FallbackUpstream::new(
        BridgeFigmaClient::new(server.state()),
        FixedRemote {
            calls: counts.remote.clone(),
            raw: json!({ "content": [] }),
        },
    )
    .with_meter(meter(&counts.metered, false));
    let reading = tokio::spawn(async move {
        upstream
            .call_read_tool(ReadToolCall::screenshot(FILE_KEY, "1:2"))
            .await
    });
    let job = next_job(&mut plugin).await;
    plugin
        .send(Message::Text(
            json!({ "kind": "devup-result", "requestId": job["requestId"],
                "error": "DEVUP_BRIDGE_UNKNOWN_SCRIPT: referencePng (아는 스크립트: snapshot)" })
            .to_string()
            .into(),
        ))
        .await
        .expect("the refusal is sent");
    let error = reading
        .await
        .expect("the read finishes")
        .expect_err("there was no room to read it on the metered path");
    assert_eq!(error.code, ErrorCode::DevupFigmaRateLimited);
    assert!(error.retryable);
    assert_eq!(counts.get(), (0, 1));
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
