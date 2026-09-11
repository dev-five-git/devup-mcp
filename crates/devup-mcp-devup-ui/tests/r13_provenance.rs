use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_devup_ui::provenance::attributes::{
    attribute_contract, generated_attributes, uncovered_attributes,
};
use devup_mcp_figma::Snapshot;
use oxc_allocator::Allocator;
use oxc_ast::ast::JSXAttribute;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde_json::json;

#[test]
fn r13_contract_rejects_new_attributes_changed_values_and_wrong_element() {
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("../../../fixtures/r13/f13-1-snapshot.json")).unwrap();
    let output = generate_component(
        &snapshot,
        "3997:46715",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    let contract = attribute_contract(&output);
    assert!(uncovered_attributes(&output.tsx, &contract).is_empty());
    for changed in [
        output
            .tsx
            .replacen("<VStack", "<VStack data-new-postprocess=\"preserved\"", 1),
        output
            .tsx
            .replacen("objectFit=\"none\"", "objectFit=\"cover\"", 1),
        output
            .tsx
            .replacen("<VStack", "<VStack {...futureProps}", 1),
    ] {
        assert!(!uncovered_attributes(&changed, &contract).is_empty());
    }
    let mut wrong_owner = contract.clone();
    for entry in &mut wrong_owner {
        entry["elementIndex"] = json!(9999);
    }
    assert!(!uncovered_attributes(&output.tsx, &wrong_owner).is_empty());
}

#[test]
fn r13_parser_covers_expressions_spreads_and_boolean_props_without_reading_comments() {
    let code = r#"export const Example = () => <Box title="a > b" hidden data-x={value > 1 ? "x" : "y"} {...props}>{/* <Box fake="x" /> */}{"<Box fake='x' />"}</Box>;"#;
    let attrs = generated_attributes(code);
    assert_eq!(
        attrs.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
        ["title", "hidden", "data-x", "..."]
    );
    let contract: Vec<_> = attrs.iter().map(|a| json!({"elementIndex":a.element,"tag":a.tag,"generatedProperty":a.source,"verified":true})).collect();
    assert!(uncovered_attributes(code, &contract).is_empty());
    assert_eq!(
        uncovered_attributes(&code.replace("value > 1", "value > 2"), &contract).len(),
        1
    );
}

#[test]
fn r13_every_final_attribute_has_property_mapping_or_projection_evidence() {
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("../../../fixtures/r13/f13-1-snapshot.json")).unwrap();
    let output = generate_component(
        &snapshot,
        "3997:46715",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    struct Attributes<'s> {
        source: &'s str,
        values: Vec<String>,
    }
    impl<'a> Visit<'a> for Attributes<'_> {
        fn visit_jsx_attribute(&mut self, attr: &JSXAttribute<'a>) {
            self.values
                .push(self.source[attr.span.start as usize..attr.span.end as usize].into());
            walk::walk_jsx_attribute(self, attr);
        }
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &output.tsx, SourceType::tsx()).parse();
    let mut attrs = Attributes {
        source: &output.tsx,
        values: vec![],
    };
    attrs.visit_program(&parsed.program);
    assert!(attrs.values.len() > 100);
    let missing: Vec<_> = attrs
        .values
        .iter()
        .filter(|attr| {
            !output
                .source_map
                .property_entries()
                .iter()
                .any(|e| e.generated_property.as_ref() == Some(attr))
                && !output.diagnostics.iter().any(|d| {
                    d.code == "DEVUP_CODEGEN_PROPERTY_EVIDENCE"
                        && d.details
                            .as_ref()
                            .is_some_and(|v| v["generatedProperty"] == attr.as_str())
                })
        })
        .collect();
    assert!(
        missing.is_empty(),
        "Unmapped generated attributes: {missing:?}"
    );
    for value in [
        "boxShadow=\"0 4px 32px 0 #46608740\"",
        "objectFit=\"none\"",
        "objectPos=\"-32px -28px\"",
    ] {
        assert!(
            output.tsx.contains(value),
            "Original generated value changed: {value}"
        );
    }
}
