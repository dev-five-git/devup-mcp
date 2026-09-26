//! 한 기기에 devup-mcp 가 여럿 떠 있어도 브리지를 함께 쓰는지 진짜 소켓으로 본다.
//!
//! 플러그인이 붙을 수 있는 포트는 하나다 — manifest 의 `allowedDomains` 가
//! `ws://localhost:1993` 하나뿐이고, Figma 는 그 목록을 실행 중에 바꾸지 못한다.
//! 그래서 포트를 잡은 프로세스(호스트)가 플러그인을 받고, 잡지 못한 프로세스는
//! 호스트를 통해 읽는다. 여기서는 임의 포트 하나에 서버 여럿과 가짜 플러그인
//! 하나를 세운다. 1993 은 건드리지 않는다.

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use devup_mcp_figma::{
    BridgeFigmaClient, BridgeRole, BridgeServer, FigmaUpstream, PreferredUpstream, ReadToolCall,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{self, Message, client::IntoClientRequest, http::HeaderValue},
};

const FILE_KEY: &str = "FileKey123";

/// 넉넉하게 잡는다. 느린 CI 러너에서도 흔들리지 않아야 하고, 이 안에 끝나지
/// 않으면 기다림이 아니라 결함이다.
const DEADLINE: Duration = Duration::from_secs(30);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// 조건이 설 때까지 기다린다. 시간을 짐작해 재우지 않고, 관찰한 상태로 판정한다.
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

/// 포트를 잡지 못한 devup-mcp. 예전에는 여기서 `None` 이었고, 그 프로세스는
/// 끝까지 브리지를 쓰지 못했다.
fn second_process_on(port: u16) -> BridgeServer {
    BridgeServer::start(port).expect(
        "a devup-mcp that finds the bridge port held must still get a bridge, through the \
         process that holds it",
    )
}

/// 플러그인 흉내: 호스트에 붙어 자기 파일을 알린다.
async fn plugin(port: u16) -> Socket {
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{port}/plugin"))
        .await
        .expect("the holder accepts a plugin");
    socket
        .send(Message::Text(
            json!({
                "kind": "hello", "fileKey": FILE_KEY, "fileName": "Landing",
                "currentPage": { "id": "0:1", "name": "Page 1" },
                "selection": [{ "id": "1:2", "name": "Hero", "type": "FRAME" }],
                "selectionCount": 1,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("hello is sent");
    socket
}

async fn sees_the_plugin(server: &BridgeServer) {
    let state = server.state();
    eventually("the plugin attached to the holder to show up here", || {
        let state = state.clone();
        async move { state.connected_files().await == vec![FILE_KEY.to_owned()] }
    })
    .await;
}

async fn next_text(socket: &mut Socket) -> Value {
    loop {
        let message = tokio::time::timeout(DEADLINE, socket.next())
            .await
            .expect("a message arrives in time")
            .expect("the socket stays open")
            .expect("the socket reads cleanly");
        if let Message::Text(text) = message {
            return serde_json::from_str(&text).expect("messages are JSON");
        }
    }
}

async fn answer(plugin: &mut Socket, job: &Value, data: Value) {
    plugin
        .send(Message::Text(
            json!({ "kind": "devup-result", "requestId": job["requestId"], "data": data })
                .to_string()
                .into(),
        ))
        .await
        .expect("the result is sent");
}

/// 상대가 연결을 닫을 때까지 읽는다. 닫지 않으면 실패다.
async fn assert_closed(socket: &mut Socket, why: &str) {
    let closed = tokio::time::timeout(DEADLINE, async {
        loop {
            match socket.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "{why}");
}

/// 받아들일 수 없는 핸드셰이크의 HTTP 상태. 업그레이드되면 `None`.
async fn refused_status(request: tungstenite::handshake::client::Request) -> Option<u16> {
    match connect_async(request).await {
        Err(tungstenite::Error::Http(response)) => Some(response.status().as_u16()),
        Err(error) => panic!("the holder answered with something other than HTTP: {error}"),
        Ok(_) => None,
    }
}

/// 이 라운드가 고치는 결함 그 자체다. 포트를 잡지 못한 프로세스가 플러그인이
/// 붙은 호스트를 통해 읽기를 끝내고, 그 답은 프로세스가 하나일 때와 같은
/// 모양이다 — 디코더는 `content[].text` 안의 JSON 을 찾는다.
#[tokio::test]
async fn a_process_that_found_the_port_held_reads_through_the_holder() {
    let host = BridgeServer::start(0).expect("an ephemeral port is free");
    let port = host.port();
    let relay = second_process_on(port);
    let mut plugin = plugin(port).await;
    sees_the_plugin(&relay).await;

    let client = BridgeFigmaClient::new(relay.state()).with_port(port);
    assert!(
        client
            .can_serve(&ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
            .await
    );
    assert!(client.serves_without_credentials(FILE_KEY).await);

    let reading = tokio::spawn(async move {
        client
            .call_read_tool(ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
            .await
    });
    let job = next_text(&mut plugin).await;
    assert_eq!(job["kind"], "devup-job");
    assert_eq!(job["script"], "fastSnapshot");
    assert_eq!(job["params"]["nodeId"], "1:2");
    let payload = json!({ "fileKey": FILE_KEY, "nodes": [{ "id": "1:2" }] });
    answer(&mut plugin, &job, payload.clone()).await;

    let result = reading
        .await
        .expect("the read task finishes")
        .expect("the read is served through the holder");
    let text = result.raw["content"][0]["text"]
        .as_str()
        .expect("the payload rides in content[].text, as it does with one process");
    assert_eq!(serde_json::from_str::<Value>(text).unwrap(), payload);
    assert_eq!(result.raw["isError"], false);
    let served = &result.raw["_meta"]["devup/bridge"];
    assert_eq!(served["port"], port);
    assert_eq!(served["fileKey"], FILE_KEY);
    assert_eq!(served["fileName"], "Landing");
    assert_eq!(served["pageName"], "Page 1");
}

/// 요청 번호는 프로세스마다 따로 센다. 여러 프로세스의 읽기가 한 호스트로
/// 모여도 플러그인이 보는 번호는 겹치지 않아야 하고, 답은 물은 프로세스에게만
/// 가야 한다. 도착 순서의 반대로 답해, 순서로 짝을 맞추는 구현이면 드러나게 한다.
#[tokio::test]
async fn reads_from_several_processes_at_once_each_get_their_own_answer() {
    let host = BridgeServer::start(0).expect("an ephemeral port is free");
    let port = host.port();
    let first = second_process_on(port);
    let second = second_process_on(port);
    let mut plugin = plugin(port).await;
    for server in [&host, &first, &second] {
        sees_the_plugin(server).await;
    }

    let mut readers = Vec::new();
    for (server, node) in [(&host, "1:1"), (&first, "2:2"), (&second, "3:3")] {
        let client = BridgeFigmaClient::new(server.state()).with_port(port);
        readers.push((
            node,
            tokio::spawn(async move {
                client
                    .call_read_tool(ReadToolCall::fast_snapshot(FILE_KEY, node))
                    .await
            }),
        ));
    }

    let mut jobs = Vec::new();
    for _ in 0..readers.len() {
        jobs.push(next_text(&mut plugin).await);
    }
    let ids: HashSet<String> = jobs
        .iter()
        .map(|job| job["requestId"].as_str().expect("a request id").to_owned())
        .collect();
    assert_eq!(
        ids.len(),
        3,
        "every read reaches the plugin under its own id"
    );

    jobs.reverse();
    for job in &jobs {
        let node = job["params"]["nodeId"].clone();
        answer(
            &mut plugin,
            job,
            json!({ "fileKey": FILE_KEY, "nodes": [{ "id": node }] }),
        )
        .await;
    }
    for (node, reader) in readers {
        let result = reader
            .await
            .expect("the read task finishes")
            .expect("every process gets an answer");
        let text = result.raw["content"][0]["text"].as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            payload["nodes"][0]["id"], node,
            "an answer went to a process that did not ask for it"
        );
    }
}

/// 중계 문은 같은 사용자의 devup-mcp 에게만 열린다.
///
/// 브라우저 페이지는 `ws://localhost` 에 붙을 수 있고, 그 요청에는 늘 `Origin`
/// 이 붙는다. 로컬의 다른 프로그램은 `Origin` 없이 붙을 수 있지만 비밀값을 모르니
/// 증명을 내지 못한다. 어느 쪽도 열린 Figma 문서를 읽는 통로가 되면 안 된다.
#[tokio::test]
async fn the_relay_door_turns_away_browsers_and_unproven_callers() {
    let host = BridgeServer::start(0).expect("an ephemeral port is free");
    let port = host.port();
    let mut plugin = plugin(port).await;
    sees_the_plugin(&host).await;
    let relay_url = format!("ws://127.0.0.1:{port}/relay");

    let mut from_a_page = relay_url.as_str().into_client_request().unwrap();
    from_a_page
        .headers_mut()
        .insert("Origin", HeaderValue::from_static("https://example.com"));
    assert_eq!(
        refused_status(from_a_page).await,
        Some(403),
        "a relay handshake carrying Origin comes from a browser page and is refused before \
         the upgrade"
    );

    // Skips the proof and asks for a read straight away.
    let (mut stranger, _) = connect_async(relay_url.as_str())
        .await
        .expect("a native client reaches the handshake");
    let hello = next_text(&mut stranger).await;
    assert_eq!(hello["kind"], "relay-hello");
    assert!(hello["protocol"].is_u64(), "{hello}");
    stranger
        .send(Message::Text(
            json!({
                "kind": "relay-job", "id": 1, "fileKey": FILE_KEY,
                "script": "fastSnapshot", "params": { "nodeId": "1:2" },
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_text(&mut stranger).await["kind"], "relay-refused");
    assert_closed(&mut stranger, "an unauthenticated relay is disconnected").await;

    // Offers a proof it could not have made.
    let (mut forger, _) = connect_async(relay_url.as_str()).await.unwrap();
    let hello = next_text(&mut forger).await;
    forger
        .send(Message::Text(
            json!({
                "kind": "relay-auth", "protocol": hello["protocol"],
                "nonce": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "proof": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "peer": { "pid": 1, "version": "0.0.0", "buildId": null },
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let refused = next_text(&mut forger).await;
    assert_eq!(refused["kind"], "relay-refused");
    assert_eq!(refused["reason"], "auth");
    assert_closed(&mut forger, "a relay with a wrong proof is disconnected").await;

    // Says nothing at all: it is not kept waiting on the door.
    let (mut silent, _) = connect_async(relay_url.as_str()).await.unwrap();
    let _hello = next_text(&mut silent).await;
    assert_closed(
        &mut silent,
        "a relay that never proves itself is disconnected",
    )
    .await;

    // None of them got a read through. A forwarded job would have been queued
    // on this socket long before now, so a short look is enough to see none.
    assert!(
        tokio::time::timeout(Duration::from_millis(300), plugin.next())
            .await
            .is_err(),
        "no job may reach the plugin from an unauthenticated relay"
    );
}

/// 이어받기와 인계가 이중 호스트를 만들지 않는다는 근거: 잡힌 포트는 누구도
/// 다시 bind 하지 못한다. std 는 Windows 에서 `SO_REUSEADDR` 를 켜지 않고
/// (켜면 잡힌 포트를 가로챌 수 있다), Unix 의 `SO_REUSEADDR` 는 듣고 있는
/// 소켓과 같은 주소를 허락하지 않는다. 세 OS 의 CI 가 이 테스트로 그것을 잰다.
#[tokio::test]
async fn a_held_port_is_never_bound_twice() {
    let host = BridgeServer::start(0).expect("an ephemeral port is free");
    assert!(
        std::net::TcpListener::bind(("127.0.0.1", host.port())).is_err(),
        "a second listener on the held bridge port would be a second host"
    );
}

/// `plugin/src/ui.ts` 처럼 소켓이 닫히면 다시 붙는 플러그인. 받은 작업마다 물은
/// 노드를 담아 곧장 답한다. 테스트가 빨리 끝나도록 재시도 간격만 줄였다.
fn reconnecting_plugin(port: u16) {
    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) =
                connect_async(format!("ws://127.0.0.1:{port}/plugin")).await
            {
                let hello = json!({
                    "kind": "hello", "fileKey": FILE_KEY, "fileName": "Landing",
                    "currentPage": { "id": "0:1", "name": "Page 1" },
                    "selection": [], "selectionCount": 0,
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
                        let job: Value = serde_json::from_str(&text).expect("a job is JSON");
                        let data = json!({
                            "fileKey": FILE_KEY, "nodes": [{ "id": job["params"]["nodeId"] }]
                        });
                        let result = json!({
                            "kind": "devup-result", "requestId": job["requestId"], "data": data
                        });
                        if socket
                            .send(Message::Text(result.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
}

async fn role_of(client: &BridgeFigmaClient) -> (BridgeRole, bool) {
    let snapshot = client
        .bridge_path_snapshot()
        .await
        .expect("a bridge client always reports its path");
    (snapshot.role, snapshot.available())
}

/// 클라이언트 세션이 끝나면 호스트는 사라진다. 흔한 일이다. 남은 프로세스 가운데
/// 정확히 하나가 포트를 이어받고, 다른 하나는 그를 통해 읽으며, 플러그인이 다시
/// 붙은 뒤에는 둘 다 읽는다. 사람이 무엇을 죽이거나 다시 띄울 필요가 없다.
#[tokio::test]
async fn when_the_holder_goes_away_exactly_one_remaining_process_takes_the_port_over() {
    let host = BridgeServer::start(0).expect("an ephemeral port is free");
    let port = host.port();
    let first = second_process_on(port);
    let second = second_process_on(port);
    reconnecting_plugin(port);
    for server in [&host, &first, &second] {
        sees_the_plugin(server).await;
    }

    host.shutdown();

    let clients =
        [&first, &second].map(|server| BridgeFigmaClient::new(server.state()).with_port(port));
    eventually(
        "one remaining process to hold the port and the other to read through it, both \
         seeing the plugin again",
        || {
            let clients = &clients;
            async move {
                let roles = [role_of(&clients[0]).await, role_of(&clients[1]).await];
                let hosts = roles.iter().filter(|(role, _)| *role == BridgeRole::Host);
                let relays = roles.iter().filter(|(role, _)| *role == BridgeRole::Relay);
                hosts.count() == 1
                    && relays.count() == 1
                    && roles.iter().all(|(_, available)| *available)
            }
        },
    )
    .await;
    assert!(
        std::net::TcpListener::bind(("127.0.0.1", port)).is_err(),
        "the port is held again"
    );
    for (client, node) in clients.iter().zip(["4:4", "5:5"]) {
        let result = client
            .call_read_tool(ReadToolCall::fast_snapshot(FILE_KEY, node))
            .await
            .expect("reads resume through whichever process holds the port now");
        let text = result.raw["content"][0]["text"].as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        assert_eq!(payload["nodes"][0]["id"], node);
    }
}

/// 호스트가 떠날 때 진행 중이던 읽기는 90초를 기다리지 않고 곧장 실패한다. 이유를
/// 말하고, 다시 부르면 된다는 것도 말한다.
#[tokio::test]
async fn a_read_in_flight_when_the_holder_leaves_fails_at_once() {
    let host = BridgeServer::start(0).expect("an ephemeral port is free");
    let port = host.port();
    let relay = second_process_on(port);
    let mut plugin = plugin(port).await;
    sees_the_plugin(&relay).await;

    let client = BridgeFigmaClient::new(relay.state()).with_port(port);
    let reading = tokio::spawn(async move {
        client
            .call_read_tool(ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
            .await
    });
    let _job = next_text(&mut plugin).await;
    host.shutdown();

    let error = tokio::time::timeout(DEADLINE, reading)
        .await
        .expect("the read ends without waiting out its 90 seconds")
        .expect("the read task finishes")
        .expect_err("the process that held the port is gone");
    assert!(error.message.contains("went away"), "{}", error.message);
}

/// 읽기를 맡긴 프로세스가 사라지면 호스트는 그 읽기를 거둔다. 플러그인에는 취소를
/// 보낼 길이 없어 작업은 끝까지 돌지만, 늦게 온 답은 누구에게도 가지 않고 대기표도
/// 남지 않는다.
#[tokio::test]
async fn a_read_is_cleared_from_the_holder_when_the_process_that_asked_goes_away() {
    let host = BridgeServer::start(0).expect("an ephemeral port is free");
    let port = host.port();
    let relay = second_process_on(port);
    let mut plugin = plugin(port).await;
    sees_the_plugin(&relay).await;

    let client = BridgeFigmaClient::new(relay.state()).with_port(port);
    let reading = tokio::spawn(async move {
        client
            .call_read_tool(ReadToolCall::fast_snapshot(FILE_KEY, "1:2"))
            .await
    });
    let job = next_text(&mut plugin).await;
    let state = host.state();
    assert_eq!(state.pending_reads(), 1, "the holder waits for the plugin");

    relay.shutdown();
    let error = tokio::time::timeout(DEADLINE, reading)
        .await
        .expect("the read ends promptly in the process that is leaving")
        .expect("the read task finishes");
    assert!(error.is_err());
    eventually("the holder to drop the read nobody waits for", || {
        let state = state.clone();
        async move { state.pending_reads() == 0 }
    })
    .await;

    answer(
        &mut plugin,
        &job,
        json!({ "fileKey": FILE_KEY, "nodes": [] }),
    )
    .await;
    // The late answer reached the holder and was dropped there; the plugin is
    // still served normally afterwards.
    let client = BridgeFigmaClient::new(host.state()).with_port(port);
    let reading = tokio::spawn(async move {
        client
            .call_read_tool(ReadToolCall::fast_snapshot(FILE_KEY, "2:2"))
            .await
    });
    let job = next_text(&mut plugin).await;
    answer(
        &mut plugin,
        &job,
        json!({ "fileKey": FILE_KEY, "nodes": [{ "id": "2:2" }] }),
    )
    .await;
    reading.await.unwrap().expect("the holder keeps serving");
    assert_eq!(state.pending_reads(), 0);
}
