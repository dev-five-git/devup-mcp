//! Final JSX property contract. Enumerate syntax, never a whitelist of props.
use devup_mcp_figma::{Diagnostic, DiagnosticSeverity, FidelityImpact, Snapshot};
use oxc_allocator::Allocator;
use oxc_ast::ast::{JSXAttributeItem, JSXOpeningElement};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::{Value, json};

use crate::codegen::{CodegenOptions, CodegenOutput};

#[derive(Debug, Clone)]
pub struct GeneratedAttribute {
    pub start: usize,
    pub end: usize,
    pub element: usize,
    pub tag: String,
    pub name: String,
    pub source: String,
}

/// Includes boolean, expression, namespaced and spread attributes. Text and
/// comments that look like JSX are intentionally excluded by the parser.
pub fn generated_attributes(source: &str) -> Vec<GeneratedAttribute> {
    struct Attributes<'s> {
        source: &'s str,
        elements: usize,
        attributes: Vec<GeneratedAttribute>,
    }
    impl<'a> Visit<'a> for Attributes<'_> {
        fn visit_jsx_opening_element(&mut self, element: &JSXOpeningElement<'a>) {
            let ordinal = self.elements;
            self.elements += 1;
            let tag_span = element.name.span();
            let tag = &self.source[tag_span.start as usize..tag_span.end as usize];
            for attr in &element.attributes {
                let span = attr.span();
                let name = match attr {
                    JSXAttributeItem::Attribute(attr) => {
                        let span = attr.name.span();
                        self.source[span.start as usize..span.end as usize].to_owned()
                    }
                    JSXAttributeItem::SpreadAttribute(_) => "...".into(),
                };
                self.attributes.push(GeneratedAttribute {
                    start: span.start as usize,
                    end: span.end as usize,
                    element: ordinal,
                    tag: tag.to_owned(),
                    name,
                    source: self.source[span.start as usize..span.end as usize].into(),
                });
            }
            walk::walk_jsx_opening_element(self, element);
        }
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    let mut visitor = Attributes {
        source,
        elements: 0,
        attributes: Vec::new(),
    };
    visitor.visit_program(&parsed.program);
    visitor.attributes
}

fn owner<'a>(output: &'a CodegenOutput, attr: &GeneratedAttribute) -> Option<&'a str> {
    output
        .source_map
        .entries
        .iter()
        .filter(|e| e.property.is_none() && e.resolution == "node")
        .filter_map(|e| e.generated_range.as_ref().map(|r| (e, r)))
        .filter(|(_, r)| r.start <= attr.start && attr.end <= r.end)
        .min_by_key(|(_, r)| r.end - r.start)
        .and_then(|(e, _)| e.node_id.as_deref())
}

fn mapped(output: &CodegenOutput, attr: &GeneratedAttribute) -> bool {
    output.source_map.entries.iter().any(|e| {
        e.property.is_some()
            && e.node_id.as_deref() == owner(output, attr)
            && e.generated_property.as_deref() == Some(&attr.source)
            && e.generated_range
                .as_ref()
                .is_some_and(|r| r.start <= attr.start && attr.end <= r.end)
    })
}

pub(crate) fn audit_properties(
    snapshot: &Snapshot,
    output: &mut CodegenOutput,
    options: &CodegenOptions,
    root_id: &str,
) {
    // Replay each real stage once per emitted node. This is a derivation check,
    // not a second property-name table that can silently lag the renderer.
    let mut stages = std::collections::BTreeMap::new();
    for attr in generated_attributes(&output.tsx) {
        if mapped(output, &attr) {
            continue;
        }
        let id = owner(output, &attr).map(str::to_owned);
        let derivation = id.as_deref().and_then(|id| {
            let node = snapshot.nodes.get(id)?;
            let candidates = stages
                .entry((id.to_owned(), attr.tag.clone()))
                .or_insert_with(|| {
                    crate::codegen::property_derivations(
                        snapshot,
                        node,
                        &attr.tag,
                        options,
                        id == root_id,
                    )
                });
            candidates
                .iter()
                .find(|d| d["generatedProperty"] == attr.source)
                .cloned()
        });
        let verified = derivation.is_some();
        let mut details = derivation.unwrap_or_else(|| json!({
            "classification":"mapping-incomplete", "stage":"final-jsx-audit",
            "sourceFields":[], "calculation":null,
            "nextAction":"Inspect this emitted attribute and add its source derivation to the generating stage. The value is preserved; this is a mapping gap, not measured value loss."
        }));
        details["generatedProperty"] = json!(attr.source);
        details["elementIndex"] = json!(attr.element);
        details["propertyMappingVerified"] = json!(verified);
        if let Some(asset_id) = output
            .source_map
            .entries
            .iter()
            .find(|e| e.node_id == id && e.asset_id.is_some())
            .and_then(|e| e.asset_id.as_deref())
        {
            details["assetId"] = json!(asset_id);
        }
        details["evidenceLimit"] = json!("No browser or SVG/CSS composition measurement.");
        output.diagnostics.push(Diagnostic {
            code: if verified {
                "DEVUP_CODEGEN_PROPERTY_EVIDENCE"
            } else {
                "DEVUP_CODEGEN_PROPERTY_UNMAPPED"
            }
            .into(),
            message: if verified {
                "Source derivation verified."
            } else {
                "A generated attribute has no verified source mapping or derivation."
            }
            .into(),
            node_id: id,
            property: Some(attr.name),
            severity: Some(if verified {
                DiagnosticSeverity::Info
            } else {
                DiagnosticSeverity::Warning
            }),
            fidelity_impact: Some(FidelityImpact::None),
            details: Some(details),
            ..Default::default()
        });
    }
}

/// Private transport ledger, produced before server postprocessing and checked
/// after it. Element identity and complete expressions prevent another node or
/// a shared attribute-name prefix from satisfying a missing mapping.
pub fn attribute_contract(output: &CodegenOutput) -> Vec<Value> {
    generated_attributes(&output.tsx)
        .into_iter()
        .map(|attr| {
            let node_id = owner(output, &attr);
            let verified = mapped(output, &attr)
                || output.diagnostics.iter().any(|d| {
                    d.code == "DEVUP_CODEGEN_PROPERTY_EVIDENCE"
                        && d.node_id.as_deref() == node_id
                        && d.details.as_ref().is_some_and(|v| {
                            v["elementIndex"] == attr.element
                                && v["generatedProperty"] == attr.source
                                && v["propertyMappingVerified"] == true
                        })
                });
            json!({"elementIndex":attr.element,"tag":attr.tag,"nodeId":node_id,
            "generatedProperty":attr.source,"verified":verified})
        })
        .collect()
}

pub fn uncovered_attributes(source: &str, contract: &[Value]) -> Vec<Value> {
    if crate::validation::validate_tsx(source).is_err() {
        return vec![
            json!({"reason":"invalid-final-tsx","propertyMappingVerified":false,
            "message":"The final TSX cannot be parsed; attribute provenance cannot be verified."}),
        ];
    }
    let attributes = generated_attributes(source);
    let mut missing: Vec<Value> = attributes
        .iter()
        .filter(|attr| {
            !contract.iter().any(|e| {
                e["verified"] == true
                    && e["elementIndex"] == attr.element
                    && e["tag"] == attr.tag
                    && e["generatedProperty"] == attr.source
            })
        })
        .map(|attr| {
            json!({"elementIndex":attr.element,"tag":attr.tag,"property":attr.name,
        "generatedProperty":attr.source})
        })
        .collect();
    // The inverse transport check catches deletion: enumerating only surviving
    // attributes would make an omitted FILL property invisible again.
    for entry in contract {
        let property = entry["generatedProperty"].as_str().unwrap_or("");
        let name = if property.starts_with("{...") {
            "..."
        } else {
            property.split('=').next().unwrap_or("").trim()
        };
        if !attributes.iter().any(|attr| {
            entry["elementIndex"] == attr.element && entry["tag"] == attr.tag && attr.name == name
        }) {
            missing.push(
                json!({"elementIndex":entry["elementIndex"],"tag":entry["tag"],
                "nodeId":entry["nodeId"],"property":name,"generatedProperty":property,
                "reason":"expected-attribute-absent","propertyMappingVerified":false}),
            );
        }
    }
    missing
}
