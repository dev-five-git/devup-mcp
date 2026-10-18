

#[cfg(test)]
mod p2_recovery_tests {
    use super::*;

    #[test]
    fn an_id_from_another_process_says_so_and_cannot_be_reprojected() {
        let error = to_mcp_error(DevupError::with_details(
            ErrorCode::DevupFigmaHandoffExpired,
            "The Figma artifact is missing or expired.",
            true,
            json!({"artifactState":"issued-by-another-process"}),
        ));
        let details = &error.data.unwrap()["details"];
        assert_eq!(details["recovery"]["cause"], "issued-by-another-process");
        assert_eq!(details["recovery"]["reprojectable"], false);
        assert_eq!(details["recoveryState"], "unrecoverable");
        assert!(
            details["recoveryReason"]
                .as_str()
                .unwrap()
                .contains("different devup-mcp process")
        );
    }

    #[test]
    fn resource_misses_distinguish_reproject_reread_and_recollect() {
        let read = |details: Value, code| {
            let mut error = DevupError::new(code, "missing", false);
            error.details = details;
            resource_read_error(error).data.unwrap()["recovery"].clone()
        };
        let output = read(
            json!({"resourceKind":"output","artifactState":"live"}),
            ErrorCode::DevupFigmaHandoffInvalid,
        );
        assert_eq!(output["mode"], "reproject");
        assert_eq!(output["reprojectable"], true);
        let chunk = read(
            json!({"resourceKind":"chunk","artifactState":"live","chunkCount":2}),
            ErrorCode::DevupFigmaHandoffInvalid,
        );
        assert_eq!(chunk["mode"], "reread-manifest");
        let expired = read(
            json!({"artifactState":"expired","recoveryArguments":{"url":"u","refresh":true}}),
            ErrorCode::DevupFigmaHandoffExpired,
        );
        assert_eq!(expired["mode"], "recollect");
        assert_eq!(expired["requiresOriginalUrl"], false);
        let foreign = read(
            json!({"resourceKind":"artifact","artifactState":"issued-by-another-process"}),
            ErrorCode::DevupFigmaHandoffInvalid,
        );
        assert_eq!(foreign["cause"], "issued-by-another-process");
        assert_eq!(foreign["requiresOriginalUrl"], true);
        let invalid = read(json!(null), ErrorCode::DevupFigmaHandoffInvalid);
        assert_eq!(invalid["mode"], "fix-uri");
    }
}

#[cfg(test)]
mod section_retry_tests {
    use super::*;
    use devup_mcp_devup_ui::codegen::RootLayout;

    fn export(frame_ids: Vec<String>) -> PendingOperation {
        PendingOperation::Export {
            outputs: vec!["tsx".into()],
            previous_design_fingerprints: None,
            component_name: None,
            include_diagnostics: false,
            root_layout: RootLayout::Standalone,
            asset_names_per_node: true,
            scope: "node".into(),
            theme_options: Default::default(),
            strict: false,
            output_paths: Default::default(),
            page_scaffold: None,
            frame_ids,
            all_screens: false,
            asset_captures: vec![],
            web_ready_images: false,
            asset_output_paths: Default::default(),
            asset_public_root: None,
            fields: vec![],
            response_selection: Default::default(),
            delivery: DeliveryMode::Auto,
            width_policy: None,
        }
    }

    fn target() -> FigmaTarget {
        FigmaTarget {
            file_key: "abc".into(),
            node_id: Some("1:2".into()),
            branch_key: None,
        }
    }

    #[test]
    fn frame_link_is_retried_against_its_section_with_the_frame_selected() {
        let error = DevupError::with_details(
            ErrorCode::DevupSnapshotUnsupported,
            "DEVUP_SECTION_REQUIRED",
            false,
            json!({"pluginCode":"DEVUP_SECTION_REQUIRED","sectionId":"9:9","nodeId":"1:2"}),
        );
        let request = CollectionRequest::new(target(), CollectionScope::Node);
        let (operation, request, note) = section_retry(&error, &export(vec![]), &request).unwrap();
        assert_eq!(request.target.node_id.as_deref(), Some("9:9"));
        assert_eq!(request.section.unwrap().frame_ids, vec!["1:2".to_owned()]);
        assert!(
            matches!(operation, PendingOperation::Export { frame_ids, .. } if frame_ids == ["1:2"])
        );
        assert_eq!(note["sectionId"], "9:9");
    }

    #[test]
    fn no_section_or_other_codes_are_not_retried() {
        let request = CollectionRequest::new(target(), CollectionScope::Node);
        for details in [
            json!({"pluginCode":"DEVUP_SECTION_REQUIRED","sectionId":null}),
            json!({"pluginCode":"DEVUP_NODE_NOT_FOUND","sectionId":"9:9"}),
        ] {
            let error =
                DevupError::with_details(ErrorCode::DevupSnapshotUnsupported, "x", false, details);
            assert!(section_retry(&error, &export(vec![]), &request).is_none());
        }
    }
}
