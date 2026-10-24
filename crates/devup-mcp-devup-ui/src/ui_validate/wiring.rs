use super::{Severity, Violation};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};

fn error(span: Span, rule: &'static str, message: &str) -> Violation {
    Violation {
        rule,
        severity: Severity::Error,
        byte_range: [span.start as usize, span.end as usize],
        context: Default::default(),
        message: message.into(),
        suggestion: Some("Wire the control and data to the real application contract before reporting completion.".into()),
    }
}

pub fn findings(source: &str) -> Vec<Violation> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if !parsed.diagnostics.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for comment in &parsed.program.comments {
        let text = &source[comment.span.start as usize..comment.span.end as usize];
        let lower = text.to_lowercase();
        if ["todo", "api 생기면", "replace when"].iter().any(|marker| lower.contains(marker)) {
            found.push(error(comment.span, "deferred-wiring", "Deferred implementation comment leaves application wiring unfinished."));
        }
    }
    let mut collector = Collector { source, found, functions: std::collections::BTreeMap::new(), forms: Vec::new(), submit_depth: 0, link_depth: 0 };
    collector.visit_program(&parsed.program);
    for (span, handler) in &collector.forms {
        let body = collector.functions.get(handler).map(String::as_str).unwrap_or(handler);
        if fake_success(body) {
            collector.found.push(error(*span, "fake-submit", "Form submission only changes local success state without a request."));
        }
    }
    collector.found
}

fn fake_success(body: &str) -> bool {
    let lower = body.to_lowercase();
    let success = ["setsuccess(", "setsubmitted(", "setsent(", "setissuccess(", "setissubmitted(", "setissent("].iter().any(|name| lower.contains(name));
    success && !["fetch(", ".post(", ".mutate(", ".mutateasync(", "axios(", ".request("].iter().any(|call| lower.contains(call))
}

pub fn fixture_findings(files: &[crate::ui_validate::SourceFile], path: &str, source: &str) -> Vec<Violation> {
    struct Imports(Vec<(String, Span)>);
    impl<'a> Visit<'a> for Imports {
        fn visit_import_declaration(&mut self, node: &ImportDeclaration<'a>) {
            self.0.push((node.source.value.to_string(), node.span));
        }
    }
    struct Records(bool);
    impl<'a> Visit<'a> for Records {
        fn visit_array_expression(&mut self, node: &ArrayExpression<'a>) {
            if !node.elements.is_empty() && node.elements.iter().all(|item| matches!(item, ArrayExpressionElement::ObjectExpression(object) if object.properties.len() >= 2 && object.properties.iter().all(|property| matches!(property, ObjectPropertyKind::ObjectProperty(property) if matches!(property.value, Expression::StringLiteral(_) | Expression::NumericLiteral(_) | Expression::BooleanLiteral(_) | Expression::NullLiteral(_)))))) {
                self.0 = true;
            }
            walk::walk_array_expression(self, node);
        }
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    let mut imports = Imports(Vec::new());
    imports.visit_program(&parsed.program);
    imports.0.into_iter().filter_map(|(import, span)| {
        let import = import.replace('\\', "/");
        let tail = import.split("utils/").nth(1)?;
        let target = files.iter().find(|file| {
            let normalized = file.path.replace('\\', "/");
            normalized.ends_with(&format!("utils/{tail}")) || normalized.ends_with(&format!("utils/{tail}.ts"))
        })?;
        if target.path == path { return None; }
        let parsed = Parser::new(&allocator, &target.content, SourceType::ts()).parse();
        let mut records = Records(false);
        records.visit_program(&parsed.program);
        records.0.then(|| {
            let mut finding = error(span, "fixture-instead-of-api", "UI imports a hardcoded record array from utils instead of an API data source.");
            finding.context.insert("fixturePath".into(), serde_json::json!(target.path));
            finding.context.insert("confidence".into(), serde_json::json!("medium"));
            finding
        })
    }).collect()
}

struct Collector<'s> {
    source: &'s str,
    found: Vec<Violation>,
    functions: std::collections::BTreeMap<String, String>,
    forms: Vec<(Span, String)>,
    submit_depth: usize,
    link_depth: usize,
}

fn attr_name<'a>(item: &'a JSXAttributeItem<'a>) -> Option<&'a str> {
    match item {
        JSXAttributeItem::Attribute(attribute) => attribute.name.as_identifier().map(|name| name.name.as_str()),
        _ => None,
    }
}

fn value_text(value: &JSXAttributeValue<'_>) -> Option<String> {
    match value {
        JSXAttributeValue::StringLiteral(value) => Some(value.value.to_string()),
        JSXAttributeValue::ExpressionContainer(value) => match &value.expression {
            JSXExpression::Identifier(value) => Some(value.name.to_string()),
            JSXExpression::StringLiteral(value) => Some(value.value.to_string()),
            _ => None,
        },
        _ => None,
    }
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_variable_declarator(&mut self, node: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &node.id
            && let Some(Expression::ArrowFunctionExpression(function)) = &node.init
        {
            self.functions.insert(id.name.to_string(), self.source[function.span.start as usize..function.span.end as usize].into());
        }
        walk::walk_variable_declarator(self, node);
    }

    fn visit_statement(&mut self, node: &Statement<'a>) {
        if let Statement::FunctionDeclaration(function) = node
            && let Some(id) = &function.id
        {
            self.functions.insert(id.name.to_string(), self.source[function.span.start as usize..function.span.end as usize].into());
        }
        walk::walk_statement(self, node);
    }

    fn visit_jsx_element(&mut self, node: &JSXElement<'a>) {
        let opening = &node.opening_element;
        let tag = opening.name.get_identifier_name().map(|name| name.as_str()).unwrap_or_default();
        let attribute = |name: &str| opening.attributes.iter().find_map(|item| match item {
            JSXAttributeItem::Attribute(value) if attr_name(item) == Some(name) => value.value.as_ref(),
            _ => None,
        });
        let has = |name: &str| opening.attributes.iter().any(|item| attr_name(item) == Some(name));
        let effective = attribute("as").and_then(value_text).unwrap_or_else(|| tag.into());
        let spread = opening.attributes.iter().any(|item| matches!(item, JSXAttributeItem::SpreadAttribute(_)));
        let form = effective == "form";
        let submits = form && (has("onSubmit") || has("action"));
        if let Some(handler) = attribute("onSubmit") {
            let text = value_text(handler).unwrap_or_else(|| {
                use oxc_span::GetSpan;
                let span = handler.span();
                self.source[span.start as usize..span.end as usize].into()
            });
            self.forms.push((opening.span, text));
        }
        let links = matches!(effective.as_str(), "a" | "Link") || has("href");
        if !spread && !has("disabled") && !has("readOnly") && self.link_depth == 0 {
            let input = matches!(effective.as_str(), "input" | "Input" | "select" | "Select" | "textarea" | "Textarea");
            let button = matches!(effective.as_str(), "button" | "Button");
            let submit = attribute("type").and_then(value_text).is_none_or(|kind| kind == "submit") && self.submit_depth > 0;
            if (input && !["value", "onChange", "name", "defaultValue", "checked", "onInput", "props"].iter().any(|name| has(name)))
                || (button && !submit && !["onClick", "href", "formAction", "form", "props"].iter().any(|name| has(name)))
            {
                self.found.push(error(opening.span, "dead-control", "Visible control has no value, form binding, handler or destination."));
            }
        }
        self.submit_depth += usize::from(submits);
        self.link_depth += usize::from(links);
        walk::walk_jsx_element(self, node);
        self.link_depth -= usize::from(links);
        self.submit_depth -= usize::from(submits);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_controls_and_deferred_comments_are_errors_not_string_matches() {
        let source = r#"// TODO wire API
const label = 'TODO';
const view = <><input/><button>Sort</button><input name="q"/><button onClick={sort}>Sort</button></>;"#;
        let rules = findings(source).into_iter().map(|item| item.rule).collect::<Vec<_>>();
        assert_eq!(rules, ["deferred-wiring", "dead-control", "dead-control"]);
    }

    #[test]
    fn a_control_rendered_inside_a_link_takes_the_link_destination() {
        let source = r#"const view = <><Link href="/products"><Button text1="Go" /></Link><a href="/x"><button>X</button></a><Button text1="Dead" /></>;"#;
        let rules = findings(source).into_iter().map(|item| item.rule).collect::<Vec<_>>();
        assert_eq!(rules, ["dead-control"]);
    }

    #[test]
    fn imported_utils_records_fail_but_navigation_strings_do_not() {
        let mut files = vec![crate::ui_validate::SourceFile { path: "src/utils/news.ts".into(), content: "export const news = [{ id: 1, title: 'News' }];".into() }];
        let source = "import { news } from '@/utils/news'; export default function NewsPage(){return <main>{news.length}</main>}";
        assert_eq!(fixture_findings(&files, "src/app/news/page.tsx", source)[0].rule, "fixture-instead-of-api");
        files[0].content = "export const news = ['Home', 'About'];".into();
        assert!(fixture_findings(&files, "src/app/news/page.tsx", source).is_empty());
    }

    #[test]
    fn submit_success_requires_a_request_in_its_handler_not_elsewhere() {
        let source = r#"const load = () => fetch('/news');
const submit = (event) => { event.preventDefault(); setSuccess(true); };
const view = <form onSubmit={submit}><button type="submit">Send</button></form>;"#;
        assert!(findings(source).iter().any(|item| item.rule == "fake-submit"));
        assert!(!findings(&source.replace("setSuccess(true)", "await fetch('/contact', { method: 'POST' }); setSuccess(true)").replace("(event) =>", "async (event) =>")).iter().any(|item| item.rule == "fake-submit"));
    }
}
