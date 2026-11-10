//! Which module-level `const`s of one file are build-time values.
//!
//! devup-ui documents that `css()`, `globalCss()` and `keyframes()` accept a
//! `const` declared in the same file — a `keyframes()` name, a `css()` class,
//! or a literal built from other such constants — because the build reads it
//! in place (`css({ animationName: spin })`). Treating every identifier as a
//! runtime value reported that documented pattern as `runtime-value`.
//!
//! Only what this file proves is accepted. An import, a parameter, a `let`, a
//! name bound more than once (so a parameter may shadow the constant), a call
//! to anything but `css`/`keyframes`, and an object or array constant that the
//! file visibly changes all stay runtime values.
use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::ast::{
    Argument, AssignmentExpression, BindingIdentifier, CallExpression, Declaration, Expression,
    Program, Statement, UnaryExpression, UnaryOperator, UpdateExpression, VariableDeclaration,
    VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};

/// Calls whose result is a name fixed at build time.
const BUILD_TIME_NAME_CALLS: &[&str] = &["keyframes", "css"];
/// Calls that only read a constant handed to them.
const READ_ONLY_CALLS: &[&str] = &["css", "globalCss", "keyframes", "styled"];

#[derive(Default)]
struct Scan {
    bindings: BTreeMap<String, usize>,
    /// Object/array constants the file writes to, calls a method on, or hands
    /// to a function that may change them.
    changed: BTreeSet<String>,
}

fn root_identifier<'b>(expression: &'b Expression<'_>) -> Option<&'b str> {
    let mut current = expression.get_inner_expression();
    loop {
        match current {
            Expression::Identifier(identifier) => return Some(identifier.name.as_str()),
            Expression::StaticMemberExpression(member) => {
                current = member.object.get_inner_expression();
            }
            Expression::ComputedMemberExpression(member) => {
                current = member.object.get_inner_expression();
            }
            _ => return None,
        }
    }
}

fn callee_name<'b>(call: &'b CallExpression<'_>) -> Option<&'b str> {
    match call.callee.get_inner_expression() {
        Expression::Identifier(identifier) => Some(identifier.name.as_str()),
        Expression::StaticMemberExpression(member) => Some(member.property.name.as_str()),
        _ => None,
    }
}

impl Scan {
    fn mark(&mut self, name: Option<&str>) {
        if let Some(name) = name {
            self.changed.insert(name.to_owned());
        }
    }
}

impl<'a> Visit<'a> for Scan {
    fn visit_binding_identifier(&mut self, identifier: &BindingIdentifier<'a>) {
        *self.bindings.entry(identifier.name.to_string()).or_default() += 1;
    }

    fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
        if let Some(member) = assignment.left.as_member_expression() {
            self.mark(root_identifier(member.object()));
        }
        walk::walk_assignment_expression(self, assignment);
    }

    fn visit_update_expression(&mut self, update: &UpdateExpression<'a>) {
        if let Some(member) = update.argument.as_member_expression() {
            self.mark(root_identifier(member.object()));
        }
        walk::walk_update_expression(self, update);
    }

    fn visit_unary_expression(&mut self, unary: &UnaryExpression<'a>) {
        if unary.operator == UnaryOperator::Delete {
            self.mark(root_identifier(&unary.argument));
        }
        walk::walk_unary_expression(self, unary);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        let read_only = callee_name(call).is_some_and(|name| READ_ONLY_CALLS.contains(&name));
        if !read_only {
            // `list.push(x)`, `Object.assign(base, …)`, `mutate(base)`.
            if let Expression::StaticMemberExpression(member) = call.callee.get_inner_expression() {
                self.mark(root_identifier(&member.object));
            }
            for argument in &call.arguments {
                match argument {
                    Argument::SpreadElement(spread) => {
                        self.mark(root_identifier(&spread.argument));
                    }
                    _ => {
                        if let Some(expression) = argument.as_expression() {
                            self.mark(root_identifier(expression));
                        }
                    }
                }
            }
        }
        walk::walk_call_expression(self, call);
    }
}

fn module_consts<'b, 'a>(program: &'b Program<'a>) -> Vec<(&'b str, &'b Expression<'a>)> {
    fn push<'b, 'a>(
        declaration: &'b VariableDeclaration<'a>,
        found: &mut Vec<(&'b str, &'b Expression<'a>)>,
    ) {
        if declaration.kind != VariableDeclarationKind::Const {
            return;
        }
        for declarator in &declaration.declarations {
            if let (Some(identifier), Some(init)) =
                (declarator.id.get_binding_identifier(), declarator.init.as_ref())
            {
                found.push((identifier.name.as_str(), init));
            }
        }
    }
    let mut found = Vec::new();
    for statement in &program.body {
        match statement {
            Statement::VariableDeclaration(declaration) => push(declaration, &mut found),
            Statement::ExportDeclaration(export) => {
                if let Declaration::VariableDeclaration(declaration) = &export.declaration {
                    push(declaration, &mut found);
                }
            }
            _ => {}
        }
    }
    found
}

/// Names of this file's module-level constants that hold a build-time value.
pub(super) fn static_const_names(program: &Program<'_>) -> BTreeSet<String> {
    let mut scan = Scan::default();
    scan.visit_program(program);
    let candidates: Vec<_> = module_consts(program)
        .into_iter()
        .filter(|(name, _)| scan.bindings.get(*name) == Some(&1))
        .collect();
    let mut names = BTreeSet::new();
    // A constant may be built from one declared later in source order only if
    // that one is static too, so repeat until nothing more resolves.
    loop {
        let before = names.len();
        for (name, init) in &candidates {
            if names.contains(*name) {
                continue;
            }
            let inner = init.get_inner_expression();
            let is_static = match inner {
                Expression::CallExpression(call) => {
                    callee_name(call).is_some_and(|callee| BUILD_TIME_NAME_CALLS.contains(&callee))
                }
                Expression::ObjectExpression(_) | Expression::ArrayExpression(_) => {
                    !scan.changed.contains(*name) && super::is_static_expression(inner, false, &names)
                }
                _ => super::is_static_expression(inner, false, &names),
            };
            if is_static {
                names.insert((*name).to_owned());
            }
        }
        if names.len() == before {
            return names;
        }
    }
}
