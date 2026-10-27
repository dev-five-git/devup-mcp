pub(super) fn job_timeout(script: &str, params: &Value) -> Duration {
    if job_is_asset_read(script, params) {
        let options = if script == "assets" {
            &params["asset"]
        } else {
            &params["largeValue"]
        };
        let millis = options["timeoutMs"]
            .as_u64()
            .filter(|v| (1000..=600000).contains(v))
            .unwrap_or(90000);
        Duration::from_millis(millis) + Duration::from_secs(5)
    } else {
        JOB_TIMEOUT
    }
}

pub(super) fn job_is_asset_read(script: &str, params: &Value) -> bool {
    script == "assets"
        || (script == "largeValue"
            && params["largeValue"]["field"]
                .as_str()
                .is_some_and(|field| field.starts_with("$export:")))
}

/// 한 번에 담을 수 있는 양. 로컬 소켓이라 공식 MCP 의 자름을 따를 이유가 없다.
///
/// 기본값(봉투 19KB · 필드 4KB)은 공식 MCP 가 텍스트 결과를 20,480 바이트에서
/// 자르기 때문에 생긴 것이다. 그 탓에 화면 하나가 30번 넘는 왕복으로 쪼개지고,
/// 창이 뒤로 가 있으면 왕복 하나가 60초까지 걸린다 — 실측한 값이다.
pub(crate) const BRIDGE_ENVELOPE_BYTES: usize = 8 * 1024 * 1024;
const BRIDGE_FIELD_BYTES: usize = 4 * 1024 * 1024;

/// 스냅샷 읽기의 한도를 브리지 전송에 맞게 올린다.
///
/// 값만 바꿀 뿐 무엇을 읽을지는 그대로다. 스크립트는 한 페이지에 다 담기면
/// `complete` 로 표시하고, 큰 필드도 잘리지 않으므로 수집기가 그 필드를 따로
/// 받으러 가지 않는다. 두 왕복이 한 왕복이 되는 것이 아니라 서른이 하나가 된다.
fn widen_budgets(params: &mut Value) {
    let Some(object) = params.as_object_mut() else {
        return;
    };
    let Some(snapshot) = object.get_mut("snapshot").and_then(Value::as_object_mut) else {
        return;
    };
    snapshot.insert("maxEnvelopeBytes".to_owned(), BRIDGE_ENVELOPE_BYTES.into());
    snapshot.insert("maxPayloadBytes".to_owned(), BRIDGE_ENVELOPE_BYTES.into());
    snapshot.insert("maxFieldBytes".to_owned(), BRIDGE_FIELD_BYTES.into());
}
