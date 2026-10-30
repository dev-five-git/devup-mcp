use super::{Severity, Violation, site};
use crate::effective_attributes::AttributeContext;
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;
use std::collections::BTreeSet;

pub const MARQUEE_RECIPE: &str = "Review this heuristic marquee candidate; Figma does not prove animation intent. Duplicate the content in one nowrap track; use keyframes from translateX(0) to translateX(-50%) with infinite linear animation, _motionReduce={{ animation: 'none' }}, and aria-hidden on the duplicate.";

pub fn repeated_separator(text: &str) -> bool {
    if text.contains(['\n', '\r']) || text.chars().count() < 24 {
        return false;
    }
    ['✦', '★', '•', '·', '|', '/', '✧', '●', '◆']
        .into_iter()
        .any(|separator| {
            (!matches!(separator, '/' | '|') || text.contains(&format!(" {separator} ")))
                && text.chars().filter(|character| *character == separator).count() >= 3
                && text.split(separator).filter(|part| {
                    let part = part.trim();
                    !part.is_empty()
                        && part.chars().any(char::is_alphanumeric)
                }).count() >= 3
        })
}

#[derive(Default)]
struct Globals {
    names: BTreeSet<String>,
    namespaces: BTreeSet<String>,
    decoration: bool,
    motion: bool,
}

impl<'a> Visit<'a> for Globals {
    fn visit_import_declaration(&mut self, node: &ImportDeclaration<'a>) {
        if node.source.value == "@devup-ui/react" {
            for specifier in node.specifiers.iter().flatten() {
                match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(specifier)
                        if specifier.imported.to_string() == "globalCss" => {
                            self.names.insert(specifier.local.name.to_string());
                        }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                        self.namespaces.insert(specifier.local.name.to_string());
                    }
                    _ => {}
                }
            }
        }
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        let global = match call.callee.get_inner_expression() {
            Expression::Identifier(id) => self.names.contains(id.name.as_str()),
            Expression::StaticMemberExpression(member) => {
                member.property.name == "globalCss"
                    && matches!(&member.object, Expression::Identifier(id) if self.namespaces.contains(id.name.as_str()))
            }
            _ => false,
        };
        if global {
            for argument in &call.arguments {
                if let Some(Expression::ObjectExpression(object)) = argument.as_expression() {
                    self.decoration |= object.properties.iter().any(|property| {
                        let ObjectPropertyKind::ObjectProperty(property) = property else { return false };
                        property.key.static_name().is_some_and(|selector| {
                            selector.split(',').any(|selector| selector.trim() == "a")
                        }) && matches!(&property.value, Expression::ObjectExpression(styles) if styles.properties.iter().any(|style| {
                            matches!(style, ObjectPropertyKind::ObjectProperty(style) if style.key.static_name().is_some_and(|key| matches!(key.as_ref(), "textDecoration" | "text-decoration")))
                        }))
                    });
                }
            }
        }
        self.motion |= matches!(&call.callee, Expression::Identifier(id) if id.name == "keyframes");
        walk::walk_call_expression(self, call);
    }

    fn visit_jsx_attribute(&mut self, attribute: &JSXAttribute<'a>) {
        self.motion |= matches!(&attribute.name, JSXAttributeName::Identifier(id) if id.name.starts_with("animation"));
        walk::walk_jsx_attribute(self, attribute);
    }

    fn visit_object_property(&mut self, property: &ObjectProperty<'a>) {
        self.motion |= property.key.static_name().is_some_and(|name| name.starts_with("animation"));
        walk::walk_object_property(self, property);
    }
}

pub fn has_global_link_decoration(source: &str) -> bool {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    let mut globals = Globals::default();
    globals.names.insert("globalCss".into());
    globals.visit_program(&parsed.program);
    !parsed.panicked && parsed.diagnostics.is_empty() && globals.decoration
}

struct Collector<'s> {
    source: &'s str,
    attributes: AttributeContext,
    globals: Globals,
    findings: Vec<Violation>,
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_jsx_opening_element(&mut self, opening: &JSXOpeningElement<'a>) {
        let name = opening.name.get_identifier_name().map(|name| name.as_str()).unwrap_or("");
        let attributes = self.attributes.read(opening);
        let effective = attributes.rendered_as().unwrap_or_else(|| name.to_owned());
        if (effective == "a" || self.attributes.link_names.contains(&effective))
            && !attributes.contains("textDecoration")
            && !attributes.unresolved_spread
            && !self.globals.decoration
        {
            self.findings.push(site::violation(self.source, opening.span, "link-underline", Severity::Warning,
                format!("<{name}> renders a link without an explicit textDecoration; the browser can underline its button label. @devup-ui/reset-css does not reset anchor decoration."),
                "Set textDecoration=\"none\" on the link, or use globalCss({ a: { textDecoration: 'none', color: 'inherit' } })."));
        }
        walk::walk_jsx_opening_element(self, opening);
    }

    fn visit_jsx_text(&mut self, text: &JSXText<'a>) {
        if !self.globals.motion && repeated_separator(text.value.trim()) {
            let mut finding = site::violation(self.source, text.span, "marquee-static", Severity::Info,
                "Long single-line text repeats separator glyphs but this file declares no keyframes or animation; review whether a scrolling strip was shipped as static text.".into(), MARQUEE_RECIPE);
            finding.context.insert("confidence".into(), serde_json::json!("heuristic"));
            self.findings.push(finding);
        }
    }
}

pub(super) fn findings(source: &str) -> Vec<Violation> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() { return Vec::new() }
    let mut globals = Globals::default();
    globals.names.insert("globalCss".into());
    globals.visit_program(&parsed.program);
    let mut collector = Collector {
        source,
        attributes: AttributeContext::from_program(&parsed.program),
        globals,
        findings: Vec::new(),
    };
    collector.visit_program(&parsed.program);
    collector.findings
}
