use super::*;
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Expression, JSXAttribute, JSXAttributeValue, ObjectProperty, StringLiteral, TemplateLiteral,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

pub(super) fn scan(
    file: &str,
    source: &str,
) -> (Vec<Reference>, Vec<DynamicReference>, Vec<Value>) {
    let extension = Path::new(file)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if ["ts", "tsx", "js", "jsx", "mjs", "cjs"].contains(&extension) {
        return javascript(file, source);
    }
    let mut references = Vec::new();
    let mut diagnostics = Vec::new();
    let mut cursor = 0;
    let bytes = source.as_bytes();
    while cursor < bytes.len() {
        let quote = bytes[cursor];
        if extension == "rs" && quote == b'\'' {
            cursor += 1;
            continue;
        }
        if quote != b'"' && quote != b'\'' {
            cursor += 1;
            continue;
        }
        let start = cursor;
        cursor += 1;
        while cursor < bytes.len() {
            if bytes[cursor] == b'\\' {
                cursor = (cursor + 2).min(bytes.len());
                continue;
            }
            if bytes[cursor] == quote {
                if extension == "sql" && cursor + 1 < bytes.len() && bytes[cursor + 1] == quote {
                    cursor += 2;
                    continue;
                }
                break;
            }
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        cursor += 1;
        let raw = &source[start..cursor];
        let value = if quote == b'"' {
            serde_json::from_str::<String>(raw).ok()
        } else if extension == "sql" {
            Some(raw[1..raw.len() - 1].replace("''", "'"))
        } else {
            (!raw[1..raw.len() - 1].contains('\\')).then(|| raw[1..raw.len() - 1].to_owned())
        };
        if let Some(url) = value.filter(|url| url.starts_with('/') && !url.starts_with("//")) {
            references.push(Reference {
                file: file.into(),
                url,
                start,
                end: cursor,
                syntax: if extension == "sql" {
                    "sql-literal"
                } else {
                    "json-literal"
                }
                .into(),
            });
        }
    }
    // CSS url() and Markdown links carry exact URL token ranges too.
    for marker in ["url(", "]("] {
        for (at, _) in source.match_indices(marker) {
            let rest = &source[at + marker.len()..];
            let leading = rest.len() - rest.trim_start().len();
            let rest = rest.trim_start();
            let quoted = rest.starts_with(['\'', '"']);
            let start = at + marker.len() + leading + usize::from(quoted);
            let Some(end) = source[start..].find([')', '\'', '"']) else {
                continue;
            };
            let end = start + end;
            let url = source[start..end].trim_end();
            if url.starts_with('/')
                && !references
                    .iter()
                    .any(|reference| reference.start <= start && reference.end >= end)
            {
                references.push(Reference {
                    file: file.into(),
                    url: url.into(),
                    start,
                    end: start + url.len(),
                    syntax: "bare-url".into(),
                });
            }
        }
    }
    for (url, _) in super::super::asset_guard::asset_urls(source) {
        if !references.iter().any(|r| r.url == url) {
            diagnostics.push(json!({"file":file,"url":url,"reason":"asset-reference-syntax-needs-review","rewritten":false}));
        }
    }
    (references, Vec::new(), diagnostics)
}

fn javascript(file: &str, source: &str) -> (Vec<Reference>, Vec<DynamicReference>, Vec<Value>) {
    struct Visitor<'s> {
        file: &'s str,
        source: &'s str,
        references: Vec<Reference>,
        dynamic: Vec<DynamicReference>,
    }
    impl<'a> Visit<'a> for Visitor<'_> {
        fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
            let url = literal.value.as_str();
            if url.len() > 4096 {
                self.dynamic.push(DynamicReference {
                    file: self.file.into(),
                    start: literal.span.start as usize,
                    reason: "oversized string literal; inspect its runtime asset use".into(),
                });
                return;
            }
            if url.starts_with('/') && !url.starts_with("//") {
                self.references.push(Reference {
                    file: self.file.into(),
                    url: url.into(),
                    start: literal.span.start as usize,
                    end: literal.span.end as usize,
                    syntax: "js-literal".into(),
                });
            } else if let Some(url) = css_url(url) {
                self.references.push(Reference {
                    file: self.file.into(),
                    url,
                    start: literal.span.start as usize,
                    end: literal.span.end as usize,
                    syntax: "js-css-url".into(),
                });
            } else if url.contains("url(") && url.contains('/') {
                self.dynamic.push(DynamicReference {
                    file: self.file.into(),
                    start: literal.span.start as usize,
                    reason: "compound CSS image expression requires reference review".into(),
                });
            }
            walk::walk_string_literal(self, literal);
        }
        fn visit_jsx_attribute(&mut self, attribute: &JSXAttribute<'a>) {
            let span = attribute.name.span();
            let name = &self.source[span.start as usize..span.end as usize];
            if name == "srcSet" {
                self.dynamic.push(DynamicReference{file:self.file.into(),start:span.start as usize,reason:"srcSet compound URLs require an explicit reference audit before source retirement".into()});
            }
            if ["src", "srcSet", "maskImage", "backgroundImage", "poster"].contains(&name)
                && let Some(JSXAttributeValue::ExpressionContainer(expression)) = &attribute.value
                && !matches!(
                    expression.expression.as_expression(),
                    Some(Expression::StringLiteral(_))
                )
            {
                self.dynamic.push(DynamicReference {
                    file: self.file.into(),
                    start: span.start as usize,
                    reason: format!("dynamic-{name}-expression; runtime URL coverage is unproven"),
                });
            }
            walk::walk_jsx_attribute(self, attribute);
        }
        fn visit_template_literal(&mut self, literal: &TemplateLiteral<'a>) {
            if !literal.expressions.is_empty() {
                let span = literal.span;
                let source = &self.source[span.start as usize..span.end as usize];
                if source.contains("/images/")
                    || source.contains("/icons/")
                    || source.contains("/public/")
                {
                    self.dynamic.push(DynamicReference {
                        file: self.file.into(),
                        start: span.start as usize,
                        reason: "computed-asset-template; exact runtime URLs are unresolved".into(),
                    });
                }
            } else {
                let span = literal.span;
                let raw = &self.source[span.start as usize + 1..span.end as usize - 1];
                if raw.starts_with('/') && !raw.contains('\\') {
                    self.references.push(Reference {
                        file: self.file.into(),
                        url: raw.into(),
                        start: span.start as usize,
                        end: span.end as usize,
                        syntax: "js-literal".into(),
                    });
                }
            }
            walk::walk_template_literal(self, literal);
        }
        fn visit_object_property(&mut self, property: &ObjectProperty<'a>) {
            if let Some(name) = property.key.static_name()
                && ["src", "maskImage", "backgroundImage", "poster"].contains(&name.as_ref())
                && !matches!(
                    property.value,
                    Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
                )
            {
                self.dynamic.push(DynamicReference {
                    file: self.file.into(),
                    start: property.span.start as usize,
                    reason: format!("dynamic-{name}-property; exact URLs are unresolved"),
                });
            }
            walk::walk_object_property(self, property);
        }
    }
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(file).unwrap_or_else(|_| SourceType::tsx());
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let diagnostics = parsed
        .diagnostics
        .iter()
        .map(|error| json!({"file":file,"reason":"source-parse-failed","detail":error.to_string()}))
        .collect();
    let mut visitor = Visitor {
        file,
        source,
        references: Vec::new(),
        dynamic: Vec::new(),
    };
    visitor.visit_program(&parsed.program);
    visitor.references.sort_by_key(|r| r.start);
    visitor.references.dedup_by_key(|r| r.start);
    (visitor.references, visitor.dynamic, diagnostics)
}

pub(super) fn replacement(reference: &Reference, url: &str) -> String {
    if reference.syntax == "bare-url" {
        url.into()
    } else if reference.syntax == "js-css-url" {
        let safe = url
            .chars()
            .map(|c| {
                if c.is_ascii_whitespace() || matches!(c, '\'' | '"' | '(' | ')') {
                    format!("%{:02X}", u32::from(c))
                } else {
                    c.to_string()
                }
            })
            .collect::<String>();
        serde_json::to_string(&format!("url({safe})")).unwrap()
    } else if reference.syntax == "sql-literal" {
        format!("'{}'", url.replace('\'', "''"))
    } else {
        serde_json::to_string(url).expect("URL string")
    }
}

fn css_url(value: &str) -> Option<String> {
    let inner = value
        .strip_prefix("url(")?
        .strip_suffix(')')?
        .trim()
        .trim_matches(['\'', '"']);
    (inner.starts_with('/') && !inner.contains(['(', ')'])).then(|| inner.into())
}
