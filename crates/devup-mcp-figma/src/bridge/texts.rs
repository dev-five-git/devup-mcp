

fn texts(raw: &Value) -> impl Iterator<Item = &str> {
    raw["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
}

/// 브리지만 읽을 수 있는 키의 읽기를 브리지가 맡지 못할 때의 거절.
///
/// 두 경우는 고치는 방법이 다르다. 스크립트 읽기면 플러그인이 끊긴 것이니 다시
/// 띄우면 되고, 그 밖의 읽기(수집이 쓰지 않는 공식 도구 — 변수 정의, 디자인 컨텍스트,
/// Code Connect)는 브리지가 애초에 못 하는 일이라 그 파일의 진짜 Figma 링크가 있어야
/// direct 경로로 읽을 수 있다.
fn bridge_only_refusal(call: &ReadToolCall) -> DevupError {
    let message = if call.bridge_job().is_some() {
        "The Devup Bridge plugin this request was addressed to is no longer attached. Run it \
         again on the file in the Figma desktop app, then repeat the call."
    } else {
        include_str!("../text/bridge-bridge_only_refusal-4e141d6f.md")
    };
    DevupError::with_details(
        ErrorCode::DevupFigmaDirectUnavailable,
        message,
        false,
        json!({ "source": "bridge", "stage": "bridge-routing", "tool": call.tool_name() }),
    )
}
