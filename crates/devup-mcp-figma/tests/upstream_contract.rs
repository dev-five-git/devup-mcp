use devup_mcp_figma::{
    BuiltinScript, ExploreReadOptions, ReadToolCall, ResourceBatch, ResourceStyleRef,
    SearchReadOptions, SnapshotReadOptions,
};

#[test]
fn maps_every_read_call_to_the_fixed_figma_tool_contract() {
    let calls = [
        (
            ReadToolCall::metadata("file-key", Some("1:2")),
            "get_metadata",
        ),
        (
            ReadToolCall::variable_defs("file-key", "1:2"),
            "get_variable_defs",
        ),
        (
            ReadToolCall::design_context("file-key", "1:2"),
            "get_design_context",
        ),
        (
            ReadToolCall::code_connect_map("file-key", "1:2"),
            "get_code_connect_map",
        ),
        (
            ReadToolCall::screenshot("file-key", "1:2"),
            "get_screenshot",
        ),
        (
            ReadToolCall::snapshot("file-key", "1:2", BuiltinScript::NodeSnapshot),
            "use_figma",
        ),
    ];

    for (call, expected_name) in calls {
        assert_eq!(call.tool_name(), expected_name);
        let arguments = call.arguments();
        assert_eq!(
            arguments.get("fileKey").and_then(|v| v.as_str()),
            Some("file-key")
        );
    }
}

#[test]
fn snapshot_accepts_only_compiled_in_scripts() {
    let call = ReadToolCall::snapshot("file-key", "1:2", BuiltinScript::NodeSnapshot);
    let arguments = call.arguments();
    let code = arguments
        .get("code")
        .and_then(|value| value.as_str())
        .expect("built-in script");

    assert!(code.contains("figma.getNodeByIdAsync"));
    assert!(!code.contains("eval("));
    assert!(!code.contains("Function("));
}

#[test]
fn snapshot_is_byte_bounded_and_cursor_driven() {
    let call = ReadToolCall::snapshot_chunk(
        "file-key",
        "1:2",
        SnapshotReadOptions {
            offset: 7,
            max_payload_bytes: 12_000,
            max_field_bytes: 4_096,
        },
    );
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert!(code.contains("snapshotOptions"));
    assert!(code.contains("maxPayloadBytes"));
    assert!(code.contains("maxFieldBytes"));
    assert!(code.contains("$largeValue"));
    assert!(code.contains("__DEVUP_SNAPSHOT_CURSOR__"));
    assert!(code.contains("\"offset\":7"));
    assert!(code.contains("\"maxPayloadBytes\":12000"));
}

#[test]
fn large_values_use_a_compiled_bounded_read_only_continuation() {
    let call = ReadToolCall::large_value(
        "file-key",
        devup_mcp_figma::LargeValueReadOptions {
            node_id: "1:2".to_owned(),
            field: "characters".to_owned(),
            offset: 4096,
            max_chunk_bytes: 8192,
            byte_length: 20000,
            sha256: "abc123".to_owned(),
            version: Some("v1".to_owned()),
        },
    );
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert_eq!(call.tool_name(), "use_figma");
    assert!(code.contains("getNodeByIdAsync"));
    assert!(code.contains("getStyledTextSegments"));
    assert!(code.contains("dataBase64"));
    assert!(code.contains("\"offset\":4096"));
    assert!(code.contains("\"maxChunkBytes\":8192"));
    assert!(!code.contains("eval("));
    assert!(!code.contains("Function("));
}

#[test]
fn asset_export_uses_only_compiled_read_only_export_settings() {
    let call = ReadToolCall::asset_export(
        "file-key",
        Some("v1".to_owned()),
        devup_mcp_figma::AssetRequest {
            asset_id: "1:2:node".to_owned(),
            node_id: "1:2".to_owned(),
            field: "node".to_owned(),
            image_hash: None,
            format: devup_mcp_figma::AssetFormat::Svg,
            scale: 1,
        },
    );
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert_eq!(call.tool_name(), "use_figma");
    assert!(code.contains("exportAsync"));
    assert!(code.contains("figma.io.write"));
    assert!(code.contains("DEVUP_ASSET_EXPORT_FAILED"));
    assert!(code.contains("\"format\":\"svg\""));
    assert!(!code.contains("eval("));
    assert!(!code.contains("Function("));
}

#[test]
fn snapshot_manifest_covers_fields_the_devup_ui_converter_actually_reads() {
    // The manifest is scoped to devup-ui codegen consumption (verified
    // against `crates/devup-mcp-devup-ui`), not the full official Plugin API
    // surface — `maskType`, `detachedInfo`, `exposedInstances` and
    // `isExposedInstance` were removed because nothing reads them.
    let call = ReadToolCall::snapshot("file-key", "1:2", BuiltinScript::NodeSnapshot);
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    for property in [
        "\"primaryAxisAlignItems\"",
        "\"componentPropertyReferences\"",
        "\"layoutSizingHorizontal\"",
        "\"boundVariables\"",
        "\"strokeStyleId\"",
        "\"textStyleId\"",
    ] {
        assert!(code.contains(property), "manifest omitted {property}");
    }
    for property in ["\"maskType\"", "\"detachedInfo\"", "\"exposedInstances\""] {
        assert!(
            !code.contains(property),
            "manifest still carries unused {property}"
        );
    }
}

#[test]
fn snapshot_reads_only_manifest_properties_supported_by_the_runtime_node() {
    let call = ReadToolCall::snapshot("file-key", "1:2", BuiltinScript::NodeSnapshot);
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert!(code.contains("if (name in value) names.add(name)"));
    assert!(!code.contains("const names = new Set(manifest)"));
}

#[test]
fn snapshot_collects_styled_text_segments_from_the_compiled_manifest() {
    let call = ReadToolCall::snapshot("file-key", "1:2", BuiltinScript::NodeSnapshot);
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert!(code.contains("getStyledTextSegments(textSegmentManifest)"));
    assert!(code.contains("fields.styledTextSegments"));
    for field in [
        "fontName",
        "fontWeight",
        "fontSize",
        "textDecoration",
        "textCase",
        "lineHeight",
        "letterSpacing",
        "fills",
        "textStyleId",
        "fillStyleId",
        "listOptions",
        "indentation",
        "hyperlink",
    ] {
        assert!(code.contains(&format!("\"{field}\"")), "missing {field}");
    }
}

#[test]
fn search_uses_a_compiled_read_only_page_projection() {
    let call = ReadToolCall::search_snapshot(
        "file-key",
        "0:1",
        SearchReadOptions {
            query: "본연체".to_owned(),
            node_types: vec!["FRAME".to_owned()],
            match_kind: "normalized".to_owned(),
            limit: 20,
        },
    );
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert_eq!(call.tool_name(), "use_figma");
    assert!(code.contains("figma.setCurrentPageAsync(page)"));
    assert!(code.contains("page.findAll"));
    assert!(code.contains("본연체"));
    assert!(!code.contains("eval("));
    assert!(!code.contains("Function("));
}

#[test]
fn explore_uses_a_bounded_spatial_projection() {
    let call = ReadToolCall::explore_snapshot(
        "file-key",
        "3879:35481",
        ExploreReadOptions {
            projection_limit: 120,
            text_preview_limit: 96,
        },
    );
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert_eq!(call.tool_name(), "use_figma");
    assert!(code.contains("3879:35481"));
    assert!(code.contains("\"projectionLimit\":120"));
    assert!(code.contains("\"textPreviewLimit\":96"));
    assert!(!code.contains("__DEVUP_NODE_ID__"));
    assert!(!code.contains("__DEVUP_EXPLORE__"));
    assert!(!code.contains("eval("));
    assert!(!code.contains("Function("));
}

#[test]
fn section_index_exposes_compact_candidates_without_descendant_fields() {
    let call = ReadToolCall::section_index("file-key", "4217:7743");
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert_eq!(call.tool_name(), "use_figma");
    assert!(code.contains("subtreeNodeCount"));
    assert!(code.contains("estimatedSerializedBytes"));
    assert!(code.contains("directChildCount"));
    assert!(code.contains("selectionReasons"));
    assert!(code.contains("projectionTruncated"));
    assert!(!code.contains("snapshotNode"));
    assert!(!code.contains("styledTextSegments"));
    assert!(!code.contains("__DEVUP_PLUGIN_API_MANIFEST__"));
    assert!(!code.contains("eval("));
    assert!(!code.contains("Function("));
}

#[test]
fn multi_root_fast_snapshot_embeds_only_validated_root_ids() {
    let call = ReadToolCall::multi_root_snapshot(
        "file-key",
        "4217:7743",
        vec!["10:3".to_owned(), "10:2".to_owned()],
    );
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert_eq!(call.tool_name(), "use_figma");
    // The official `use_figma` schema forbids a `nodeId` argument
    // (`additionalProperties: false`); the target node is tracked outside
    // `arguments` (`PlannedCall::expected_node_id` / `HandoffCall::node_id`).
    assert!(!call.arguments().contains_key("nodeId"));
    assert!(
        call.arguments()["description"]
            .as_str()
            .unwrap()
            .contains("4217:7743")
    );
    assert!(code.contains("[\"10:3\",\"10:2\"]"));
    assert!(code.contains("requestedRootIds"));
    assert!(code.contains("rootIds: roots.map"));
    assert!(code.contains("getStyledTextSegments(textSegmentManifest)"));
    assert!(code.contains("devupFastSnapshotEnvelope"));
    assert!(!code.contains("figma.io.write"));
    assert!(!code.contains("eval("));
    assert!(!code.contains("Function("));
}

#[test]
fn style_consumers_use_async_compact_range_projection() {
    let call = ReadToolCall::resource_batch(
        "file-key",
        "1:2",
        ResourceBatch {
            variable_ids: Vec::new(),
            styles: vec![ResourceStyleRef {
                id: "s1".to_owned(),
                style_type: "TEXT".to_owned(),
                consumer_start: Some(320),
                consumer_end: Some(640),
            }],
        },
    );
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert!(code.contains("getStyleConsumersAsync"));
    assert!(code.contains("$consumerEntries"));
    assert!(code.contains("[\"parent\", \"children\", \"consumers\"]"));
    assert!(code.contains("\"consumerStart\":320"));
    assert!(code.contains("\"consumerEnd\":640"));
}

#[test]
fn used_resources_use_exact_ids_without_file_catalog_or_consumers() {
    let call = ReadToolCall::used_resources(
        "file-key",
        "1:2",
        ResourceBatch {
            variable_ids: vec!["VariableID:1:2".to_owned()],
            styles: vec![ResourceStyleRef {
                id: "S:text".to_owned(),
                style_type: "TEXT".to_owned(),
                consumer_start: None,
                consumer_end: None,
            }],
        },
    );
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert_eq!(call.tool_name(), "use_figma");
    assert!(code.contains("getVariableByIdAsync"));
    assert!(code.contains("getVariableCollectionByIdAsync"));
    assert!(code.contains("getStyleByIdAsync"));
    assert!(code.contains("usedVariableIds"));
    assert!(code.contains("usedStyleIds"));
    assert!(!code.contains("getLocalVariableCollectionsAsync"));
    assert!(!code.contains("getStyleConsumersAsync"));
}

#[test]
fn fast_snapshot_is_paginated_manifest_scoped_and_read_only() {
    let call = ReadToolCall::fast_snapshot("file-key", "1:2");
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert_eq!(call.tool_name(), "use_figma");
    assert!(code.contains("figma.getNodeByIdAsync"));
    // Node property collection no longer walks the prototype chain (that only
    // remains for variable/style *resource* serialization, which has no
    // manifest) and never buckets unlisted fields into "extra" — only the
    // checked-in manifest is ever collected for a node.
    assert!(code.contains("for (const name of manifest)"));
    assert!(!code.contains("(manifestSet.has(name) ? fields : extra)"));
    assert!(!code.contains("const manifestSet = new Set(manifest)"));
    // Default-valued fields are dropped; the tables must stay in sync with
    // `devup-mcp-devup-ui/tests/default_omission_golden.rs`.
    assert!(code.contains("const SCALAR_DEFAULTS = new Map(["));
    assert!(code.contains(r#"const NULL_SENSITIVE_FIELDS = new Set(["maxWidth", "maxHeight"]);"#));
    // Presence-sensitive fields must never appear in the omission table.
    for presence_sensitive in [
        "[\"opacity\"",
        "[\"visible\"",
        "[\"layoutPositioning\"",
        "[\"topLeftRadius\"",
        "[\"strokeWeight\"",
    ] {
        assert!(
            !code.contains(presence_sensitive),
            "{presence_sensitive} must not be omittable"
        );
    }
    // One serializer now covers both node fields and resources.
    assert!(!code.contains("function serializeResource("));
    assert!(!code.contains("function resourcePropertyNames("));
    // Byte length is measured without building a throwaway byte array.
    assert!(!code.contains("function utf8Encode("));
    assert!(code.contains("utf8ByteLength(JSON.stringify(envelope))"));
    // The cursor marker is the only page-state carrier; no `pagination` mirror.
    assert!(!code.contains("pagination:"));
    assert!(code.contains("getStyledTextSegments(textSegmentManifest)"));
    for field in [
        "strokeTopWeight",
        "strokeRightWeight",
        "strokeBottomWeight",
        "strokeLeftWeight",
    ] {
        assert!(code.contains(&format!("\"{field}\"")), "missing {field}");
    }
    assert!(code.contains("getVariableByIdAsync"));
    assert!(code.contains("getVariableCollectionByIdAsync"));
    assert!(code.contains("getStyleByIdAsync"));
    assert!(code.contains("async function collectResources(nodes)"));
    assert!(code.contains("usedVariableIds"));
    assert!(code.contains("usedStyleIds"));
    // A page carries the resources its nodes reference, so the envelope is
    // only bounded once both are built - the script must shrink the page and
    // retry rather than emit an oversized envelope.
    assert!(code.contains("nodeBudget = Math.floor(nodeBudget / 2)"));
    // Item B: PNG-chunked binary transport is gone entirely — text only,
    // dynamically byte-budgeted and cursor-paginated like the legacy path.
    assert!(!code.contains("duVp"));
    assert!(!code.contains("figma.io.write"));
    assert!(!code.contains("devup-fast-snapshot"));
    assert!(!code.contains("devupFastSnapshotDescriptor"));
    assert!(!code.contains("pngChunk"));
    assert!(!code.contains("crc32"));
    assert!(code.contains("maxPayloadBytes"));
    assert!(code.contains("__DEVUP_SNAPSHOT_CURSOR__"));
    // Every field the Rust decoder reads off the cursor marker must actually
    // be emitted. `offset` in particular is what distinguishes a first page
    // from a continuation page in `envelope.rs::peek_page_cursor`; omitting
    // it silently downgraded the whole fast path to legacy collection.
    for cursor_field in [
        "        offset,",
        "        nextOffset,",
        "        complete: nextOffset >= allNodes.length,",
        "        totalNodes: allNodes.length,",
    ] {
        assert!(
            code.contains(cursor_field),
            "cursor marker must emit {cursor_field}"
        );
    }
    // The 15KB text limit is the only envelope ceiling left; the old 1MB
    // companion check could never fire ahead of it.
    assert!(code.contains("MAX_TEXT_ENVELOPE_BYTES"));
    assert!(!code.contains("MAX_ENVELOPE_BYTES"));
    assert!(code.contains("devupFastSnapshotEnvelope"));
    assert!(code.contains("DEVUP_TARGET_IS_SECTION"));
    assert!(!code.contains("DEVUP_FIELD_VALUE_TRUNCATED"));
    assert!(!code.contains("MAX_INLINE_FIELD_BYTES"));
    assert!(!code.contains("devupLargeValueDescriptor"));
    assert!(!code.contains("$largeValue"));
    assert!(!code.contains("eval("));
    assert!(!code.contains("Function("));
}

#[test]
fn fast_snapshot_resolves_every_compiled_placeholder_for_the_requested_root() {
    let call = ReadToolCall::fast_snapshot("file-key", "3879:35518");
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert!(code.contains("const requestedRootIds = [\"3879:35518\"]"));
    // `__DEVUP_SNAPSHOT_CURSOR__` is a real runtime node-ID sentinel (same
    // one the legacy cursor snapshot uses), not a template placeholder — it
    // is never meant to be substituted, so it is excluded from this check.
    let without_cursor_sentinel = code.replace("__DEVUP_SNAPSHOT_CURSOR__", "");
    assert!(
        !without_cursor_sentinel.contains("__DEVUP_"),
        "compiled fast snapshot leaked an unresolved template placeholder"
    );
}

#[test]
fn explore_never_reads_scene_only_visibility_from_a_page() {
    let call = ReadToolCall::explore_snapshot(
        "file-key",
        "1:2",
        ExploreReadOptions {
            projection_limit: 200,
            text_preview_limit: 160,
        },
    );
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert!(!code.contains("page.visible"));
    assert!(code.contains("visible: true"));
    assert!(!code.contains("sectionQueue"));
    assert!(code.contains("traversalLimit"));
}

#[test]
fn fast_theme_collects_complete_local_theme_and_used_remote_resources_read_only() {
    let call = ReadToolCall::fast_theme("file-key");
    let arguments = call.arguments();
    let code = arguments["code"].as_str().unwrap();

    assert_eq!(call.tool_name(), "use_figma");
    assert_eq!(arguments["fileKey"], "file-key");
    assert!(arguments.get("nodeId").is_none());
    for read in [
        "getLocalVariableCollectionsAsync",
        "getLocalVariablesAsync",
        "getLocalPaintStylesAsync",
        "getLocalTextStylesAsync",
        "getLocalEffectStylesAsync",
        "getLocalGridStylesAsync",
        "getVariableByIdAsync",
        "getVariableCollectionByIdAsync",
    ] {
        assert!(code.contains(read), "missing theme read {read}");
    }
    assert!(code.contains("usedRemoteVariables"));
    // No binary transport exists any more — a theme that doesn't fit as text
    // throws and the caller falls back to the legacy per-resource path.
    assert!(!code.contains("devupFastThemeDescriptor"));
    assert!(!code.contains("devup-fast-theme"));
    assert!(!code.contains("duVp"));
    assert!(!code.contains("figma.io.write"));
    assert!(!code.contains("pngChunk"));
    assert!(code.contains("MAX_ENVELOPE_BYTES"));
    assert!(code.contains("MAX_TEXT_ENVELOPE_BYTES"));
    assert!(code.contains("devupFastThemeEnvelope"));
    assert!(!code.contains("eval("));
    assert!(!code.contains("Function("));
    for mutation in [
        "figma.create",
        ".setPluginData(",
        ".setSharedPluginData(",
        ".remove(",
        ".appendChild(",
        ".insertChild(",
        "deleteAsync(",
    ] {
        assert!(!code.contains(mutation), "theme script exposed {mutation}");
    }
}

#[test]
fn built_in_scripts_expose_only_read_operations() {
    for script in [
        BuiltinScript::NodeSnapshot,
        BuiltinScript::PageCatalog,
        BuiltinScript::SearchSnapshot,
        BuiltinScript::VariableCatalog,
        BuiltinScript::LocalVariables,
        BuiltinScript::UsedResources,
        BuiltinScript::ExploreSnapshot,
        BuiltinScript::SectionIndex,
        BuiltinScript::FastSnapshotEnvelope,
        BuiltinScript::MultiRootSnapshotEnvelope,
        BuiltinScript::FastThemeEnvelope,
    ] {
        let call = ReadToolCall::snapshot("file-key", "1:2", script);
        let code = call.arguments()["code"].as_str().unwrap().to_owned();
        for write_operation in [
            "figma.create",
            ".setPluginData(",
            ".setSharedPluginData(",
            ".remove(",
            ".appendChild(",
            ".insertChild(",
            "deleteAsync(",
        ] {
            assert!(
                !code.contains(write_operation),
                "built-in script exposed {write_operation}"
            );
        }
    }
}
