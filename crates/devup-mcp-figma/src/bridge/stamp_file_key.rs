

/// 봉투의 `fileKey` 가 비어 있으면 요청한 값으로 채운다.
///
/// 스크립트는 `figma.fileKey` 를 그대로 싣는데, 그 값이 늘 오지는 않는다 —
/// 비어 오는 것을 실기기에서 확인했다. 디코더는 비어 있지 않은 `fileKey` 를 요구하므로
/// 그대로 두면 노드를 다 받고도 "스냅샷을 찾지 못했다"며 버린다.
///
/// 지어내는 값이 아니다. 이 읽기가 어느 파일을 향했는지는 호출자가 알고 있고,
/// 그 파일을 이 플러그인이 맡는다는 판단은 이미 `resolve` 가 내렸다. 이 값은
/// 디코더의 대조에만 쓰이고, 파일 키로 보고되는 것은 [`BridgeServed`] 가 싣는
/// 플러그인 자신의 보고뿐이다.
///
/// 빠른 스냅샷·테마 봉투는 이 키를 `source` 와 `snapshot` 안에 싣는다. 거기가 빈
/// 채로 가면 디코더가 대상 불일치로 보고 빠른 경로를 통째로 버린다 — 실기기에서
/// 화면마다 그렇게 노드마다 읽는 예전 경로로 떨어졌고, breakpoint 형제 폭도 잃었다.
fn stamp_file_key(data: &mut Value, file_key: &str) {
    let Some(object) = data.as_object_mut() else {
        return;
    };
    let empty = object
        .get("fileKey")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty);
    if empty {
        object.insert("fileKey".to_owned(), Value::String(file_key.to_owned()));
    }
    for nested in ["source", "snapshot"] {
        if let Some(inner) = object.get_mut(nested).and_then(Value::as_object_mut)
            && inner
                .get("fileKey")
                .is_some_and(|key| key.as_str().is_none_or(str::is_empty))
        {
            inner.insert("fileKey".to_owned(), Value::String(file_key.to_owned()));
        }
    }
}

/// 플러그인이 돌려준 값을 원격 `use_figma` 가 주는 모양으로 감싼다.
///
/// 디코더들은 `content[].text` 안의 JSON 문자열을 찾도록 쓰여 있다. 값을 그대로
/// 올리면 일부 디코더는 통과하고 일부는 실패해, 두 경로가 화면 단위로 갈라진다.
///
/// 어느 플러그인이 답했는지는 `_meta` 에 싣는다. 디코더는 그 자리를 읽지 않으므로
/// 두 경로의 모양은 그대로이고, 수집기는 이것으로 응답의 출처를 브리지로 적는다.
fn wrap_as_tool_result(data: &Value, served: &BridgeServed) -> Result<Value, DevupError> {
    let encode =
        |error: serde_json::Error| unavailable(format!("bridge result encode failed: {error}"));
    let text = serde_json::to_string(data).map_err(encode)?;
    let mut result = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "_meta": {},
    });
    result["_meta"][SERVED_META_KEY] = serde_json::to_value(served).map_err(encode)?;
    Ok(result)
}

/// 노드 없는 metadata 읽기를 원격 `get_metadata` 가 주는 모양 — 최상위 페이지 목록 —
/// 으로 쓴다. 수집기는 두 경로의 답을 같은 파서(`parse_top_level_pages`)로 받는다.
pub(crate) fn wrap_as_top_level_pages(
    data: &Value,
    served: &BridgeServed,
) -> Result<Value, DevupError> {
    let pages = data["nodes"]
        .as_array()
        .ok_or_else(|| unavailable("the plugin's page catalog has no nodes"))?;
    let mut text = String::from("Top-level pages of the document:\n");
    for page in pages {
        let name = page["fields"]["name"].as_str().unwrap_or_default();
        text.push_str(&format!(
            "- {}: {}\n",
            page["id"].as_str().unwrap_or_default(),
            name.replace(['\r', '\n'], " ")
        ));
    }
    let mut result = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
        "_meta": {},
    });
    result["_meta"][SERVED_META_KEY] = serde_json::to_value(served)
        .map_err(|error| unavailable(format!("bridge result encode failed: {error}")))?;
    Ok(result)
}

/// 참조 PNG 의 답이 브리지 답 하나에 들어가지 않으면 플러그인은 그것을 붙들고 조각으로
/// 내준다. 이어 받아 하나로 되돌린다 — 원격 `get_screenshot` 이 16 MiB 까지 받는 것을
/// 브리지도 받는다.
async fn whole_reference_answer(
    state: &BridgeState,
    file_key: &str,
    data: Value,
) -> Result<Value, DevupError> {
    if data["kind"] != "devupReferencePngChunked" {
        return Ok(data);
    }
    let id = data["id"].as_str().unwrap_or_default().to_owned();
    let length = data["length"].as_u64().unwrap_or(0) as usize;
    if id.is_empty() || length == 0 || length > REFERENCE_ANSWER_LIMIT {
        return Err(unavailable(
            "the plugin's reference PNG answer is too large or unnamed",
        ));
    }
    let mut text = String::with_capacity(length);
    while text.len() < length {
        let (piece, _) = state
            .dispatch(
                file_key,
                "referencePngChunk",
                json!({ "largeValue": { "id": id, "offset": text.len() } }),
            )
            .await?;
        let part = piece["text"].as_str().unwrap_or_default();
        // 붙든 글은 ASCII 라 오프셋이 두 쪽에서 같은 자리를 가리킨다. 아니면 어긋난 것이다.
        if piece["kind"] != "devupReferencePngChunk"
            || piece["id"] != id.as_str()
            || piece["offset"].as_u64() != Some(text.len() as u64)
            || part.is_empty()
            || !part.is_ascii()
            || text.len() + part.len() > length
        {
            return Err(unavailable(
                "the plugin's reference PNG pieces do not line up",
            ));
        }
        text.push_str(part);
    }
    serde_json::from_str(&text)
        .map_err(|error| unavailable(format!("the plugin's reference PNG answer: {error}")))
}

/// 참조 PNG 의 답 전체.
///
/// 플러그인은 큰 답을 읽혀 갈 때까지 붙들어 두고, 그것들이 상한만큼 쌓여 있으면 새 그림을
/// 그리지 않고 바쁘다고 답한다(`DEVUP_REFERENCE_PNG_BUSY`). 붙든 답을 밀어내면 그 답을 읽던
/// 요청이 실패하기 때문이다. 그동안에도 다른 요청의 조각 읽기는 처리되므로 잠시 뒤 다시
/// 묻는다. 붙들어 둔 답이 읽기 전에 사라졌으면(플러그인이 다시 실행되는 등) 한 번 다시 받는다.
async fn reference_answer(
    state: &BridgeState,
    file_key: &str,
    script: &'static str,
    params: Value,
) -> Result<(Value, BridgeServed), DevupError> {
    let deadline = tokio::time::Instant::now() + REFERENCE_BUSY_WAIT;
    let mut fetched_again = false;
    loop {
        let (data, served) = match state.dispatch(file_key, script, params.clone()).await {
            Err(error)
                if plugin_says(&error, "DEVUP_REFERENCE_PNG_BUSY")
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(REFERENCE_BUSY_RETRY).await;
                continue;
            }
            answered => answered?,
        };
        match whole_reference_answer(state, file_key, data).await {
            Err(error)
                if plugin_says(&error, "DEVUP_REFERENCE_PNG_ANSWER_GONE") && !fetched_again =>
            {
                fetched_again = true;
            }
            whole => return whole.map(|data| (data, served)),
        }
    }
}

fn plugin_says(error: &DevupError, code: &str) -> bool {
    is_script_failure(error) && error.message.contains(code)
}

/// 바쁜 플러그인에게 참조 PNG 를 다시 묻는 간격과 그 상한. 상한은 export 작업이 읽기 하나에
/// 주는 90초 안에 그림과 조각 읽기까지 마치도록 잡았다.
const REFERENCE_BUSY_RETRY: std::time::Duration = std::time::Duration::from_millis(500);
const REFERENCE_BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(45);

/// 조각으로 받는 참조 PNG 답의 상한. 이미지 채움의 원본까지 실으므로 PNG 상한(16 MiB)
/// 보다 크다.
const REFERENCE_ANSWER_LIMIT: usize = 96 * 1024 * 1024;

/// 참조 PNG 결과의 `_meta` 에서, 플러그인이 `get_screenshot` 과 같은 그림을 다 그리지
/// 못했다는 이유를 싣는 자리. 원격 경로가 열려 있으면 그쪽이 대신 답한다.
const REFERENCE_PNG_APPROXIMATE_META_KEY: &str = "devup/referencePngApproximate";

/// 원격 경로가 브리지 대신 답했을 때 그 이유를 결과의 `_meta` 에 싣는 자리.
pub(crate) const DIRECT_FALLBACK_META_KEY: &str = "devup/directFallback";

/// 플러그인이 export 한 참조 PNG 를 공식 `get_screenshot` 이 주는 모양 — 이미지
/// 블록 하나 — 으로 쓴다. 수집기는 두 경로의 답을 같은 디코더로 받는다.
///
/// 그림도 같게 맞춘다. 다른 레이어가 닿지 않으면 플러그인은 캔버스가 보여 주는 그
/// 자리를 export 하고 페이지 배경을 알려 주며, 여기서 그 위로 투명한 자리를 덮는다.
/// 닿으면 노드만 export 하고 조상들을 설명하며, 여기서 나머지를 그린다
/// (`reference_compose`). 다 그리지 못한 것이 있으면 그 이유를 `_meta` 에 실어
/// 원격 경로가 대신 답하게 한다.
fn wrap_as_image_result(data: &Value, served: &BridgeServed) -> Result<Value, DevupError> {
    if data["kind"] != "devupReferencePng" || data["mimeType"] != "image/png" {
        return Err(unavailable(
            "the plugin's reference PNG answer is not a devupReferencePng",
        ));
    }
    // 그리다 멈추는 일이 생겨도 함께 온 캔버스 그림이 답이 된다.
    let composed = data["compose"].is_object().then(|| {
        std::panic::catch_unwind(|| crate::reference_compose::compose(&data["compose"]))
            .unwrap_or_else(|_| Err(unavailable("composing it stopped unexpectedly")))
    });
    let mut failure = None;
    let png = match composed {
        Some(Ok(png)) => png,
        failed => {
            failure = failed.and_then(Result::err);
            let png = data["data"].as_str().ok_or_else(|| {
                unavailable("the plugin's reference PNG answer carries no picture")
            })?;
            match named_backdrop(data) {
                Some(backdrop) => crate::collector::flatten_reference_png(png, backdrop)?,
                None => png.to_owned(),
            }
        }
    };
    let mut result = json!({
        "content": [{ "type": "image", "data": png, "mimeType": "image/png" }],
        "isError": false,
        "_meta": {},
    });
    result["_meta"][SERVED_META_KEY] = serde_json::to_value(served)
        .map_err(|error| unavailable(format!("bridge result encode failed: {error}")))?;
    if let Some(note) = data.get("note").and_then(Value::as_str) {
        result["_meta"][REFERENCE_PNG_NOTE_META_KEY] = json!(note);
    }
    if let Some(reason) = data.get("approximate").and_then(Value::as_str) {
        result["_meta"][REFERENCE_PNG_APPROXIMATE_META_KEY] = json!(reason);
    }
    if let Some(error) = failure {
        result["_meta"][REFERENCE_PNG_APPROXIMATE_META_KEY] =
            json!(format!("devup-mcp could not compose it: {}", error.message));
        result["_meta"][REFERENCE_PNG_NOTE_META_KEY] = json!(format!(
            include_str!("../text/bridge-wrap_as_image_result-467d2f14.md"),
            error.message,
            data["reaching"].as_str().unwrap_or("other layers")
        ));
    }
    Ok(result)
}

/// 플러그인이 export 한 노드 아래의 색, `[r, g, b]`. 이 값을 보내기 전에 빌드된
/// 플러그인은 보내지 않으며, 그 export 는 받은 그대로 넘긴다.
fn named_backdrop(data: &Value) -> Option<[u8; 3]> {
    let channels = data
        .get("backdrop")?
        .as_array()?
        .iter()
        .map(|value| value.as_u64().and_then(|value| u8::try_from(value).ok()))
        .collect::<Option<Vec<_>>>()?;
    <[u8; 3]>::try_from(channels).ok()
}

/// 플러그인이 붙는 문에 들이는 `Origin`.
///
/// 브라우저 페이지도 `ws://localhost` 에 붙을 수 있다. 그 페이지가 여기서 `hello`
/// 를 보내 플러그인인 척하면 읽기 요청을 받고 지어낸 디자인을 돌려줄 수 있다 — 같은
/// 파일 키로 나중에 붙은 연결이 그 파일의 읽기를 맡으므로 진짜 플러그인을 가로챌
/// 수도 있다. 브라우저는 WebSocket 핸드셰이크에 늘 `Origin` 을 싣고 페이지는 그것을
/// 바꾸지 못한다.
///
/// Figma 플러그인의 UI 는 origin 이 `null` 인 iframe 에서 돈다(Figma 개발자 문서
/// "Making Network Requests": "Plugin iframes have a null origin"). 이 플러그인은
/// iframe 을 다른 주소로 옮기지 않는다. 혹시 Figma 가 제 origin 을 싣는 판이 있어도
/// 끊기지 않도록 figma.com 도 들인다 — 다른 페이지는 그 값을 지어낼 수 없다.
/// `Origin` 이 없는 요청은 브라우저가 아닌 프로그램(테스트, 프로브)이다.
///
/// 한계가 있다: 샌드박스 iframe 이나 `file://` 페이지도 `null` 을 싣는다. 흔한 웹
/// 페이지는 막지만 그런 페이지까지 막지는 못한다.
fn a_plugin_origin(headers: &HeaderMap) -> bool {
    headers.get(ORIGIN).is_none_or(|origin| {
        matches!(
            origin.as_bytes(),
            b"null" | b"https://www.figma.com" | b"https://figma.com"
        )
    })
}

/// 플러그인이 붙는 문.
async fn plugin_socket(
    State(state): State<BridgeState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !a_plugin_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    upgrade
        .max_message_size(128 * 1024 * 1024)
        .max_frame_size(128 * 1024 * 1024)
        .on_upgrade(move |socket| handle_plugin(state, socket))
}

async fn handle_plugin(state: BridgeState, mut socket: WebSocket) {
    let (outbox, mut outbox_rx) = mpsc::unbounded_channel::<String>();
    let id = state.counter.fetch_add(1, Ordering::Relaxed);
    let mut shutdown = state.link.shutdown_signal();

    // 보내기와 받기를 한 루프에서 번갈아 본다. 소켓을 쪼개려면 Stream/Sink 트레이트
    // 의존성이 필요한데, 그것을 들이는 것보다 select 가 싸다.
    loop {
        tokio::select! {
            // 이 프로세스가 브리지를 내려놓는다. 소켓을 닫아야 플러그인이 다음 호스트를 찾는다.
            () = shut_down(&mut shutdown) => break,
            outgoing = outbox_rx.recv() => {
                let Some(text) = outgoing else { break };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                let Message::Text(text) = message else { continue };
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };

                match value.get("kind").and_then(Value::as_str) {
                    Some("hello") => {
                        // 키가 없어도 등록한다. 건너뛰면 창은 "연결됨"이라고 하는데
                        // 어떤 읽기도 오지 않아 원인을 찾을 수 없다. 키 없는 연결을
                        // 어디까지 믿을지는 resolve 가 정한다.
                        //
                        // 필드마다 따로 읽는다. 페이지나 선택이 어긋난 모양으로 와도
                        // 파일 키까지 잃으면 안 된다.
                        let file_key = value
                            .get("fileKey")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        let plugin = Connected {
                            outbox: outbox.clone(),
                            routing_key: routing_key(&file_key, session_of(&value).as_deref(), id),
                            file_key,
                            file_name: value
                                .get("fileName")
                                .and_then(Value::as_str)
                                .filter(|name| !name.is_empty())
                                .map(str::to_owned),
                            context: serde_json::from_value(value).unwrap_or_default(),
                        };
                        let mut inner = state.inner.lock().await;
                        inner.plugins.insert(id, plugin);
                        state.plugins_changed(&mut inner);
                    }
                    // 페이지를 옮기거나 선택을 바꿀 때마다 온다. url 없는 요청은
                    // 이 값으로 대상을 고르므로, 늦게라도 최신이어야 한다. 중계들도
                    // 같은 선택을 보도록 함께 알린다.
                    Some("context") => {
                        let Ok(context) = serde_json::from_value::<PluginContext>(value) else {
                            continue;
                        };
                        let mut inner = state.inner.lock().await;
                        if let Some(plugin) = inner.plugins.get_mut(&id) {
                            plugin.context = context;
                            state.plugins_changed(&mut inner);
                        }
                    }
                    Some("devup-result") => {
                        let Ok(result) = serde_json::from_value::<PluginResult>(value) else {
                            continue;
                        };
                        let waiting = state
                            .pending
                            .lock()
                            .expect("the pending table is never poisoned")
                            .remove(&result.request_id);
                        // 받는 쪽이 이미 타임아웃했거나 떠났으면 버린다.
                        if let Some(waiting) = waiting {
                            let _ = waiting.waiter.send(result);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let mut inner = state.inner.lock().await;
    if inner.plugins.remove(&id).is_some() {
        state.plugin_left(&mut inner, id);
    }
}

/// 브리지를 여는 데 쓰는 것.
#[derive(Debug, Clone, Default)]
pub struct BridgeOptions {
    /// 이 빌드의 식별자(`devup-mcp --version` 의 괄호 안). 중계 핸드셰이크가 상대에게
    /// 알리고, status 가 포트를 쥔 프로세스를 밝힐 때 쓴다.
    pub build_id: Option<String>,
    /// 중계 비밀값 파일. 비우면 그 사용자만 쓰는 기본 자리다.
    pub secret_path: Option<PathBuf>,
}

/// 중계 핸드셰이크에서 밝히는 devup-mcp 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgePeer {
    pub pid: u32,
    pub version: String,
    #[serde(default)]
    pub build_id: Option<String>,
}

/// 이 프로세스가 브리지에서 맡은 역할.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeRole {
    /// 포트를 쥐었다. 플러그인은 이 프로세스에 붙는다.
    Host,
    /// 다른 devup-mcp 가 쥔 포트를 통해 읽는다.
    Relay,
    /// 누가 포트를 쥐었는지 확인하는 중이다 — 시작할 때, 또는 호스트가 떠난 뒤.
    Connecting,
    /// 지금은 브리지를 쓸 수 없다. 이유는 [`BridgeIssue`] 다.
    Unavailable,
}

/// 브리지를 쓸 수 없는 이유. 고치는 방법이 서로 다르다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeIssue {
    /// 중계를 모르는 예전 devup-mcp 가 포트를 쥐었다. `/plugin` 만 있고 `/relay` 가 없다.
    /// 자신을 밝히지 않으므로 `owner` 는 OS 에 물어 안 것이다.
    LegacyHost { owner: Option<PortOwner> },
    /// devup-mcp 가 아닌 프로그램이 포트를 쥐었다. `owner` 는 OS 에 물어 안 것이다.
    ForeignProgram {
        detail: String,
        owner: Option<PortOwner>,
    },
    /// 다른 판의 중계 규약을 쓰는 devup-mcp 가 쥐었다. 틀린 답을 내느니 잇지 않는다.
    IncompatibleProtocol { theirs: Option<u64>, ours: u64 },
    /// 서로 같은 사용자의 devup-mcp 임을 증명하지 못했다.
    AuthenticationFailed { detail: String },
    /// 중계 비밀값 파일을 읽거나 만들 수 없다.
    SecretUnavailable { detail: String },
    /// 포트를 잡을 수 없는데 그 포트에서 듣는 쪽도 없다.
    BindFailed { detail: String },
}

impl BridgeIssue {
    /// status 가 싣는 짧은 이름.
    pub fn code(&self) -> &'static str {
        match self {
            Self::LegacyHost { .. } => "legacy-host",
            Self::ForeignProgram { .. } => "foreign-program",
            Self::IncompatibleProtocol { .. } => "incompatible-protocol",
            Self::AuthenticationFailed { .. } => "authentication-failed",
            Self::SecretUnavailable { .. } => "secret-unavailable",
            Self::BindFailed { .. } => "bind-failed",
        }
    }
}

/// 브리지 서버.
///
/// 포트를 잡으면 호스트가 되고, 잡지 못하면 그 포트를 쥔 devup-mcp 를 통해 읽는다.
/// 어느 쪽이든 [`BridgeFigmaClient`] 는 같은 방식으로 쓴다.
pub struct BridgeServer {
    state: BridgeState,
    port: u16,
}

impl BridgeServer {
    /// 포트를 잡아 서버를 띄운다.
    ///
    /// 이미 다른 프로세스가 같은 포트를 쓰고 있으면 그쪽을 통해 읽는다. 한 기기에서
    /// MCP 클라이언트를 여러 개 띄우는 것은 정상이고, 플러그인이 붙을 수 있는 포트는
    /// 하나뿐이다. 그 포트를 쥔 쪽이 떠나면 이 프로세스가 이어받을 수 있다.
    ///
    /// 서버가 만들어지는 자리가 동기 함수라 bind 도 동기로 한다 — 듣기 소켓을
    /// 여는 것은 어차피 기다리지 않는 일이고, 기다리는 부분만 따로 띄운다.
    pub fn start(port: u16) -> Option<Self> {
        Self::start_with(port, BridgeOptions::default())
    }

    pub fn start_with(port: u16, options: BridgeOptions) -> Option<Self> {
        // 런타임 밖에서 만들어졌다면 붙일 곳이 없다. 그때도 원격 경로는 멀쩡하다.
        // bind 보다 먼저 본다 — 쓰지도 못할 포트를 잠깐이라도 쥐면 안 된다.
        let runtime = tokio::runtime::Handle::try_current().ok()?;
        let state = BridgeState::new(options);
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                // 0 을 주면 커널이 빈 포트를 고른다. 실제로 잡힌 번호를 알아야 붙을 수 있다.
                let bound = listener.local_addr().ok()?.port();
                state.serve(listener, None).ok()?;
                Some(Self { state, port: bound })
            }
            // 0 은 빈 포트를 달라는 뜻이라, 누가 쥐고 있을 포트가 없다.
            Err(_) if port == 0 => None,
            Err(_) => {
                runtime.spawn(relay::supervise(state.clone(), port));
                Some(Self { state, port })
            }
        }
    }

    /// 환경 설정을 읽어 띄운다.
    ///
    /// `DEVUP_FIGMA_BRIDGE_PORT` 가 포트를 정하며, `0` 이나 `off` 는 끈다. 값이
    /// 없으면 기본 포트로 켠다 — 플러그인이 안 떠 있으면 어차피 원격으로 가므로
    /// 켜 두는 쪽이 손해가 없다.
    pub fn from_env() -> Option<Self> {
        Self::from_env_with(BridgeOptions::default())
    }

    pub fn from_env_with(options: BridgeOptions) -> Option<Self> {
        let setting = std::env::var("DEVUP_FIGMA_BRIDGE_PORT").ok();
        let port = match setting.as_deref().map(str::trim) {
            None | Some("") => DEFAULT_BRIDGE_PORT,
            Some("off") | Some("0") => return None,
            Some(value) => value.parse().ok()?,
        };
        Self::start_with(port, options)
    }

    pub fn state(&self) -> BridgeState {
        self.state.clone()
    }

    /// 실제로 잡은 포트, 또는 이 프로세스가 통해 읽는 포트. `start(0)` 으로 띄웠을 때
    /// 어디에 붙어야 하는지 알려 준다.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// 브리지를 내려놓는다 — 프로세스가 끝날 때처럼 포트와 모든 연결을 닫는다.
    ///
    /// 서버를 버리는 것만으로는 닫지 않는다. 운영에서는 프로세스가 끝날 때까지
    /// 살아 있어야 하고, 이것은 한 프로세스 안에서 호스트가 떠나는 일을 재현한다.
    pub fn shutdown(self) {
        self.state.link.shutdown.send_replace(true);
    }
}

/// 이 요청을 맡을 수 있는지 **부르기 전에** 답하는 상류.
///
/// 실패한 뒤에 폴백하면 안 되기 때문에 따로 둔다. `CapabilityUnavailable` 과
/// `Transport` 는 같은 `ErrorCode` 로 접히므로, 에러만 보고는 "브리지가 못 하는
/// 일"과 "브리지가 하다가 실패한 일"을 구분할 수 없다. 뒤엣것까지 원격으로 넘기면
/// 아끼려던 요금을 그대로 쓰게 된다.
#[async_trait]
pub trait PreferredUpstream: FigmaUpstream {
    async fn can_serve(&self, call: &ReadToolCall) -> bool;

    /// 지금 이 상류가 쓸 수 있는 상태인지. 배치 크기를 정하는 자리는 async 가
    /// 아니어서 `can_serve` 를 부를 수 없다.
    fn is_live(&self) -> bool;
}

/// 브리지 경로의 실측 상태. `devup_figma_auth doctor` 의 `paths.bridge` 가 된다.
///
/// 이 값이 있다는 것 자체가 "이 프로세스가 브리지를 쓰려 한다"는 뜻이다.
/// `DEVUP_FIGMA_BRIDGE_PORT=off` 면 브리지 상류가 아예 만들어지지 않으므로 `None`
/// 이 된다. 포트를 잡지 못한 것은 이제 `None` 이 아니다 — 그 포트를 쥔 쪽을 통해
/// 읽거나([`BridgeRole::Relay`]), 왜 그럴 수 없는지를 말한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgePathSnapshot {
    /// 플러그인이 붙는 포트 — 이 프로세스가 잡았든 다른 프로세스가 잡았든.
    /// 플러그인 manifest 의 `allowedDomains` 와 같아야 붙는다.
    pub port: Option<u16>,
    /// 지금 붙어 있는 플러그인, 붙은 순서대로. 포트를 쥔 프로세스 기준이라 어느
    /// 프로세스에서 보든 같다. 파일 키를 보고하지 못한 플러그인은 혼자 붙어 있을
    /// 때만 다른 키의 읽기를 받는다.
    pub attached_files: Vec<AttachedFile>,
    pub role: BridgeRole,
    /// 포트를 쥔 devup-mcp. 호스트면 이 프로세스 자신이다. 모르면 `None` 이다 —
    /// 예전 devup-mcp 는 자신을 밝히지 않는다.
    pub host: Option<BridgePeer>,
    /// [`BridgeRole::Unavailable`] 의 이유.
    pub issue: Option<BridgeIssue>,
    /// 방금 떠난 호스트. 포트가 넘어가는 중이거나, 넘어갔는데 그때 붙어 있던
    /// 플러그인이 아직 다시 붙지 않았다.
    pub handover_from: Option<BridgePeer>,
}

impl BridgePathSnapshot {
    /// 지금 이 경로로 읽기를 보낼 수 있는지 — 플러그인이 보이고, 그것에 닿는 길이 있다.
    pub fn available(&self) -> bool {
        matches!(self.role, BridgeRole::Host | BridgeRole::Relay) && !self.attached_files.is_empty()
    }
}

/// 플러그인을 통해 Figma 를 읽는 `FigmaUpstream`.
#[derive(Clone)]
pub struct BridgeFigmaClient {
    state: BridgeState,
    /// 진단에만 쓴다. 읽기 경로는 포트를 알 필요가 없다.
    port: Option<u16>,
}

impl BridgeFigmaClient {
    pub fn new(state: BridgeState) -> Self {
        Self { state, port: None }
    }

    /// 잡은 포트를 함께 들고 있게 한다. "문이 어디에 열려 있는가"는 붙지 않는
    /// 플러그인을 진단할 때 가장 먼저 확인할 값이다.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }
}

#[async_trait]
impl FigmaUpstream for BridgeFigmaClient {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        // 스크립트 경로만 대신하므로 광고하는 것도 그 하나다.
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        let Some(job) = call.bridge_job() else {
            return Err(unavailable("the bridge serves script reads only"));
        };
        let mut params = job.params;
        widen_budgets(&mut params);
        let answered = if matches!(call, ReadToolCall::Screenshot { .. }) {
            reference_answer(&self.state, call.file_key(), job.script, params).await
        } else {
            self.state
                .dispatch(call.file_key(), job.script, params)
                .await
        };
        let (mut data, served) = match answered {
            Ok(answered) => answered,
            Err(error) if is_script_failure(&error) => {
                return Ok(script_failure_result(&error.message));
            }
            Err(error) => return Err(error),
        };
        let served = BridgeServed {
            port: self.port,
            ..served
        };
        if matches!(call, ReadToolCall::Screenshot { .. }) {
            // 그리는 일은 픽셀마다 도는 계산이라 비동기 작업자를 붙들지 않게 넘긴다.
            let raw = tokio::task::spawn_blocking(move || wrap_as_image_result(&data, &served))
                .await
                .map_err(|error| {
                    unavailable(format!("composing the reference PNG stopped: {error}"))
                })??;
            return Ok(UpstreamResult { raw });
        }
        if matches!(call, ReadToolCall::Metadata { node_id: None, .. }) {
            return Ok(UpstreamResult {
                raw: wrap_as_top_level_pages(&data, &served)?,
            });
        }
        stamp_file_key(&mut data, call.file_key());
        Ok(UpstreamResult {
            raw: wrap_as_tool_result(&data, &served)?,
        })
    }

    fn batch_budget(&self) -> BatchBudget {
        bridge_batch_budget()
    }

    async fn serves_without_credentials(&self, file_key: &str) -> bool {
        self.state.has_plugin(file_key).await
    }

    async fn bridge_path_snapshot(&self) -> Option<BridgePathSnapshot> {
        Some(self.state.path_snapshot(self.port).await)
    }

    async fn refresh_bridge_context(&self, file_key: &str) -> Result<PluginContext, DevupError> {
        let (data, _) = self
            .state
            .dispatch(file_key, "bridgeContext", json!({}))
            .await?;
        if data.to_string().len() > 16_384
            || data["kind"] != "devup-bridge-context"
            || !data["selection"].is_array()
            || !data["selectionCount"].is_u64()
            || !data["currentPage"]["id"].is_string()
        {
            return Err(unavailable(
                "The plugin did not return a current selection context.",
            ));
        }
        let context: PluginContext = serde_json::from_value(data)
            .map_err(|_| unavailable("Invalid plugin selection context."))?;
        if context.selection.as_ref().is_some_and(|s| {
            s.len() > 20 || context.selection_count.is_some_and(|count| count < s.len())
        }) {
            return Err(unavailable("Invalid plugin selection count."));
        }
        Ok(context)
    }

    async fn is_metered(&self, _call: &ReadToolCall) -> bool {
        false
    }
}

#[async_trait]
impl PreferredUpstream for BridgeFigmaClient {
    async fn can_serve(&self, call: &ReadToolCall) -> bool {
        call.bridge_job().is_some() && self.state.has_plugin(call.file_key()).await
    }

    fn is_live(&self) -> bool {
        self.state.connected.load(Ordering::Relaxed) > 0
    }
}

/// 자르지 않는 전송이므로 쪼갤 이유가 없다. 변수와 스타일을 한 번에 다 묻는다.
const BRIDGE_BATCH: BatchBudget = BatchBudget {
    variable_items: 4096,
    style_items: 4096,
    used_resource_items: 4096,
    used_resource_bytes: BRIDGE_ENVELOPE_BYTES,
};

/// 한 번에 물을 리소스 수를 밖에서 낮춰 보기 위한 손잡이.
///
/// 리소스가 하나도 해석되지 않는 것을 실기기에서 봤고, 원인이 이 크기인지
/// (플러그인이 그만한 동시 조회를 감당하지 못하는지) 아니면 애초에 그 변수를
/// 읽을 권한이 없는지 가려야 했다. 둘은 고치는 곳이 다르다.
const BRIDGE_USED_RESOURCE_ITEMS_ENV: &str = "DEVUP_FIGMA_BRIDGE_USED_RESOURCE_ITEMS";

fn bridge_batch_budget() -> BatchBudget {
    let mut budget = BRIDGE_BATCH;
    if let Some(items) = std::env::var(BRIDGE_USED_RESOURCE_ITEMS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|items| *items > 0)
    {
        budget.used_resource_items = items;
    }
    budget
}

/// 브리지가 맡을 수 있으면 브리지로, 아니면 원격으로 보낸다.
///
/// 판정은 부르기 전에 끝난다. 브리지가 맡은 요청이 실패하면 그 실패를 그대로
/// 올린다 — 노드가 없거나 봉투가 너무 큰 것은 원격에서도 같은 결과이고, 다시
/// 물으면 아낀 요금을 도로 쓰게 된다. 예외는 원격이라면 해낼 수 있는 거절
/// (`remote_may_answer`)이다. 원격이 읽을 수 있는 키면 원격에 묻되, 원격도 다시 해 볼
/// 여지 없이 거절하면(로그인이 없다 등) 플러그인의 거절을 올린다 — 한도를 쓰지 않는
/// 처방(플러그인 갱신)을 말하는 쪽이 그것이다.
pub struct FallbackUpstream<P, S> {
    preferred: P,
    secondary: S,
    meter: Option<Arc<dyn Meter>>,
}

/// 한도를 쓰는 경로로 보낼 읽기를 한도 안에 들 때까지 붙든다.
///
/// 원격으로 가는 읽기는 부르는 쪽이 스스로 늦춘다. 플러그인이 답한 읽기를 원격으로 다시
/// 읽을지는 그 판단 뒤 여기서 정해지므로 여기서 이것을 기다린다 — 다만 정해진 시간 안에
/// 자리가 나지 않으면 기다리지 않는다. 기다리는 시간도 읽기 하나에 주어진 시간 안에 들어
/// 있어서, 오래 기다리면 이미 받은 플러그인의 답까지 시간 초과로 잃는다.
#[async_trait]
pub trait Meter: Send + Sync {
    /// `budget` 안에 한도 자리를 얻었으면 `true`. 얻지 못했으면 자리를 쓰지 않는다.
    async fn acquire_within(&self, budget: std::time::Duration) -> bool;
}

/// 읽기 하나에 주어진 시간. export 작업은 이 시간이 지난 읽기를 멈추고 다시 하게 한다.
pub const READ_TIME_LIMIT: std::time::Duration = std::time::Duration::from_secs(90);

/// 원격으로 다시 읽는 일 — 한도 자리를 기다리는 것과 원격의 답까지 — 에 주는 시간의 상한.
const DIRECT_FALLBACK_BUDGET: std::time::Duration = std::time::Duration::from_secs(40);

/// 다시 읽는 일은 플러그인의 읽기와 합쳐 `READ_TIME_LIMIT` 보다 이만큼 일찍 끝낸다. 그래야
/// 시간이 다 됐을 때 이미 받은 플러그인의 답을 돌려줄 수 있다.
const FALLBACK_MARGIN: std::time::Duration = std::time::Duration::from_secs(5);

impl<P, S> FallbackUpstream<P, S> {
    pub fn new(preferred: P, secondary: S) -> Self {
        Self {
            preferred,
            secondary,
            meter: None,
        }
    }

    pub fn with_meter(mut self, meter: Arc<dyn Meter>) -> Self {
        self.meter = Some(meter);
        self
    }

    /// 원격 읽기를 한도 자리를 얻어 `deadline` 안에 끝낸다. 자리를 얻지 못했거나 시간 안에
    /// 끝나지 않았으면 `None`.
    async fn metered<T>(
        &self,
        deadline: tokio::time::Instant,
        read: impl std::future::Future<Output = T>,
    ) -> Option<T> {
        let budget = deadline.saturating_duration_since(tokio::time::Instant::now());
        if budget.is_zero() {
            return None;
        }
        if let Some(meter) = &self.meter
            && !tokio::time::timeout_at(deadline, meter.acquire_within(budget))
                .await
                .unwrap_or(false)
        {
            return None;
        }
        tokio::time::timeout_at(deadline, read).await.ok()
    }
}

#[async_trait]
impl<P, S> FigmaUpstream for FallbackUpstream<P, S>
where
    P: PreferredUpstream,
    S: FigmaUpstream,
{
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        // 능력 목록은 원격이 정본이다. 브리지는 그중 하나를 대신할 뿐이다.
        self.secondary.list_tools().await
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        let started = tokio::time::Instant::now();
        if self.preferred.can_serve(&call).await {
            let answered = self.preferred.call_read_tool(call.clone()).await?;
            if is_bridge_only_key(call.file_key()) {
                return Ok(answered);
            }
            let better = remote_does_better(&answered.raw);
            let Some(reason) = better.or_else(|| remote_may_answer(&answered.raw)) else {
                return Ok(answered);
            };
            // 로그인 확인, 한도 자리, 원격의 답이 모두 이 시간 안이다 — `DIRECT_FALLBACK_BUDGET`
            // 안에, 그리고 이 읽기가 시작된 때부터 `READ_TIME_LIMIT - FALLBACK_MARGIN` 안에.
            let deadline = (tokio::time::Instant::now() + DIRECT_FALLBACK_BUDGET)
                .min(started + READ_TIME_LIMIT - FALLBACK_MARGIN);
            let direct =
                match tokio::time::timeout_at(deadline, self.secondary.has_credentials()).await {
                    Ok(false) => return Ok(answered),
                    Ok(true) => {
                        self.metered(deadline, self.secondary.call_read_tool(call))
                            .await
                    }
                    Err(_) => None,
                };
            return match direct {
                // 플러그인의 답을 쓸 수 있으면 그것을 쓴다. 쓸 수 없는 답(거절)이면 한도가
                // 비는 대로 다시 해 볼 일이다.
                None if better.is_some() => Ok(answered),
                None => Err(no_room_on_the_metered_path()),
                Some(Ok(direct)) if better.is_some() && direct_unusable(&direct.raw) => {
                    Ok(answered)
                }
                Some(Ok(mut direct)) => {
                    direct.raw["_meta"][DIRECT_FALLBACK_META_KEY] = json!(reason);
                    Ok(direct)
                }
                Some(Err(error)) if better.is_none() && error.retryable => Err(error),
                Some(Err(_)) => Ok(answered),
            };
        }
        // 브리지만 읽을 수 있는 키다. 원격으로 넘기면 Figma 에 없는 키를 묻게 되고,
        // 돌아오는 것은 원인과 무관한 거절 — 로그인하라거나 파일이 없다는 — 뿐이다.
        if is_bridge_only_key(call.file_key()) {
            return Err(bridge_only_refusal(&call));
        }
        self.secondary.call_read_tool(call).await
    }

    /// 붙어 있는 플러그인이 있으면 그쪽 크기로 묶는다.
    ///
    /// 배치는 부르기 한참 전에 나뉘므로 어느 전송이 받을지 그때는 알 수 없다.
    /// 다만 크게 묶어도 되는 읽기는 전부 스크립트 경로이고, 플러그인이 붙어 있는
    /// 한 그것은 브리지가 받는다. 수집 도중 플러그인이 닫히면 남은 배치가 원격에
    /// 너무 커서 거절되는데, 그때는 수집 자체가 이어질 수 없으므로 실패가 드러나는
    /// 편이 맞다.
    fn batch_budget(&self) -> BatchBudget {
        if self.preferred.is_live() {
            self.preferred.batch_budget()
        } else {
            self.secondary.batch_budget()
        }
    }

    /// 이 파일을 맡은 플러그인이 있으면 원격 자격증명 없이도 수집이 성립한다.
    /// 뒤엣것은 원격이므로 물을 것이 없다.
    ///
    /// 브리지만 읽을 수 있는 키는 로그인을 기다리지 않는다. direct 경로는 그 키로는
    /// 무엇을 치러도 읽을 수 없으므로, 로그인을 요구하는 것은 엉뚱한 처방이다.
    /// 플러그인이 그새 끊겼다면 그 거절은 첫 읽기가 제 이유와 함께 한다.
    async fn serves_without_credentials(&self, file_key: &str) -> bool {
        is_bridge_only_key(file_key) || self.preferred.serves_without_credentials(file_key).await
    }

    async fn bridge_path_snapshot(&self) -> Option<BridgePathSnapshot> {
        self.preferred.bridge_path_snapshot().await
    }

    async fn refresh_bridge_context(&self, file_key: &str) -> Result<PluginContext, DevupError> {
        self.preferred.refresh_bridge_context(file_key).await
    }

    /// 브리지가 맡는 읽기는 한도를 쓰지 않는다. 브리지만 읽을 수 있는 키는 원격으로
    /// 가지 않으므로(거절된다) 역시 세지 않는다.
    async fn is_metered(&self, call: &ReadToolCall) -> bool {
        !(is_bridge_only_key(call.file_key()) || self.preferred.can_serve(call).await)
    }
}

/// 플러그인이 답은 했지만 원격 경로가 더 낫게 답할 수 있는 경우와 그 이유. 원격이
/// 어떤 이유로든 답하지 못하면 플러그인의 답을 그대로 쓴다 — 쓸 수 있는 답을 버리고
/// 실패를 올리지 않는다.
///
/// - Figma 가 화면에 보이지 않는 파일의 데이터를 가져오지 않아 읽기의 일부나 전부가
///   "Unable to establish connection to Figma" 로 빠졌다(`figma_unreachable_hint`).
///   원격 경로는 그 상태와 무관하다.
/// - 참조 PNG 에서 플러그인이 `get_screenshot` 과 같은 그림을 다 그리지 못했다.
fn remote_does_better(raw: &Value) -> Option<&'static str> {
    if raw["_meta"][REFERENCE_PNG_APPROXIMATE_META_KEY].is_string() {
        return Some("reference-png-approximate");
    }
    texts(raw)
        .any(|text| {
            crate::figma_unreachable_hint(text).is_some()
                && (raw["isError"] == true || reports_unreachable(text))
        })
        .then_some("file-out-of-sight")
}

/// 플러그인 답의 JSON 에서 오류를 싣는 자리 — 값을 읽지 못한 `$error`, 필드 읽기 실패
/// `fieldErrors`, 미해석 리소스의 `error`, 자산 export 의 `errorDetail` — 에 그 말이 있는지.
/// 디자인의 글(`characters`, 이름, 설명)에 같은 문장이 있어도 한도를 써서 다시 읽지 않는다.
fn reports_unreachable(text: &str) -> bool {
    fn within(value: &Value, in_error: bool) -> bool {
        match value {
            Value::String(text) => in_error && crate::figma_unreachable_hint(text).is_some(),
            Value::Array(items) => items.iter().any(|item| within(item, in_error)),
            Value::Object(fields) => fields.iter().any(|(key, value)| {
                let error_field = matches!(
                    key.as_str(),
                    "$error" | "error" | "errors" | "fieldErrors" | "errorDetail" | "lookupError"
                );
                within(value, in_error || error_field)
            }),
            _ => false,
        }
    }
    serde_json::from_str::<Value>(text).is_ok_and(|value| within(&value, false))
}

/// 플러그인이 답하지 못한 읽기를 원격으로 넘기려 했지만 정해진 시간 안에 한도 자리가 나지
/// 않았다. 한도가 찬 것과 같으므로 그렇게 알려, 수집이 한도가 비기를 기다렸다 다시 하게 한다.
fn no_room_on_the_metered_path() -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaRateLimited,
        include_str!("../text/bridge-no_room_on_the_metered_path-41a0802f.md"),
        true,
        json!({ "source": "direct", "stage": "bridge-fallback" }),
    )
}

/// 원격 경로라면 해낼 수 있는 플러그인의 거절과 그 이유:
///
/// - 이 devup-mcp 보다 먼저 빌드돼 스크립트를 모른다 — 읽기는 시도되지 않았다.
/// - 참조 PNG 가 브리지 답 하나(8 MiB)를 넘는다 — 조각으로 내주기 전의 플러그인이다.
/// - 큰 값의 바이트가 읽기가 가리키는 해시와 다르다 — 그 값은 원격 경로의 렌더러가
///   만든 것이라(플러그인이 답하지 못해 원격이 읽은 자산의 이어 읽기), 거기서만 같은
///   바이트가 나온다.
fn remote_may_answer(raw: &Value) -> Option<&'static str> {
    if raw["isError"] != true {
        return None;
    }
    let text = raw["content"][0]["text"].as_str()?;
    [
        ("DEVUP_BRIDGE_UNKNOWN_SCRIPT", "plugin-older-than-server"),
        ("DEVUP_REFERENCE_PNG_TOO_LARGE", "plugin-answer-too-large"),
        ("DEVUP_REFERENCE_PNG_BUSY", "plugin-busy"),
        ("DEVUP_LARGE_VALUE_CHANGED", "value-from-direct-path"),
    ]
    .into_iter()
    .find_map(|(code, reason)| text.contains(code).then_some(reason))
}

/// 원격의 답을 쓸 수 없는지. 그때는 플러그인의 답이 낫다.
///
/// - 오류로 표시됐다.
/// - 한도에 걸렸다는 문장 하나다 — `isError` 없이 오기도 한다(`is_quota_refusal`).
/// - 공식 MCP 의 텍스트 상한(20 KB)에서 잘렸다. 플러그인의 크기로 묶은 읽기를 원격이 다시
///   읽으면 생기고, 그 답은 디코딩되지 않는다.
fn direct_unusable(raw: &Value) -> bool {
    raw["isError"] == true
        || texts(raw).any(|text| {
            crate::is_quota_refusal(text, false)
                || text.trim_end().ends_with("// truncated to 20kb")
        })
}