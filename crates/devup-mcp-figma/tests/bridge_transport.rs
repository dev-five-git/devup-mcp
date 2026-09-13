//! 브리지 전송 계층을 진짜 소켓으로 통과시킨다.
//!
//! Figma 앱 자체는 여기서 띄울 수 없으므로 플러그인 자리에 WebSocket 클라이언트를
//! 세운다. 그 너머(스크립트 실행)는 플러그인 빌드가 책임지고, 여기서는 그 앞의
//! 모든 것 — 등록, 이름·값 매핑, 상관, 봉투 모양, 라우팅 판정 — 을 확인한다.

use devup_mcp_figma::{
    BridgeFigmaClient, BridgeServer, FigmaUpstream, PreferredUpstream, ReadToolCall,
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

    // 공식 도구 이름으로 가는 읽기는 플러그인이 붙어 있어도 맡지 않는다. 응답
    // 모양이 달라 흉내 내면 두 경로가 갈라진다.
    assert!(
        !client
            .can_serve(&ReadToolCall::metadata(FILE_KEY, Some("1:2")))
            .await
    );
    // 다른 파일은 그 파일의 플러그인이 필요하다.
    assert!(
        !client
            .can_serve(&ReadToolCall::fast_snapshot("OtherFile", "1:2"))
            .await
    );
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
    // 둘 다 키가 없으면 같은 자리에 들어가므로, 서로 다른 파일의 플러그인을 하나
    // 더 붙여 "여럿"을 만든다.
    let _second = connect_plugin_as(&server, Some("AnotherFile")).await;

    assert!(
        !client
            .can_serve(&ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
            .await
    );
}
