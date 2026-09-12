//! Syntactic reuse evidence, deliberately not a TypeScript type checker.
//! Source spans preserve declared types; unsupported types remain unresolved.
use std::collections::{BTreeMap, BTreeSet};

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::{Value, json};

#[derive(Clone)]
pub(super) struct Import {
    pub source: String,
    pub names: Vec<String>,
}

#[derive(Clone)]
pub(super) struct Reexport {
    pub exported: String,
    pub imported: String,
    pub source: String,
}

pub(super) struct Evidence {
    pub component: Option<Value>,
    pub imports: Vec<Import>,
    pub reexports: Vec<Reexport>,
    pub local_exports: BTreeSet<String>,
}

fn capitalized(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn devup(source: &str) -> bool {
    matches!(source, "@devup-ui/components" | "@devup-ui/react")
}

type Declarations<'a> = BTreeMap<String, &'a Declaration<'a>>;

struct Props {
    fields: Vec<Value>,
    complete: bool,
    annotation: Option<String>,
}

fn annotated(ty: &TSType<'_>, declarations: &Declarations<'_>, source: &str) -> Props {
    let (fields, complete) = type_fields(ty, declarations, source, 0);
    Props {
        fields,
        complete,
        annotation: Some(ty.span().source_text(source).to_owned()),
    }
}

fn parameters(
    params: &FormalParameters<'_>,
    declarations: &Declarations<'_>,
    source: &str,
) -> Props {
    if let Some(param) = params.items.first() {
        if let Some(annotation) = &param.type_annotation {
            return annotated(&annotation.type_annotation, declarations, source);
        }
        return Props {
            fields: vec![],
            complete: false,
            annotation: None,
        };
    }
    Props {
        fields: vec![],
        complete: params.rest.is_none(),
        annotation: None,
    }
}

fn signatures(members: &[TSSignature<'_>], source: &str) -> (Vec<Value>, bool) {
    let mut fields = vec![];
    let mut complete = true;
    for member in members {
        match member {
            TSSignature::TSPropertySignature(property) if !property.computed => {
                if let Some(name) = property.key.static_name() {
                    fields.push(json!({
                        "name":name, "optional":property.optional,
                        "type":property.type_annotation.as_ref().map(|a| a.type_annotation.span().source_text(source))
                    }));
                    complete &= property.type_annotation.is_some();
                } else {
                    complete = false;
                }
            }
            TSSignature::TSMethodSignature(method) if !method.computed => {
                if let Some(name) = method.key.static_name() {
                    // Keep the method signature as written, including generic parameters.
                    let start = method
                        .type_parameters
                        .as_ref()
                        .map_or(method.params.span.start, |p| p.span.start);
                    let end = method
                        .return_type
                        .as_ref()
                        .map_or(method.params.span.end, |r| r.span.end);
                    fields.push(json!({"name":name, "optional":method.optional,
                        "type":&source[start as usize..end as usize]}));
                } else {
                    complete = false;
                }
            }
            _ => complete = false,
        }
    }
    (fields, complete)
}

fn named_fields(
    name: &str,
    declarations: &Declarations<'_>,
    source: &str,
    depth: usize,
) -> (Vec<Value>, bool) {
    if depth > 24 {
        return (vec![], false);
    }
    match declarations.get(name) {
        Some(Declaration::TSTypeAliasDeclaration(alias)) => {
            let (fields, complete) =
                type_fields(&alias.type_annotation, declarations, source, depth + 1);
            (fields, complete && alias.type_parameters.is_none())
        }
        Some(Declaration::TSInterfaceDeclaration(interface)) => {
            let (mut fields, mut complete) = signatures(&interface.body.body, source);
            for base in &interface.extends {
                let (inherited, resolved) = named_fields(
                    base.type_name.span().source_text(source),
                    declarations,
                    source,
                    depth + 1,
                );
                fields.extend(inherited);
                complete &= resolved && base.type_arguments.is_none();
            }
            (fields, complete && interface.type_parameters.is_none())
        }
        _ => (vec![], false),
    }
}

fn type_fields(
    ty: &TSType<'_>,
    declarations: &Declarations<'_>,
    source: &str,
    depth: usize,
) -> (Vec<Value>, bool) {
    if depth > 24 {
        return (vec![], false);
    }
    match ty {
        TSType::TSTypeLiteral(literal) => signatures(&literal.members, source),
        TSType::TSTypeReference(reference) if reference.type_arguments.is_none() => named_fields(
            reference.type_name.span().source_text(source),
            declarations,
            source,
            depth + 1,
        ),
        TSType::TSIntersectionType(intersection) => {
            let mut fields = vec![];
            let mut complete = true;
            for ty in &intersection.types {
                let (part, resolved) = type_fields(ty, declarations, source, depth + 1);
                fields.extend(part);
                complete &= resolved;
            }
            (fields, complete)
        }
        _ => (vec![], false),
    }
}

fn expression_props(
    expression: &Expression<'_>,
    declarations: &Declarations<'_>,
    source: &str,
) -> Option<Props> {
    match expression {
        Expression::ArrowFunctionExpression(function) => {
            Some(parameters(&function.params, declarations, source))
        }
        Expression::FunctionExpression(function) => {
            Some(parameters(&function.params, declarations, source))
        }
        Expression::ParenthesizedExpression(p) => {
            expression_props(&p.expression, declarations, source)
        }
        Expression::TSAsExpression(p) => expression_props(&p.expression, declarations, source),
        Expression::TSSatisfiesExpression(p) => {
            expression_props(&p.expression, declarations, source)
        }
        Expression::CallExpression(call) => {
            let callee = call.callee.span().source_text(source);
            if callee == "forwardRef" || callee == "React.forwardRef" {
                if let Some(ty) = call
                    .type_arguments
                    .as_ref()
                    .and_then(|args| args.params.get(1))
                {
                    return Some(annotated(ty, declarations, source));
                }
            } else if callee != "memo" && callee != "React.memo" {
                // An uppercase export initialized by an unfamiliar factory might be a
                // component. Preserve it as unresolved instead of asserting absence.
                return Some(Props {
                    fields: vec![],
                    complete: false,
                    annotation: None,
                });
            }
            call.arguments
                .first()
                .and_then(Argument::as_expression)
                .and_then(|expression| expression_props(expression, declarations, source))
                .or(Some(Props {
                    fields: vec![],
                    complete: false,
                    annotation: None,
                }))
        }
        _ => None,
    }
}

fn declaration_candidates(
    declaration: &Declaration<'_>,
    declarations: &Declarations<'_>,
    source: &str,
) -> Vec<(String, Props)> {
    match declaration {
        Declaration::FunctionDeclaration(function) => function
            .id
            .as_ref()
            .map(|id| {
                vec![(
                    id.name.to_string(),
                    parameters(&function.params, declarations, source),
                )]
            })
            .unwrap_or_default(),
        Declaration::VariableDeclaration(variables) => variables
            .declarations
            .iter()
            .filter_map(|variable| {
                let BindingPattern::BindingIdentifier(id) = &variable.id else {
                    return None;
                };
                let mut props = variable
                    .init
                    .as_ref()
                    .and_then(|e| expression_props(e, declarations, source))?;
                // React.FC<Props> and FC<Props> declare the props on the variable.
                if let Some(annotation) = &variable.type_annotation
                    && let TSType::TSTypeReference(reference) = &annotation.type_annotation
                    && matches!(
                        reference.type_name.span().source_text(source),
                        "FC" | "React.FC" | "FunctionComponent" | "React.FunctionComponent"
                    )
                    && let Some(ty) = reference
                        .type_arguments
                        .as_ref()
                        .and_then(|args| args.params.first())
                {
                    props = annotated(ty, declarations, source);
                }
                Some((id.name.to_string(), props))
            })
            .collect(),
        Declaration::ClassDeclaration(class) => {
            let Some(id) = &class.id else {
                return vec![];
            };
            let Some(heritage) = &class.heritage else {
                return vec![];
            };
            let props = heritage
                .type_arguments
                .as_ref()
                .and_then(|args| args.params.first())
                .map(|ty| annotated(ty, declarations, source))
                .unwrap_or(Props {
                    fields: vec![],
                    complete: false,
                    annotation: None,
                });
            vec![(id.name.to_string(), props)]
        }
        _ => vec![],
    }
}

fn class_props(class: &Class<'_>, declarations: &Declarations<'_>, source: &str) -> Props {
    class
        .heritage
        .as_ref()
        .and_then(|h| h.type_arguments.as_ref())
        .and_then(|args| args.params.first())
        .map(|ty| annotated(ty, declarations, source))
        .unwrap_or(Props {
            fields: vec![],
            complete: false,
            annotation: None,
        })
}

/// Parse one file completely before publishing any evidence from it.
fn declaration_of<'a>(statement: &'a Statement<'a>) -> Option<&'a Declaration<'a>> {
    match statement {
        Statement::ExportDeclaration(export) => Some(&export.declaration),
        _ => statement.as_declaration(),
    }
}

pub(super) fn parse(source: &str, source_type: SourceType) -> Result<Evidence, String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(parsed
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "));
    }
    let statements = &parsed.program.body;
    let mut declarations = BTreeMap::new();
    for statement in statements {
        if let Some(declaration) = declaration_of(statement) {
            let name = match declaration {
                Declaration::TSTypeAliasDeclaration(alias) => Some(alias.id.name.to_string()),
                Declaration::TSInterfaceDeclaration(interface) => {
                    Some(interface.id.name.to_string())
                }
                _ => None,
            };
            if let Some(name) = name {
                declarations.insert(name, declaration);
            }
        }
    }
    let mut candidates = BTreeMap::new();
    let mut exports = BTreeMap::<String, String>::new();
    let mut imports = vec![];
    let mut imported_locals = BTreeMap::new();
    let mut reexports = vec![];
    for statement in statements {
        if let Some(declaration) = declaration_of(statement) {
            for (name, props) in declaration_candidates(declaration, &declarations, source) {
                if matches!(statement, Statement::ExportDeclaration(_)) {
                    exports.insert(name.clone(), name.clone());
                }
                candidates.insert(name, props);
            }
        }
        match statement {
            Statement::ImportDeclaration(import)
                if import.import_kind == ImportOrExportKind::Value =>
            {
                let mut names = vec![];
                for specifier in import.specifiers.iter().flatten() {
                    let (name, local) = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(specifier)
                            if specifier.import_kind == ImportOrExportKind::Value =>
                        {
                            (
                                specifier.imported.to_string(),
                                specifier.local.name.to_string(),
                            )
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                            ("default".to_owned(), specifier.local.name.to_string())
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                            ("*".to_owned(), specifier.local.name.to_string())
                        }
                        _ => continue,
                    };
                    imported_locals.insert(local, (import.source.value.to_string(), name.clone()));
                    names.push(name);
                }
                // `import { type Foo }` carries no value edge, unlike a bare
                // side-effect import or an explicitly empty import list.
                if !names.is_empty() || import.specifiers.as_ref().is_none_or(|s| s.is_empty()) {
                    imports.push(Import {
                        source: import.source.value.to_string(),
                        names,
                    });
                }
            }
            Statement::ExportNamedDeclaration(export)
                if export.export_kind == ImportOrExportKind::Value =>
            {
                for specifier in &export.specifiers {
                    if specifier.export_kind == ImportOrExportKind::Value {
                        exports.insert(specifier.exported.to_string(), specifier.local.to_string());
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => {
                let (name, props) = match &export.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(function) => (
                        function
                            .id
                            .as_ref()
                            .map_or("default".to_owned(), |id| id.name.to_string()),
                        Some(parameters(&function.params, &declarations, source)),
                    ),
                    ExportDefaultDeclarationKind::Identifier(id) => (id.name.to_string(), None),
                    ExportDefaultDeclarationKind::ClassDeclaration(class) => (
                        class
                            .id
                            .as_ref()
                            .map_or("default".to_owned(), |id| id.name.to_string()),
                        Some(class_props(class, &declarations, source)),
                    ),
                    other => (
                        "default".to_owned(),
                        other
                            .as_expression()
                            .and_then(|e| expression_props(e, &declarations, source)),
                    ),
                };
                exports.insert("default".to_owned(), name.clone());
                if let Some(props) = props {
                    candidates.insert(name, props);
                }
            }
            Statement::ExportFromDeclaration(export)
                if export.export_kind == ImportOrExportKind::Value =>
            {
                let mut names = vec![];
                for specifier in &export.specifiers {
                    if specifier.export_kind == ImportOrExportKind::Type {
                        continue;
                    }
                    let imported = specifier.local.to_string();
                    names.push(imported.clone());
                    reexports.push(Reexport {
                        exported: specifier.exported.to_string(),
                        imported,
                        source: export.source.value.to_string(),
                    });
                }
                imports.push(Import {
                    source: export.source.value.to_string(),
                    names,
                });
            }
            Statement::ExportAllDeclaration(export)
                if export.export_kind == ImportOrExportKind::Value =>
            {
                reexports.push(Reexport {
                    exported: export
                        .exported
                        .as_ref()
                        .map_or("*".to_owned(), ToString::to_string),
                    imported: "*".to_owned(),
                    source: export.source.value.to_string(),
                });
                imports.push(Import {
                    source: export.source.value.to_string(),
                    names: vec!["*".to_owned()],
                });
            }
            _ => {}
        }
    }
    let mut local_names = BTreeSet::new();
    let mut local_exports = BTreeSet::new();
    let mut exported_names = BTreeSet::new();
    for (exported, local) in &exports {
        if exported != "default" && !capitalized(exported) {
            continue;
        }
        if let Some((source, imported)) = imported_locals.get(local) {
            reexports.push(Reexport {
                exported: exported.clone(),
                imported: imported.clone(),
                source: source.clone(),
            });
        } else if candidates.contains_key(local) {
            exported_names.insert(exported.clone());
            local_exports.insert(exported.clone());
            local_names.insert(local.clone());
        }
    }
    for reexport in &reexports {
        if capitalized(&reexport.exported)
            || reexport.exported == "default"
            || reexport.exported == "*"
        {
            exported_names.insert(reexport.exported.clone());
        }
    }
    let mut props = vec![];
    let mut annotations = vec![];
    let mut complete = true;
    for name in &local_names {
        let candidate = &candidates[name];
        complete &= candidate.complete;
        annotations.push(
            json!({"component":name,"type":candidate.annotation,"resolved":candidate.complete}),
        );
        for field in &candidate.fields {
            let mut field = field.clone();
            field["component"] = json!(name);
            props.push(field);
        }
    }
    let kind = if !local_names.is_empty() {
        if reexports.is_empty() {
            "project-component"
        } else {
            "mixed"
        }
    } else if reexports.iter().all(|r| devup(&r.source)) {
        "devup-ui-reexport"
    } else {
        "project-reexport"
    };
    let component = (!exported_names.is_empty()).then(|| json!({
        "exports":exported_names, "localNames":local_names.into_iter().filter(|n| n != "default").collect::<Vec<_>>(),
        "client":parsed.program.directives.iter().any(|d| d.expression.value == "use client"),
        "kind":kind, "props":props,
        "propsStatus":if !complete { "unresolved" } else if !local_exports.is_empty() { "resolved" } else { "not-declared-here" },
        "propsDeclarations":annotations,
        "reexports":reexports.iter().map(|r| json!({"exported":r.exported,"imported":r.imported,"source":r.source})).collect::<Vec<_>>()
    }));
    Ok(Evidence {
        component,
        imports,
        reexports,
        local_exports,
    })
}
