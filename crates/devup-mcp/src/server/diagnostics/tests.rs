

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

    #[test]
    fn plugin_identity_status_distinguishes_current_stale_and_unknown() {
        for (hash, protocol, state) in [
            (Some(devup_mcp_figma::PLUGIN_SCRIPTS_HASH), Some(devup_mcp_figma::PLUGIN_PROTOCOL), "current"),
            (Some("old-build"), Some(devup_mcp_figma::PLUGIN_PROTOCOL), "stale"),
            (None, None, "stale-unknown"),
        ] {
            let mut file = attached(None, Some(1));
            file.context.scripts_hash = hash.map(str::to_owned);
            file.context.protocol = protocol;
            let report = connection_report(AuthStatus::Disconnected, &absent_direct_snapshot(), Some(&bridge_with(vec![file])));
            assert_eq!(report["paths"]["bridge"]["pluginStale"], state != "current");
            assert_eq!(report["paths"]["bridge"]["attachedFiles"][0]["pluginBuild"]["state"], state);
            if state == "current" {
                assert_eq!(report["nextAction"]["tool"], "devup_figma_export");
            } else {
                assert_eq!(report["nextAction"]["skill"], "figma-bridge-setup");
            }
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
                in_view: Some(false),
                plugin_version: Some("0.4.6".to_owned()),
                scripts_hash: Some(devup_mcp_figma::PLUGIN_SCRIPTS_HASH.into()),
                protocol: Some(devup_mcp_figma::PLUGIN_PROTOCOL),
                ..Default::default()
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
                issue: Some(BridgeIssue::LegacyHost { owner: None }),
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
        // Out of view, status says so before a collection meets Figma's refusals.
        assert_eq!(file["inView"], false);
        assert_eq!(file["pluginVersion"], "0.4.6");
        assert!(
            file["inViewNote"]
                .as_str()
                .is_some_and(|note| note.contains("to the front"))
        );
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
        let summary = direct_only["summary"].as_str().unwrap();
        assert!(summary.contains("metered direct path"), "{summary}");
        assert!(!summary.contains("disconnected"), "{summary}");
        assert!(
            bridge_only["summary"]
                .as_str()
                .unwrap()
                .contains("no Figma allowance spent")
        );


        let neither = connection_report(
            AuthStatus::Disconnected,
            &absent_direct_snapshot(),
            Some(&bridge_with(vec![])),
        );
        assert_eq!(neither["connected"], false);
        assert_eq!(neither["status"], "disconnected");
        assert!(neither["activePath"].is_null());
        assert!(
            neither["summary"]
                .as_str()
                .is_some_and(|s| s.starts_with("Neither"))
        );
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
