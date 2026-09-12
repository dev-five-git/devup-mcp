//! Supplied-source structural checks. No path is ever opened or resolved on disk.
use super::{Severity, UiValidation, Violation, validate_source};
use crate::theme::{ProjectTheme, parse_project_theme};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_FILES: usize = 64;
pub const MAX_TOTAL_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub path: String,
    pub content: String,
}

/// Check the entire envelope before parsing any source or theme.
pub fn check_bundle(files: &[SourceFile]) -> Result<(), String> {
    if files.len() > MAX_FILES {
        return Err(format!("files exceeds {MAX_FILES} entries"));
    }
    let mut total = 0usize;
    let mut paths = BTreeSet::new();
    for file in files {
        total = total
            .saturating_add(file.path.len())
            .saturating_add(file.content.len());
        if total > MAX_TOTAL_BYTES {
            return Err(format!(
                "files exceeds {MAX_TOTAL_BYTES} total UTF-8 bytes (paths and contents)"
            ));
        }
        let path = file.path.replace('\\', "/");
        if path.is_empty() || !paths.insert(path) {
            return Err("files must have nonempty, unique paths".into());
        }
    }
    Ok(())
}

pub fn bundle_theme(files: &[SourceFile]) -> Result<Option<ProjectTheme>, String> {
    check_bundle(files)?;
    let themes: Vec<_> = files
        .iter()
        .filter(|f| f.path.replace('\\', "/").rsplit('/').next() == Some("devup.json"))
        .collect();
    match themes.as_slice() {
        [] => Ok(None),
        [file] => parse_project_theme(&file.content)
            .map(Some)
            .map_err(|e| e.to_string()),
        _ => Err("files contains multiple devup.json themes; supply one unambiguous theme".into()),
    }
}

pub fn validate_bundle(
    files: &[SourceFile],
    theme: Option<&ProjectTheme>,
    strict: bool,
) -> Result<UiValidation, String> {
    check_bundle(files)?;
    let boundary = files.iter().filter(|f| is_source(&f.path)).any(|file| {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, &file.content, source_type(&file.path)).parse();
        let mut facts = Facts::default();
        facts.visit_program(&parsed.program);
        facts.boundary
    });
    let mut result = UiValidation {
        ok: true,
        violations: vec![],
        theme_notes: vec![],
        checked_tokens: 0,
        available_token_count: theme.map_or(0, ProjectTheme::token_count),
    };
    for file in files.iter().filter(|f| is_source(&f.path)) {
        let mut report = validate_source(&file.content, theme, strict, source_type(&file.path));
        if !report.violations.iter().any(|v| v.rule == "invalid-syntax") {
            report.violations.extend(structural(file, boundary));
        }
        for finding in &mut report.violations {
            finding
                .context
                .insert("path".into(), serde_json::json!(file.path));
        }
        result.checked_tokens += report.checked_tokens;
        result.theme_notes.extend(report.theme_notes);
        result.violations.extend(report.violations);
    }
    result.theme_notes.sort();
    result.theme_notes.dedup();
    result.ok = result
        .violations
        .iter()
        .all(|v| v.severity != Severity::Error && !(strict && v.severity == Severity::Warning));
    Ok(result)
}
fn is_source(path: &str) -> bool {
    path.ends_with(".tsx")
        || path.ends_with(".ts")
        || path.ends_with(".jsx")
        || path.ends_with(".js")
}

fn finding(
    file: &SourceFile,
    span: Span,
    rule: &'static str,
    severity: Severity,
    message: &str,
    suggestion: &str,
) -> Violation {
    let start = span.start as usize;
    let end = span.end as usize;
    let prefix = file.content.get(..start).unwrap_or("");
    let mut context = serde_json::Map::new();
    context.insert(
        "line".into(),
        serde_json::json!(prefix.bytes().filter(|b| *b == b'\n').count() + 1),
    );
    context.insert(
        "column".into(),
        serde_json::json!(prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1),
    );
    context.insert(
        "positionEncoding".into(),
        serde_json::json!("1-based Unicode scalar columns; byteRange uses UTF-8 bytes"),
    );
    context.insert(
        "snippet".into(),
        serde_json::json!(
            file.content
                .get(start..end)
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>()
        ),
    );
    Violation {
        rule,
        severity,
        byte_range: [start, end],
        context,
        message: message.into(),
        suggestion: Some(suggestion.into()),
    }
}

#[derive(Default)]
struct Facts {
    boundary: bool,
    boundary_names: BTreeSet<String>,
    parameters: BTreeSet<String>,
    pending_parameters: Option<BTreeSet<String>>,
    parameter_names: BTreeSet<String>,
    wrapped_handlers: Vec<Span>,
    jsx_depth: usize,
    rendered_identifiers: BTreeSet<String>,
    rendered_members: Vec<(String, bool)>,
    functions: BTreeSet<String>,
    component_sites: Vec<(String, Span)>,
    handler_refs: Vec<(String, Span)>,
    jsx: bool,
    identifiers: BTreeSet<String>,
    members: Vec<(String, bool)>,
    calls: Vec<(String, Span)>,
    handlers: Vec<Span>,
    styles: Vec<Span>,
    control_flow: bool,
    delegated: bool,
}
impl<'a> Visit<'a> for Facts {
    fn visit_formal_parameters(&mut self, node: &FormalParameters<'a>) {
        let names = parameter_names(node);
        self.parameter_names.extend(names.iter().cloned());
        self.pending_parameters = Some(names);
        walk::walk_formal_parameters(self, node);
    }
    fn visit_function_body(&mut self, node: &FunctionBody<'a>) {
        let parameters = self.pending_parameters.take().unwrap_or_default();
        let previous = std::mem::replace(&mut self.parameters, parameters);
        walk::walk_function_body(self, node);
        self.parameters = previous;
    }
    fn visit_arrow_function_expression(&mut self, node: &ArrowFunctionExpression<'a>) {
        let pending = self.pending_parameters.take();
        let previous = std::mem::replace(&mut self.parameters, parameter_names(&node.params));
        walk::walk_arrow_function_expression(self, node);
        self.parameters = previous;
        self.pending_parameters = pending;
    }
    fn visit_import_declaration(&mut self, node: &ImportDeclaration<'a>) {
        if node.source.value == "react"
            && let Some(specifiers) = &node.specifiers
        {
            for specifier in specifiers {
                if let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier
                    && specifier.imported.name() == "Suspense"
                {
                    self.boundary_names.insert(specifier.local.name.to_string());
                }
            }
        }
        walk::walk_import_declaration(self, node);
    }
    fn visit_method_definition(&mut self, node: &MethodDefinition<'a>) {
        self.boundary |= node.key.static_name().is_some_and(|name| {
            matches!(
                name.as_ref(),
                "componentDidCatch" | "getDerivedStateFromError"
            )
        });
        walk::walk_method_definition(self, node);
    }
    fn visit_jsx_element(&mut self, node: &JSXElement<'a>) {
        self.jsx_depth += 1;
        walk::walk_jsx_element(self, node);
        self.jsx_depth -= 1;
    }
    fn visit_declaration(&mut self, node: &Declaration<'a>) {
        if let Declaration::FunctionDeclaration(f) = node
            && let Some(id) = &f.id
        {
            self.functions.insert(id.name.to_string());
        }
        let mut components = Vec::new();
        add_declaration(&mut components, node);
        self.component_sites
            .extend(components.into_iter().map(|c| (c.name, c.span)));
        walk::walk_declaration(self, node);
    }
    fn visit_variable_declarator(&mut self, node: &VariableDeclarator<'a>) {
        if let (
            BindingPattern::BindingIdentifier(id),
            Some(Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)),
        ) = (&node.id, &node.init)
        {
            self.functions.insert(id.name.to_string());
        }
        walk::walk_variable_declarator(self, node);
    }
    fn visit_jsx_opening_element(&mut self, node: &JSXOpeningElement<'a>) {
        self.jsx = true;
        let element_name = match &node.name {
            JSXElementName::Identifier(id) => Some(id.name.as_str()),
            JSXElementName::IdentifierReference(id) => Some(id.name.as_str()),
            _ => None,
        };
        if let Some(name) = element_name {
            self.boundary |= name == "Suspense"
                || name.ends_with("ErrorBoundary")
                || self.boundary_names.contains(name);
            self.delegated |=
                name.chars().next().is_some_and(char::is_uppercase) && !node.attributes.is_empty();
        }
        if let JSXElementName::MemberExpression(member) = &node.name {
            self.boundary |= member.property.name == "Suspense";
        }
        walk::walk_jsx_opening_element(self, node);
    }
    fn visit_jsx_fragment(&mut self, node: &JSXFragment<'a>) {
        self.jsx = true;
        self.jsx_depth += 1;
        walk::walk_jsx_fragment(self, node);
        self.jsx_depth -= 1;
    }
    fn visit_identifier_reference(&mut self, node: &IdentifierReference<'a>) {
        self.identifiers.insert(node.name.to_string());
        if self.jsx_depth > 0 {
            self.rendered_identifiers.insert(node.name.to_string());
        }
    }
    fn visit_static_member_expression(&mut self, node: &StaticMemberExpression<'a>) {
        if let Some(name) = expression_name(&node.object) {
            let member = (format!("{name}.{}", node.property.name), node.optional);
            if self.jsx_depth > 0 {
                self.rendered_members.push(member.clone());
            }
            self.members.push(member);
        }
        walk::walk_static_member_expression(self, node);
    }
    fn visit_call_expression(&mut self, node: &CallExpression<'a>) {
        if let Some(name) = expression_name(&node.callee) {
            self.calls.push((name, node.span));
        }
        walk::walk_call_expression(self, node);
    }
    fn visit_jsx_attribute(&mut self, node: &JSXAttribute<'a>) {
        if let (
            JSXAttributeName::Identifier(name),
            Some(JSXAttributeValue::ExpressionContainer(value)),
        ) = (&node.name, &node.value)
        {
            if name.name.starts_with("on")
                && matches!(
                    value.expression,
                    JSXExpression::ArrowFunctionExpression(_)
                        | JSXExpression::FunctionExpression(_)
                )
            {
                self.handlers.push(node.span);
                if let JSXExpression::ArrowFunctionExpression(arrow) = &value.expression {
                    let mut inner = Facts::default();
                    inner.visit_arrow_function_expression(arrow);
                    if inner.calls.iter().any(|(callee, _)| {
                        self.parameters
                            .contains(callee.split('.').next().unwrap_or(callee))
                            && !inner
                                .parameter_names
                                .contains(callee.split('.').next().unwrap_or(callee))
                    }) {
                        self.wrapped_handlers.push(node.span);
                    }
                }
            }
            if name.name.starts_with("on")
                && let JSXExpression::Identifier(id) = &value.expression
            {
                self.handler_refs.push((id.name.to_string(), node.span));
            }
            if name.name == "style"
                && matches!(value.expression, JSXExpression::ObjectExpression(_))
            {
                self.styles.push(node.span);
            }
        }
        walk::walk_jsx_attribute(self, node);
    }
    fn visit_if_statement(&mut self, node: &IfStatement<'a>) {
        self.control_flow = true;
        walk::walk_if_statement(self, node);
    }
    fn visit_conditional_expression(&mut self, node: &ConditionalExpression<'a>) {
        self.control_flow = true;
        walk::walk_conditional_expression(self, node);
    }
    fn visit_logical_expression(&mut self, node: &LogicalExpression<'a>) {
        self.control_flow = true;
        walk::walk_logical_expression(self, node);
    }
}
fn expression_name(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::Identifier(id) => Some(id.name.to_string()),
        Expression::StaticMemberExpression(m) => Some(format!(
            "{}.{}",
            expression_name(&m.object)?,
            m.property.name
        )),
        _ => None,
    }
}
#[derive(Clone, Copy)]
enum Body<'a> {
    Function(&'a FunctionBody<'a>),
    Arrow(&'a ArrowFunctionExpression<'a>),
}
impl<'a> Body<'a> {
    fn visit(self, visitor: &mut impl Visit<'a>) {
        match self {
            Self::Function(body) => visitor.visit_function_body(body),
            Self::Arrow(arrow) => visitor.visit_arrow_function_expression(arrow),
        }
    }
}
struct Component<'a> {
    name: String,
    span: Span,
    body: Body<'a>,
}
fn add_function<'a>(components: &mut Vec<Component<'a>>, function: &'a Function<'a>) {
    if let (Some(id), Some(body)) = (&function.id, &function.body) {
        add_body(
            components,
            id.name.as_str(),
            function.span,
            Body::Function(body),
        );
    }
}

/// Only direct JSX returns establish a component. JSX in an unrelated callback
/// does not make an uppercase utility function a React component.
#[derive(Default)]
struct RenderedComponent {
    jsx: bool,
    entered: bool,
}
impl<'a> Visit<'a> for RenderedComponent {
    fn visit_function_body(&mut self, node: &FunctionBody<'a>) {
        if self.entered {
            return;
        }
        self.entered = true;
        walk::walk_function_body(self, node);
        self.entered = false;
    }
    fn visit_arrow_function_expression(&mut self, node: &ArrowFunctionExpression<'a>) {
        if self.entered {
            return;
        }
        match &node.body {
            ArrowFunctionBody::FunctionBody(body) => self.visit_function_body(body),
            body => self.jsx |= body.as_expression().is_some_and(renders_jsx),
        }
    }
    fn visit_return_statement(&mut self, node: &ReturnStatement<'a>) {
        self.jsx |= node.argument.as_ref().is_some_and(renders_jsx);
    }
}

fn renders_jsx(expression: &Expression<'_>) -> bool {
    match expression.get_inner_expression() {
        Expression::JSXElement(_) | Expression::JSXFragment(_) => true,
        Expression::ConditionalExpression(conditional) => {
            renders_jsx(&conditional.consequent) || renders_jsx(&conditional.alternate)
        }
        Expression::LogicalExpression(logical) => renders_jsx(&logical.right),
        _ => false,
    }
}

fn add_body<'a>(components: &mut Vec<Component<'a>>, name: &str, span: Span, body: Body<'a>) {
    if !name.chars().next().is_some_and(char::is_uppercase) {
        return;
    }
    let mut facts = RenderedComponent::default();
    body.visit(&mut facts);
    if facts.jsx {
        components.push(Component {
            name: name.into(),
            span,
            body,
        });
    }
}
fn add_declaration<'a>(components: &mut Vec<Component<'a>>, declaration: &'a Declaration<'a>) {
    match declaration {
        Declaration::FunctionDeclaration(f) => add_function(components, f),
        Declaration::VariableDeclaration(v) => {
            for d in &v.declarations {
                if let (BindingPattern::BindingIdentifier(id), Some(init)) = (&d.id, &d.init) {
                    match init {
                        Expression::ArrowFunctionExpression(f) => {
                            add_body(components, id.name.as_str(), d.span, Body::Arrow(f))
                        }
                        Expression::FunctionExpression(f) => {
                            if let Some(body) = &f.body {
                                add_body(components, id.name.as_str(), d.span, Body::Function(body))
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
}

fn structural(file: &SourceFile, bundle_boundary: bool) -> Vec<Violation> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &file.content, source_type(&file.path)).parse();
    let program = &parsed.program;
    let mut facts = Facts::default();
    facts.visit_program(program);
    let mut imports = BTreeMap::new();
    let mut uncertain_import = false;
    let mut motion = false;
    let mut components = Vec::new();
    let mut defaults = Vec::new();
    for statement in &program.body {
        match statement {
            Statement::ImportDeclaration(import) => {
                let source = import.source.value.as_str();
                motion |= source == "framer-motion" || source == "motion/react";
                uncertain_import |= !matches!(
                    source,
                    "react" | "next/navigation" | "@devup-ui/react" | "@tanstack/react-query"
                );
                if let Some(specifiers) = &import.specifiers {
                    for specifier in specifiers {
                        match specifier {
                            ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                                imports.insert(
                                    specifier.local.name.to_string(),
                                    (source.to_owned(), specifier.imported.name().to_string()),
                                );
                            }
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                                imports.insert(
                                    specifier.local.name.to_string(),
                                    (source.to_owned(), "*".into()),
                                );
                            }
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                                imports.insert(
                                    specifier.local.name.to_string(),
                                    (source.to_owned(), "*".into()),
                                );
                            }
                        }
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => {
                defaults.push(export.span);
                if let ExportDefaultDeclarationKind::FunctionDeclaration(function) =
                    &export.declaration
                {
                    add_function(&mut components, function);
                }
            }
            Statement::ExportDeclaration(export) => {
                add_declaration(&mut components, &export.declaration)
            }
            Statement::FunctionDeclaration(function) => add_function(&mut components, function),
            Statement::VariableDeclaration(declaration) => {
                for d in &declaration.declarations {
                    if let (
                        BindingPattern::BindingIdentifier(id),
                        Some(Expression::ArrowFunctionExpression(f)),
                    ) = (&d.id, &d.init)
                    {
                        add_body(&mut components, id.name.as_str(), d.span, Body::Arrow(f));
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let path = file.path.replace('\\', "/");
    let path = path.trim_start_matches("./");
    let basename = path.rsplit('/').next().unwrap_or(path);
    for span in defaults {
        if basename != "page.tsx" {
            out.push(finding(
                file,
                span,
                "file-placement",
                Severity::Warning,
                "Default export outside page.tsx violates the named-export convention.",
                "Use a named export; retain framework-required exports when applicable.",
            ));
        }
    }
    for component in &components {
        if (path.starts_with("src/app/") || path.contains("/src/app/"))
            && !matches!(basename, "page.tsx" | "layout.tsx")
        {
            out.push(finding(
                file,
                component.span,
                "file-placement",
                Severity::Warning,
                "Component is defined under src/app/ outside page.tsx or layout.tsx.",
                "Move reusable components to src/components/.",
            ));
        }
        if basename == "page.tsx" && !component.name.ends_with("Page") {
            out.push(finding(
                file,
                component.span,
                "file-placement",
                Severity::Warning,
                "Page component name does not end in Page.",
                "Give the page component a name ending in Page.",
            ));
        }
    }
    if matches!(basename, "index.ts" | "index.tsx")
        && !program.body.is_empty()
        && program.directives.is_empty()
        && program.body.iter().all(|s| {
            matches!(
                s,
                Statement::ExportFromDeclaration(_)
                    | Statement::ExportAllDeclaration(_)
                    | Statement::ExportNamedDeclaration(_)
            )
        })
    {
        out.push(finding(
            file,
            program.span,
            "file-placement",
            Severity::Warning,
            "This index file only re-exports symbols.",
            "Import directly from the defining component module.",
        ));
    }
    let mut sites = facts.component_sites.clone();
    sites.extend(components.iter().map(|c| (c.name.clone(), c.span)));
    sites.sort_by_key(|(_, span)| span.start);
    sites.dedup_by_key(|(_, span)| span.start);
    for (_, span) in sites.iter().skip(1) {
        out.push(finding(
            file,
            *span,
            "one-component-per-file",
            Severity::Warning,
            "More than one JSX component is defined in this file.",
            "Move each component to its own file.",
        ));
    }
    let internal_handler = facts
        .handler_refs
        .iter()
        .any(|(name, _)| facts.functions.contains(name));
    let client = program
        .directives
        .iter()
        .find(|d| d.directive == "use client");
    let hooks: Vec<_> = facts
        .calls
        .iter()
        .filter(|(name, _)| {
            resolve_import(&imports, name).is_some_and(|(source, imported)| {
                (source == "react"
                    && matches!(
                        imported,
                        "useState" | "useEffect" | "useRef" | "useLayoutEffect" | "useReducer"
                    ))
                    || (source == "next/navigation"
                        && matches!(imported, "useRouter" | "usePathname" | "useSearchParams"))
            })
        })
        .collect();
    for span in &facts.wrapped_handlers {
        out.push(finding(file,*span,"client-boundary",Severity::Warning,"An arrow wrapper calls a received handler and defines a new client-only function.","Pass received handlers directly: onClick={onClick} or onClick={onClick ? onClick : undefined}."));
    }
    if let Some(directive) = client {
        if hooks.is_empty() && facts.handlers.is_empty() && !internal_handler && !motion {
            let uncertain = uncertain_import || !facts.calls.is_empty();
            out.push(finding(file,directive.span,"client-boundary",if uncertain {Severity::Info} else {Severity::Warning},if uncertain {"No listed client-only feature is visible; imported or called code may still require this client boundary."} else {"The use client directive is unnecessary for the visible component: no client-only feature is present."},"Review the boundary and remove the directive when this module needs no client-only behavior."));
        }
    } else if !hooks.is_empty() || !facts.handlers.is_empty() || internal_handler || motion {
        let span = hooks
            .first()
            .map(|(_, span)| *span)
            .or_else(|| facts.handlers.first().copied())
            .unwrap_or(program.span);
        out.push(finding(file,span,"client-boundary",Severity::Info,"Client-only behavior is visible without a local use client directive; an importing client boundary may already cover this module, which a partial bundle cannot disprove.","Check the importing boundary before adding use client to this module."));
    }
    for span in facts.styles {
        out.push(finding(
            file,
            span,
            "inline-style",
            Severity::Warning,
            "Inline style object bypasses the css class convention.",
            "Use className={css({ ... })} for statically expressible styles.",
        ));
    }
    for component in components {
        query_findings(file, &component, &imports, bundle_boundary, &mut out);
    }
    out
}

#[derive(Default)]
struct Queries {
    bindings: Vec<(String, String, String, Span, bool)>,
}
impl<'a> Visit<'a> for Queries {
    fn visit_variable_declarator(&mut self, node: &VariableDeclarator<'a>) {
        if let Some(Expression::CallExpression(call)) = &node.init
            && let Some(callee) = expression_name(&call.callee)
        {
            let uncertain_options = call.arguments.iter().any(|arg| match arg {
                Argument::ObjectExpression(object) => object.properties.iter().any(|p| match p {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        p.key.static_name().is_none_or(|key| {
                            matches!(
                                key.as_ref(),
                                "initialData" | "placeholderData" | "suspense" | "throwOnError"
                            )
                        })
                    }
                    _ => true,
                }),
                _ => true,
            });
            match &node.id {
                BindingPattern::ObjectPattern(pattern) => {
                    let mut data = None;
                    let mut pending = String::new();
                    let mut error = String::new();
                    for property in &pattern.properties {
                        if let (Some(key), BindingPattern::BindingIdentifier(id)) =
                            (property.key.static_name(), &property.value)
                        {
                            match key.as_ref() {
                                "data" => data = Some(id.name.to_string()),
                                "isPending" => pending = id.name.to_string(),
                                "isError" => error = id.name.to_string(),
                                _ => {}
                            }
                        }
                    }
                    if let Some(data) = data {
                        self.bindings.push((
                            format!("{callee}|{data}"),
                            pending,
                            error,
                            node.span,
                            uncertain_options,
                        ));
                    }
                }
                BindingPattern::BindingIdentifier(id) => self.bindings.push((
                    format!("{callee}|{}.data", id.name),
                    format!("{}.isPending", id.name),
                    format!("{}.isError", id.name),
                    node.span,
                    uncertain_options,
                )),
                _ => {}
            }
        }
        walk::walk_variable_declarator(self, node);
    }
}
fn query_findings(
    file: &SourceFile,
    component: &Component<'_>,
    imports: &BTreeMap<String, (String, String)>,
    boundary: bool,
    out: &mut Vec<Violation>,
) {
    let mut queries = Queries::default();
    component.body.visit(&mut queries);
    let mut facts = Facts::default();
    component.body.visit(&mut facts);
    for (binding, pending, error, span, uncertain_options) in queries.bindings {
        let (callee, data) = binding.split_once('|').unwrap();
        if !resolve_import(imports, callee).is_some_and(|(source, name)| {
            source == "@tanstack/react-query"
                && matches!(name, "useQuery" | "useSuspenseQuery" | "useInfiniteQuery")
        }) {
            continue;
        }
        let referenced = |name: &str| {
            facts.identifiers.contains(name)
                || facts.members.iter().any(|(member, _)| member == name)
        };
        let rendered = facts.rendered_identifiers.contains(data)
            || facts
                .rendered_members
                .iter()
                .any(|(member, _)| member == data || member.starts_with(&format!("{data}.")));
        if !rendered {
            continue;
        }
        if facts.control_flow && referenced(&pending) && referenced(&error) {
            continue;
        }
        let unsafe_access = facts
            .rendered_members
            .iter()
            .any(|(member, optional)| !optional && member.starts_with(&format!("{data}.")));
        let suspense_query = imports
            .get(callee)
            .is_some_and(|(_, name)| name == "useSuspenseQuery");
        let reason = if boundary {
            Some(
                "A Suspense or error boundary is visible in the supplied bundle; its relationship to this query needs review.",
            )
        } else if suspense_query {
            Some("The query delegates pending and error behavior through suspense.")
        } else if facts.delegated {
            Some(
                "Rendering delegates to a child; the partial bundle may delegate pending and error handling too.",
            )
        } else if !facts.functions.is_empty()
            || facts
                .parameter_names
                .contains(data.split('.').next().unwrap_or(data))
        {
            Some(
                "Nested function scopes or callback parameters can bind the same data name; this syntax-only analysis cannot prove that every access refers to this query.",
            )
        } else if uncertain_options {
            Some(
                "Query options can provide initial data or delegate state handling; the result cannot be proven unsafe from this source.",
            )
        } else if facts.control_flow || !unsafe_access {
            Some(
                "Data is rendered without both visible query-state guards, but optional access or other control flow prevents proving unsafe rendering.",
            )
        } else {
            None
        };
        out.push(finding(file,span,"react-query-states",if reason.is_some(){Severity::Info}else{Severity::Error},reason.unwrap_or("Query data is dereferenced during rendering without local isPending and isError handling; data can be undefined while pending. This does not assert that external boundaries are absent."),"Handle isPending and isError before rendering data, or verify the boundary or child responsible for those states."));
    }
}

fn resolve_import<'a>(
    imports: &'a BTreeMap<String, (String, String)>,
    name: &'a str,
) -> Option<(&'a str, &'a str)> {
    if let Some((source, imported)) = imports.get(name) {
        return Some((source, imported));
    }
    let (namespace, member) = name.split_once('.')?;
    let (source, imported) = imports.get(namespace)?;
    (imported == "*").then_some((source.as_str(), member))
}

fn parameter_names(parameters: &FormalParameters<'_>) -> BTreeSet<String> {
    parameters
        .items
        .iter()
        .flat_map(|p| p.pattern.get_binding_identifiers())
        .map(|id| id.name.to_string())
        .collect()
}

fn source_type(path: &str) -> SourceType {
    if path.ends_with(".ts") {
        SourceType::ts()
    } else {
        SourceType::tsx()
    }
}
