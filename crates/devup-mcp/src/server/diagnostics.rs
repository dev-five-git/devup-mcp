//! Self-diagnosis for the "host has no Figma MCP registered" failure mode.
//!
//! `devup-mcp` never talks to Figma directly unless `direct` credentials are
//! stored (see `oauth.rs`). Everything else depends on the *host* exposing
//! an already-authenticated official Figma MCP for the `host` handoff path.
//! When that assumption is false, the agent driving `devup-mcp` used to get
//! a bare `needs_figma` envelope with no indication of what to do next, or a
//! one-line `{"status":"disconnected"}` from `devup_figma_auth status` that
//! gave no actionable next step. This module turns both responses into
//! structured, factual guidance:
//!
//! - [`host_requirement`] is attached to every `needs_figma` handoff step
//!   and tells the agent exactly which tool to call, what not to touch, and
//!   to stop and report rather than guess when no Figma MCP is reachable.
//! - [`doctor_report`] backs the `devup_figma_auth {"action":"doctor"}`
//!   action and reports which of the three connection paths (direct OAuth,
//!   local Dev Mode MCP, host handoff) are actually usable right now, plus
//!   client-specific setup data for the constraints that were verified by
//!   hand (client_name allowlist, redirect_uri shape, the silent callback
//!   port collision, PAT rejection).
//!
//! All facts embedded here (allowlist behavior, redirect_uri constraints,
//! the callback-port trap) were measured against the real Figma Remote MCP
//! registration endpoint; see `README.md`'s "Figma 연결 설정" section for
//! the same data in prose form. `doctor_report` performs exactly one
//! network-free-adjacent probe (a bounded local TCP connect) and no
//! external HTTP calls, so it stays cheap enough to call on every
//! diagnosis.

use std::time::Duration;

use devup_mcp_figma::AuthStatus;
use serde_json::{Value, json};

/// Loopback address the Figma desktop app's local Dev Mode MCP server binds
/// when enabled. OAuth-free; reachable regardless of which MCP client host
/// is in use.
pub const LOCAL_DEV_MODE_ADDR: &str = "127.0.0.1:3845";
/// The MCP endpoint URL for the local Dev Mode server (same host/port as
/// [`LOCAL_DEV_MODE_ADDR`], with the `/mcp` path Figma serves it on).
pub const LOCAL_DEV_MODE_ENDPOINT: &str = "http://127.0.0.1:3845/mcp";

/// Upper bound on how long a local reachability probe may block a tool
/// call. Deliberately short: this is a same-host TCP connect, not a network
/// round trip, so anything slower than a few hundred milliseconds means the
/// port simply is not listening.
const PROBE_TIMEOUT: Duration = Duration::from_millis(300);

/// Best-effort, error-swallowing TCP reachability probe. A refused
/// connection, a timeout, or any other I/O failure is reported as `false`
/// rather than propagated: a diagnostic probe must never fail the request
/// it is trying to help diagnose.
async fn probe_reachable(addr: &str, timeout: Duration) -> bool {
    tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr))
        .await
        .is_ok_and(|connection| connection.is_ok())
}

/// Probes [`LOCAL_DEV_MODE_ADDR`] with a short timeout. Never errors.
pub async fn local_dev_mode_reachable() -> bool {
    probe_reachable(LOCAL_DEV_MODE_ADDR, PROBE_TIMEOUT).await
}

fn local_dev_mode_hint(reachable: bool) -> String {
    if reachable {
        format!(
            "{LOCAL_DEV_MODE_ENDPOINT}가 응답하고 있습니다. 호스트에 이 로컬 Dev Mode MCP가 등록되어 있다면 OAuth 없이 그 도구를 바로 사용할 수 있습니다."
        )
    } else {
        format!(
            "{LOCAL_DEV_MODE_ENDPOINT}가 응답하지 않습니다. Figma 데스크톱 앱 → Preferences → Dev Mode MCP 서버를 켜면 OAuth 없이 사용할 수 있습니다 (Dev 또는 Full 시트가 있는 유료 플랜 필요)."
        )
    }
}

/// Builds the `hostRequirement` block attached to every `needs_figma`
/// handoff step. This is the single most important payload in this module:
/// without it, an agent has to infer from a bare `calls` array that it must
/// find and invoke a *different*, host-registered MCP tool, verbatim, and
/// feed the raw result back — and has no signal that guessing the design
/// instead of stopping is unacceptable. `ifUnavailable.action` is always
/// the literal string `"stop-and-report"`; do not remove or soften it.
///
/// Performs exactly one bounded local TCP probe
/// ([`local_dev_mode_reachable`]); never makes an external network call and
/// never fails the handoff it is attached to.
pub async fn host_requirement() -> Value {
    let reachable = local_dev_mode_reachable().await;
    json!({
        "reason": "devup-mcp는 Figma에 직접 접속하지 않습니다. 호스트에 등록된 공식 Figma MCP가 이 read-only 호출을 대신 실행해야 합니다.",
        "steps": [
            "이 세션에 등록된 공식 Figma MCP를 찾으세요. 흔한 이름: figma, figma-desktop, figma-local, figma-remote-mcp.",
            "calls[].tool 이름의 도구를 calls[].arguments 그대로 호출하세요. arguments의 code 필드를 절대 수정하지 마세요.",
            "받은 원본 결과를 가공 없이 devup_figma_continue { sessionId, callId, result } 로 넘기세요.",
            "status가 needs_figma면 만료(expiresAt) 전까지 반복하세요."
        ],
        "localDevMode": {
            "endpoint": LOCAL_DEV_MODE_ENDPOINT,
            "reachable": reachable,
            "hint": local_dev_mode_hint(reachable)
        },
        "ifUnavailable": {
            "action": "stop-and-report",
            "message": "Figma MCP에 접근할 수 없으면 즉시 멈추고 보고하세요. 디자인 수치를 추측해서 구현하지 마세요.",
            "setupHint": "devup_figma_auth { action: \"doctor\" } 를 호출하면 사용 가능한 경로와 클라이언트별 설정 방법을 얻을 수 있습니다."
        }
    })
}

/// Builds the response for `devup_figma_auth {"action":"doctor"}`.
///
/// `status` mirrors the existing `status` action's value so a caller that
/// only reads `status` sees no behavior change. Everything under `paths`
/// and `clientSetup` is new: `paths` reports what was actually measured
/// (stored-credential presence, a live local-TCP probe, and the structural
/// fact that host handoff availability cannot be observed from inside this
/// process), and `clientSetup` is static, verified reference data — never
/// an instruction to register under a specific product name. Registration
/// is allowlisted by Figma outside devup-mcp's control; this only reports
/// the constraint and points at the public waitlist.
pub async fn doctor_report(status: AuthStatus) -> Value {
    let reachable = local_dev_mode_reachable().await;
    let direct_available = status == AuthStatus::Connected;
    json!({
        "status": status,
        "paths": {
            "direct": {
                "available": direct_available,
                "reason": if direct_available {
                    "저장된 자격증명이 있습니다."
                } else {
                    "저장된 자격증명 없음. Figma는 allowlist된 client_name으로 등록한 client에만 Dynamic Client Registration을 허용합니다."
                }
            },
            "localDevMode": {
                "endpoint": LOCAL_DEV_MODE_ENDPOINT,
                "reachable": reachable,
                "hint": "Figma 데스크톱 → Preferences → Dev Mode MCP 서버 활성화 (Dev/Full 시트 필요)"
            },
            "hostHandoff": {
                "expectedTool": "use_figma",
                "note": "devup-mcp 내부에서는 확인 불가합니다. 호스트가 공식 Figma MCP를 노출해야 합니다."
            }
        },
        "clientSetup": client_setup()
    })
}

fn client_setup() -> Value {
    json!({
        "constraints": {
            "registerEndpoint": "POST https://api.figma.com/v1/oauth/mcp/register",
            "clientNameAllowlist": "Figma는 등록 요청의 client_name을 정확히 일치하는 allowlist로만 승인합니다(예: Codex, Claude Code는 200; OpenCode, opencode, Cursor, VS Code는 403). 승인되지 않은 이름은 JSON이 아닌 평문 'Forbidden' 본문과 함께 403을 반환하므로 여러 클라이언트의 OAuth 오류 파싱까지 함께 깨집니다. 신규 client 등록은 waitlist를 통해서만 가능합니다: https://www.figma.com/mcp-catalog/",
            "redirectUri": "redirect_uri는 경로가 정확히 /callback이어야 하고 호스트는 127.0.0.1이어야 합니다(200). localhost 호스트나 /mcp/oauth/callback 같은 다른 경로는 400으로 거절됩니다.",
            "callbackPortCaution": "OS나 보안 소프트웨어가 로컬 OAuth 콜백 포트를 이미 점유하고 있으면 브라우저는 리다이렉트에 성공한 것처럼 보이지만, 그 요청은 다른 프로세스로 전달되어 클라이언트는 에러 없이 'Waiting for authorization...' 상태로 무한 대기합니다. 콜백 포트를 다른 프로세스가 쓰고 있지 않은지 먼저 확인하세요.",
            "personalAccessToken": "Figma PAT(figd_...)는 Authorization: Bearer, X-Figma-Token 어느 방식으로도 원격 MCP에서 지원되지 않습니다."
        },
        "opencode": {
            "hint": "mcp.<name>.oauth에 clientId/clientSecret/scope/callbackPort/redirectUri를 직접 지정하면 Dynamic Client Registration을 건너뜁니다. clientId/clientSecret은 allowlist된 client_name으로 직접 등록해 발급받아야 합니다.",
            "example": {
                "mcp": {
                    "figma": {
                        "type": "remote",
                        "url": "https://mcp.figma.com/mcp",
                        "oauth": {
                            "clientId": "<allowlist된 client_name으로 등록해 발급받은 client_id>",
                            "clientSecret": "<allowlist된 client_name으로 등록해 발급받은 client_secret>",
                            "scope": "mcp:connect",
                            "callbackPort": 19876,
                            "redirectUri": "http://127.0.0.1:19876/callback"
                        }
                    }
                }
            }
        },
        "claudeCode": "claude mcp add --transport http figma https://mcp.figma.com/mcp",
        "codex": "codex mcp add figma --url https://mcp.figma.com/mcp",
        "localDevMode": {
            "endpoint": LOCAL_DEV_MODE_ENDPOINT,
            "hint": "OAuth가 필요 없습니다. Figma 데스크톱 앱에서 Dev Mode MCP 서버를 켜면 어떤 MCP 클라이언트에서도 동일하게 동작합니다. Dev 또는 Full 시트가 있는 유료 플랜이 필요합니다."
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_reachable_when_a_listener_is_bound() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        assert!(probe_reachable(&addr, PROBE_TIMEOUT).await);
    }

    #[tokio::test]
    async fn reports_unreachable_without_erroring_when_the_port_is_closed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        drop(listener);
        assert!(!probe_reachable(&addr, PROBE_TIMEOUT).await);
    }

    #[tokio::test]
    async fn host_requirement_always_instructs_stop_and_report_when_unavailable() {
        let value = host_requirement().await;
        assert_eq!(value["ifUnavailable"]["action"], "stop-and-report");
        assert!(
            !value["ifUnavailable"]["message"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        assert!(value["steps"].as_array().unwrap().len() >= 4);
        assert!(value["localDevMode"]["reachable"].is_boolean());
    }

    #[tokio::test]
    async fn doctor_report_reflects_measured_auth_status_without_changing_status_shape() {
        let connected = doctor_report(AuthStatus::Connected).await;
        assert_eq!(connected["status"], "connected");
        assert_eq!(connected["paths"]["direct"]["available"], true);

        let disconnected = doctor_report(AuthStatus::Disconnected).await;
        assert_eq!(disconnected["status"], "disconnected");
        assert_eq!(disconnected["paths"]["direct"]["available"], false);
        assert_eq!(
            disconnected["paths"]["localDevMode"]["endpoint"],
            LOCAL_DEV_MODE_ENDPOINT
        );
        assert_eq!(
            disconnected["paths"]["hostHandoff"]["expectedTool"],
            "use_figma"
        );
        assert!(disconnected["clientSetup"]["constraints"]["clientNameAllowlist"].is_string());
        assert!(disconnected["clientSetup"]["opencode"]["example"].is_object());
    }
}
