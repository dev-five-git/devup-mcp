use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_devup_ui::{
    codegen::{
        CodegenOptions, generate_component, generate_component_set_target,
        generate_inlined_component_instance,
    },
    provenance::{FidelityImpactCounts, FidelityReport, ProjectionDisposition, validate_fidelity},
    theme::{
        ThemeScope, VariableCollection, VariableDefinition, VariableMode, VariableSnapshot,
        VariableStyle, generate_devup_json,
    },
};
use devup_mcp_figma::{RawNode, Snapshot};
use serde_json::{Map, json};

fn node(id: &str, node_type: &str, fields: serde_json::Value) -> RawNode {
    RawNode {
        id: id.to_owned(),
        node_type: node_type.to_owned(),
        fields: serde_json::from_value(fields).unwrap(),
        extra: Map::new(),
        field_errors: BTreeMap::new(),
    }
}

fn fidelity_snapshot(characters: &str) -> Snapshot {
    Snapshot {
        file_key: "FileKey123".to_owned(),
        version: Some("v1".to_owned()),
        roots: vec!["1:1".to_owned()],
        nodes: [
            node(
                "1:1",
                "FRAME",
                json!({
                    "name": "Card", "childrenIds": ["1:2"], "layoutMode": "VERTICAL",
                    "inferredAutoLayout": {"layoutMode": "VERTICAL"},
                    "width": 320, "height": 200, "clipsContent": true,
                    "fills": [{
                        "type": "SOLID", "color": {"r": 1, "g": 0, "b": 0},
                        "boundVariables": {"color": {"type": "VARIABLE_ALIAS", "id": "v"}}
                    }]
                }),
            ),
            node(
                "1:2",
                "TEXT",
                json!({
                    "name": "Label", "parentId": "1:1", "childrenIds": [],
                    "characters": characters, "textStyleId": "s",
                    "styledTextSegments": [{
                        "characters": characters, "start": 0, "end": characters.len(),
                        "textStyleId": "s", "fontName": {"family": "Inter", "style": "Regular"},
                        "fontSize": 16, "fontWeight": 400,
                        "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0}}]
                    }],
                    "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0}}]
                }),
            ),
        ]
        .into_iter()
        .map(|node| (node.id.clone(), node))
        .collect(),
        diagnostics: Vec::new(),
    }
}

#[test]
fn strict_fidelity_rejects_an_approximated_impact() {
    let report = FidelityReport {
        syntax_valid: true,
        impacts: FidelityImpactCounts {
            approximated: 1,
            ..FidelityImpactCounts::default()
        },
        ..FidelityReport::default()
    };

    assert!(!report.strict_compatible());
}

fn fidelity_options() -> CodegenOptions {
    CodegenOptions {
        variable_tokens: BTreeMap::from([
            ("short".to_owned(), "pri".to_owned()),
            ("v".to_owned(), "primary".to_owned()),
        ]),
        text_style_tokens: BTreeMap::from([("s".to_owned(), "body".to_owned())]),
        ..CodegenOptions::default()
    }
}

#[test]
fn tsx_byte_ranges_trace_components_text_props_and_resources() {
    let snapshot = fidelity_snapshot("Hello provenance");
    let output = generate_component(&snapshot, "1:1", &fidelity_options()).unwrap();

    assert!(!output.tsx.contains("DEVUP_PROVENANCE"));
    assert!(!output.tsx.contains("1:1"));
    for entry in &output.source_map.entries {
        let range = entry.generated_range.as_ref().expect("tsx range");
        assert!(range.start < range.end && range.end <= output.tsx.len());
        assert!(output.tsx.is_char_boundary(range.start));
        assert!(output.tsx.is_char_boundary(range.end));
    }
    let find = |node_id: &str, property: &str| {
        output
            .source_map
            .entries
            .iter()
            .find(|entry| {
                entry.node_id.as_deref() == Some(node_id)
                    && entry.property.as_deref() == Some(property)
            })
            .expect("provenance entry")
    };
    let component = find("1:1", "type");
    assert_eq!(slice(&output.tsx, component), "VStack");
    let fill = find("1:1", "fills");
    assert_eq!(slice(&output.tsx, fill), "bg=\"$primary\"");
    assert_eq!(fill.variable_id.as_deref(), Some("v"));
    assert_eq!(fill.resolution, "variable-token");
    let width = find("1:1", "width");
    assert!(matches!(
        slice(&output.tsx, width),
        "boxSize=\"100%\"" | "w=\"320px\""
    ));
    let overflow = find("1:1", "clipsContent");
    assert_eq!(slice(&output.tsx, overflow), "overflow=\"hidden\"");
    let text = find("1:2", "characters");
    assert_eq!(slice(&output.tsx, text), "Hello provenance");
    let typography = find("1:2", "textStyleId");
    assert_eq!(slice(&output.tsx, typography), "typography=\"body\"");
    assert_eq!(typography.style_id.as_deref(), Some("s"));
    assert_eq!(typography.resolution, "style-token");

    assert_eq!(output.fidelity_report.nodes.total, 2);
    assert_eq!(output.fidelity_report.nodes.covered, 2);
    assert_eq!(output.fidelity_report.nodes.basis_points, 10_000);
    assert!(output.fidelity_report.strict_compatible());
    assert_eq!(output.projection_trace.entries.len(), 2);
    assert!(output.projection_trace.entries.iter().all(|entry| {
        entry.disposition == ProjectionDisposition::Emitted
            && entry
                .generated_range
                .as_ref()
                .is_some_and(|range| range.start < range.end)
    }));

    let mut incomplete = output.clone();
    incomplete.projection_trace.entries.pop();
    let error = validate_fidelity(&snapshot, "1:1", &incomplete)
        .expect_err("missing trace disposition must fail");
    assert_eq!(error.code, devup_mcp_figma::ErrorCode::DevupCodegenFailed);
    assert!(!error.details.to_string().contains("Hello provenance"));
}

#[test]
fn strict_fidelity_uses_snapshot_variables_instead_of_emitted_mappings_as_denominator() {
    let snapshot = fidelity_snapshot("Hello provenance");
    let mut output = generate_component(&snapshot, "1:1", &fidelity_options()).unwrap();
    let variable = output
        .source_map
        .entries
        .iter_mut()
        .find(|entry| entry.variable_id.as_deref() == Some("v"))
        .unwrap();
    variable.variable_id = Some("wrong-variable".to_owned());

    let report = validate_fidelity(&snapshot, "1:1", &output).unwrap();
    assert_eq!(report.variables.total, 1);
    assert_eq!(report.variables.covered, 0);
    assert!(!report.strict_compatible());
}

#[test]
fn strict_fidelity_rejects_corrupted_text_style_identity() {
    let snapshot = fidelity_snapshot("Hello provenance");
    let mut output = generate_component(&snapshot, "1:1", &fidelity_options()).unwrap();
    let mut corrupted = 0;
    for typography in output
        .source_map
        .entries
        .iter_mut()
        .filter(|entry| entry.style_id.as_deref() == Some("s"))
    {
        typography.style_id = Some("wrong-style".to_owned());
        corrupted += 1;
    }
    assert!(corrupted > 0);

    let report = validate_fidelity(&snapshot, "1:1", &output).unwrap();
    assert_eq!(report.typography.total, 1);
    assert_eq!(report.typography.covered, 0);
    assert!(!report.strict_compatible());
}

#[test]
fn strict_fidelity_uses_snapshot_semantics_even_if_trace_disposition_is_corrupted() {
    let snapshot = fidelity_snapshot("Hello provenance");
    let mut output = generate_component(&snapshot, "1:1", &fidelity_options()).unwrap();
    let text = output
        .projection_trace
        .entries
        .iter_mut()
        .find(|entry| entry.node_id == "1:2")
        .unwrap();
    text.disposition = ProjectionDisposition::Ignored;
    output
        .source_map
        .entries
        .retain(|entry| entry.node_id.as_deref() != Some("1:2"));

    let report = validate_fidelity(&snapshot, "1:1", &output).unwrap();
    assert_eq!(
        report.text,
        devup_mcp_devup_ui::provenance::FidelityCoverage {
            total: 1,
            covered: 0,
            basis_points: 0,
        }
    );
    assert_eq!(report.typography.total, 1);
    assert_eq!(report.typography.covered, 0);
    assert!(!report.strict_compatible());
}

#[test]
fn strict_fidelity_requires_every_multiline_text_segment_in_the_mapped_final_range() {
    let snapshot = fidelity_snapshot("First line\nSecond line");
    let mut output = generate_component(&snapshot, "1:1", &fidelity_options()).unwrap();
    let characters = output
        .source_map
        .entries
        .iter_mut()
        .find(|entry| {
            entry.node_id.as_deref() == Some("1:2")
                && entry.property.as_deref() == Some("characters")
        })
        .unwrap();
    let first_line = output.tsx.find("First line").unwrap();
    characters.generated_range = Some(devup_mcp_devup_ui::provenance::GeneratedRange {
        start: first_line,
        end: first_line + "First line".len(),
    });

    let report = validate_fidelity(&snapshot, "1:1", &output).unwrap();
    assert_eq!(report.text.total, 1);
    assert_eq!(report.text.covered, 0);
    assert!(!report.strict_compatible());
}

#[test]
fn text_fidelity_matches_the_final_jsx_encoding_of_multiline_special_characters() {
    let snapshot = fidelity_snapshot("A < B\nC & D");
    let output = generate_component(&snapshot, "1:1", &fidelity_options()).unwrap();

    assert!(output.tsx.contains("A {\"<\"} B<br />C {\"&\"} D"));
    assert!(output.fidelity_report.text.complete());
    assert!(output.fidelity_report.strict_compatible());
}

#[test]
fn strict_fidelity_consumes_repeated_text_segment_mappings_one_to_one() {
    let mut snapshot = fidelity_snapshot("AA");
    snapshot.nodes.get_mut("1:2").unwrap().fields.insert(
        "styledTextSegments".to_owned(),
        json!([
            {
                "characters": "A", "start": 0, "end": 1, "textStyleId": "s",
                "fontName": {"family": "Inter", "style": "Regular"},
                "fontSize": 16, "fontWeight": 400,
                "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0}}]
            },
            {
                "characters": "A", "start": 1, "end": 2, "textStyleId": "s",
                "fontName": {"family": "Inter", "style": "Regular"},
                "fontSize": 16, "fontWeight": 400,
                "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0}}]
            }
        ]),
    );
    let mut output = generate_component(&snapshot, "1:1", &fidelity_options()).unwrap();
    let mut seen = false;
    output.source_map.entries.retain(|entry| {
        let is_text = entry.node_id.as_deref() == Some("1:2")
            && entry.property.as_deref() == Some("characters");
        if is_text && seen {
            false
        } else {
            seen |= is_text;
            true
        }
    });

    let report = validate_fidelity(&snapshot, "1:1", &output).unwrap();
    assert_eq!(report.text.total, 2);
    assert_eq!(report.text.covered, 1);
    assert!(!report.strict_compatible());
}

#[test]
fn strict_fidelity_requires_layout_property_mappings_not_only_node_trace() {
    let snapshot = fidelity_snapshot("Hello provenance");
    let mut output = generate_component(&snapshot, "1:1", &fidelity_options()).unwrap();
    output.source_map.entries.retain(|entry| {
        entry.node_id.as_deref() != Some("1:1")
            || !matches!(
                entry.property.as_deref(),
                Some("layoutMode" | "width" | "height")
            )
    });

    let report = validate_fidelity(&snapshot, "1:1", &output).unwrap();
    assert!(report.layout.total >= 3);
    assert_eq!(report.layout.covered, 0);
    assert!(!report.strict_compatible());
}

#[test]
fn strict_fidelity_requires_source_asset_identity_mapping() {
    let snapshot = Snapshot {
        file_key: "FileKey123".to_owned(),
        version: Some("v1".to_owned()),
        roots: vec!["asset".to_owned()],
        nodes: [node(
            "asset",
            "VECTOR",
            json!({
                "name": "Check", "childrenIds": [],
                "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0}}]
            }),
        )]
        .into_iter()
        .map(|node| (node.id.clone(), node))
        .collect(),
        diagnostics: Vec::new(),
    };
    let mut output = generate_component(&snapshot, "asset", &CodegenOptions::default()).unwrap();
    let serialized = serde_json::to_value(&output.source_map).unwrap();
    assert!(
        serialized["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["assetId"] == "asset:node")
    );
    let asset = output
        .source_map
        .entries
        .iter_mut()
        .find(|entry| entry.resolution == "asset")
        .unwrap();
    asset.asset_id = Some("asset:wrong".to_owned());

    let report = validate_fidelity(&snapshot, "asset", &output).unwrap();
    assert_eq!(report.assets.total, 1);
    assert_eq!(report.assets.covered, 0);
    assert!(!report.strict_compatible());
}

#[test]
fn component_set_provenance_offsets_reference_the_wrapped_final_tsx() {
    let snapshot = component_set_snapshot(false);
    let output =
        generate_component_set_target(&snapshot, "set", "Card", &CodegenOptions::default())
            .unwrap();

    let component_type = output
        .source_map
        .entries
        .iter()
        .find(|entry| {
            entry.node_id.as_deref() == Some("default") && entry.property.as_deref() == Some("type")
        })
        .expect("final component type provenance");
    assert_eq!(slice(&output.tsx, component_type), "Box");
    assert!(!output.projection_trace.entries.is_empty());
    assert!(output.fidelity_report.nodes.total > 0);
}

#[test]
fn inline_instance_provenance_offsets_reference_sorted_and_prefixed_final_tsx() {
    let mut snapshot = component_set_snapshot(false);
    snapshot.roots = vec!["root".to_owned()];
    snapshot.nodes.insert(
        "root".to_owned(),
        node(
            "root",
            "FRAME",
            json!({"name": "Host", "childrenIds": ["instance"], "width": 200, "height": 80}),
        ),
    );
    snapshot.nodes.insert(
        "instance".to_owned(),
        node(
            "instance",
            "INSTANCE",
            json!({
                "name": "Card", "parentId": "root", "childrenIds": [],
                "componentProperties": {"size": {"type": "VARIANT", "value": "small"}}
            }),
        ),
    );
    let output = generate_inlined_component_instance(
        &snapshot,
        "root",
        "instance",
        &CodegenOptions::default(),
    )
    .unwrap();

    let component_type = output
        .source_map
        .entries
        .iter()
        .find(|entry| {
            entry.node_id.as_deref() == Some("default") && entry.property.as_deref() == Some("type")
        })
        .expect("final inline component type provenance");
    assert_eq!(slice(&output.tsx, component_type), "Box");
    assert!(output.tsx.starts_with("{/* <Card size=\"small\" /> */}"));
    assert!(!output.projection_trace.entries.is_empty());
    assert!(output.fidelity_report.nodes.total > 0);
}

#[test]
fn variant_component_set_rebuilds_nonempty_provenance_over_final_tsx() {
    let snapshot = component_set_snapshot(true);
    let output =
        generate_component_set_target(&snapshot, "set", "Card", &CodegenOptions::default())
            .unwrap();

    assert!(output.tsx.contains("_hover"));
    assert!(!output.source_map.entries.is_empty());
    assert!(!output.projection_trace.entries.is_empty());
    assert!(output.fidelity_report.nodes.total > 0);
    let hover_opacity = output
        .source_map
        .entries
        .iter()
        .find(|entry| {
            entry.node_id.as_deref() == Some("hover")
                && entry.property.as_deref() == Some("opacity")
        })
        .expect("non-default variant selector provenance");
    assert!(slice(&output.tsx, hover_opacity).contains("opacity"));
    assert!(
        output.fidelity_report.layout.complete(),
        "layout fidelity: {:?}; source map: {:#?}",
        output.fidelity_report.layout,
        output.source_map.entries
    );
    assert!(output.fidelity_report.strict_compatible());
    for entry in &output.source_map.entries {
        let range = entry.generated_range.as_ref().expect("final TSX range");
        assert!(range.start < range.end && range.end <= output.tsx.len());
        assert!(output.tsx.is_char_boundary(range.start));
        assert!(output.tsx.is_char_boundary(range.end));
    }
}

#[test]
fn non_default_nested_variant_difference_is_lossy_not_falsely_covered() {
    let mut snapshot = component_set_snapshot(true);
    snapshot
        .nodes
        .get_mut("default")
        .unwrap()
        .fields
        .insert("childrenIds".to_owned(), json!(["default-child"]));
    snapshot
        .nodes
        .get_mut("hover")
        .unwrap()
        .fields
        .insert("childrenIds".to_owned(), json!(["hover-child"]));
    for (id, parent_id, width) in [
        ("default-child", "default", 100),
        ("hover-child", "hover", 200),
    ] {
        snapshot.nodes.insert(
            id.to_owned(),
            node(
                id,
                "FRAME",
                json!({
                    "name": "Nested", "parentId": parent_id, "childrenIds": [],
                    "width": width, "height": 20,
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED"
                }),
            ),
        );
    }

    let output =
        generate_component_set_target(&snapshot, "set", "Card", &CodegenOptions::default())
            .unwrap();

    assert!(output.tsx.contains("w=\"100px\""));
    assert!(!output.tsx.contains("w=\"200px\""));
    assert!(!output.source_map.entries.iter().any(|entry| {
        entry.node_id.as_deref() == Some("hover-child") && entry.property.is_some()
    }));
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "DEVUP_CODEGEN_VARIANT_CHILD_FALLBACK"
            && diagnostic.node_id.as_deref() == Some("hover-child")
            && diagnostic.fidelity_impact() == devup_mcp_figma::FidelityImpact::Lossy
    }));
    assert_eq!(output.fidelity_report.impacts.lossy, 1);
    assert!(!output.fidelity_report.layout.complete());
    assert!(!output.fidelity_report.strict_compatible());
}

#[test]
fn non_default_variant_missing_child_is_lossy_not_falsely_covered() {
    let mut snapshot = component_set_snapshot(true);
    snapshot
        .nodes
        .get_mut("default")
        .unwrap()
        .fields
        .insert("childrenIds".to_owned(), json!(["default-child"]));
    snapshot.nodes.insert(
        "default-child".to_owned(),
        node(
            "default-child",
            "FRAME",
            json!({
                "name": "Default only", "parentId": "default", "childrenIds": [],
                "width": 100, "height": 20,
                "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED"
            }),
        ),
    );

    let output =
        generate_component_set_target(&snapshot, "set", "Card", &CodegenOptions::default())
            .unwrap();

    assert!(output.tsx.contains("Default only") || output.tsx.contains("w=\"100px\""));
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "DEVUP_CODEGEN_VARIANT_CHILD_FALLBACK"
            && diagnostic.node_id.as_deref() == Some("hover")
            && diagnostic.fidelity_impact() == devup_mcp_figma::FidelityImpact::Lossy
    }));
    assert!(!output.fidelity_report.strict_compatible());
}

fn component_set_snapshot(with_effect: bool) -> Snapshot {
    let definitions = if with_effect {
        json!({
            "effect": {
                "type": "VARIANT", "defaultValue": "default",
                "variantOptions": ["default", "hover"]
            }
        })
    } else {
        json!({
            "size": {
                "type": "VARIANT", "defaultValue": "small", "variantOptions": ["small"]
            }
        })
    };
    let default_variants = if with_effect {
        json!({"effect": "default"})
    } else {
        json!({"size": "small"})
    };
    let mut nodes = vec![
        node(
            "set",
            "COMPONENT_SET",
            json!({
                "name": "Card", "childrenIds": if with_effect { json!(["default", "hover"]) } else { json!(["default"]) },
                "defaultVariant": {"name": "Default"},
                "componentPropertyDefinitions": definitions
            }),
        ),
        node(
            "default",
            "COMPONENT",
            json!({
                "name": "Default", "parentId": "set", "childrenIds": [],
                "variantProperties": default_variants, "width": 100, "height": 40,
                "opacity": 1
            }),
        ),
    ];
    if with_effect {
        nodes.push(node(
            "hover",
            "COMPONENT",
            json!({
                "name": "Hover", "parentId": "set", "childrenIds": [],
                "variantProperties": {"effect": "hover"}, "width": 100, "height": 40,
                "opacity": 0.5
            }),
        ));
    }
    Snapshot {
        file_key: "FileKey123".to_owned(),
        version: Some("v1".to_owned()),
        roots: vec!["set".to_owned()],
        nodes: nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect(),
        diagnostics: Vec::new(),
    }
}

#[test]
fn devup_json_pointers_trace_variable_alias_and_style_sources() {
    let snapshot = VariableSnapshot {
        collections: vec![VariableCollection {
            id: "c".to_owned(),
            name: "Theme".to_owned(),
            default_mode_id: "m".to_owned(),
            modes: vec![VariableMode {
                mode_id: "m".to_owned(),
                name: "Default".to_owned(),
            }],
        }],
        variables: vec![
            VariableDefinition {
                id: "base".to_owned(),
                name: "base".to_owned(),
                resolved_type: "COLOR".to_owned(),
                variable_collection_id: "c".to_owned(),
                code_syntax: BTreeMap::new(),
                values_by_mode: BTreeMap::from([(
                    "m".to_owned(),
                    json!({"r": 1, "g": 0, "b": 0, "a": 1}),
                )]),
            },
            VariableDefinition {
                id: "primary".to_owned(),
                name: "primary".to_owned(),
                resolved_type: "COLOR".to_owned(),
                variable_collection_id: "c".to_owned(),
                code_syntax: BTreeMap::from([("WEB".to_owned(), "primary".to_owned())]),
                values_by_mode: BTreeMap::from([(
                    "m".to_owned(),
                    json!({"type": "VARIABLE_ALIAS", "id": "base"}),
                )]),
            },
        ],
        styles: vec![VariableStyle {
            id: "body-style".to_owned(),
            name: "body".to_owned(),
            style_type: "TEXT".to_owned(),
            value: json!({"fontSize": 16, "fontWeight": 400}),
        }],
        used_remote_variables: Vec::new(),
        used_variable_ids: Vec::new(),
        used_style_ids: Vec::new(),
        local_complete: true,
        used_remote_complete: true,
    };
    let output = generate_devup_json(&snapshot, ThemeScope::File).unwrap();
    let entries = &output.source_map.entries;
    let primary = entries
        .iter()
        .find(|entry| entry.json_pointer.as_deref() == Some("/theme/colors/default/primary"))
        .expect("primary pointer");
    assert_eq!(primary.variable_id.as_deref(), Some("primary"));
    assert_eq!(primary.resolution, "alias");
    let typography = entries
        .iter()
        .find(|entry| entry.json_pointer.as_deref() == Some("/theme/typography/body"))
        .expect("typography pointer");
    assert_eq!(typography.style_id.as_deref(), Some("body-style"));
    assert_eq!(typography.resolution, "style");
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.json_pointer.as_deref().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        entries.len()
    );
}

fn slice<'a>(tsx: &'a str, entry: &devup_mcp_devup_ui::provenance::ProvenanceEntry) -> &'a str {
    let range = entry.generated_range.as_ref().unwrap();
    &tsx[range.start..range.end]
}
