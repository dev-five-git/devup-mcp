use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};

#[derive(Default, PartialEq, Eq)]
pub struct AttributeValue {
    pub literal: Option<String>,
    pub boolean: Option<bool>,
    pub identifier: Option<String>,
}

#[derive(Default)]
pub struct EffectiveAttributes {
    pub values: BTreeMap<String, AttributeValue>,
    pub transition: bool,
    pub unresolved_spread: bool,
}

impl EffectiveAttributes {
    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    pub fn literal(&self, name: &str) -> Option<String> {
        self.values.get(name)?.literal.clone()
    }

    pub fn boolean(&self, name: &str) -> Option<bool> {
        self.values.get(name)?.boolean
    }

    pub fn rendered_as(&self) -> Option<String> {
        let value = self.values.get("as")?;
        value.literal.clone().or_else(|| value.identifier.clone())
    }

    fn expression(&mut self, expression: &Expression<'_>) {
        match expression.get_inner_expression() {
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            if let Some(name) = property.key.static_name() {
                                let name = name.into_owned();
                                if name == "props" {
                                    self.expression(&property.value);
                                } else {
                                    self.values.insert(name, expression_value(&property.value));
                                }
                            }
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.expression(&spread.argument)
                        }
                    }
                }
            }
            Expression::ConditionalExpression(conditional) => {
                let mut consequent = Self::default();
                consequent.expression(&conditional.consequent);
                let mut alternate = Self::default();
                alternate.expression(&conditional.alternate);
                self.unresolved_spread |=
                    consequent.unresolved_spread || alternate.unresolved_spread;
                for (name, value) in consequent.values.iter_mut() {
                    if alternate.values.get(name) != Some(value) {
                        *value = AttributeValue::default();
                    }
                }
                for name in alternate.values.into_keys() {
                    consequent.values.entry(name).or_default();
                }
                self.values.extend(consequent.values);
            }
            Expression::LogicalExpression(logical) => {
                self.expression(&logical.left);
                self.expression(&logical.right);
            }
            _ => self.unresolved_spread = true,
        }
    }
}

fn expression_value(expression: &Expression<'_>) -> AttributeValue {
    match expression.get_inner_expression() {
        Expression::StringLiteral(value) => AttributeValue {
            literal: Some(value.value.to_string()),
            boolean: match value.value.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            ..Default::default()
        },
        Expression::BooleanLiteral(value) => AttributeValue {
            boolean: Some(value.value),
            ..Default::default()
        },
        Expression::Identifier(value) => AttributeValue {
            identifier: Some(value.name.to_string()),
            ..Default::default()
        },
        _ => AttributeValue::default(),
    }
}

#[derive(Default)]
struct TransitionReader {
    found: bool,
}

impl<'a> Visit<'a> for TransitionReader {
    fn visit_object_property(&mut self, property: &ObjectProperty<'a>) {
        self.found |= property
            .key
            .static_name()
            .is_some_and(|name| name.starts_with("transition"));
        walk::walk_object_property(self, property);
    }
}

#[derive(Default)]
pub struct AttributeContext {
    pub link_names: BTreeSet<String>,
    transition_classes: BTreeSet<String>,
}

impl AttributeContext {
    pub fn from_program(program: &Program<'_>) -> Self {
        let mut context = Self::default();
        for statement in &program.body {
            if let Statement::ImportDeclaration(import) = statement
                && import.source.value == "next/link"
            {
                for specifier in import.specifiers.iter().flatten() {
                    if let ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) = specifier
                    {
                        context.link_names.insert(specifier.local.name.to_string());
                    }
                }
            }
            if let Statement::VariableDeclaration(declaration) = statement {
                for variable in &declaration.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &variable.id
                        && let Some(expression) = &variable.init
                        && css_transition(expression)
                    {
                        context.transition_classes.insert(id.name.to_string());
                    }
                }
            }
        }
        context
    }

    pub fn read(&self, opening: &JSXOpeningElement<'_>) -> EffectiveAttributes {
        let mut result = EffectiveAttributes::default();
        for item in &opening.attributes {
            match item {
                JSXAttributeItem::SpreadAttribute(spread) => result.expression(&spread.argument),
                JSXAttributeItem::Attribute(attribute) => {
                    let JSXAttributeName::Identifier(name) = &attribute.name else {
                        continue;
                    };
                    let name = name.name.as_str();
                    result.transition |= name.starts_with("transition");
                    let value = match &attribute.value {
                        None => AttributeValue {
                            boolean: Some(true),
                            ..Default::default()
                        },
                        Some(JSXAttributeValue::StringLiteral(value)) => AttributeValue {
                            literal: Some(value.value.to_string()),
                            boolean: match value.value.as_str() {
                                "true" => Some(true),
                                "false" => Some(false),
                                _ => None,
                            },
                            ..Default::default()
                        },
                        Some(JSXAttributeValue::ExpressionContainer(container)) => {
                            let Some(expression) = container.expression.as_expression() else {
                                continue;
                            };
                            if name == "props" {
                                result.expression(expression);
                            }
                            if name == "className" {
                                result.transition |= css_transition(expression)
                                    || matches!(expression.get_inner_expression(), Expression::Identifier(id) if self.transition_classes.contains(id.name.as_str()));
                            } else {
                                let mut reader = TransitionReader::default();
                                reader.visit_expression(expression);
                                result.transition |= reader.found;
                            }
                            expression_value(expression)
                        }
                        _ => AttributeValue::default(),
                    };
                    result.values.insert(name.to_string(), value);
                }
            }
        }
        result.transition |= result
            .values
            .keys()
            .any(|name| name.starts_with("transition"));
        for item in &opening.attributes {
            if let JSXAttributeItem::SpreadAttribute(spread) = item {
                let mut reader = TransitionReader::default();
                reader.visit_expression(&spread.argument);
                result.transition |= reader.found;
            }
        }
        result
    }
}

fn css_transition(expression: &Expression<'_>) -> bool {
    match expression.get_inner_expression() {
        Expression::CallExpression(call) if matches!(&call.callee, Expression::Identifier(id) if id.name == "css") =>
        {
            let mut reader = TransitionReader::default();
            reader.visit_call_expression(call);
            reader.found
        }
        Expression::ConditionalExpression(value) => {
            css_transition(&value.consequent) || css_transition(&value.alternate)
        }
        Expression::LogicalExpression(value) => {
            css_transition(&value.left) || css_transition(&value.right)
        }
        _ => false,
    }
}
