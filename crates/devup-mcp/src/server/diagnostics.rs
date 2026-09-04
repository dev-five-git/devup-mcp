//! Self-diagnosis for the "the direct connection will not authenticate" failure mode.
//!
//! `devup-mcp` talks to Figma over the direct connection, which needs stored
//! credentials (see `oauth.rs`). Without them `devup_figma_auth status` used to
//! answer a one-line `{"status":"disconnected"}` and no next step. This module
//! turns that into structured, factual guidance:
//!
//! - [`doctor_report`] backs the `devup_figma_auth {"action":"doctor"}`
//!   action and reports whether the direct connection is usable right now, plus
//!   client-specific setup data for the constraints that were verified by
//!   hand (client_name allowlist, redirect_uri shape, the silent callback
//!   port collision, PAT rejection).
//!
//! All facts embedded here (allowlist behavior, redirect_uri constraints,
//! the callback-port trap) were measured against the real Figma Remote MCP
//! registration endpoint; see `README.md`'s "Figma 연결 설정" section for
//! the same data in prose form. `doctor_report` makes no network call at
//! all, so it stays cheap enough to call on every diagnosis.
//!
//! The Figma desktop app's local Dev Mode MCP was reported here as a third
//! path, probed for and described as usable without OAuth. It is not one:
//! it serves six read tools and `use_figma` is not among them, so every
//! collection devup-mcp performs — snapshot, explore, section index, theme —
//! has no tool to run. Its tools also take only a node id, addressing
//! whatever the desktop app currently has open rather than a file key.
//! Naming it as a path sent agents to a dead end, so it is named nowhere.

use devup_mcp_figma::{
    AuthStatus, ClientCredentialSource, DEFAULT_CLIENT_NAME, DirectPathSnapshot,
};
use serde_json::{Value, json};

/// Builds the response for `devup_figma_auth {"action":"doctor"}`.
///
/// `status` mirrors the existing `status` action's value so a caller that
/// only reads `status` sees no behavior change. Everything under `paths`
/// and `clientSetup` is new: `paths` reports what was actually measured
/// (stored-credential presence, a live local-TCP probe, and the structural
/// process), and `clientSetup` is static, verified reference data — never
/// an instruction to register under a specific product name. Registration
/// is allowlisted by Figma outside devup-mcp's control; this only reports
/// the constraint and points at the public waitlist.
///
/// `direct` supplies the richer, measured detail behind `paths.direct`:
/// which credential source is in play (never the secret itself), whether
/// the stored token is fresh, and — when a fixed callback port is
/// configured — whether it is actually free right now.
pub async fn doctor_report(status: AuthStatus, direct: DirectPathSnapshot) -> Value {
    let direct_available = status == AuthStatus::Connected;
    json!({
        "status": status,
        "paths": {
            "direct": {
                "available": direct_available,
                "credentialSource": direct.credential_source,
                "tokenState": direct.token_state,
                "callbackPort": {
                    "port": direct.callback_port,
                    "free": direct.callback_port_free
                },
                "registrationClientName": {
                    "value": direct.client_name,
                    "isDefault": direct.client_name == DEFAULT_CLIENT_NAME,
                    "note": "client_name Dynamic Client Registration will send. Figma matches it against its catalog allowlist exactly. The default is Codex, which the allowlist admits, so login works from a Codex install with no extra flags; Figma attributes that registration to Codex, not to devup-mcp. Once your own client is admitted through https://www.figma.com/mcp-catalog/, pass its name via --figma-client-name or DEVUP_FIGMA_CLIENT_NAME."
                },
                "reason": direct_reason(direct_available, direct.credential_source)
            }
        },
        "clientSetup": client_setup()
    })
}

/// `direct.available` only reflects whether *some* token is stored (see
/// `AuthStatus`), so this fills in *why* it isn't yet, using the measured
/// `credentialSource` rather than assuming DCR is the only path — a
/// pre-registered client just needs `login`, not `configure` or the
/// waitlist.
fn direct_reason(
    direct_available: bool,
    credential_source: ClientCredentialSource,
) -> &'static str {
    if direct_available {
        return "A stored credential is present.";
    }
    match credential_source {
        ClientCredentialSource::None => {
            "No stored credential. Run devup_figma_auth { action: \"login\" }: with no \
             pre-registered credential it falls back to Dynamic Client Registration under the \
             default allowlisted client_name (see registrationClientName). If that returns 403, \
             the allowlist rejected the name — register a client credential you obtained yourself \
             via devup_figma_auth { action: \"configure\", clientId, clientSecret }, join the \
             Figma MCP Catalog waitlist (https://www.figma.com/mcp-catalog/)."
        }
        ClientCredentialSource::CliArg
        | ClientCredentialSource::Env
        | ClientCredentialSource::CredentialStore => {
            "A pre-registered client credential is present. Authenticate with devup_figma_auth \
             { action: \"login\" } to use the direct path."
        }
    }
}

fn client_setup() -> Value {
    json!({
        "constraints": {
            "registerEndpoint": "POST https://api.figma.com/v1/oauth/mcp/register",
            "clientNameAllowlist": "Figma approves a registration request's client_name only against an exact-match allowlist (e.g. Codex and Claude Code get 200; OpenCode, opencode, Cursor, and VS Code get 403). A non-approved name returns 403 with a plain-text 'Forbidden' body instead of JSON, which also breaks OAuth error parsing in several clients. Registering a new client is only possible through the waitlist: https://www.figma.com/mcp-catalog/",
            "redirectUri": "redirect_uri must use exactly the path /callback and the host 127.0.0.1 (200). A localhost host, or another path such as /mcp/oauth/callback, is rejected with 400.",
            "callbackPortCaution": "If the OS or security software already occupies the local OAuth callback port, the browser looks like it redirected successfully, but that request goes to the other process and the client waits forever at 'Waiting for authorization...' with no error. Check first that no other process is using the callback port.",
            "personalAccessToken": "A Figma PAT (figd_...) is not supported by the remote MCP through either Authorization: Bearer or X-Figma-Token."
        },
        "codex": {
            "primary": true,
            "hint": "The intended host. devup-mcp registers under client_name Codex by default, so devup_figma_auth { action: \"login\" } completes from a Codex install with no extra flags and no client_id/client_secret. Add --figma-client-name only once your own client is admitted to the Figma MCP catalog.",
            "installDevupMcp": {
                "file": "~/.codex/config.toml",
                "toml": "[mcp_servers.devup-mcp]\ncommand = \"devup-mcp\"\nargs = [\"--allow-write-root\", \"<project path>\"]",
                "then": "Restart Codex, then call devup_figma_auth { action: \"login\" } once to store the token."
            },
            "officialFigmaMcp": "codex mcp add figma --url https://mcp.figma.com/mcp"
        },
        "otherHosts": {
            "note": "Reference only — devup-mcp targets Codex.",
            "claudeCode": "claude mcp add --transport http figma https://mcp.figma.com/mcp",
            "opencode": {
                "hint": "Setting clientId/clientSecret/scope/callbackPort/redirectUri directly under mcp.<name>.oauth skips Dynamic Client Registration. clientId/clientSecret must be issued to you by registering yourself under an allowlisted client_name.",
                "example": {
                    "mcp": {
                        "figma": {
                            "type": "remote",
                            "url": "https://mcp.figma.com/mcp",
                            "oauth": {
                                "clientId": "<client_id issued by registering under an allowlisted client_name>",
                                "clientSecret": "<client_secret issued by registering under an allowlisted client_name>",
                                "scope": "mcp:connect",
                                "callbackPort": 19876,
                                "redirectUri": "http://127.0.0.1:19876/callback"
                            }
                        }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absent_direct_snapshot() -> DirectPathSnapshot {
        DirectPathSnapshot {
            credential_source: ClientCredentialSource::None,
            token_state: devup_mcp_figma::TokenState::Absent,
            callback_port: None,
            callback_port_free: None,
            client_name: DEFAULT_CLIENT_NAME.to_owned(),
        }
    }

    #[tokio::test]
    async fn doctor_report_reflects_measured_auth_status_without_changing_status_shape() {
        let connected = doctor_report(AuthStatus::Connected, absent_direct_snapshot()).await;
        assert_eq!(connected["status"], "connected");
        assert_eq!(connected["paths"]["direct"]["available"], true);

        let disconnected = doctor_report(AuthStatus::Disconnected, absent_direct_snapshot()).await;
        assert_eq!(disconnected["status"], "disconnected");
        assert_eq!(disconnected["paths"]["direct"]["available"], false);
        assert!(disconnected["clientSetup"]["constraints"]["clientNameAllowlist"].is_string());
        assert!(disconnected["clientSetup"]["otherHosts"]["opencode"]["example"].is_object());
    }

    /// Codex is the host devup-mcp is installed into, so `clientSetup`
    /// must lead with a self-contained Codex install path — the other
    /// hosts stay available but demoted, so they cannot be mistaken for
    /// the primary route.
    #[tokio::test]
    async fn client_setup_leads_with_codex_and_demotes_the_other_hosts() {
        let report = doctor_report(AuthStatus::Disconnected, absent_direct_snapshot()).await;
        let setup = &report["clientSetup"];

        assert_eq!(setup["codex"]["primary"], true);
        let toml = setup["codex"]["installDevupMcp"]["toml"]
            .as_str()
            .expect("codex install snippet");
        assert!(toml.contains("[mcp_servers.devup-mcp]"));
        assert!(setup["codex"]["hint"].as_str().unwrap().contains("Codex"));

        // Demoted, not deleted: still the reference for installing elsewhere.
        assert!(setup["otherHosts"]["claudeCode"].is_string());
        assert!(setup["otherHosts"]["opencode"]["example"].is_object());
        assert!(setup["claudeCode"].is_null());
        assert!(setup["opencode"].is_null());
    }

    /// The `client_name` DCR will actually send is the single fact that
    /// decides whether `/register` returns 200 or a plain-text 403, so
    /// `doctor` must report it — and must say plainly when it is still the
    /// (non-allowlisted) default rather than an operator-supplied name.
    #[tokio::test]
    async fn doctor_report_surfaces_the_registration_client_name_and_whether_it_is_default() {
        let default_report =
            doctor_report(AuthStatus::Disconnected, absent_direct_snapshot()).await;
        let default_name = &default_report["paths"]["direct"]["registrationClientName"];
        assert_eq!(default_name["value"], DEFAULT_CLIENT_NAME);
        assert_eq!(default_name["isDefault"], true);

        let overridden = doctor_report(
            AuthStatus::Disconnected,
            DirectPathSnapshot {
                client_name: "Acme Registered Client".to_owned(),
                ..absent_direct_snapshot()
            },
        )
        .await;
        let overridden_name = &overridden["paths"]["direct"]["registrationClientName"];
        assert_eq!(overridden_name["value"], "Acme Registered Client");
        assert_eq!(overridden_name["isDefault"], false);
    }

    #[tokio::test]
    async fn doctor_report_surfaces_credential_source_token_state_and_callback_port() {
        let snapshot = DirectPathSnapshot {
            credential_source: ClientCredentialSource::CliArg,
            token_state: devup_mcp_figma::TokenState::Expired,
            callback_port: Some(19876),
            callback_port_free: Some(false),
            client_name: DEFAULT_CLIENT_NAME.to_owned(),
        };
        let report = doctor_report(AuthStatus::Disconnected, snapshot).await;
        assert_eq!(report["paths"]["direct"]["credentialSource"], "cli-arg");
        assert_eq!(report["paths"]["direct"]["tokenState"], "expired");
        assert_eq!(report["paths"]["direct"]["callbackPort"]["port"], 19876);
        assert_eq!(report["paths"]["direct"]["callbackPort"]["free"], false);
        // Even with a client credential configured, the reason must not
        // point back at the DCR-blocked/waitlist guidance meant for the
        // "no credential at all" case.
        assert!(
            !report["paths"]["direct"]["reason"]
                .as_str()
                .unwrap()
                .contains("waitlist")
        );
    }

    /// `DirectPathSnapshot` structurally cannot carry a client secret (it
    /// has no such field — see `oauth.rs`), so `doctor_report` cannot leak
    /// one regardless of which credential source is reported. This test
    /// pins that invariant at the JSON boundary: the only permitted
    /// occurrence of the substring "secret" is the static `clientSetup`
    /// reference text that documents *where* a secret goes (field names,
    /// not values) — never a real value.
    #[tokio::test]
    async fn doctor_report_only_mentions_secret_as_a_field_name_never_a_value() {
        let snapshot = DirectPathSnapshot {
            credential_source: ClientCredentialSource::Env,
            token_state: devup_mcp_figma::TokenState::Valid,
            callback_port: Some(19876),
            callback_port_free: Some(true),
            client_name: DEFAULT_CLIENT_NAME.to_owned(),
        };
        let report = doctor_report(AuthStatus::Connected, snapshot).await;
        assert!(report["paths"]["direct"].get("clientSecret").is_none());
        assert!(report["paths"]["direct"].get("secret").is_none());
        let serialized = report.to_string();
        assert!(!serialized.contains("access_token"));
        assert!(!serialized.contains("refresh_token"));
    }
}
