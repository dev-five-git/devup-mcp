//! Positive fixture provenance for the runtime binding actually imported.
//! Unknown calls are not proof of fixture data. In particular a schema passed
//! to an opaque decoder is not the decoder's returned application data.
use super::error;
use crate::ui_validate::{SourceFile, Violation};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
enum Flow {
    #[default]
    Unknown,
    Records,
    Reference(String),
    Import(String, String),
    Namespace(String),
    Object(BTreeMap<String, Self>),
    Union(Vec<Self>),
    Member(Box<Self>, String),
    Call(Box<Self>, Vec<Self>),
    Function(Vec<String>, BTreeMap<String, Self>, Vec<Self>),
    Scoped(BTreeMap<String, Self>, Box<Self>),
}

#[derive(Default)]
struct Module {
    locals: BTreeMap<String, Flow>,
    exports: BTreeMap<String, Flow>,
    stars: Vec<String>,
}

fn expression(node: &Expression<'_>) -> Flow {
    match node.get_inner_expression() {
        Expression::Identifier(id) => Flow::Reference(id.name.to_string()),
        Expression::ArrayExpression(array) => {
            if !array.elements.is_empty()
                && array.elements.iter().all(|item| {
                    matches!(item,
                ArrayExpressionElement::ObjectExpression(object) if object.properties.len() >= 2
                && object.properties.iter().all(|property| matches!(property,
                    ObjectPropertyKind::ObjectProperty(property) if matches!(property.value,
                        Expression::StringLiteral(_) | Expression::NumericLiteral(_)
                        | Expression::BooleanLiteral(_) | Expression::NullLiteral(_)))))
                })
            {
                Flow::Records
            } else {
                Flow::Union(
                    array
                        .elements
                        .iter()
                        .filter_map(|item| match item {
                            ArrayExpressionElement::SpreadElement(spread) => {
                                Some(expression(&spread.argument))
                            }
                            other => other.as_expression().map(expression),
                        })
                        .collect(),
                )
            }
        }
        Expression::ObjectExpression(object) => Flow::Object(
            object
                .properties
                .iter()
                .enumerate()
                .filter_map(|(index, property)| match property {
                    ObjectPropertyKind::ObjectProperty(property) => Some((
                        property.key.static_name()?.to_string(),
                        expression(&property.value),
                    )),
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        Some((format!("...{index}"), expression(&spread.argument)))
                    }
                })
                .collect(),
        ),
        Expression::StaticMemberExpression(member) => Flow::Member(
            Box::new(expression(&member.object)),
            member.property.name.to_string(),
        ),
        Expression::ComputedMemberExpression(member) => Flow::Member(
            Box::new(expression(&member.object)),
            if let Expression::StringLiteral(key) = member.expression.get_inner_expression() {
                key.value.to_string()
            } else {
                "*".into()
            },
        ),
        Expression::AwaitExpression(awaited) => expression(&awaited.argument),
        Expression::ConditionalExpression(value) => Flow::Union(vec![
            expression(&value.consequent),
            expression(&value.alternate),
        ]),
        Expression::LogicalExpression(value) => {
            Flow::Union(vec![expression(&value.left), expression(&value.right)])
        }
        Expression::SequenceExpression(value) => {
            value.expressions.last().map(expression).unwrap_or_default()
        }
        Expression::CallExpression(call) => Flow::Call(
            Box::new(expression(&call.callee)),
            call.arguments
                .iter()
                .map(|arg| arg.as_expression().map(expression).unwrap_or_default())
                .collect(),
        ),
        Expression::FunctionExpression(function) => function_flow(function),
        Expression::ArrowFunctionExpression(function) => {
            let mut body = Body::default();
            if let ArrowFunctionBody::FunctionBody(block) = &function.body {
                body.visit_function_body(block);
            } else if let Some(value) = function.body.as_expression() {
                body.returns.push(expression(value));
            }
            Flow::Function(parameters(&function.params), body.locals, body.returns)
        }
        _ => Flow::Unknown,
    }
}

fn parameters(params: &FormalParameters<'_>) -> Vec<String> {
    params
        .items
        .iter()
        .map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(id) => id.name.to_string(),
            _ => String::new(),
        })
        .collect()
}

fn function_flow(function: &Function<'_>) -> Flow {
    let mut body = Body::default();
    if let Some(block) = &function.body {
        body.visit_function_body(block);
    }
    Flow::Function(parameters(&function.params), body.locals, body.returns)
}

#[derive(Default)]
struct Body {
    locals: BTreeMap<String, Flow>,
    returns: Vec<Flow>,
}

impl<'a> Visit<'a> for Body {
    fn visit_variable_declarator(&mut self, node: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &node.id {
            self.locals.insert(
                id.name.to_string(),
                node.init.as_ref().map(expression).unwrap_or_default(),
            );
        }
    }
    fn visit_return_statement(&mut self, node: &ReturnStatement<'a>) {
        if let Some(value) = &node.argument {
            self.returns.push(Flow::Scoped(
                self.locals.clone(),
                Box::new(expression(value)),
            ));
        }
    }
    fn visit_block_statement(&mut self, node: &BlockStatement<'a>) {
        let outer = self.locals.clone();
        walk::walk_block_statement(self, node);
        self.locals = outer;
    }
    fn visit_statement(&mut self, node: &Statement<'a>) {
        if let Statement::FunctionDeclaration(function) = node {
            if let Some(id) = &function.id {
                self.locals
                    .insert(id.name.to_string(), function_flow(function));
            }
        } else {
            walk::walk_statement(self, node);
        }
    }
    fn visit_expression(&mut self, _node: &Expression<'a>) {}
    fn visit_arrow_function_expression(&mut self, _node: &ArrowFunctionExpression<'a>) {}
    fn visit_ts_type(&mut self, _node: &TSType<'a>) {}
}

fn imports(node: &ImportDeclaration<'_>) -> Vec<(String, Flow)> {
    if node.import_kind == ImportOrExportKind::Type {
        return Vec::new();
    }
    node.specifiers
        .iter()
        .flatten()
        .filter_map(|specifier| match specifier {
            ImportDeclarationSpecifier::ImportSpecifier(value)
                if value.import_kind == ImportOrExportKind::Value =>
            {
                Some((
                    value.local.name.to_string(),
                    Flow::Import(node.source.value.to_string(), value.imported.to_string()),
                ))
            }
            ImportDeclarationSpecifier::ImportDefaultSpecifier(value) => Some((
                value.local.name.to_string(),
                Flow::Import(node.source.value.to_string(), "default".into()),
            )),
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(value) => Some((
                value.local.name.to_string(),
                Flow::Namespace(node.source.value.to_string()),
            )),
            _ => None,
        })
        .collect()
}

fn declaration(node: &Declaration<'_>, module: &mut Module, exported: bool) {
    match node {
        Declaration::VariableDeclaration(variables) => {
            for var in &variables.declarations {
                if let BindingPattern::BindingIdentifier(id) = &var.id {
                    let name = id.name.to_string();
                    module.locals.insert(
                        name.clone(),
                        var.init.as_ref().map(expression).unwrap_or_default(),
                    );
                    if exported {
                        module.exports.insert(name.clone(), Flow::Reference(name));
                    }
                }
            }
        }
        Declaration::FunctionDeclaration(function) => {
            if let Some(id) = &function.id {
                let name = id.name.to_string();
                module.locals.insert(name.clone(), function_flow(function));
                if exported {
                    module.exports.insert(name.clone(), Flow::Reference(name));
                }
            }
        }
        _ => {}
    }
}

fn module(source: &str) -> Module {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    let mut module = Module::default();
    if !parsed.diagnostics.is_empty() {
        return module;
    }
    for statement in &parsed.program.body {
        match statement {
            Statement::ImportDeclaration(node) => module.locals.extend(imports(node)),
            Statement::ExportDeclaration(node) => declaration(&node.declaration, &mut module, true),
            Statement::ExportNamedDeclaration(node)
                if node.export_kind == ImportOrExportKind::Value =>
            {
                for specifier in &node.specifiers {
                    if specifier.export_kind == ImportOrExportKind::Value {
                        module.exports.insert(
                            specifier.exported.to_string(),
                            Flow::Reference(specifier.local.to_string()),
                        );
                    }
                }
            }
            Statement::ExportFromDeclaration(node)
                if node.export_kind == ImportOrExportKind::Value =>
            {
                for specifier in &node.specifiers {
                    if specifier.export_kind == ImportOrExportKind::Value {
                        module.exports.insert(
                            specifier.exported.to_string(),
                            Flow::Import(
                                node.source.value.to_string(),
                                specifier.local.to_string(),
                            ),
                        );
                    }
                }
            }
            Statement::ExportAllDeclaration(node)
                if node.export_kind == ImportOrExportKind::Value =>
            {
                module.stars.push(node.source.value.to_string())
            }
            Statement::ExportDefaultDeclaration(node) => {
                let value = if let ExportDefaultDeclarationKind::FunctionDeclaration(function) =
                    &node.declaration
                {
                    function_flow(function)
                } else {
                    node.declaration
                        .as_expression()
                        .map(expression)
                        .unwrap_or_default()
                };
                module.exports.insert("default".into(), value);
            }
            other => {
                if let Some(node) = other.as_declaration() {
                    declaration(node, &mut module, false);
                }
            }
        }
    }
    module
}

fn normalized(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

fn resolve_file<'a>(files: &'a [SourceFile], from: &str, import: &str) -> Option<&'a SourceFile> {
    let from = normalized(from);
    let base = if let Some(tail) = import.strip_prefix("@/") {
        format!(
            "{}/{}",
            from.split("/src/")
                .next()
                .filter(|_| from.contains("/src/"))
                .map(|root| format!("{root}/src"))
                .unwrap_or_else(|| "src".into()),
            tail
        )
    } else if import.starts_with('.') {
        format!(
            "{}/{}",
            from.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(""),
            import
        )
    } else {
        return None;
    };
    let base = normalized(&base);
    files.iter().find(|file| {
        let target = normalized(&file.path);
        target == base
            || [".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.tsx"]
                .iter()
                .any(|extension| target == format!("{base}{extension}"))
    })
}

struct Evaluator<'a> {
    files: &'a [SourceFile],
    modules: BTreeMap<String, Module>,
}
impl Evaluator<'_> {
    fn exported(&mut self, from: &str, import: &str, name: &str, depth: usize) -> bool {
        if depth > 48 {
            return false;
        }
        let Some(file) = resolve_file(self.files, from, import) else {
            return false;
        };
        let path = file.path.clone();
        if !self.modules.contains_key(&path) {
            self.modules.insert(path.clone(), module(&file.content));
        }
        let facts = &self.modules[&path];
        let flow = facts.exports.get(name).cloned();
        let stars = facts.stars.clone();
        if let Some(flow) = flow {
            self.value(&path, &flow, &BTreeMap::new(), depth + 1)
        } else {
            stars
                .iter()
                .any(|star| self.exported(&path, star, name, depth + 1))
        }
    }
    fn lookup(&self, path: &str, name: &str, locals: &BTreeMap<String, Flow>) -> Flow {
        locals
            .get(name)
            .or_else(|| self.modules.get(path)?.locals.get(name))
            .cloned()
            .unwrap_or_default()
    }
    fn dereference(
        &self,
        path: &str,
        flow: &Flow,
        locals: &BTreeMap<String, Flow>,
        depth: usize,
    ) -> Flow {
        if depth > 48 {
            return Flow::Unknown;
        }
        if let Flow::Reference(name) = flow {
            self.dereference(path, &self.lookup(path, name, locals), locals, depth + 1)
        } else {
            flow.clone()
        }
    }
    fn function(
        &mut self,
        path: &str,
        flow: &Flow,
        args: &[Flow],
        locals: &BTreeMap<String, Flow>,
        depth: usize,
    ) -> bool {
        if depth > 48 {
            return false;
        }
        let flow = self.dereference(path, flow, locals, depth + 1);
        if let Flow::Function(params, own, returns) = flow {
            let mut scope = locals.clone();
            scope.extend(own);
            for (index, name) in params.iter().enumerate() {
                let value = args
                    .get(index)
                    .is_some_and(|arg| self.value(path, arg, locals, depth + 1));
                scope.insert(
                    name.clone(),
                    if value { Flow::Records } else { Flow::Unknown },
                );
            }
            returns
                .iter()
                .any(|value| self.value(path, value, &scope, depth + 1))
        } else if let Flow::Import(import, name) = flow {
            self.exported(path, &import, &name, depth + 1)
        } else {
            false
        }
    }
    fn value(
        &mut self,
        path: &str,
        flow: &Flow,
        locals: &BTreeMap<String, Flow>,
        depth: usize,
    ) -> bool {
        if depth > 48 {
            return false;
        }
        match flow {
            Flow::Unknown | Flow::Namespace(_) => false,
            Flow::Records => true,
            Flow::Reference(name) => {
                self.value(path, &self.lookup(path, name, locals), locals, depth + 1)
            }
            Flow::Import(import, name) => self.exported(path, import, name, depth + 1),
            Flow::Object(values) => values
                .values()
                .any(|value| self.value(path, value, locals, depth + 1)),
            Flow::Union(values) => values
                .iter()
                .any(|value| self.value(path, value, locals, depth + 1)),
            Flow::Function(..) => self.function(path, flow, &[], locals, depth + 1),
            Flow::Scoped(own, value) => {
                let mut scope = locals.clone();
                scope.extend(own.clone());
                self.value(path, value, &scope, depth + 1)
            }
            Flow::Member(object, key) => match self.dereference(path, object, locals, depth + 1) {
                Flow::Namespace(import) => self.exported(path, &import, key, depth + 1),
                Flow::Object(values) if key != "*" => {
                    values
                        .get(key)
                        .is_some_and(|value| self.value(path, value, locals, depth + 1))
                        || values
                            .iter()
                            .filter(|(name, _)| name.starts_with("..."))
                            .any(|(_, value)| {
                                self.value(
                                    path,
                                    &Flow::Member(Box::new(value.clone()), key.clone()),
                                    locals,
                                    depth + 1,
                                )
                            })
                }
                other => self.value(path, &other, locals, depth + 1),
            },
            Flow::Call(callee, args) => {
                if let Flow::Member(object, method) = callee.as_ref() {
                    let namespace = matches!(
                        self.dereference(path, object, locals, depth + 1),
                        Flow::Namespace(_)
                    );
                    if namespace {
                        return self.function(
                            path,
                            &self.dereference(path, callee, locals, depth + 1),
                            args,
                            locals,
                            depth + 1,
                        ) || self.value(path, callee, locals, depth + 1);
                    }
                    if [
                        "map", "filter", "slice", "find", "sort", "toSorted", "at", "concat",
                        "flat", "flatMap", "reverse",
                    ]
                    .contains(&method.as_str())
                    {
                        return self.value(path, object, locals, depth + 1);
                    }
                    if ["then", "catch"].contains(&method.as_str()) {
                        return args
                            .iter()
                            .any(|arg| self.function(path, arg, &[], locals, depth + 1));
                    }
                }
                self.function(path, callee, args, locals, depth + 1)
            }
        }
    }
}

#[derive(Default)]
struct RuntimeUses {
    names: BTreeSet<String>,
    members: BTreeMap<String, BTreeSet<String>>,
    route_metadata: bool,
}
impl<'a> Visit<'a> for RuntimeUses {
    fn visit_export_declaration(&mut self, node: &ExportDeclaration<'a>) {
        if self.route_metadata {
            match &node.declaration {
                Declaration::VariableDeclaration(variables) => {
                    for variable in &variables.declarations {
                        if !matches!(&variable.id, BindingPattern::BindingIdentifier(id) if id.name == "metadata")
                        {
                            self.visit_variable_declarator(variable);
                        }
                    }
                    return;
                }
                Declaration::FunctionDeclaration(function)
                    if function
                        .id
                        .as_ref()
                        .is_some_and(|id| id.name == "generateMetadata") =>
                {
                    return;
                }
                _ => {}
            }
        }
        walk::walk_export_declaration(self, node);
    }
    fn visit_identifier_reference(&mut self, node: &IdentifierReference<'a>) {
        self.names.insert(node.name.to_string());
    }
    fn visit_ts_type(&mut self, _node: &TSType<'a>) {}
    fn visit_static_member_expression(&mut self, node: &StaticMemberExpression<'a>) {
        if let Expression::Identifier(id) = node.object.get_inner_expression() {
            self.members
                .entry(id.name.to_string())
                .or_default()
                .insert(node.property.name.to_string());
        }
        walk::walk_static_member_expression(self, node);
    }
    fn visit_computed_member_expression(&mut self, node: &ComputedMemberExpression<'a>) {
        if let Expression::Identifier(id) = node.object.get_inner_expression()
            && let Expression::StringLiteral(key) = node.expression.get_inner_expression()
        {
            self.members
                .entry(id.name.to_string())
                .or_default()
                .insert(key.value.to_string());
        }
        walk::walk_computed_member_expression(self, node);
    }
}

pub(super) fn findings(files: &[SourceFile], path: &str, source: &str) -> Vec<Violation> {
    let normalized = normalized(path);
    if normalized.contains("/__tests__/")
        || [".test.", ".spec."].iter().any(|part| {
            normalized
                .rsplit('/')
                .next()
                .is_some_and(|file| file.contains(part))
        })
    {
        return Vec::new();
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if !parsed.diagnostics.is_empty() {
        return Vec::new();
    }
    let mut uses = RuntimeUses {
        route_metadata: crate::ui_validate::site::is_route_file(path),
        ..RuntimeUses::default()
    };
    uses.visit_program(&parsed.program);
    let mut evaluator = Evaluator {
        files,
        modules: BTreeMap::new(),
    };
    let mut found = Vec::new();
    for statement in &parsed.program.body {
        let Statement::ImportDeclaration(node) = statement else {
            continue;
        };
        let Some(target) = resolve_file(files, path, node.source.value.as_str()) else {
            continue;
        };
        if !normalized_path_is_utils(&target.path) || target.path == path {
            continue;
        }
        for (local, flow) in imports(node) {
            if !uses.names.contains(&local) {
                continue;
            }
            let names = match flow {
                Flow::Import(_, name) => vec![name],
                Flow::Namespace(_) => uses
                    .members
                    .get(&local)
                    .map(|names| names.iter().cloned().collect())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            for imported in names {
                if evaluator.exported(path, node.source.value.as_str(), &imported, 0) {
                    let mut finding = error(
                        node.span,
                        "fixture-instead-of-api",
                        "UI reads a runtime import whose returned data is a hardcoded record array instead of an API source.",
                    );
                    finding
                        .context
                        .insert("fixturePath".into(), serde_json::json!(target.path));
                    finding
                        .context
                        .insert("fixtureBinding".into(), serde_json::json!(imported));
                    finding
                        .context
                        .insert("localBinding".into(), serde_json::json!(local));
                    finding.context.insert(
                        "confidence".into(),
                        serde_json::json!("proven-binding-flow"),
                    );
                    found.push(finding);
                }
            }
        }
    }
    found
}
fn normalized_path_is_utils(path: &str) -> bool {
    normalized(path).split('/').any(|part| part == "utils")
}

#[cfg(test)]
#[path = "fixture_flow_tests.rs"]
mod tests;
