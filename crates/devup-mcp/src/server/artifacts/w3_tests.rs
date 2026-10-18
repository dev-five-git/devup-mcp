

#[cfg(test)]
pub(crate) mod w3_tests {
    use super::*;
    use devup_mcp_figma::{
        AssetFormat, CollectionStats, FigmaTarget, PayloadCompleteness, Snapshot,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Default)]
    pub(crate) struct Clock(pub AtomicU64);
    impl ArtifactClock for Clock {
        fn now_epoch_seconds(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }
    pub(crate) fn request() -> CollectionRequest {
        CollectionRequest::new(
            FigmaTarget {
                file_key: "fixture".into(),
                node_id: Some("1:1".into()),
                branch_key: None,
            },
            CollectionScope::Node,
        )
    }
    pub(crate) fn payload() -> CollectedPayload {
        CollectedPayload {
            target: request().target,
            scope: CollectionScope::Node,
            metadata: json!({}),
            snapshot: Snapshot {
                file_key: "fixture".into(),
                version: None,
                roots: vec!["1:1".into()],
                nodes: BTreeMap::new(),
                diagnostics: vec![],
            },
            variables: None,
            styles: None,
            completeness: PayloadCompleteness::ResolvedValuesOnly,
            source_version: None,
            stats: CollectionStats::default(),
            assets: vec![],
            reference_png: None,
            failures: vec![],
        }
    }

    /// Settles the question W3 left open about `ArtifactRequestKey::digest`.
    ///
    /// It ends in `unwrap_or_default()`, so a serialisation failure would not
    /// raise anything — it would hand back the digest of the *empty* byte
    /// string, and every key that failed would land on that one entry. Two
    /// unrelated collections would then read each other's design.
    ///
    /// It cannot fail today. The key is `String`, `Option<String>`, `bool`,
    /// `usize`, `u8`, three fieldless enums and `Vec`s of those, every one of
    /// them `#[derive(Serialize)]`; `serde_json` fails on a non-string map
    /// key, a hand-written `Serialize` that returns an error, or a writer that
    /// errors, and the writer here is a `Vec<u8>`. There is no float, so not
    /// even a non-finite one to argue about. So the fallback stays.
    ///
    /// What it stays subject to is a field added later that *can* fail — at
    /// which point the collapse is silent. This fills every field and asserts
    /// each one still reaches a digest of its own, none of them the empty one.
    #[test]
    fn w3_every_field_of_a_request_key_reaches_its_digest() {
        let populated = || {
            let mut request = request();
            request.resource_scope = ResourceScope::Used;
            request.include_context = true;
            request.metadata_only = true;
            request.variables_only = true;
            request.reference_png = true;
            request.search = Some(SearchReadOptions {
                query: "STORY-F-PROOFREAD".into(),
                node_types: vec!["FRAME".into()],
                match_kind: "normalized".into(),
                limit: 20,
            });
            request.explore = Some(ExploreReadOptions {
                projection_limit: 50,
                text_preview_limit: 10,
            });
            request.section = Some(SectionReadOptions {
                frame_ids: vec!["2:2".into()],
                all_screens: false,
            });
            request.asset_selections = vec![AssetSelection {
                usage_aspect_ratio: None,
                max_width: None,
                srcset: false,
                visible_scope_node_id: None,
                render_mode: devup_mcp_figma::AssetRenderMode::Node,
                quality: None,
                lossless: None,
                asset_id: "asset".into(),
                format: AssetFormat::Png,
                scale: 2.0,
            }];
            request
        };

        let mut branch_key = populated();
        branch_key.target.branch_key = Some("branch".into());
        let mut file_key = populated();
        file_key.target.file_key = "other".into();
        let mut node_id = populated();
        node_id.target.node_id = Some("9:9".into());
        let mut scope = populated();
        scope.scope = CollectionScope::File;
        let mut resource_scope = populated();
        resource_scope.resource_scope = ResourceScope::File;
        let mut include_context = populated();
        include_context.include_context = false;
        let mut metadata_only = populated();
        metadata_only.metadata_only = false;
        let mut variables_only = populated();
        variables_only.variables_only = false;
        let mut reference_png = populated();
        reference_png.reference_png = false;
        let mut search = populated();
        search.search = None;
        let mut explore = populated();
        explore.explore = None;
        let mut section = populated();
        section.section = None;
        let mut asset_format = populated();
        asset_format.asset_selections[0].format = AssetFormat::Svg;
        let mut asset_scale = populated();
        asset_scale.asset_selections[0].scale = 3.0;
        let mut asset_mode = populated();
        asset_mode.asset_selections[0].render_mode = devup_mcp_figma::AssetRenderMode::Fill;
        let mut asset_auto_mode = populated();
        asset_auto_mode.asset_selections[0].render_mode = devup_mcp_figma::AssetRenderMode::Auto;
        let mut asset_id = populated();
        asset_id.asset_selections[0].asset_id = "elsewhere".into();
        let mut quality = populated();
        quality.asset_selections[0].quality = Some(43);
        let mut lossless = populated();
        lossless.asset_selections[0].lossless = Some(true);
        let mut preset = populated();
        preset.web_ready_images = true;

        let mut max_width = populated();
        max_width.asset_selections[0].max_width = Some(240);
        let mut srcset = populated();
        srcset.asset_selections[0].srcset = true;
        let mut visible = populated();
        visible.asset_selections[0].render_mode = devup_mcp_figma::AssetRenderMode::Visible;
        visible.asset_selections[0].visible_scope_node_id = Some("1:1".into());
        let mut original = populated();
        original.asset_selections[0].render_mode = devup_mcp_figma::AssetRenderMode::OriginalSource;
        let mut fractional = populated();
        fractional.asset_selections[0].scale = 1.5;
        let mut usage = populated();
        usage.asset_selections[0].usage_aspect_ratio = Some(1.25);
        let empty = sha256_hex(&[]);
        let digests = [
            max_width,
            srcset,
            visible,
            original,
            fractional,
            usage,
            quality,
            lossless,
            preset,
            asset_auto_mode,
            populated(),
            branch_key,
            file_key,
            node_id,
            scope,
            resource_scope,
            include_context,
            metadata_only,
            variables_only,
            reference_png,
            search,
            explore,
            section,
            asset_format,
            asset_scale,
            asset_mode,
            asset_id,
        ]
        .iter()
        .map(|request| ArtifactRequestKey::from_collection(request).digest())
        .collect::<Vec<_>>();

        for digest in &digests {
            assert_ne!(
                digest, &empty,
                "a key that serialised to nothing would share one cache entry with every other such key"
            );
        }
        let distinct = digests.iter().collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            distinct.len(),
            digests.len(),
            "each field has to reach the digest, or two different collections share an artifact"
        );
    }

    #[tokio::test]
    async fn w3_selection_is_available_after_artifact_id_lookup() {
        let store = ArtifactStore::default();
        let mut request = request();
        request.section = Some(SectionReadOptions {
            frame_ids: vec!["2:2".into(), "3:3".into()],
            all_screens: false,
        });
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request), payload())
            .await
            .unwrap();
        let restored = store.get(&artifact.artifact_id).await.unwrap();
        let metadata = serde_json::to_value(&restored.capabilities).unwrap();
        assert_eq!(
            metadata["sectionSelection"]["frameIds"],
            json!(["2:2", "3:3"])
        );
    }

    #[tokio::test]
    async fn w3_last_selection_survives_lookup_without_changing_acquisition_key() {
        let store = ArtifactStore::default();
        let mut request = request();
        request.section = Some(SectionReadOptions {
            frame_ids: vec![],
            all_screens: false,
        });
        let key = ArtifactRequestKey::from_collection(&request);
        let artifact = store.insert(key.clone(), payload()).await.unwrap();
        store
            .remember_section_selection(
                &artifact.artifact_id,
                SectionReadOptions {
                    frame_ids: vec!["3:3".into()],
                    all_screens: false,
                },
            )
            .await
            .unwrap();
        let restored = store.lookup(&key).await.unwrap();
        assert_eq!(restored.artifact_id, artifact.artifact_id);
        assert_eq!(
            serde_json::to_value(&restored.capabilities).unwrap()["sectionSelection"]["frameIds"],
            json!(["3:3"])
        );
        store
            .remember_section_selection(
                &artifact.artifact_id,
                SectionReadOptions {
                    frame_ids: vec![],
                    all_screens: true,
                },
            )
            .await
            .unwrap();
        let restored = store.get(&artifact.artifact_id).await.unwrap();
        assert_eq!(
            serde_json::to_value(&restored.capabilities).unwrap()["sectionSelection"]["allScreens"],
            true
        );
    }

    #[tokio::test]
    async fn w3_unselected_section_index_remembers_its_first_selection() {
        let store = ArtifactStore::default();
        let mut payload = payload();
        payload.metadata = json!({"sectionIndex": {}});
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request()), payload)
            .await
            .unwrap();
        assert_eq!(artifact.capabilities.kind, ArtifactKind::SectionIndex);
        let selection = SectionReadOptions {
            frame_ids: vec!["2:2".into()],
            all_screens: false,
        };
        store
            .remember_section_selection(&artifact.artifact_id, selection.clone())
            .await
            .unwrap();
        assert_eq!(
            store
                .get(&artifact.artifact_id)
                .await
                .unwrap()
                .capabilities
                .section_selection,
            Some(selection)
        );
    }

    #[tokio::test]
    async fn r8_expired_branch_url_preserves_target_and_selection() {
        let clock = Arc::new(Clock::default());
        let store = ArtifactStore::with_clock(
            clock.clone(),
            ArtifactLimits {
                ttl: Duration::from_secs(1),
                ..Default::default()
            },
        );
        let mut request = request();
        request.target.branch_key = Some("Branch123".into());
        request.section = Some(SectionReadOptions {
            frame_ids: vec!["2:2".into()],
            all_screens: false,
        });
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request), payload())
            .await
            .unwrap();
        clock.0.store(1, Ordering::SeqCst);
        let error = store
            .get_for_export(&artifact.artifact_id)
            .await
            .unwrap_err();
        let arguments = &error.details["recoveryArguments"];
        let recovered =
            devup_mcp_figma::FigmaTarget::parse(arguments["url"].as_str().unwrap()).unwrap();
        assert_eq!(recovered, request.target);
        assert_eq!(arguments["frameIds"], json!(["2:2"]));
    }

    #[tokio::test]
    async fn w3_expired_key_is_reacquired_at_ttl_boundary() {
        let clock = Arc::new(Clock::default());
        let store = ArtifactStore::with_clock(
            clock.clone(),
            ArtifactLimits {
                ttl: Duration::from_secs(10),
                ..ArtifactLimits::default()
            },
        );
        let key = ArtifactRequestKey::from_collection(&request());
        let first = store.insert(key.clone(), payload()).await.unwrap();
        clock.0.store(10, Ordering::SeqCst);
        assert!(store.lookup(&key).await.is_none());
        let expired = store.get_for_export(&first.artifact_id).await.unwrap_err();
        assert_eq!(expired.details["artifactState"], "expired");
        assert!(
            expired.details["recoveryArguments"]["url"]
                .as_str()
                .unwrap()
                .contains("figma.com/design/")
        );
        let unknown = ArtifactStore::default()
            .get_for_export(&first.artifact_id)
            .await
            .unwrap_err();
        assert_eq!(unknown.details["artifactState"], "unknown");
        assert!(unknown.details.get("recoveryArguments").is_none());
        let second = store
            .get_or_acquire(key, false, || async { Ok(payload()) })
            .await
            .unwrap();
        assert!(!second.cache_hit);
        assert_ne!(first.artifact_id, second.artifact_id);
    }

    #[tokio::test]
    async fn w3_invalid_or_oversized_selection_does_not_replace_previous_selection() {
        let store = ArtifactStore::with_limits(ArtifactLimits {
            max_entry_bytes: 2048,
            ..ArtifactLimits::default()
        });
        let mut request = request();
        request.section = Some(SectionReadOptions {
            frame_ids: vec!["2:2".into()],
            all_screens: false,
        });
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request), payload())
            .await
            .unwrap();
        let original_bytes = store.stats().await.total_bytes;
        for selection in [
            SectionReadOptions {
                frame_ids: vec![],
                all_screens: false,
            },
            SectionReadOptions {
                frame_ids: vec!["3:3".into()],
                all_screens: true,
            },
            SectionReadOptions {
                frame_ids: vec!["3:3".into(), "3:3".into()],
                all_screens: false,
            },
            SectionReadOptions {
                frame_ids: vec!["x".repeat(4096)],
                all_screens: false,
            },
        ] {
            assert!(
                store
                    .remember_section_selection(&artifact.artifact_id, selection)
                    .await
                    .is_err()
            );
            assert_eq!(
                store
                    .get(&artifact.artifact_id)
                    .await
                    .unwrap()
                    .capabilities
                    .section_selection,
                request.section
            );
            assert_eq!(store.stats().await.total_bytes, original_bytes);
        }
        let updated = store
            .remember_section_selection(
                &artifact.artifact_id,
                SectionReadOptions {
                    frame_ids: vec![],
                    all_screens: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.size_bytes, store.stats().await.total_bytes);
        assert_eq!(updated.content_hash, artifact.content_hash);
        assert_eq!(
            updated.expires_at_epoch_seconds,
            artifact.expires_at_epoch_seconds
        );
    }

    #[test]
    fn w3_asset_capture_validation_reports_only_missing_exact_variants() {
        let mut request = request();
        let captured = AssetSelection {
            usage_aspect_ratio: None,
            max_width: None,
            srcset: false,
            visible_scope_node_id: None,
            render_mode: devup_mcp_figma::AssetRenderMode::Node,
            quality: None,
            lossless: None,
            asset_id: "2:2:node".into(),
            format: AssetFormat::Svg,
            scale: 1.0,
        };
        request.asset_selections = vec![captured.clone()];
        let capabilities = ArtifactRequestKey::from_collection(&request).capabilities();
        assert!(capabilities.validate_asset_captures(&[]).is_ok());
        assert!(!capabilities.supports_asset_captures(&[AssetSelection {
            usage_aspect_ratio: None,
            max_width: None,
            srcset: false,
            visible_scope_node_id: None,
            quality: Some(80),
            ..captured.clone()
        }]));
        assert!(!capabilities.supports_asset_captures(&[AssetSelection {
            lossless: Some(true),
            ..captured.clone()
        }]));
        assert!(
            capabilities
                .validate_asset_captures(std::slice::from_ref(&captured))
                .is_ok()
        );
        let missing = AssetSelection {
            scale: 2.0,
            ..captured.clone()
        };
        let error = capabilities
            .validate_asset_captures(&[captured, missing])
            .unwrap_err();
        assert_eq!(
            error.details["missingAssetCaptures"],
            json!([{"assetId":"2:2:node", "format":"svg", "scale":2.0,"renderMode":"node","srcset":false}])
        );
        assert!(!error.retryable);
    }

    #[test]
    fn artifact_node_capture_cannot_serve_original_fill_mode() {
        let mut request = request();
        let node: AssetSelection =
            serde_json::from_value(json!({"assetId":"sample:fills:0","format":"png","scale":1}))
                .unwrap();
        request.asset_selections = vec![node.clone()];
        let capabilities = ArtifactRequestKey::from_collection(&request).capabilities();
        let fill = AssetSelection {
            usage_aspect_ratio: None,
            max_width: None,
            srcset: false,
            visible_scope_node_id: None,
            render_mode: devup_mcp_figma::AssetRenderMode::Fill,
            quality: None,
            lossless: None,
            ..node
        };
        assert!(!capabilities.supports_asset_captures(std::slice::from_ref(&fill)));
        let error = capabilities.validate_asset_captures(&[fill]).unwrap_err();
        assert_eq!(
            error.details["missingAssetCaptures"][0]["renderMode"],
            "fill"
        );
    }

    #[test]
    fn w3_missing_asset_capture_has_actionable_details() {
        let capabilities = ArtifactRequestKey::from_collection(&request()).capabilities();
        let selection = AssetSelection {
            usage_aspect_ratio: None,
            max_width: None,
            srcset: false,
            visible_scope_node_id: None,
            render_mode: devup_mcp_figma::AssetRenderMode::Node,
            quality: None,
            lossless: None,
            asset_id: "2:2:node".into(),
            format: AssetFormat::Svg,
            scale: 1.0,
        };
        // The storage-layer validator will be consumed by W1's projection validation.
        let error = capabilities
            .validate_asset_captures(&[selection])
            .unwrap_err();
        assert!(
            error.message.contains("Remove artifactId")
                && error.message.contains("original url")
                && error.message.contains("assetRequests")
        );
        assert_eq!(
            error.details["missingAssetCaptures"][0]["assetId"],
            "2:2:node"
        );
    }

    #[tokio::test]
    async fn p2_an_id_from_another_process_is_named_as_such_not_as_unknown() {
        let store = ArtifactStore::default();
        let ours = store
            .insert(ArtifactRequestKey::from_collection(&request()), payload())
            .await
            .unwrap();
        assert!(!issued_by_another_process(&ours.artifact_id));
        assert_eq!(ours.artifact_id.len(), 43);

        let mut bytes = URL_SAFE_NO_PAD.decode(&ours.artifact_id).unwrap();
        bytes[0] ^= 0xff;
        let foreign = URL_SAFE_NO_PAD.encode(bytes);
        let error = store.get_for_export(&foreign).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupFigmaHandoffExpired);
        assert_eq!(error.details["artifactState"], "issued-by-another-process");
        // A different store in this same process is still this process.
        let unknown = ArtifactStore::default()
            .get_for_export(&ours.artifact_id)
            .await
            .unwrap_err();
        assert_eq!(unknown.details["artifactState"], "unknown");
        assert!(!issued_by_another_process("not-an-artifact-id"));
    }

    #[tokio::test]
    async fn p2_resource_misses_say_whether_the_artifact_is_still_alive() {
        let store = ArtifactStore::default();
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request()), payload())
            .await
            .unwrap();
        let manifests = store
            .attach_outputs(
                &artifact.artifact_id,
                "projection",
                vec![super::super::delivery::ProjectedOutput::text(
                    "tsx",
                    "text/plain",
                    b"hello".to_vec(),
                )],
            )
            .await
            .unwrap();
        let output = store
            .read_resource_output(&artifact.artifact_id, &"z".repeat(22), None)
            .await
            .unwrap_err();
        assert_eq!(output.details["resourceKind"], "output");
        assert_eq!(output.details["artifactState"], "live");
        let chunk = store
            .read_resource_output(&artifact.artifact_id, &manifests[0].output_id, Some(7))
            .await
            .unwrap_err();
        assert_eq!(chunk.details["resourceKind"], "chunk");
        assert_eq!(chunk.details["chunkCount"], 1);
        assert_eq!(chunk.details["manifestUri"], manifests[0].manifest_uri);
    }
}
