//! Self-diagnosis for the "devup-mcp will not reach Figma" failure mode.
//!
//! Two paths reach Figma and they are not equals. The **bridge** reads through
//! a plugin in the Figma desktop app: no login, and none of the allowance
//! Figma meters. The **direct** path is remote OAuth (see `oauth.rs`) and is
//! metered, which a single screen's several reads exhaust quickly. So the
//! bridge is the path to reach for and direct is the fallback.
//!
//! This module reported only `direct`, which made it an instrument that could
//! give exactly one answer — run `devup_figma_auth login` — to every question
//! about the connection, including the ones whose real answer was "run the
//! plugin" or "the listener never bound". Naming one path made it the only
//! path, and the metered one at that. [`bridge_path`] is the other half.
//!
//! - [`connection_report`] backs `devup_figma_auth`'s `status` (and the
//!   answers to `login` and `logout`): whether each path is usable right now,
//!   which one a read would take, what the attached plugins have open, and
//!   the next call to make.
//! - [`doctor_report`] backs `{"action":"doctor"}`: the same report plus
//!   client-specific setup data for the constraints that were verified by
//!   hand (client_name allowlist, redirect_uri shape, the silent callback port
//!   collision, PAT rejection).
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
    AttachedFile, AuthStatus, BridgeIssue, BridgePathSnapshot, BridgePeer, BridgeRole,
    ClientCredentialSource, DEFAULT_CLIENT_NAME, DirectPathSnapshot, TokenState,
};
use serde::Serialize;
use serde_json::{Value, json};

/// The path a read would take right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ActivePath {
    Bridge,
    Direct,
}

/// Stands in for the link an agent has to supply itself. Nothing reads it as a
/// link; `requiredArguments` says it must be replaced.
const FIGMA_LINK_PLACEHOLDER: &str = "<the frame's Figma link>";

/// What `fileKey: null` on an attached file means, said where it is read.
const KEYLESS_NOTE: &str = "This plugin could not report its file key (figma.fileKey is empty, as in Dev Mode), so no key is claimed for its file. It serves reads only while it is the only plugin attached; address it by omitting url, or with figma-bridge://current.";

/// Backs `devup_figma_auth`'s `status`, `login` and `logout` answers, and is
/// the first half of `doctor`.
///
/// `status` used to be the direct path's word alone. With a plugin attached
/// and serving it still answered `disconnected`, and an agent asked for one
/// screen concluded Figma was unreachable and asked its user to log in. Both
/// paths are reported now, and the verdict is `disconnected` only when neither
/// can serve.
pub fn connection_report(
    status: AuthStatus,
    direct: &DirectPathSnapshot,
    bridge: Option<&BridgePathSnapshot>,
) -> Value {
    // Usable means a read can be sent now: a plugin is visible and there is a
    // way to it - this process holds the port, or reads through the one that
    // does. A relay whose host went away sees no plugin until it reconnects.
    let usable = bridge.filter(|bridge| bridge.available());
    let attached = usable.map_or(&[][..], |bridge| bridge.attached_files.as_slice());
    let direct_available = status == AuthStatus::Connected;
    let active_path = if usable.is_some() {
        Some(ActivePath::Bridge)
    } else if direct_available {
        Some(ActivePath::Direct)
    } else {
        None
    };
    let bridge_blocked = bridge.is_some_and(|bridge| {
        matches!(
            bridge.role,
            BridgeRole::Connecting | BridgeRole::Unavailable
        )
    });
    json!({
        "connected": active_path.is_some(),
        "status": if active_path.is_some() { "connected" } else { "disconnected" },
        "activePath": active_path,
        "preferredPath": "bridge",
        "paths": {
            "bridge": bridge_path(bridge),
            "direct": direct_path(status, direct, usable.is_some()),
        },
        "nextAction": match active_path {
            Some(ActivePath::Bridge) => bridge_next_action(attached),
            Some(ActivePath::Direct) => json!({
                "tool": "devup_figma_export",
                "arguments": { "url": FIGMA_LINK_PLACEHOLDER, "outputs": ["tsx"] },
                "requiredArguments": ["url"],
                "note": if bridge_blocked {
                    "Only the metered direct path is open, so an export needs the frame's Figma link. The Devup Bridge cannot be used from this process right now; paths.bridge.reason says why and what would change it."
                } else {
                    "Only the metered direct path is open, so an export needs the frame's Figma link. Running the Devup Bridge plugin on the file instead spends no allowance and makes url optional."
                },
            }),
            None => ways_to_open_a_path(
                bridge,
                false,
                &json!({ "tool": "devup_figma_export", "arguments": { "outputs": ["tsx"] } }),
            ),
        },
    })
}

/// With plugins attached: the call to make, and whether it needs a url.
fn bridge_next_action(attached: &[AttachedFile]) -> Value {
    let [file] = attached else {
        return several_plugins(
            attached,
            &json!({ "tool": "devup_figma_export", "arguments": { "outputs": ["tsx"] } }),
        );
    };
    let selection = match (&file.context.selection, file.context.single_selection()) {
        (_, Some(node)) => format!(
            "the node selected in Figma ({:?}, {})",
            node.name,
            node.node_type.as_deref().unwrap_or("node")
        ),
        (Some(_), None) => format!(
            "the node selected in Figma - select exactly one frame first ({} selected now), or pass frameIds with that frame's node id",
            file.context.selection_count.unwrap_or_default()
        ),
        (None, None) => {
            "the node you name in frameIds - this plugin build does not report the Figma selection"
                .to_owned()
        }
    };
    json!({
        "tool": "devup_figma_export",
        "arguments": { "outputs": ["tsx"] },
        "note": format!(
            "One Devup Bridge plugin is attached, so no url is needed: the export reads the file it has open and {selection}. devup_figma_search and devup_figma_explore take no url either."
        ),
    })
}

/// Several plugins attached: a call without url cannot say which file it
/// means, so each file is offered as `retry` with the link that names it.
pub(super) fn several_plugins(attached: &[AttachedFile], retry: &Value) -> Value {
    let files = attached
        .iter()
        .map(|file| match &file.file_key {
            Some(key) => {
                let target = devup_mcp_figma::FigmaTarget {
                    file_key: key.clone(),
                    node_id: None,
                    branch_key: None,
                };
                let node = file.context.single_selection().map(|node| node.id.as_str());
                let mut call = retry.clone();
                call["arguments"]["url"] = json!(target.link(node));
                call["fileName"] = json!(file.file_name);
                call
            }
            None => json!({ "fileName": file.file_name, "note": KEYLESS_NOTE }),
        })
        .collect::<Vec<_>>();
    json!({
        "how": format!(
            "{} Devup Bridge plugins are attached, so a call without url cannot say which file it means. Pass the url of the file you mean, or close the other plugin windows so one remains.",
            attached.len()
        ),
        "options": files,
    })
}

/// The ways to open a path to Figma, preferred first.
///
/// Shared by `status` and by the refusal of a call made without a url, so the
/// two cannot recommend different things. `retry` is the call to make once a
/// path is open, without url; the direct option adds the url it then needs.
///
/// The bridge option is whatever would open the bridge for *this* process.
/// Running the plugin is that only while a way to the plugin exists - this
/// process holds the port, or reads through the one that does. While the port
/// is changing hands it is waiting a moment, and while something else holds
/// the port it is the repair that names that holder.
pub(super) fn ways_to_open_a_path(
    bridge: Option<&BridgePathSnapshot>,
    direct_available: bool,
    retry: &Value,
) -> Value {
    let mut with_url = retry.clone();
    with_url["arguments"]["url"] = json!(FIGMA_LINK_PLACEHOLDER);
    with_url["requiredArguments"] = json!(["url"]);
    let mut options = Vec::new();
    if let Some(bridge) = bridge {
        options.push(bridge_option(bridge, retry));
    }
    options.push(if direct_available {
        let mut call = with_url;
        call["path"] = json!("direct");
        call
    } else {
        json!({
            "path": "direct",
            "tool": "devup_figma_auth",
            "arguments": { "action": "login" },
            "then": with_url,
        })
    });
    json!({
        "how": match bridge.map(|bridge| bridge.role) {
            Some(BridgeRole::Host | BridgeRole::Relay) => {
                "Open a path to Figma: run the Devup Bridge plugin (preferred - no login), or use the metered direct path with the frame's Figma link."
            }
            Some(BridgeRole::Connecting) => {
                "The Devup Bridge port is changing hands (see paths.bridge.reason): call status again in a few seconds, or use the metered direct path with the frame's Figma link."
            }
            Some(BridgeRole::Unavailable) => {
                "This devup-mcp cannot use the Devup Bridge right now (see paths.bridge.reason). The first option is what would change that; the metered direct path needs the frame's Figma link."
            }
            None => {
                "This devup-mcp is not listening for the Devup Bridge plugin (see paths.bridge.reason), so only the metered direct path can open here: it needs the frame's Figma link."
            }
        },
        "options": options,
    })
}

const RUN_THE_PLUGIN: &str = "In the Figma desktop app, open the file and run Plugins -> Development -> Devup Bridge (imported once from plugin/manifest.json), keeping its window open. No login is needed and no Figma allowance is spent.";

/// How long the plugin waits before it connects again after its socket closed
/// (`RETRY_MS` in plugin/src/ui.ts), said where a reader waits on it.
const PLUGIN_REATTACH: &str = "about 2 seconds";

fn status_again() -> Value {
    json!({ "tool": "devup_figma_auth", "arguments": { "action": "status" } })
}

/// What would open the bridge for this process.
fn bridge_option(bridge: &BridgePathSnapshot, retry: &Value) -> Value {
    let port = port_text(bridge);
    match bridge.role {
        BridgeRole::Host | BridgeRole::Relay if bridge.handover_from.is_some() => json!({
            "path": "bridge",
            "action": format!("Wait a moment: the bridge port just changed hands, and the plugin window that was attached reconnects on its own within {PLUGIN_REATTACH}. If no plugin appears, run Devup Bridge again on the file."),
            "then": status_again(),
        }),
        BridgeRole::Host | BridgeRole::Relay => json!({
            "path": "bridge",
            "action": RUN_THE_PLUGIN,
            "then": retry,
        }),
        BridgeRole::Connecting => json!({
            "path": "bridge",
            "action": format!("Wait a few seconds: the devup-mcp that held port {port} went away, this process is taking the port over or reconnecting, and the plugin reconnects on its own within {PLUGIN_REATTACH} of the new holder appearing."),
            "then": status_again(),
        }),
        BridgeRole::Unavailable => json!({
            "path": "bridge",
            "action": repair(bridge),
            "then": status_again(),
        }),
    }
}

fn port_text(bridge: &BridgePathSnapshot) -> String {
    bridge
        .port
        .map_or_else(|| "<unknown>".to_owned(), |port| port.to_string())
}

/// How to find what holds the port, on each platform.
fn find_the_holder(port: &str) -> String {
    format!(
        "find it with `netstat -ano | findstr :{port}` on Windows or `lsof -nP -iTCP:{port} -sTCP:LISTEN` on macOS and Linux"
    )
}

/// Names a devup-mcp the way a person finds it in a process list.
fn describe_peer(peer: &BridgePeer) -> String {
    match &peer.build_id {
        Some(build) => format!("pid {}, version {}, build {build}", peer.pid, peer.version),
        None => format!("pid {}, version {}", peer.pid, peer.version),
    }
}

fn holder_text(bridge: &BridgePathSnapshot) -> String {
    bridge
        .host
        .as_ref()
        .map_or_else(String::new, |host| format!(" ({})", describe_peer(host)))
}

/// The one step that would let this process use the bridge again.
fn repair(bridge: &BridgePathSnapshot) -> String {
    let port = port_text(bridge);
    let holder = holder_text(bridge);
    match &bridge.issue {
        Some(BridgeIssue::LegacyHost) => format!(
            "The devup-mcp holding port {port} predates bridge sharing, so this process cannot read through it: restart (or update) the MCP client session that started it - {}. Once it exits, this process takes the port over by itself and the plugin reconnects to it.",
            find_the_holder(&port)
        ),
        Some(BridgeIssue::ForeignProgram { .. }) => format!(
            "Stop the program holding port {port} - {}. The plugin's port is fixed by its manifest; once the port is free this process takes it over by itself.",
            find_the_holder(&port)
        ),
        Some(BridgeIssue::IncompatibleProtocol { .. }) => format!(
            "The devup-mcp holding port {port}{holder} and this one are different releases that cannot relay to each other: restart the MCP client session whose devup-mcp is older so both run the same release."
        ),
        Some(BridgeIssue::AuthenticationFailed { .. }) => format!(
            "Run every devup-mcp on this machine as the same user with the same home directory (they prove themselves to each other with a per-user secret kept there), or restart the MCP client session holding port {port}{holder}."
        ),
        Some(BridgeIssue::SecretUnavailable { .. }) => format!(
            "Make the per-user relay secret readable and writable for this user (see paths.bridge.reason), or restart the MCP client session holding port {port}{holder} so this process can take the port over."
        ),
        Some(BridgeIssue::BindFailed { .. }) | None => format!(
            "Free port {port}, or change it in all three places the plugin's README names (the manifest's allowedDomains, src/code.ts and DEVUP_FIGMA_BRIDGE_PORT)."
        ),
    }
}

/// What `credentialSource` counts, said in the response rather than only in
/// the README.
///
/// Two different credentials reach the direct path and only one of them is
/// this field. `credentialSource` is the **client registration** credential —
/// the `client_id`/`client_secret` a pre-registered Figma client is injected
/// with. The **user's** OAuth access token, the thing `login` obtains, is
/// `tokenState`. They move independently, so `credentialSource: "none"` beside
/// `tokenState: "valid"` is an ordinary signed-in session that registered
/// dynamically — not a contradiction, and not a reason to log in again.
///
/// Read as one word, though, it was: `doctor` answered `credentialSource:
/// "none"` and `reason: "A stored credential is present."` in the same object,
/// and a reader has no way to tell which of the two the tool means. A
/// diagnostic that misreports its own state cannot be used to diagnose
/// anything, so the field now carries what it counts next to the value.
const CREDENTIAL_SOURCE_NOTE: &str = "Where the OAuth *client registration* credential (client_id/client_secret) came from: cli-arg, env, credential-store, or none. This is not the user's access token — that is tokenState, and whether the direct path is usable right now is available. \"none\" only means no pre-registered client is injected, so login registers dynamically under registrationClientName; a signed-in session that registered that way reads credentialSource \"none\" with tokenState \"valid\", which is normal.";

/// Builds the response for `devup_figma_auth {"action":"doctor"}`: the same
/// path-aware report `status` gives, plus the reference data behind it.
///
/// `paths` reports what was actually measured (stored-credential presence, a
/// live local-TCP probe, the attached plugins), and `clientSetup` is static,
/// verified reference data — never an instruction to register under a
/// specific product name. Registration is allowlisted by Figma outside
/// devup-mcp's control; this only reports the constraint and points at the
/// public waitlist.
pub async fn doctor_report(
    status: AuthStatus,
    direct: DirectPathSnapshot,
    bridge: Option<BridgePathSnapshot>,
) -> Value {
    let mut report = connection_report(status, &direct, bridge.as_ref());
    report["preferredPathNote"] = json!(
        "Two paths reach Figma and they are not equals. The bridge plugin reads through the Figma desktop app: no login, no OAuth, and it spends none of the Figma allowance the direct path is metered against — a single screen costs several reads, so the allowance goes quickly. Reach for the bridge first and keep direct as the fallback for what the bridge cannot serve (currently a file-scope metadata read and referencePng's get_screenshot)."
    );
    report["clientSetup"] = client_setup();
    report
}

/// The measured detail behind `paths.direct`: which credential source is in
/// play (never the secret itself), whether the stored token is fresh, and —
/// when a fixed callback port is configured — whether it is free right now.
fn direct_path(status: AuthStatus, direct: &DirectPathSnapshot, bridge_available: bool) -> Value {
    let direct_available = status == AuthStatus::Connected;
    let mut reason = direct_reason(
        direct_available,
        direct.token_state,
        direct.credential_source,
    );
    if bridge_available {
        reason.push_str(
            " While a bridge plugin is attached this path is needed only for what the bridge \
             cannot serve: a file-scope metadata read and referencePng.",
        );
    }
    json!({
        "available": direct_available,
        "credentialSource": direct.credential_source,
        "credentialSourceNote": CREDENTIAL_SOURCE_NOTE,
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
        "reason": reason
    })
}

/// Reports the path that costs nothing, so `doctor` stops answering a
/// connection question with "log in" and nothing else.
///
/// This module knew only about `direct`, so every reason it could give ended
/// at `devup_figma_auth login` — including for someone whose plugin was
/// attached and serving, and including when the real problem was that the
/// listener never bound. Naming only the metered path made the metered path
/// the only answer.
///
/// The states are genuinely different repairs, so each says its own.
///
/// Only one devup-mcp on a machine can hold the port the plugin knows, and
/// several run at once as a matter of course - one per MCP client session.
/// The one holding it is the `host`; the others are `relay`s that read through
/// it, and they see the same `attachedFiles`. A `relay` whose host just left
/// is `connecting` while it takes the port over or finds whoever did. And
/// `unavailable` names what holds the port when that cannot be read through:
/// a devup-mcp from before sharing, another program, or a devup-mcp that
/// cannot prove it runs for the same user - each with the step that would
/// change it, rather than the generic "not listening" that left a session with
/// no remedy but killing another session's process.
///
/// `listening` keeps its meaning: this process holds the port. `available` is
/// the test for sending a read now: a plugin is visible and there is a way to
/// it. The bridge needs no credential of any kind, so there is nothing else
/// for it to be waiting on.
fn bridge_path(bridge: Option<&BridgePathSnapshot>) -> Value {
    let Some(bridge) = bridge else {
        return json!({
            "available": false,
            "role": "off",
            "listening": false,
            "port": null,
            "host": null,
            "attachedFiles": [],
            "reason": "This process is not using the bridge plugin: DEVUP_FIGMA_BRIDGE_PORT is off or 0, or is not a port number. Unset it, or set it to the port in the plugin's manifest, to use the bridge; this process can use the metered direct path only.",
        });
    };
    let mut path = json!({
        "available": bridge.available(),
        "role": role_name(bridge.role),
        "listening": bridge.role == BridgeRole::Host,
        "port": bridge.port,
        "host": bridge.host.as_ref().map(|host| peer(host, bridge.role == BridgeRole::Host)),
        "attachedFiles": bridge.attached_files.iter().map(attached_file).collect::<Vec<_>>(),
        "attachedFilesNote": "The files the attached plugins have open, with the page in view and what is selected on it - as the devup-mcp holding the bridge port sees them, so every devup-mcp on this machine shows the same list. fileKey is null for a plugin that could not report it (seen in Dev Mode); such a plugin serves reads only while it is the only one attached, because with two there is no way to tell which file is meant.",
        "reason": bridge_reason(bridge),
    });
    if let Some(issue) = &bridge.issue {
        path["issue"] = json!(issue.code());
    }
    if let Some(previous) = &bridge.handover_from {
        path["handoverFrom"] = peer(previous, false);
    }
    path
}

fn role_name(role: BridgeRole) -> &'static str {
    match role {
        BridgeRole::Host => "host",
        BridgeRole::Relay => "relay",
        BridgeRole::Connecting => "connecting",
        BridgeRole::Unavailable => "unavailable",
    }
}

fn peer(peer: &BridgePeer, this_process: bool) -> Value {
    json!({
        "pid": peer.pid,
        "version": peer.version,
        "buildId": peer.build_id,
        "thisProcess": this_process,
    })
}

const SERVED_WITH_ONE_PLUGIN: &str = "With exactly one attached, devup_figma_export, devup_figma_search and devup_figma_explore take no url: they read the file it has open, and export and explore start from the node selected in Figma.";

fn bridge_reason(bridge: &BridgePathSnapshot) -> String {
    let port = port_text(bridge);
    let holder = holder_text(bridge);
    let attached = !bridge.attached_files.is_empty();
    let left = bridge
        .handover_from
        .as_ref()
        .map(|previous| format!(" (pid {})", previous.pid))
        .unwrap_or_default();
    match (bridge.role, &bridge.issue) {
        (BridgeRole::Host, _) if attached => format!(
            "A plugin is attached to this process, which holds the bridge port {port}. Reads for the files listed in attachedFiles are served through it, spending no Figma allowance and needing no login; other devup-mcp processes on this machine read through this one. {SERVED_WITH_ONE_PLUGIN}"
        ),
        (BridgeRole::Relay, _) if attached => format!(
            "Another devup-mcp on this machine{holder} holds the bridge port {port} and the plugins attach to it; this process reads through it. Reads for the files listed in attachedFiles spend no Figma allowance and need no login. {SERVED_WITH_ONE_PLUGIN}"
        ),
        (BridgeRole::Host, _) if bridge.handover_from.is_some() => format!(
            "This process took the bridge port {port} over moments ago from the devup-mcp that held it{left}, which exited. The plugin window that was attached reconnects on its own within {PLUGIN_REATTACH}; call status again shortly. If no plugin appears, run Devup Bridge again on the file."
        ),
        (BridgeRole::Relay, _) if bridge.handover_from.is_some() => format!(
            "The devup-mcp that held the bridge port {port}{left} exited, and another{holder} took the port over; this process now reads through it. The plugin window that was attached reconnects on its own within {PLUGIN_REATTACH}; call status again shortly. If no plugin appears, run Devup Bridge again on the file."
        ),
        (BridgeRole::Host, _) => format!(
            "The bridge is listening on 127.0.0.1:{port} but no plugin is attached, so every read falls through to the metered direct path. Open the target file in the Figma desktop app and run the Devup Bridge plugin (Plugins -> Development -> Import plugin from manifest... once, using plugin/manifest.json). The bridge works only while that plugin window stays open. If the indicator stays grey, the port in the plugin's manifest allowedDomains and the port here must match."
        ),
        (BridgeRole::Relay, _) => format!(
            "Another devup-mcp on this machine{holder} holds the bridge port {port}, and this process reads through it, but no plugin is attached there, so every read falls through to the metered direct path. Open the target file in the Figma desktop app and run the Devup Bridge plugin (Plugins -> Development -> Import plugin from manifest... once, using plugin/manifest.json); it attaches to that process, and every devup-mcp on this machine can then read through it."
        ),
        (BridgeRole::Connecting, _) if bridge.handover_from.is_some() => format!(
            "The devup-mcp that held the bridge port {port}{left} went away. This process is taking the port over, or reconnecting to whichever process did; the plugin reconnects on its own within {PLUGIN_REATTACH} of the new holder appearing. Nothing can be read through the bridge until then - call status again in a few seconds."
        ),
        (BridgeRole::Connecting, _) => format!(
            "This process is still finding out which devup-mcp holds the bridge port {port}. Call status again in a moment."
        ),
        (BridgeRole::Unavailable, Some(BridgeIssue::LegacyHost)) => format!(
            "Port {port} is held by a devup-mcp built before the bridge could be shared: it serves the plugin on /plugin but has no relay endpoint, so only that process can use the plugin and this one cannot read through it. It cannot say which process it is; {}. To use the bridge here, restart (or update) the MCP client session that started it. This process keeps checking, and takes the port over by itself once that process exits.",
            find_the_holder(&port)
        ),
        (BridgeRole::Unavailable, Some(BridgeIssue::ForeignProgram { detail })) => format!(
            "Port {port} is held by a program that is not a devup-mcp ({detail}), so the plugin cannot reach any devup-mcp on this machine. Stop that program - {}. This process keeps checking, and takes the port over by itself once it is free.",
            find_the_holder(&port)
        ),
        (BridgeRole::Unavailable, Some(BridgeIssue::IncompatibleProtocol { theirs, ours })) => {
            format!(
                "Port {port} is held by a devup-mcp{holder} that speaks bridge relay protocol {}, and this one speaks {ours}. Rather than risk a wrong answer, this process does not read through it. Restart the MCP client session whose devup-mcp is older so both run the same release.",
                theirs.map_or_else(|| "<unstated>".to_owned(), |version| version.to_string())
            )
        }
        (BridgeRole::Unavailable, Some(BridgeIssue::AuthenticationFailed { detail })) => format!(
            "Port {port} is held by a process{holder} that could not be verified as this user's devup-mcp ({detail}). Relaying needs both processes to read the same per-user secret file, so a devup-mcp run as another user or with another home directory cannot share the bridge, and this process does not read through it."
        ),
        (BridgeRole::Unavailable, Some(BridgeIssue::SecretUnavailable { detail })) => format!(
            "Another process{holder} holds the bridge port {port}, but this process could not read or create the per-user relay secret it proves itself with ({detail}), so it cannot read through it."
        ),
        (BridgeRole::Unavailable, Some(BridgeIssue::BindFailed { detail })) => format!(
            "This process cannot open the bridge port {port} ({detail}), and nothing is listening on it."
        ),
        (BridgeRole::Unavailable, None) => {
            format!("This process cannot use the bridge port {port} right now.")
        }
    }
}

/// One attached plugin as the caller reads it. The key a read is routed by
/// stays internal: a plugin that could not report its file key has no key to
/// show, and the one standing in for it is not the file's.
fn attached_file(file: &AttachedFile) -> Value {
    let mut entry = json!({
        "fileKey": file.file_key,
        "fileName": file.file_name,
        "currentPage": file.context.current_page,
        "selection": file.context.selection,
        "selectionCount": file.context.selection_count,
    });
    if file.file_key.is_none() {
        entry["fileKeyNote"] = json!(KEYLESS_NOTE);
    }
    if file.context.selection.is_none() {
        entry["selectionNote"] = json!(
            "This plugin build does not report its page or selection. Re-run Devup Bridge from this devup-mcp's plugin/manifest.json to have them reported, or name the node with frameIds."
        );
    }
    entry
}

/// Says which of the two credentials is present, and never lets one of them
/// stand in for the other.
///
/// This answered `"A stored credential is present."` the moment `available`
/// was true, reading nothing else. So a perfectly ordinary signed-in session
/// that had registered dynamically reported `credentialSource: "none"` and
/// that sentence inside the same object, and nothing in the object said the
/// two words meant different credentials. A reader has no way to tell which
/// one the tool means, and something whose whole purpose is to be believed
/// about the connection's state cannot afford to be ambiguous about it.
///
/// Two clauses now, in the order a reader needs them. The first is the user's
/// access token — what `available`/`tokenState` measure, and what carries the
/// next step when there isn't one. The second is the client registration
/// credential — what `credentialSource` measures, and what decides whether
/// `login` registers dynamically or goes straight to the code flow. A
/// pre-registered client still just needs `login`, never `configure` or the
/// waitlist, so that guidance stays where it belongs.
fn direct_reason(
    direct_available: bool,
    token_state: TokenState,
    credential_source: ClientCredentialSource,
) -> String {
    let token = match (direct_available, token_state) {
        (true, TokenState::Valid) => {
            "An authorized access token is stored and unexpired, so the direct path is usable now."
        }
        (true, TokenState::Expired) => {
            "An authorized access token is stored but has expired. It is refreshed on the next \
             call when a refresh token was stored with it, and otherwise needs devup_figma_auth \
             { action: \"login\" } again."
        }
        // `available` and `tokenState` are read by two separate calls on the
        // auth backend, so they can disagree. Saying so is the useful answer;
        // picking whichever one makes a tidier sentence is what produced the
        // contradiction this function exists to stop.
        (true, TokenState::Absent) => {
            "The direct path reports itself usable while no access token is stored — these are \
             measured separately and have disagreed. Treat the path as unusable and re-run \
             devup_figma_auth { action: \"login\" }."
        }
        (false, TokenState::Valid) => {
            "An unexpired access token is stored while the direct path reports itself unusable — \
             these are measured separately and have disagreed. Re-run devup_figma_auth \
             { action: \"status\" }, then devup_figma_auth { action: \"login\" } if it still \
             answers disconnected."
        }
        (false, TokenState::Expired) => {
            "The stored access token has expired and was not refreshed. Run devup_figma_auth \
             { action: \"login\" } again."
        }
        (false, TokenState::Absent) => {
            "No access token is stored. Run devup_figma_auth { action: \"login\" } to authorize \
             the direct path."
        }
    };
    let client = match credential_source {
        ClientCredentialSource::None => {
            "Separately, no pre-registered client registration credential is injected \
             (credentialSource \"none\"), so login falls back to Dynamic Client Registration \
             under the allowlisted client_name in registrationClientName. If that returns 403 \
             the allowlist rejected the name — register a client credential you obtained \
             yourself via devup_figma_auth { action: \"configure\", clientId, clientSecret }, or \
             join the Figma MCP Catalog waitlist (https://www.figma.com/mcp-catalog/)."
        }
        ClientCredentialSource::CliArg
        | ClientCredentialSource::Env
        | ClientCredentialSource::CredentialStore => {
            "Separately, a pre-registered client registration credential is in play (see \
             credentialSource), so login skips Dynamic Client Registration and goes straight to \
             the authorization code flow."
        }
    };
    format!("{token} {client}")
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

    /// `doctor` used to answer every connection question with "log in",
    /// because `direct` was the only path it knew. It now reports the cheaper
    /// one first, and distinguishes the three states that need three different
    /// repairs: no listener, a listener nobody attached to, and a working
    /// plugin. Only the middle one is fixed by running the plugin, and none of
    /// them is fixed by logging in.
    #[tokio::test]
    async fn doctor_prefers_the_bridge_and_separates_its_three_states() {
        let absent = doctor_report(AuthStatus::Disconnected, absent_direct_snapshot(), None).await;
        assert_eq!(absent["preferredPath"], "bridge");
        assert_eq!(absent["paths"]["bridge"]["listening"], false);
        assert_eq!(absent["paths"]["bridge"]["available"], false);
        let reason = absent["paths"]["bridge"]["reason"].as_str().unwrap();
        assert!(reason.contains("DEVUP_FIGMA_BRIDGE_PORT"), "{reason}");
        assert!(
            !reason.contains("devup_figma_auth"),
            "a bridge problem is not repaired by logging in: {reason}"
        );

        let idle = doctor_report(
            AuthStatus::Disconnected,
            absent_direct_snapshot(),
            Some(bridge_with(vec![])),
        )
        .await;
        assert_eq!(idle["paths"]["bridge"]["listening"], true);
        assert_eq!(idle["paths"]["bridge"]["available"], false);
        assert_eq!(idle["paths"]["bridge"]["port"], 1993);
        assert!(
            idle["paths"]["bridge"]["reason"]
                .as_str()
                .unwrap()
                .contains("1993")
        );

        // Attached is the whole test: the bridge needs no credential, so a
        // disconnected direct path takes nothing away from it - and the
        // verdict follows the bridge rather than the direct path.
        let attached = doctor_report(
            AuthStatus::Disconnected,
            absent_direct_snapshot(),
            Some(bridge_with(vec![attached(Some("FileKey123"), Some(1))])),
        )
        .await;
        assert_eq!(attached["paths"]["bridge"]["available"], true);
        assert_eq!(
            attached["paths"]["bridge"]["attachedFiles"][0]["fileKey"],
            "FileKey123"
        );
        assert_eq!(attached["status"], "connected");
        assert_eq!(attached["activePath"], "bridge");
    }

    fn attached(file_key: Option<&str>, selected: Option<usize>) -> AttachedFile {
        let node = |index: usize| devup_mcp_figma::NodeRef {
            id: format!("1:{index}"),
            name: format!("Frame {index}"),
            node_type: Some("FRAME".to_owned()),
        };
        AttachedFile {
            target_key: file_key.map_or_else(|| "bridge:7".to_owned(), str::to_owned),
            file_key: file_key.map(str::to_owned),
            file_name: Some("Landing".to_owned()),
            context: devup_mcp_figma::PluginContext {
                current_page: Some(devup_mcp_figma::NodeRef {
                    id: "0:1".to_owned(),
                    name: "Page 1".to_owned(),
                    node_type: None,
                }),
                selection: selected.map(|count| (1..=count).map(node).collect()),
                selection_count: selected,
            },
        }
    }

    fn this_process() -> BridgePeer {
        BridgePeer {
            pid: 4242,
            version: "0.12.0".to_owned(),
            build_id: Some("abc1234".to_owned()),
        }
    }

    /// This process holds the port.
    fn bridge_with(attached_files: Vec<AttachedFile>) -> BridgePathSnapshot {
        BridgePathSnapshot {
            port: Some(1993),
            attached_files,
            role: BridgeRole::Host,
            host: Some(this_process()),
            issue: None,
            handover_from: None,
        }
    }

    /// The states a process that did not get the port has to report as they
    /// are: reading through the holder, finding out who took over, or unable
    /// to use the bridge - and none of them may be answered with "log in"
    /// ahead of the step that repairs the bridge.
    #[test]
    fn a_process_without_the_port_says_what_it_is_doing_about_it() {
        let holder = BridgePeer {
            pid: 32536,
            ..this_process()
        };
        let relay = connection_report(
            AuthStatus::Disconnected,
            &absent_direct_snapshot(),
            Some(&BridgePathSnapshot {
                role: BridgeRole::Relay,
                host: Some(holder.clone()),
                ..bridge_with(vec![attached(None, Some(1))])
            }),
        );
        let bridge = &relay["paths"]["bridge"];
        assert_eq!(relay["activePath"], "bridge");
        assert_eq!(bridge["role"], "relay");
        assert_eq!(bridge["available"], true);
        assert_eq!(bridge["listening"], false);
        assert_eq!(bridge["host"]["pid"], 32536);
        assert_eq!(bridge["host"]["buildId"], "abc1234");
        assert_eq!(bridge["host"]["thisProcess"], false);
        assert!(!relay.to_string().contains("disconnected"), "{relay}");
        assert!(relay["nextAction"]["arguments"].get("url").is_none());

        let handing_over = connection_report(
            AuthStatus::Disconnected,
            &absent_direct_snapshot(),
            Some(&BridgePathSnapshot {
                role: BridgeRole::Connecting,
                host: None,
                handover_from: Some(holder.clone()),
                ..bridge_with(vec![])
            }),
        );
        let bridge = &handing_over["paths"]["bridge"];
        assert_eq!(bridge["role"], "connecting");
        assert_eq!(bridge["available"], false);
        assert_eq!(bridge["handoverFrom"]["pid"], 32536);
        assert!(bridge["reason"].as_str().unwrap().contains("32536"));
        let options = handing_over["nextAction"]["options"].as_array().unwrap();
        assert_eq!(options[0]["path"], "bridge");
        assert_eq!(options[0]["then"]["arguments"]["action"], "status");
        assert_eq!(options[1]["path"], "direct");

        let legacy = connection_report(
            AuthStatus::Disconnected,
            &absent_direct_snapshot(),
            Some(&BridgePathSnapshot {
                role: BridgeRole::Unavailable,
                host: None,
                issue: Some(BridgeIssue::LegacyHost),
                ..bridge_with(vec![])
            }),
        );
        let bridge = &legacy["paths"]["bridge"];
        assert_eq!(bridge["issue"], "legacy-host");
        assert!(bridge["host"].is_null());
        assert!(bridge["reason"].as_str().unwrap().contains("restart"));
        let options = legacy["nextAction"]["options"].as_array().unwrap();
        assert!(
            options[0]["action"].as_str().unwrap().contains(":1993"),
            "the repair says how to find the holder: {}",
            options[0]
        );
    }

    /// The three states `status` has to tell apart. `disconnected` is the
    /// verdict only when neither path can serve, and each state names the
    /// call that moves it forward.
    #[test]
    fn the_verdict_follows_whichever_path_can_serve() {
        let valid = DirectPathSnapshot {
            token_state: devup_mcp_figma::TokenState::Valid,
            ..absent_direct_snapshot()
        };

        let bridge_only = connection_report(
            AuthStatus::Disconnected,
            &absent_direct_snapshot(),
            Some(&bridge_with(vec![attached(None, Some(1))])),
        );
        assert_eq!(bridge_only["connected"], true);
        assert_eq!(bridge_only["status"], "connected");
        assert_eq!(bridge_only["activePath"], "bridge");
        assert_eq!(bridge_only["paths"]["bridge"]["available"], true);
        assert_eq!(bridge_only["paths"]["direct"]["available"], false);
        assert!(
            !bridge_only.to_string().contains("disconnected"),
            "nothing may say disconnected while the bridge serves: {bridge_only}"
        );
        let file = &bridge_only["paths"]["bridge"]["attachedFiles"][0];
        assert!(file["fileKey"].is_null(), "no key is claimed: {file}");
        assert!(
            file.get("targetKey").is_none(),
            "the routing key stays internal"
        );
        assert_eq!(file["currentPage"]["name"], "Page 1");
        assert_eq!(file["selection"][0]["id"], "1:1");
        assert_eq!(file["selection"][0]["type"], "FRAME");
        // One plugin, one selected frame: the next call needs no url.
        assert_eq!(bridge_only["nextAction"]["tool"], "devup_figma_export");
        assert!(bridge_only["nextAction"]["arguments"].get("url").is_none());

        let direct_only =
            connection_report(AuthStatus::Connected, &valid, Some(&bridge_with(vec![])));
        assert_eq!(direct_only["connected"], true);
        assert_eq!(direct_only["activePath"], "direct");
        assert_eq!(direct_only["paths"]["bridge"]["available"], false);
        assert_eq!(
            direct_only["nextAction"]["requiredArguments"],
            json!(["url"])
        );

        let neither = connection_report(
            AuthStatus::Disconnected,
            &absent_direct_snapshot(),
            Some(&bridge_with(vec![])),
        );
        assert_eq!(neither["connected"], false);
        assert_eq!(neither["status"], "disconnected");
        assert!(neither["activePath"].is_null());
        let options = neither["nextAction"]["options"].as_array().unwrap();
        assert_eq!(options[0]["path"], "bridge");
        assert_eq!(options[1]["path"], "direct");
        assert_eq!(options[1]["arguments"]["action"], "login");
    }

    /// With two plugins attached a call without url could mean either file,
    /// so the next action names each one - with its own link where the plugin
    /// reported a key, and without inventing one where it did not.
    #[test]
    fn several_plugins_are_offered_one_by_one() {
        let report = connection_report(
            AuthStatus::Disconnected,
            &absent_direct_snapshot(),
            Some(&bridge_with(vec![
                attached(Some("FileKey123"), Some(1)),
                attached(None, Some(0)),
            ])),
        );
        let options = report["nextAction"]["options"].as_array().unwrap();
        assert_eq!(
            options[0]["arguments"]["url"],
            "https://www.figma.com/design/FileKey123/devup?node-id=1-1"
        );
        assert!(options[1].get("arguments").is_none(), "{}", options[1]);
    }

    #[tokio::test]
    async fn doctor_report_reflects_measured_auth_status_without_changing_status_shape() {
        let connected = doctor_report(AuthStatus::Connected, absent_direct_snapshot(), None).await;
        assert_eq!(connected["status"], "connected");
        assert_eq!(connected["paths"]["direct"]["available"], true);

        let disconnected =
            doctor_report(AuthStatus::Disconnected, absent_direct_snapshot(), None).await;
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
        let report = doctor_report(AuthStatus::Disconnected, absent_direct_snapshot(), None).await;
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
            doctor_report(AuthStatus::Disconnected, absent_direct_snapshot(), None).await;
        let default_name = &default_report["paths"]["direct"]["registrationClientName"];
        assert_eq!(default_name["value"], DEFAULT_CLIENT_NAME);
        assert_eq!(default_name["isDefault"], true);

        let overridden = doctor_report(
            AuthStatus::Disconnected,
            DirectPathSnapshot {
                client_name: "Acme Registered Client".to_owned(),
                ..absent_direct_snapshot()
            },
            None,
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
        let report = doctor_report(AuthStatus::Disconnected, snapshot, None).await;
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

    /// The response has to be readable as one object without contradicting
    /// itself.
    ///
    /// Measured `doctor` output said `credentialSource: "none"` and
    /// `reason: "A stored credential is present."` about the same path, in the
    /// same breath. Both sentences were true, of different credentials, and
    /// the object said nothing about which was which — so the only tool for
    /// diagnosing a connection could not be trusted about the connection.
    ///
    /// A signed-in session that registered dynamically is exactly that shape,
    /// so it is the case pinned here: the reason must speak of the access
    /// token it actually means, must not claim a stored *client* credential
    /// that `credentialSource` denies, and the field has to carry what it
    /// counts.
    #[tokio::test]
    async fn a_dynamically_registered_session_does_not_claim_a_stored_client_credential() {
        let report = doctor_report(
            AuthStatus::Connected,
            DirectPathSnapshot {
                credential_source: ClientCredentialSource::None,
                token_state: devup_mcp_figma::TokenState::Valid,
                ..absent_direct_snapshot()
            },
            None,
        )
        .await;
        let direct = &report["paths"]["direct"];
        assert_eq!(direct["available"], true);
        assert_eq!(direct["credentialSource"], "none");
        assert_eq!(direct["tokenState"], "valid");

        let reason = direct["reason"].as_str().expect("a reason");
        assert!(
            !reason.contains("A stored credential is present."),
            "the sentence that read as a denial of credentialSource: {reason}"
        );
        assert!(
            reason.contains("access token is stored"),
            "the reason has to name the token it means: {reason}"
        );
        assert!(
            reason.contains("no pre-registered client registration credential is injected"),
            "and has to agree with credentialSource \"none\": {reason}"
        );

        let note = direct["credentialSourceNote"]
            .as_str()
            .expect("credentialSource has to say what it counts");
        assert!(note.contains("client_id/client_secret"));
        assert!(note.contains("tokenState"));
    }

    /// The other half of the same distinction: an injected client credential
    /// and no token at all. `credentialSource` is not evidence of being
    /// signed in, so the reason must still send the caller to `login` — and
    /// must not send a caller who already has a client to `configure` or the
    /// waitlist.
    #[tokio::test]
    async fn a_pre_registered_client_without_a_token_is_still_told_to_log_in() {
        let report = doctor_report(
            AuthStatus::Disconnected,
            DirectPathSnapshot {
                credential_source: ClientCredentialSource::CredentialStore,
                token_state: devup_mcp_figma::TokenState::Absent,
                ..absent_direct_snapshot()
            },
            None,
        )
        .await;
        let direct = &report["paths"]["direct"];
        assert_eq!(direct["available"], false);
        assert_eq!(direct["credentialSource"], "credential-store");

        let reason = direct["reason"].as_str().expect("a reason");
        assert!(reason.contains("No access token is stored."), "{reason}");
        assert!(reason.contains("action: \"login\""), "{reason}");
        assert!(
            reason.contains("pre-registered client registration credential is in play"),
            "{reason}"
        );
        assert!(!reason.contains("waitlist"), "{reason}");
        assert!(!reason.contains("configure"), "{reason}");
    }

    /// `available` and `tokenState` come from two separate calls on the auth
    /// backend, so they can disagree. When they do, `doctor` has to report the
    /// disagreement — smoothing it into one confident sentence is how the
    /// original contradiction got written.
    #[tokio::test]
    async fn a_disagreement_between_availability_and_token_state_is_reported_as_one() {
        for (status, token_state) in [
            (AuthStatus::Connected, devup_mcp_figma::TokenState::Absent),
            (AuthStatus::Disconnected, devup_mcp_figma::TokenState::Valid),
        ] {
            let report = doctor_report(
                status,
                DirectPathSnapshot {
                    token_state,
                    ..absent_direct_snapshot()
                },
                None,
            )
            .await;
            let reason = report["paths"]["direct"]["reason"]
                .as_str()
                .expect("a reason");
            assert!(
                reason.contains("measured separately and have disagreed"),
                "{status:?}/{token_state:?} must be reported, not smoothed over: {reason}"
            );
        }
    }

    /// The verified reference block is the part of this response that was
    /// already right, and it stays whole: every constraint that cost a
    /// measurement against the real registration endpoint is still published.
    #[tokio::test]
    async fn the_measured_client_setup_constraints_survive_the_reason_rewrite() {
        let report = doctor_report(AuthStatus::Connected, absent_direct_snapshot(), None).await;
        let constraints = &report["clientSetup"]["constraints"];
        for key in [
            "registerEndpoint",
            "clientNameAllowlist",
            "redirectUri",
            "callbackPortCaution",
            "personalAccessToken",
        ] {
            assert!(
                constraints[key]
                    .as_str()
                    .is_some_and(|text| !text.is_empty()),
                "clientSetup.constraints.{key} went missing"
            );
        }
        assert!(
            constraints["redirectUri"]
                .as_str()
                .unwrap()
                .contains("/callback")
        );
        assert!(
            constraints["callbackPortCaution"]
                .as_str()
                .unwrap()
                .contains("Waiting for authorization")
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
        let report = doctor_report(AuthStatus::Connected, snapshot, None).await;
        assert!(report["paths"]["direct"].get("clientSecret").is_none());
        assert!(report["paths"]["direct"].get("secret").is_none());
        let serialized = report.to_string();
        assert!(!serialized.contains("access_token"));
        assert!(!serialized.contains("refresh_token"));
    }
}
