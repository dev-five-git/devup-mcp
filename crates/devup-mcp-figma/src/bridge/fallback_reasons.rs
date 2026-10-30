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
