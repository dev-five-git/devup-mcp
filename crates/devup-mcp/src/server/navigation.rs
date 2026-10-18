//! Static navigation checks share the UI inventory; they never guess destinations.
use std::collections::{BTreeMap, BTreeSet};

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::{Value, json};

fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn literal(a: &JSXAttribute<'_>) -> Option<String> {
    match a.value.as_ref()? {
        JSXAttributeValue::StringLiteral(s) => Some(s.value.to_string()),
        JSXAttributeValue::ExpressionContainer(c) => match &c.expression {
            JSXExpression::StringLiteral(s) => Some(s.value.to_string()),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Default)]
struct Collector<'s> {
    source: &'s str,
    linked_ancestors: usize,
    link_ancestors: usize,
    form_ancestors: usize,
    nav_ancestors: usize,
    link_names: BTreeSet<String>,
    elements: Vec<Value>,
    diagnostics: Vec<Value>,
}

// Only literal visual markers on a navigation candidate warrant this hint.
// A marker can be decorative; its presence never establishes active state.
fn fixed_visual_marker(attrs: &BTreeMap<&str, &JSXAttribute<'_>>) -> bool {
    let mut values = BTreeMap::<String, String>::new();
    for (key, attribute) in attrs {
        if let Some(value) = literal(attribute) {
            values.insert((*key).into(), value);
        } else if let Some(JSXAttributeValue::ExpressionContainer(container)) = &attribute.value {
            match &container.expression {
                JSXExpression::NumericLiteral(n) => {
                    values.insert((*key).into(), n.value.to_string());
                }
                JSXExpression::ObjectExpression(object) if *key == "style" => {
                    // Spread/computed properties can override a literal marker.
                    if object
                        .properties
                        .iter()
                        .any(|p| !matches!(p, ObjectPropertyKind::ObjectProperty(p) if !p.computed))
                    {
                        continue;
                    }
                    for property in &object.properties {
                        if let ObjectPropertyKind::ObjectProperty(property) = property
                            && let Some(key) = property.key.static_name()
                        {
                            match &property.value {
                                Expression::StringLiteral(s) => {
                                    values.insert(key.into_owned(), s.value.to_string());
                                }
                                Expression::NumericLiteral(n) => {
                                    values.insert(key.into_owned(), n.value.to_string());
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    values.get("borderBottom").is_some_and(|v| {
        v.split_whitespace()
            .any(|width| ["1px", "2px", "3px", "4px"].contains(&width))
            && v.split_whitespace().any(|s| s == "solid")
            && !v.contains("transparent")
    }) || (values
        .get("borderBottomWidth")
        .is_some_and(|v| ["1", "2", "3", "4", "1px", "2px", "3px", "4px"].contains(&v.as_str()))
        && values
            .get("borderBottomColor")
            .is_some_and(|v| !v.is_empty() && v != "transparent"))
}

fn boolean(a: &JSXAttribute<'_>) -> Option<bool> {
    match &a.value {
        None => Some(true),
        Some(JSXAttributeValue::ExpressionContainer(c)) => match &c.expression {
            JSXExpression::BooleanLiteral(b) => Some(b.value),
            _ => None,
        },
        Some(JSXAttributeValue::StringLiteral(s)) => match s.value.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_import_declaration(&mut self, declaration: &ImportDeclaration<'a>) {
        if declaration.source.value == "next/link" {
            for specifier in declaration.specifiers.iter().flatten() {
                if let ImportDeclarationSpecifier::ImportDefaultSpecifier(s) = specifier {
                    self.link_names.insert(s.local.name.to_string());
                }
            }
        }
        walk::walk_import_declaration(self, declaration);
    }

    fn visit_jsx_element(&mut self, element: &JSXElement<'a>) {
        let name = element.opening_element.name.span().source_text(self.source);
        let attrs: BTreeMap<_, _> = element
            .opening_element
            .attributes
            .iter()
            .filter_map(|a| {
                if let JSXAttributeItem::Attribute(a) = a {
                    Some((a.name.span().source_text(self.source), a.as_ref()))
                } else {
                    None
                }
            })
            .collect();
        let href = attrs.get("href").and_then(|a| literal(a));
        let is_link = name == "a" || self.link_names.contains(name);
        let native = matches!(name, "button" | "input" | "select" | "textarea")
            && !attrs
                .get("type")
                .and_then(|a| literal(a))
                .is_some_and(|s| s == "hidden");
        let handler = attrs
            .keys()
            .any(|k| matches!(*k, "onClick" | "onChange" | "onSubmit"));
        let static_target = href
            .as_deref()
            .is_some_and(|h| !h.trim().is_empty() && h != "#" && !h.starts_with("javascript:"));
        let disabled = attrs.get("disabled").and_then(|a| boolean(a)) == Some(true);
        let native_toggle = name == "input"
            && attrs
                .get("type")
                .and_then(|a| literal(a))
                .is_some_and(|s| ["checkbox", "radio"].contains(&s.as_str()));
        let form_control = native
            && self.form_ancestors > 0
            && (name != "button"
                || attrs
                    .get("type")
                    .and_then(|a| literal(a))
                    .is_none_or(|t| t == "submit"));
        let wired = (is_link && static_target) || (native && handler) || form_control;
        let form_action =
            name == "form" && (attrs.contains_key("action") || attrs.contains_key("onSubmit"));
        let own_text = element
            .children
            .iter()
            .filter_map(|c| {
                if let JSXChild::Text(t) = c {
                    Some(t.value.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let accessible_name = attrs
            .get("aria-label")
            .and_then(|a| literal(a))
            .unwrap_or(own_text);
        let role = attrs.get("role").and_then(|a| literal(a));
        let nav = name == "nav"
            || role.as_deref() == Some("navigation")
            || matches!(
                super::interactions::label_role(name),
                Some(super::interactions::Role::Nav)
            );
        let candidate = is_link
            || native
            || handler
            || role
                .as_deref()
                .is_some_and(|r| ["link", "button", "tab", "checkbox", "radio"].contains(&r))
            || super::interactions::label_role(name).is_some()
            || attrs
                .get("data-name")
                .and_then(|a| literal(a))
                .is_some_and(|s| super::interactions::label_role(&s).is_some())
            || (accessible_name.chars().count() <= 35
                && !accessible_name.contains(['.', '!', '?'])
                && super::interactions::label_role(accessible_name.trim()).is_some());
        let line = self.source[..element.span.start as usize]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
            + 1;
        let active = attrs
            .get("aria-current")
            .is_some_and(|a| literal(a).is_some_and(|s| s != "false"))
            || ["data-active", "aria-selected"]
                .iter()
                .any(|k| attrs.get(k).is_some_and(|a| boolean(a) == Some(true)))
            || attrs.get("data-state").is_some_and(|a| {
                literal(a).is_some_and(|s| ["active", "selected"].contains(&s.as_str()))
            });
        let visual_marker = !active
            && ((candidate
                && (self.nav_ancestors > 0 || nav || is_link || role.as_deref() == Some("tab")))
                || self.link_ancestors > 0)
            && fixed_visual_marker(&attrs);
        if candidate || active || visual_marker {
            self.elements.push(json!({"element":name,"name":accessible_name.trim(),"line":line,"offset":element.span.start,
                "candidate":candidate,"wired":wired || self.linked_ancestors>0,"href":href,"dynamicHref":attrs.contains_key("href") && href.is_none(),
                "nodeId":attrs.get("data-figma-node-id").and_then(|a|literal(a)),
                "semanticControl":is_link || native,"nativeToggleOnly":native_toggle && !wired,"disabled":disabled,
                "staticActive":active,"nonSemanticClick":handler && matches!(name,"div"|"span"|"Box"|"Flex"|"Text"|"Center"|"Grid"),
                "fixedVisualNavigationMarker":visual_marker,
                "ancestorControl":self.linked_ancestors>0,"isLink":is_link}));
        }
        // A dynamic Link is still a semantic ancestor, but its destination remains unresolved.
        let parent_control = is_link || native;
        self.linked_ancestors += usize::from(parent_control);
        self.link_ancestors += usize::from(is_link);
        self.form_ancestors += usize::from(form_action);
        self.nav_ancestors += usize::from(nav);
        walk::walk_jsx_element(self, element);
        self.linked_ancestors -= usize::from(parent_control);
        self.link_ancestors -= usize::from(is_link);
        self.form_ancestors -= usize::from(form_action);
        self.nav_ancestors -= usize::from(nav);
    }
}

pub(super) fn source_evidence(source: &str) -> Value {
    if source.len() > 2_097_152 {
        return json!({"elements":[],"diagnostics":[{"reason":"Source exceeds 2 MiB; navigation parsing skipped."}],"parsed":false});
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() {
        return json!({"elements":[],"diagnostics":[{"reason":"TSX parse failed; navigation coverage is unverified."}],"parsed":false});
    }
    let mut collector = Collector {
        source,
        ..Default::default()
    };
    // Imports may follow declarations; collect aliases before traversing JSX.
    for statement in &parsed.program.body {
        if let Statement::ImportDeclaration(d) = statement {
            collector.visit_import_declaration(d);
        }
    }
    collector.visit_program(&parsed.program);
    json!({"elements":collector.elements,"diagnostics":collector.diagnostics,"parsed":true})
}

/// Concrete hrefs can match dynamic routes, but a literal placeholder is not an ID.
pub(super) fn matches_route(pattern: &str, href: &str) -> bool {
    fn segments_match(p: &[&str], h: &[&str]) -> bool {
        match (p.split_first(), h.split_first()) {
            (None, None) => true,
            (Some((s, rest)), _) if s.starts_with("[[...") => rest.is_empty(),
            (Some((s, rest)), Some(_)) if s.starts_with("[...") => rest.is_empty(),
            (Some((s, rest)), Some((t, tail)))
                if (*s == *t
                    || (s.starts_with('[') && s.ends_with(']') && !t.contains(['[', ']']))) =>
            {
                segments_match(rest, tail)
            }
            _ => false,
        }
    }
    let path = href.split(['?', '#']).next().unwrap_or("");
    if path.contains(['[', ']']) {
        return false;
    }
    let p = pattern
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let h = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    segments_match(&p, &h)
}

fn route_inventory_usable(inventory: &Value) -> bool {
    inventory["found"] == true
        && !array(&inventory["routes"]).is_empty()
        && inventory["truncation"]["truncated"] != true
        && inventory["customRoutingUnverified"] != true
        && inventory["unparsedFiles"].as_u64().unwrap_or(0) == 0
        && array(&inventory["routes"])
            .iter()
            .map(|r| r["appliesTo"].as_str().unwrap_or("."))
            .collect::<BTreeSet<_>>()
            .len()
            <= 1
}

pub(super) fn report(source: &str, inventory: &Value, rendered_html: Option<&str>) -> Value {
    let evidence = source_evidence(source);
    let mut findings = vec![];
    let mut hrefs = BTreeMap::<String, usize>::new();
    let usable = route_inventory_usable(inventory);
    for element in array(&evidence["elements"]) {
        let mut finding = |code: &str, severity: &str, message: &str| {
            findings.push(json!({"code":code,"rule":code,"severity":severity,"message":message,"line":element["line"],"element":element["element"],"name":element["name"],"byteRange":[element["offset"],element["offset"]]}));
        };
        if element["candidate"] == true
            && element["disabled"] != true
            && element["nativeToggleOnly"] != true
            && element["wired"] != true
            && element["dynamicHref"] != true
        {
            finding(
                "interaction-unmapped",
                "warning",
                "Action candidate has no semantic link or native control; confirm its intended action and wire it accessibly.",
            );
        }
        if element["nonSemanticClick"] == true {
            finding(
                "interaction-accessibility",
                "warning",
                "Use a native button/input or Link/a; a click handler on visual markup lacks native keyboard behavior.",
            );
        }
        if element["staticActive"] == true {
            finding(
                "hardcoded-active-state",
                "info",
                "Active state is literal; verify route/state-based selection. For GNB use usePathname in a small client component.",
            );
        }
        if element["fixedVisualNavigationMarker"] == true {
            finding(
                "fixed-navigation-marker",
                "info",
                include_str!("text/navigation-report-928037dc.md"),
            );
        }
        if let Some(href) = element["href"]
            .as_str()
            .filter(|_| element["isLink"] == true)
        {
            *hrefs.entry(href.into()).or_default() += 1;
            // Relative, external, hash and dynamic links cannot be checked as absolute app paths.
            if usable
                && href.starts_with('/')
                && !href.starts_with("//")
                && !href
                    .split(['?', '#'])
                    .next()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .contains('.')
                && !array(&inventory["routes"])
                    .iter()
                    .any(|r| r["route"].as_str().is_some_and(|p| matches_route(p, href)))
                && !array(&inventory["nonPageDestinations"])
                    .iter()
                    .any(|p| p.as_str().is_some_and(|p| matches_route(p, href)))
            {
                finding(
                    "missing-static-route",
                    "error",
                    "Static internal href has no matching App Router page in the selected application inventory; confirm rewrites/basePath/public resources before creating a route.",
                );
                findings.last_mut().unwrap()["href"] = json!(href);
            }
        }
    }
    let candidates = array(&evidence["elements"])
        .iter()
        .filter(|e| e["candidate"] == true && e["ancestorControl"] != true)
        .collect::<Vec<_>>();
    let wired = candidates.iter().filter(|e| e["wired"] == true).count();
    let native_toggles = candidates
        .iter()
        .filter(|e| e["nativeToggleOnly"] == true)
        .count();
    let semantic = candidates
        .iter()
        .filter(|e| e["semanticControl"] == true)
        .count();
    let dynamic = candidates
        .iter()
        .filter(|e| e["dynamicHref"] == true)
        .count();
    let rendered = rendered_html.map(rendered_hrefs);
    json!({"findings":findings,"sourceHrefs":hrefs,"renderedHrefs":rendered,
        "coverage":{"candidateCount":candidates.len(),"wiredCount":wired,"semanticControlCount":semantic,"nativeToggleOnlyCount":native_toggles,"dynamicTargetCount":dynamic,"runtimeVerified":false,"sourceParsed":evidence["parsed"]},
        "elements":evidence["elements"],"diagnostics":evidence["diagnostics"],"routeCheck":{"ran":usable,"reason":if usable{"Selected application App Router inventory; rewrites, middleware, basePath and public resources require explicit review."}else{"No complete single-application route inventory; missing-route errors skipped."}},
        "limitations":["Counts are static source candidates, not a census of rendered controls; components, loops, wrappers and runtime state may differ.","Literal active attributes are evidence; constant navigation underlines are review candidates only, and other visual markers may be unclassified.","Rendered href counts are separate evidence and never establish click success or one-to-one source coverage."]})
}

pub(super) fn attach_validation(result: &mut Value, input: &super::tools::UiValidateInput) {
    if let Some(rules) = result["verificationScope"]["static"]["rules"].as_array_mut() {
        rules.extend(
            [
                "interaction-unmapped",
                "interaction-accessibility",
                "missing-static-route",
                "hardcoded-active-state",
                "fixed-navigation-marker",
            ]
            .map(|r| json!(r)),
        );
    }
    let inventory = if let Some(files) = &input.files {
        let routes=files.iter().filter_map(|f| {
            let path=f.path.replace('\\',"/");
            let (authority,rest)=path.split_once("/app/").or_else(||path.strip_prefix("app/").map(|p|("",p)))?;
            if !["page.tsx","page.ts","page.jsx","page.js"].contains(&rest.rsplit('/').next()?) {return None}
            let segments=rest.split('/').collect::<Vec<_>>();
            let directories=&segments[..segments.len()-1];
            if directories.iter().any(|s|s.starts_with('_')) {return None}
            let route=directories.iter().filter(|s|!s.starts_with('(') && !s.starts_with('@')).copied().collect::<Vec<_>>().join("/");
            Some(json!({"route":format!("/{route}"),"page":path,"appliesTo":authority.strip_suffix("/src").unwrap_or(authority)}))
        }).collect::<Vec<_>>();
        json!({"found":!routes.is_empty(),"routes":routes,"scope":"supplied-files-only; omitted pages are not proven absent","truncation":{"truncated":true}})
    } else {
        super::project_context::routes_for_validation(input.project_root.as_deref())
    };
    let sources = if let Some(files) = &input.files {
        files
            .iter()
            .filter(|f| f.path.ends_with(".tsx") || f.path.ends_with(".jsx"))
            .map(|f| (f.path.clone(), f.content.as_str()))
            .collect::<Vec<_>>()
    } else {
        vec![(
            input.source_name.clone().unwrap_or_else(|| "tsx".into()),
            input.tsx.as_str(),
        )]
    };
    let mut reports = vec![];
    for (path, source) in sources {
        let mut navigation = report(source, &inventory, None);
        for finding in navigation["findings"].as_array_mut().unwrap() {
            finding["sourceName"] = json!(path);
        }
        result["violations"]
            .as_array_mut()
            .unwrap()
            .extend(array(&navigation["findings"]).iter().cloned());
        reports.push(json!({"sourceName":path,"report":navigation}));
    }
    result["navigation"] = json!({"sources":reports,"renderedHrefs":input.rendered_html.as_deref().map(rendered_hrefs),"routeInventory":inventory});
    let errors = array(&result["violations"])
        .iter()
        .filter(|f| f["severity"] == "error")
        .count();
    let warnings = array(&result["violations"])
        .iter()
        .filter(|f| f["severity"] == "warning")
        .count();
    let infos = array(&result["violations"])
        .iter()
        .filter(|f| f["severity"] == "info")
        .count();
    result["violationCounts"] = json!({"error":errors,"warning":warnings,"info":infos});
    result["ok"] = json!(errors == 0 && (!input.strict || warnings == 0));
    if errors > 0 {
        result["okReason"] = json!("error-violations");
    } else if warnings > 0 {
        result["okReason"] = json!(if input.strict {
            "strict-warnings"
        } else if infos > 0 {
            "warnings-and-info"
        } else {
            "warnings-only"
        });
    } else if infos > 0 {
        result["okReason"] = json!("info-only");
    }
}

fn rendered_hrefs(html: &str) -> Value {
    use quick_xml::{Reader, events::Event};
    if html.len() > 2_097_152 {
        return json!({"parsed":false,"reason":"Rendered HTML exceeds 2 MiB."});
    }
    let mut reader = Reader::from_str(html);
    reader.config_mut().check_end_names = false;
    let mut counts = BTreeMap::<String, usize>::new();
    let mut errors = vec![];
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e))
                if e.name().as_ref().eq_ignore_ascii_case("a") =>
            {
                for attribute in e.html_attributes().with_checks(false) {
                    match attribute {
                        Ok(a) if a.key.as_ref().eq_ignore_ascii_case("href") => {
                            match quick_xml::escape::unescape(&a.value) {
                                Ok(v) => *counts.entry(v.to_string()).or_default() += 1,
                                Err(e) => errors.push(e.to_string()),
                            }
                        }
                        Ok(_) => {}
                        Err(e) => errors.push(e.to_string()),
                    }
                }
            }
            Ok(Event::Start(e))
                if ["script", "style"]
                    .iter()
                    .any(|s| e.name().as_ref().eq_ignore_ascii_case(s)) =>
            {
                if let Err(e) = reader.read_to_end(e.name()) {
                    errors.push(e.to_string());
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                errors.push(e.to_string());
                break;
            }
            _ => {}
        }
    }
    json!({"counts":counts,"parsed":errors.is_empty(),"diagnostics":errors,"scope":"Caller-supplied rendered HTML anchor hrefs; XML-compatible HTML parser, incomplete/malformed HTML may be unverified."})
}

pub(super) fn suggestions(candidates: &Value, inventory: &Value) -> Value {
    let mut result = vec![];
    for candidate in array(candidates) {
        let label = format!(
            "{} {}",
            candidate["name"].as_str().unwrap_or(""),
            candidate["text"].as_str().unwrap_or("")
        )
        .to_lowercase();
        let concepts = [
            ("portfolio", vec!["portfolio", "포트폴리오", "구축 사례"]),
            ("privacy", vec!["privacy", "개인정보"]),
            ("terms", vec!["terms", "이용약관"]),
            ("faq", vec!["faq", "고객센터"]),
            ("reservation", vec!["reservation", "견적", "문의하기"]),
            ("about", vec!["about", "소개"]),
            ("services", vec!["services", "서비스"]),
        ];
        let options=array(&inventory["routes"]).iter().filter(|r| {
            let route=r["route"].as_str().unwrap_or("").to_lowercase();
            concepts.iter().any(|(segment,labels)|route.split('/').any(|s|s==*segment) && labels.iter().any(|s|label.contains(s)))
        }).map(|r|json!({"route":r["route"],"page":r["page"],"appliesTo":r["appliesTo"],"requiresRecordId":r["route"].as_str().is_some_and(|s|s.contains('['))})).collect::<Vec<_>>();
        result.push(json!({"nodeId":candidate["nodeId"],"suggestions":options,"targetResolved":false,
            "reason":if options.is_empty(){"No matching route evidence; ask for destination/action."}else{"Label/path heuristic only; confirm authority and record IDs before wiring. Suggestions never resolve a target automatically."}}));
    }
    json!(result)
}

/// Compare only explicitly bound URLs. Figma destination node IDs remain unverified.
pub(super) fn graph(
    design: &Value,
    sources: &[(String, String)],
    targets: &BTreeMap<String, String>,
) -> Value {
    let mut edges = vec![];
    for (path, source) in sources {
        for element in array(&source_evidence(source)["elements"]) {
            if element["isLink"] == true {
                edges.push(json!({"from":path,"to":element["href"],"dynamic":element["dynamicHref"],"line":element["line"],"nodeId":element["nodeId"]}));
            }
        }
    }
    let mut comparisons = vec![];
    for candidate in array(design) {
        for action in array(&candidate["prototypeActions"]) {
            let url = action["url"].as_str().or_else(|| {
                action["destinationId"]
                    .as_str()
                    .and_then(|id| targets.get(id).map(String::as_str))
            });
            let matched = url.is_some_and(|url| edges.iter().any(|e| e["to"] == url));
            let mismatch = url.is_some_and(|url| {
                edges.iter().any(|e| {
                    e["nodeId"] == candidate["nodeId"] && e["to"].is_string() && e["to"] != url
                })
            });
            comparisons.push(json!({"nodeId":candidate["nodeId"],"prototype":action,"expectedTarget":url,"status":if mismatch{"MISMATCH"}else if matched{"MATCHED_STATIC_HREF"}else if url.is_some(){"MISSING_STATIC_HREF"}else{"UNVERIFIED"},
                "reason":if mismatch{"Explicit data-figma-node-id link has a different literal destination."}else if matched{"Literal URL occurs in selected source slice; origin control and click behavior not proven."}else if url.is_some(){"Literal prototype URL absent from selected source slice; wrapper/dynamic links may be unresolved."}else{"Figma node/action has no explicit code route binding; no destination invented."}}));
        }
    }
    json!({"codeEdges":edges,"comparison":comparisons,"scope":"Selected source slice; URL equality is destination presence, not node-to-control mapping or runtime navigation proof.","designAvailable":!array(design).is_empty()})
}
