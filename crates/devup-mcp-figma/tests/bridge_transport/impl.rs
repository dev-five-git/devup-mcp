

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
