use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    provenance::{ProjectionDisposition, validate_fidelity},
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

#[test]
fn tsx_byte_ranges_trace_components_text_props_and_resources() {
    let snapshot = Snapshot {
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
                    "characters": "Hello provenance", "textStyleId": "s",
                    "styledTextSegments": [{
                        "characters": "Hello provenance", "start": 0, "end": 16,
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
    };
    let output = generate_component(
        &snapshot,
        "1:1",
        &CodegenOptions {
            variable_tokens: BTreeMap::from([
                ("short".to_owned(), "pri".to_owned()),
                ("v".to_owned(), "primary".to_owned()),
            ]),
            text_style_tokens: BTreeMap::from([("s".to_owned(), "body".to_owned())]),
            ..CodegenOptions::default()
        },
    )
    .unwrap();

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
