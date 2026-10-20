
use devup_mcp_figma::{
    AttachedFile, AuthStatus, BridgeIssue, BridgePathSnapshot, BridgePeer, BridgeRole,
    ClientCredentialSource, DEFAULT_CLIENT_NAME, DirectPathSnapshot, PortOwner, TokenState,
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
const KEYLESS_NOTE: &str = include_str!("../text/diagnostics-KEYLESS_NOTE-bcb2fb02.md");

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
    let summary = match active_path {
        Some(ActivePath::Bridge) => {
            "Reading through the Devup Bridge plugin: no login needed and no Figma allowance spent."
        }
        Some(ActivePath::Direct) if bridge_blocked => {
            "Reading through the metered direct path (Figma allowance is spent). The Devup Bridge plugin is not reachable from this process; this is not a server failure - paths.bridge.reason says why."
        }
        Some(ActivePath::Direct) => {
            "Reading through the metered direct path (Figma allowance is spent). No Devup Bridge plugin is attached; running it on the file avoids the cost."
        }
        None => "Neither the Devup Bridge plugin nor the direct path can serve a read right now.",
    };
    json!({
        "summary": summary,
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
                    include_str!("../text/diagnostics-connection_report-d61f53ae.md")
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

const RUN_THE_PLUGIN: &str = include_str!("../text/diagnostics-RUN_THE_PLUGIN-6f87e1f9.md");

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
            "action": format!(include_str!("../text/diagnostics-bridge_option-1573849a.md"), PLUGIN_REATTACH = PLUGIN_REATTACH),
            "then": status_again(),
        }),
        BridgeRole::Host | BridgeRole::Relay => json!({
            "path": "bridge",
            "action": RUN_THE_PLUGIN,
            "then": retry,
        }),
        BridgeRole::Connecting => json!({
            "path": "bridge",
            "action": format!(include_str!("../text/diagnostics-bridge_option-c5748d60.md"), PLUGIN_REATTACH = PLUGIN_REATTACH, port = port),
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

/// The process the operating system says holds the port, the way a person
/// finds it: its pid, and the executable, whose path shows which MCP client
/// installed it. Without that, how to look it up.
fn owner_text(owner: Option<&PortOwner>, port: &str) -> String {
    match owner {
        Some(owner) => {
            let what = match (&owner.executable, &owner.name) {
                (Some(executable), _) => format!(", {executable}"),
                (None, Some(name)) => format!(", {name}"),
                (None, None) => String::new(),
            };
            format!("pid {}{what}", owner.pid)
        }
        None => find_the_holder(port),
    }
}

/// The one step that would let this process use the bridge again.
fn repair(bridge: &BridgePathSnapshot) -> String {
    let port = port_text(bridge);
    let holder = holder_text(bridge);
    match &bridge.issue {
        Some(BridgeIssue::LegacyHost { owner }) => format!(
            include_str!("../text/diagnostics-repair-05473b24.md"),
            owner_text(owner.as_ref(), &port),
            port = port
        ),
        Some(BridgeIssue::ForeignProgram { owner, .. }) => format!(
            "Stop the program holding port {port} ({}). The plugin's port is fixed by its manifest; once the port is free this process takes it over by itself.",
            owner_text(owner.as_ref(), &port)
        ),
        Some(BridgeIssue::IncompatibleProtocol { .. }) => {
            format!(
                include_str!("../text/diagnostics-repair-a70db245.md"),
                holder = holder,
                port = port
            )
        }
        Some(BridgeIssue::AuthenticationFailed { .. }) => {
            format!(
                include_str!("../text/diagnostics-repair-c2a44c5b.md"),
                holder = holder,
                port = port
            )
        }
        Some(BridgeIssue::SecretUnavailable { .. }) => {
            format!(
                include_str!("../text/diagnostics-repair-8e741ef8.md"),
                holder = holder,
                port = port
            )
        }
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
const CREDENTIAL_SOURCE_NOTE: &str =
    include_str!("../text/diagnostics-CREDENTIAL_SOURCE_NOTE-f6bf233e.md");

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
    report["preferredPathNote"] = json!(include_str!("../text/diagnostics-doctor_report-787ad2ad.md"));
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
        reason.push_str(include_str!("../text/diagnostics-direct_path-c63b8306.md"));
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
            "note": include_str!("../text/diagnostics-direct_path-6f63807d.md")
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
            "reason": include_str!("../text/diagnostics-bridge_path-5051fedf.md"),
        });
    };
    let mut path = json!({
        "available": bridge.available(),
        "role": role_name(bridge.role),
        "listening": bridge.role == BridgeRole::Host,
        "port": bridge.port,
        "host": bridge.host.as_ref().map(|host| peer(host, bridge.role == BridgeRole::Host)),
        "attachedFiles": bridge.attached_files.iter().map(attached_file).collect::<Vec<_>>(),
        "attachedFilesNote": include_str!("../text/diagnostics-bridge_path-10545dbe.md"),
        "reason": bridge_reason(bridge),
    });
    if let Some(issue) = &bridge.issue {
        path["issue"] = json!(issue.code());
    }
    match &bridge.issue {
        // A devup-mcp from before sharing is still the port's devup-mcp, so it
        // is the `host` - named by the operating system, since it cannot say
        // its own version.
        Some(BridgeIssue::LegacyHost { owner: Some(owner) }) => {
            path["host"] = json!({
                "pid": owner.pid,
                "version": null,
                "buildId": null,
                "name": owner.name,
                "executable": owner.executable,
                "thisProcess": false,
            });
        }
        // Something that is not a devup-mcp is no host; it is only in the way.
        Some(BridgeIssue::ForeignProgram {
            owner: Some(owner), ..
        }) => {
            path["holder"] = json!({
                "pid": owner.pid,
                "name": owner.name,
                "executable": owner.executable,
            });
        }
        _ => {}
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

const SERVED_WITH_ONE_PLUGIN: &str =
    include_str!("../text/diagnostics-SERVED_WITH_ONE_PLUGIN-a04b8617.md");

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
        (BridgeRole::Host, _) if attached => {
            format!(
                include_str!("../text/diagnostics-bridge_reason-bfc91239.md"),
                SERVED_WITH_ONE_PLUGIN = SERVED_WITH_ONE_PLUGIN,
                port = port
            )
        }
        (BridgeRole::Relay, _) if attached => {
            format!(
                include_str!("../text/diagnostics-bridge_reason-2101bbfb.md"),
                SERVED_WITH_ONE_PLUGIN = SERVED_WITH_ONE_PLUGIN,
                holder = holder,
                port = port
            )
        }
        (BridgeRole::Host, _) if bridge.handover_from.is_some() => {
            format!(
                include_str!("../text/diagnostics-bridge_reason-9a4710b8.md"),
                PLUGIN_REATTACH = PLUGIN_REATTACH,
                left = left,
                port = port
            )
        }
        (BridgeRole::Relay, _) if bridge.handover_from.is_some() => {
            format!(
                include_str!("../text/diagnostics-bridge_reason-a9b48677.md"),
                PLUGIN_REATTACH = PLUGIN_REATTACH,
                holder = holder,
                left = left,
                port = port
            )
        }
        (BridgeRole::Host, _) => {
            format!(
                include_str!("../text/diagnostics-bridge_reason-6c5cdae3.md"),
                port = port
            )
        }
        (BridgeRole::Relay, _) => {
            format!(
                include_str!("../text/diagnostics-bridge_reason-76cadd39.md"),
                holder = holder,
                port = port
            )
        }
        (BridgeRole::Connecting, _) if bridge.handover_from.is_some() => {
            format!(
                include_str!("../text/diagnostics-bridge_reason-88ec9725.md"),
                PLUGIN_REATTACH = PLUGIN_REATTACH,
                left = left,
                port = port
            )
        }
        (BridgeRole::Connecting, _) => format!(
            "This process is still finding out which devup-mcp holds the bridge port {port}. Call status again in a moment."
        ),
        (BridgeRole::Unavailable, Some(BridgeIssue::LegacyHost { owner })) => format!(
            include_str!("../text/diagnostics-bridge_reason-c2bc4670.md"),
            owner_text(owner.as_ref(), &port),
            port = port
        ),
        (BridgeRole::Unavailable, Some(BridgeIssue::ForeignProgram { detail, owner })) => format!(
            include_str!("../text/diagnostics-bridge_reason-d72562c5.md"),
            owner_text(owner.as_ref(), &port),
            detail = detail,
            port = port
        ),
        (BridgeRole::Unavailable, Some(BridgeIssue::IncompatibleProtocol { theirs, ours })) => {
            format!(
                include_str!("../text/diagnostics-bridge_reason-8a3715df.md"),
                theirs.map_or_else(|| "<unstated>".to_owned(), |version| version.to_string()),
                holder = holder,
                ours = ours,
                port = port
            )
        }
        (BridgeRole::Unavailable, Some(BridgeIssue::AuthenticationFailed { detail })) => {
            format!(
                include_str!("../text/diagnostics-bridge_reason-d82d1e27.md"),
                detail = detail,
                holder = holder,
                port = port
            )
        }
        (BridgeRole::Unavailable, Some(BridgeIssue::SecretUnavailable { detail })) => {
            format!(
                include_str!("../text/diagnostics-bridge_reason-938d46a6.md"),
                detail = detail,
                holder = holder,
                port = port
            )
        }
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
        "zoom":file.context.zoom,"uiDevicePixelRatio":file.context.ui_device_pixel_ratio,
        "contextFreshness":"last-delivered-event","observedAt":file.context.observed_at,
        "lastKnownSelection":file.context.last_known_selection,"lastKnownPage":file.context.last_known_page,
        "pluginVersion":file.context.plugin_version,
        "displayContextEvidenceLimit":"Zoom is Figma viewport zoom; DPR belongs to the plugin UI iframe and does not verify screenshot capture scale.",
    });
    if let Some(in_view) = file.context.in_view {
        entry["inView"] = json!(in_view);
        if !in_view {
            entry["inViewNote"] = json!(include_str!("../text/diagnostics-attached_file-eb5b83b1.md"));
        }
    }
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
            include_str!("../text/diagnostics-direct_reason-83c3453a.md")
        }
        // `available` and `tokenState` are read by two separate calls on the
        // auth backend, so they can disagree. Saying so is the useful answer;
        // picking whichever one makes a tidier sentence is what produced the
        // contradiction this function exists to stop.
        (true, TokenState::Absent) => {
            include_str!("../text/diagnostics-direct_reason-6336653b.md")
        }
        (false, TokenState::Valid) => {
            include_str!("../text/diagnostics-direct_reason-6f3f9489.md")
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
            include_str!("../text/diagnostics-direct_reason-3faeaac8.md")
        }
        ClientCredentialSource::CliArg
        | ClientCredentialSource::Env
        | ClientCredentialSource::CredentialStore => {
            include_str!("../text/diagnostics-direct_reason-df4d5655.md")
        }
    };
    format!("{token} {client}")
}

fn client_setup() -> Value {
    json!({
        "constraints": {
            "registerEndpoint": "POST https://api.figma.com/v1/oauth/mcp/register",
            "clientNameAllowlist": include_str!("../text/diagnostics-client_setup-b61acd6f.md"),
            "redirectUri": "redirect_uri must use exactly the path /callback and the host 127.0.0.1 (200). A localhost host, or another path such as /mcp/oauth/callback, is rejected with 400.",
            "callbackPortCaution": include_str!("../text/diagnostics-client_setup-ebbeb457.md"),
            "personalAccessToken": "A Figma PAT (figd_...) is not supported by the remote MCP through either Authorization: Bearer or X-Figma-Token."
        },
        "codex": {
            "primary": true,
            "hint": include_str!("../text/diagnostics-client_setup-4350fbba.md"),
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
                "hint": include_str!("../text/diagnostics-client_setup-a528a590.md"),
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