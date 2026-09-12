//! Bounded token parsing for stack drift evidence. This is deliberately not
//! macro expansion, name resolution, or a Rust/TypeScript compiler front end.

use std::collections::BTreeSet;

/// Direct declarations, kept separate from configuration-dependent evidence.
#[derive(Default)]
pub(in crate::server) struct RouteAttributes {
    pub(in crate::server) routes: Vec<RouteAttribute>,
    pub(in crate::server) unresolved: bool,
    /// Out-of-line modules whose descendants inherit a configuration gate.
    pub(in crate::server) conditional_modules: Vec<Vec<String>>,
}

pub(in crate::server) struct RouteAttribute {
    pub(in crate::server) method: String,
    pub(in crate::server) path: Option<String>,
    pub(in crate::server) conditional: bool,
}

struct RouteArgs {
    method: String,
    path: Option<String>,
}

impl syn::parse::Parse for RouteArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut method = None;
        let mut path = None;
        while !input.is_empty() {
            let name: syn::Ident = input.parse()?;
            let key = name.to_string().to_ascii_lowercase();
            if http_method(&key) || key == "trace" {
                if method.replace(key).is_some() {
                    return Err(input.error("duplicate HTTP method"));
                }
            } else if key == "path" {
                input.parse::<syn::Token![=]>()?;
                let value: syn::LitStr = input.parse()?;
                if path.replace(value.value()).is_some() {
                    return Err(input.error("duplicate route path"));
                }
            } else if key != "deprecated" {
                input.parse::<syn::Token![=]>()?;
                // Other options cannot supply the method or path. Skip one
                // balanced token value, without interpreting nested strings,
                // expressions or Vespera-specific header/response syntax.
                input.step(|cursor| {
                    let mut rest = *cursor;
                    let mut consumed = false;
                    while let Some((tree, next)) = rest.token_tree() {
                        if rest.punct().is_some_and(|(p, _)| p.as_char() == ',') {
                            break;
                        }
                        drop(tree);
                        consumed = true;
                        rest = next;
                    }
                    if consumed {
                        Ok(((), rest))
                    } else {
                        Err(cursor.error("missing route option value"))
                    }
                })?;
            }
            if !input.is_empty() {
                input.parse::<syn::Token![,]>()?;
            }
        }
        Ok(Self {
            method: method.ok_or_else(|| input.error("unresolved HTTP method"))?,
            path,
        })
    }
}

fn configuration_gated(attributes: &[syn::Attribute]) -> bool {
    attributes
        .iter()
        .any(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
}

fn is_route_attribute(attribute: &syn::Attribute) -> bool {
    let path = attribute.path();
    path.is_ident("route")
        || (path.segments.len() == 2
            && path.segments[0].ident == "vespera"
            && path.segments[1].ident == "route")
}

/// Parse Rust items, not arbitrary token substrings: comments, literal
/// examples, macro bodies and nested function bodies are not declarations.
/// No macro expansion, cfg evaluation or import/name resolution is attempted.
pub(in crate::server) fn route_attributes(source: &str) -> RouteAttributes {
    fn visit(
        items: &[syn::Item],
        inherited_gate: bool,
        modules: &[String],
        result: &mut RouteAttributes,
    ) {
        for item in items {
            match item {
                syn::Item::Fn(handler)
                    if matches!(handler.vis, syn::Visibility::Public(_))
                        && handler.sig.asyncness.is_some() =>
                {
                    let conditional = inherited_gate || configuration_gated(&handler.attrs);
                    for attribute in &handler.attrs {
                        if is_route_attribute(attribute) {
                            match attribute.parse_args::<RouteArgs>() {
                                Ok(args) => result.routes.push(RouteAttribute {
                                    method: args.method,
                                    path: args.path,
                                    conditional,
                                }),
                                Err(_) => result.unresolved = true,
                            }
                        } else if attribute.path().is_ident("cfg_attr") {
                            // cfg_attr can introduce a route macro itself.
                            result.unresolved = true;
                        }
                    }
                }
                syn::Item::Mod(module) => {
                    let conditional = inherited_gate || configuration_gated(&module.attrs);
                    let mut nested = modules.to_vec();
                    nested.push(module.ident.to_string());
                    if let Some((_, items)) = &module.content {
                        visit(items, conditional, &nested, result);
                    } else if conditional {
                        result.conditional_modules.push(nested);
                    }
                }
                _ => {}
            }
        }
    }

    let mut result = RouteAttributes::default();
    match syn::parse_file(source) {
        Ok(file) => visit(
            &file.items,
            configuration_gated(&file.attrs),
            &[],
            &mut result,
        ),
        Err(_) => result.unresolved = true,
    }
    result
}

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Literal(String),
    Symbol(char),
}

impl Token {
    fn word(&self) -> Option<&str> {
        if let Self::Word(value) = self {
            Some(value)
        } else {
            None
        }
    }

    fn literal(&self) -> Option<&str> {
        if let Self::Literal(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

fn word(tokens: &[Token], at: usize, expected: &str) -> bool {
    tokens.get(at).and_then(Token::word) == Some(expected)
}

fn symbol(tokens: &[Token], at: usize, expected: char) -> bool {
    tokens.get(at) == Some(&Token::Symbol(expected))
}

/// Comments (including nested Rust block comments) are discarded; strings
/// remain opaque tokens, so examples inside them cannot become code evidence.
fn tokenize(source: &str, rust: bool) -> Vec<Token> {
    let chars = source.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            let mut depth = 1;
            while i < chars.len() && depth > 0 {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if rust && c == 'r' {
            let mut quote = i + 1;
            while chars.get(quote) == Some(&'#') {
                quote += 1;
            }
            if chars.get(quote) == Some(&'"') {
                let hashes = quote - i - 1;
                i = quote + 1;
                let start = i;
                while i < chars.len() {
                    if chars[i] == '"' && (1..=hashes).all(|n| chars.get(i + n) == Some(&'#')) {
                        break;
                    }
                    i += 1;
                }
                tokens.push(Token::Literal(chars[start..i].iter().collect()));
                i = (i + hashes + 1).min(chars.len());
                continue;
            }
        }
        // Rust lifetimes are not single-quoted strings.
        let lifetime = rust
            && c == '\''
            && chars
                .get(i + 1)
                .is_some_and(|c| c.is_alphabetic() || *c == '_')
            && {
                let mut end = i + 1;
                while chars
                    .get(end)
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_')
                {
                    end += 1;
                }
                chars.get(end) != Some(&'\'')
            };
        if matches!(c, '\'' | '"' | '`') && !lifetime {
            i += 1;
            let mut value = String::new();
            while i < chars.len() && chars[i] != c {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                value.push(chars[i]);
                i += 1;
            }
            i = (i + 1).min(chars.len());
            // Template expressions are opaque, not literal references.
            tokens.push(if c == '`' {
                Token::Symbol('`')
            } else {
                Token::Literal(value)
            });
            continue;
        }
        if c.is_alphabetic() || matches!(c, '_' | '$') {
            if rust
                && c == 'r'
                && chars.get(i + 1) == Some(&'#')
                && chars
                    .get(i + 2)
                    .is_some_and(|c| c.is_alphabetic() || *c == '_')
            {
                i += 2;
            }
            let start = i;
            i += 1;
            while chars
                .get(i)
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
            {
                i += 1;
            }
            tokens.push(Token::Word(chars[start..i].iter().collect()));
        } else {
            tokens.push(Token::Symbol(c));
            i += 1;
        }
    }
    tokens
}

fn group_end(tokens: &[Token], start: usize) -> Option<usize> {
    let close = match tokens.get(start)? {
        Token::Symbol('(') => ')',
        Token::Symbol('[') => ']',
        Token::Symbol('{') => '}',
        _ => return None,
    };
    let mut i = start + 1;
    while i < tokens.len() {
        if symbol(tokens, i, close) {
            return Some(i);
        }
        if matches!(tokens[i], Token::Symbol('(' | '[' | '{')) {
            i = group_end(tokens, i)?;
        } else if matches!(tokens[i], Token::Symbol(')' | ']' | '}')) {
            return None;
        }
        i += 1;
    }
    None
}

fn comma_parts(tokens: &[Token]) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < tokens.len() {
        if symbol(tokens, i, ',') {
            parts.push(&tokens[start..i]);
            start = i + 1;
        } else if let Some(end) = group_end(tokens, i) {
            i = end;
        }
        i += 1;
    }
    if start < tokens.len() {
        parts.push(&tokens[start..]);
    }
    parts
}

enum Columns {
    All,
    Pick(BTreeSet<String>),
    Omit(BTreeSet<String>),
    Variant(String),
}

#[derive(Default)]
pub(in crate::server) struct RouteMapping {
    references: Vec<(String, Columns)>,
    pub(in crate::server) unresolved: bool,
}

impl RouteMapping {
    pub(in crate::server) fn mentions(&self, table: &str, column: &str) -> bool {
        self.references.iter().any(|(model, columns)| {
            model == table
                && match columns {
                    Columns::All => true,
                    Columns::Pick(names) => names.contains(column),
                    Columns::Omit(names) => !names.contains(column),
                    Columns::Variant(name) => name == &super::snake_to_pascal(column),
                }
        })
    }
}

fn model_path(tokens: &[Token], i: usize) -> Option<&str> {
    (word(tokens, i, "crate")
        && symbol(tokens, i + 1, ':')
        && symbol(tokens, i + 2, ':')
        && word(tokens, i + 3, "models")
        && symbol(tokens, i + 4, ':')
        && symbol(tokens, i + 5, ':')
        && symbol(tokens, i + 7, ':')
        && symbol(tokens, i + 8, ':'))
    .then(|| tokens.get(i + 6).and_then(Token::word))
    .flatten()
}

fn schema_reference(tokens: &[Token]) -> Option<(String, Columns)> {
    tokens.first()?.word()?;
    if !word(tokens, 1, "from") || !word(tokens, 11, "Model") {
        return None;
    }
    let table = model_path(tokens, 2)?.to_owned();
    let rest = &tokens[12..];
    if rest.is_empty() {
        return Some((table, Columns::All));
    }
    if !symbol(rest, 0, ',') {
        return None;
    }
    let clauses = comma_parts(&rest[1..]);
    if clauses.len() != 1 {
        return None;
    }
    let clause = clauses[0];
    let mode = clause.first()?.word()?;
    if !symbol(clause, 1, '=') || !symbol(clause, 2, '[') {
        return None;
    }
    let end = group_end(clause, 2)?;
    if end + 1 != clause.len() {
        return None;
    }
    let names = comma_parts(&clause[3..end])
        .into_iter()
        .map(|part| {
            if part.len() == 1 {
                part[0].word().map(str::to_owned)
            } else {
                None
            }
        })
        .collect::<Option<BTreeSet<_>>>()?;
    Some((
        table,
        match mode {
            "pick" => Columns::Pick(names),
            "omit" => Columns::Omit(names),
            _ => return None,
        },
    ))
}

pub(in crate::server) fn route_mapping(source: &str) -> RouteMapping {
    let tokens = tokenize(source, true);
    let mut mapping = RouteMapping::default();
    let mut i = 0;
    while i < tokens.len() {
        if word(&tokens, i, "schema_type") && symbol(&tokens, i + 1, '!') {
            if let Some(end) = group_end(&tokens, i + 2) {
                if let Some(reference) = schema_reference(&tokens[i + 3..end]) {
                    mapping.references.push(reference);
                } else {
                    mapping.unresolved = true;
                }
                // Never interpret the macro's Model source as an additional
                // whole-model use: that would defeat pick/omit.
                i = end + 1;
                continue;
            }
            mapping.unresolved = true;
            // An unfinished macro body is not an independent Model use.
            break;
        }
        if let Some(table) = model_path(&tokens, i) {
            let kind = i + 9;
            if word(&tokens, kind, "Model") || word(&tokens, kind, "Entity") {
                mapping.references.push((table.to_owned(), Columns::All));
            } else if word(&tokens, kind, "Column") {
                if symbol(&tokens, kind + 1, ':')
                    && symbol(&tokens, kind + 2, ':')
                    && let Some(variant) = tokens.get(kind + 3).and_then(Token::word)
                {
                    mapping
                        .references
                        .push((table.to_owned(), Columns::Variant(variant.to_owned())));
                } else {
                    mapping.unresolved = true;
                }
            } else if symbol(&tokens, kind, '{')
                && let Some(end) = group_end(&tokens, kind)
            {
                let imports = comma_parts(&tokens[kind + 1..end]);
                if imports
                    .iter()
                    .any(|part| word(part, 0, "Model") || word(part, 0, "Entity"))
                {
                    mapping.references.push((table.to_owned(), Columns::All));
                } else {
                    mapping.unresolved = true;
                }
                i = end;
            }
        }
        i += 1;
    }
    mapping.unresolved |= mapping.references.is_empty();
    mapping
}

fn http_method(method: &str) -> bool {
    matches!(
        method.to_ascii_lowercase().as_str(),
        "get" | "post" | "put" | "patch" | "delete" | "head" | "options"
    )
}

fn method_path(tokens: &[Token]) -> Option<String> {
    let parts = comma_parts(tokens);
    if parts.len() < 2 || parts[0].len() != 1 || parts[1].len() != 1 {
        return None;
    }
    http_method(parts[0][0].literal()?)
        .then(|| parts[1][0].literal().map(str::to_owned))
        .flatten()
}

fn config_name(tokens: &[Token]) -> Option<String> {
    if tokens.len() == 3 && word(tokens, 0, "crudConfigs") && symbol(tokens, 1, '.') {
        tokens[2].word().map(str::to_owned)
    } else {
        None
    }
}

fn object_config(tokens: &[Token]) -> Option<String> {
    comma_parts(tokens).into_iter().find_map(|part| {
        (word(part, 0, "config") && symbol(part, 1, ':'))
            .then(|| config_name(&part[2..]))
            .flatten()
    })
}

/// Returns literal operation references separately from CRUD configuration
/// references, which must first be expanded through OpenAPI's devup tags.
type References = Vec<(String, String)>;

pub(in crate::server) fn client_references(source: &str) -> (References, References) {
    let tokens = tokenize(source, false);
    let mut calls = Vec::new();
    let mut configs = Vec::new();
    for i in 0..tokens.len() {
        let Some(name) = tokens[i].word() else {
            continue;
        };
        if name == "api"
            && symbol(&tokens, i + 1, '.')
            && let Some(method) = tokens.get(i + 2).and_then(Token::word)
            && http_method(method)
            && symbol(&tokens, i + 3, '(')
            && let Some(end) = group_end(&tokens, i + 3)
            && let Some(first) = comma_parts(&tokens[i + 4..end]).first()
            && first.len() == 1
            && let Some(path) = first[0].literal()
        {
            calls.push((format!("api.{method}("), path.to_owned()));
        }
        if matches!(
            name,
            "useQuery" | "useMutation" | "useSuspenseQuery" | "useInfiniteQuery" | "getQueryKey"
        ) && symbol(&tokens, i + 1, '(')
            && let Some(end) = group_end(&tokens, i + 1)
            && let Some(path) = method_path(&tokens[i + 2..end])
        {
            calls.push((format!("{name}("), path));
        }
        if name == "useQueries"
            && symbol(&tokens, i + 1, '(')
            && symbol(&tokens, i + 2, '[')
            && let Some(end) = group_end(&tokens, i + 2)
        {
            for entry in comma_parts(&tokens[i + 3..end]) {
                if symbol(entry, 0, '[')
                    && let Some(end) = group_end(entry, 0)
                    && let Some(path) = method_path(&entry[1..end])
                {
                    calls.push(("useQueries(".into(), path));
                }
            }
        }
        if name == "pathSchemas"
            && symbol(&tokens, i + 1, '.')
            && tokens
                .get(i + 2)
                .and_then(Token::word)
                .is_some_and(http_method)
            && symbol(&tokens, i + 3, '[')
            && symbol(&tokens, i + 5, ']')
            && let Some(path) = tokens.get(i + 4).and_then(Token::literal)
        {
            calls.push(("pathSchemas".into(), path.to_owned()));
        }
        if name == "import"
            && symbol(&tokens, i + 1, '{')
            && let Some(end) = group_end(&tokens, i + 1)
            && word(&tokens, end + 1, "from")
            && tokens.get(end + 2).and_then(Token::literal) == Some("@devup-api/fetch/server")
        {
            for part in comma_parts(&tokens[i + 2..end]) {
                if let Some(operation) = part.first().and_then(Token::word)
                    && operation != "type"
                {
                    calls.push(("@devup-api/fetch/server".into(), operation.to_owned()));
                }
            }
        }
        if name == "useApiCrud"
            && symbol(&tokens, i + 1, '(')
            && symbol(&tokens, i + 2, '{')
            && let Some(end) = group_end(&tokens, i + 2)
            && let Some(config) = object_config(&tokens[i + 3..end])
        {
            configs.push(("useApiCrud(".into(), config));
        }
        if matches!(name, "ApiForm" | "ApiCrud") && i > 0 && symbol(&tokens, i - 1, '<') {
            let mut at = i + 1;
            let mut method = None;
            let mut path = None;
            let mut config = None;
            while at < tokens.len() && !symbol(&tokens, at, '>') {
                if symbol(&tokens, at + 1, '=') {
                    if word(&tokens, at, "method") {
                        method = tokens.get(at + 2).and_then(Token::literal);
                    }
                    if word(&tokens, at, "path") {
                        path = tokens.get(at + 2).and_then(Token::literal);
                    }
                    if word(&tokens, at, "config")
                        && symbol(&tokens, at + 2, '{')
                        && let Some(end) = group_end(&tokens, at + 2)
                    {
                        config = config_name(&tokens[at + 3..end]);
                    }
                }
                if let Some(end) = group_end(&tokens, at) {
                    at = end;
                }
                at += 1;
            }
            if name == "ApiForm"
                && matches!(method, Some("post" | "put" | "patch" | "delete"))
                && let Some(path) = path
            {
                calls.push(("ApiForm".into(), path.to_owned()));
            }
            if name == "ApiCrud"
                && let Some(config) = config
            {
                configs.push(("ApiCrud".into(), config));
            }
        }
    }
    (calls, configs)
}
