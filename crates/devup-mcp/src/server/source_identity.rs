//! AST evidence shared by re-export and drift inspection. Ordinals address
//! parsed source only; a semantic match never claims rendered DOM identity.
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(super) struct Attribute {
    pub name: String,
    pub source: String,
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Debug)]
pub(super) struct Element {
    pub index: usize,
    pub tag: String,
    pub text: String,
    pub semantics: BTreeMap<String, String>,
    pub children: Vec<String>,
    pub ancestor_tags: Vec<String>,
    pub end: usize,
    pub literal_text: Option<(usize, usize, String)>,
    pub attributes: Vec<Attribute>,
    pub spread: bool,
}
impl Element {
    pub fn evidence(&self) -> Value {
        json!({"tag":self.tag,"text":self.text,"semantics":self.semantics,"children":self.children,"ancestorTags":self.ancestor_tags})
    }
}
fn compact(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub(super) fn inspection_comments(source: &str) -> bool {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    parsed.program.comments.iter().any(|comment| {
        let text = comment.span.source_text(source).trim_start();
        text.strip_prefix("//")
            .or_else(|| text.strip_prefix("/*"))
            .is_some_and(|text| text.trim_start().starts_with("@devup-"))
    })
}
pub(super) fn inventory(source: &str) -> Result<Vec<Element>, String> {
    struct Collector<'s> {
        source: &'s str,
        nodes: Vec<Element>,
        ancestors: Vec<String>,
    }
    impl<'a> Visit<'a> for Collector<'_> {
        fn visit_jsx_element(&mut self, element: &JSXElement<'a>) {
            let open = &element.opening_element;
            let tag = open.name.span().source_text(self.source).to_owned();
            let attributes = open
                .attributes
                .iter()
                .map(|attribute| {
                    let span = attribute.span();
                    Attribute {
                        name: match attribute {
                            JSXAttributeItem::Attribute(a) => {
                                a.name.span().source_text(self.source).to_owned()
                            }
                            _ => "...".into(),
                        },
                        source: span.source_text(self.source).into(),
                        start: span.start as usize,
                        end: span.end as usize,
                    }
                })
                .collect::<Vec<_>>();
            let semantics = attributes
                .iter()
                .filter(|a| {
                    matches!(
                        a.name.as_str(),
                        "aria-label" | "alt" | "name" | "id" | "data-testid" | "role"
                    )
                })
                .map(|a| (a.name.clone(), compact(&a.source)))
                .collect();
            let text = compact(
                &element
                    .children
                    .iter()
                    .filter_map(|child| {
                        if let JSXChild::Text(t) = child {
                            Some(t.value.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            let children = element
                .children
                .iter()
                .filter_map(|child| {
                    if let JSXChild::Element(e) = child {
                        Some(
                            e.opening_element
                                .name
                                .span()
                                .source_text(self.source)
                                .to_owned(),
                        )
                    } else {
                        None
                    }
                })
                .collect();
            let literal_text = if element.children.len() == 1 {
                if let JSXChild::Text(child) = &element.children[0] {
                    let span = child.span;
                    Some((
                        span.start as usize,
                        span.end as usize,
                        span.source_text(self.source).to_owned(),
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            self.nodes.push(Element {
                index: self.nodes.len(),
                tag: tag.clone(),
                text,
                semantics,
                children,
                ancestor_tags: self.ancestors.clone(),
                end: open.span.end as usize,
                literal_text,
                spread: attributes.iter().any(|a| a.name == "..."),
                attributes,
            });
            self.ancestors.push(tag);
            walk::walk_jsx_element(self, element);
            self.ancestors.pop();
        }
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() {
        return Err("Source is not valid TSX; no identity inferred.".into());
    }
    let mut collector = Collector {
        source,
        nodes: vec![],
        ancestors: vec![],
    };
    collector.visit_program(&parsed.program);
    Ok(collector.nodes)
}

/// Ignore formatting outside strings and normalize ordinary quote spelling.
/// Unknown escape syntax remains exact rather than guessing equivalence.
pub(super) fn canonical_attribute(source: &str) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let quote = chars[i];
        if quote == '\'' || quote == '"' {
            let start = i;
            i += 1;
            let mut value = String::new();
            let mut escaped = false;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' {
                    escaped = true;
                    if i + 1 < chars.len() {
                        value.push(chars[i]);
                        i += 1;
                    }
                }
                value.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && !escaped {
                out.push_str(&serde_json::to_string(&value).unwrap_or_default());
            } else {
                out.extend(chars[start..i.min(chars.len())].iter());
                if i < chars.len() {
                    out.push(quote);
                }
            }
        } else if !quote.is_whitespace() {
            out.push(quote);
        }
        i += 1;
    }
    out
}

#[derive(Clone, Debug)]
pub(super) struct Match {
    pub file: String,
    pub element: Element,
    pub confidence: &'static str,
    pub reasons: Vec<&'static str>,
}
/// A unique tag alone is insufficient. Content/semantic evidence is strong;
/// structural evidence requires a non-empty child shape plus ancestor agreement.
pub(super) fn match_elements(
    base: &[Element],
    files: &[(String, Vec<Element>)],
) -> Vec<Result<Match, Value>> {
    let mut results = Vec::new();
    let mut claimed = BTreeSet::new();
    for original in base {
        let mut candidates = Vec::new();
        for (file, nodes) in files {
            for node in nodes.iter().filter(|node| node.tag == original.tag) {
                let semantic =
                    !original.semantics.is_empty() && node.semantics == original.semantics;
                let text = !original.text.is_empty() && node.text == original.text;
                let shape = !original.children.is_empty() && node.children == original.children;
                let ancestors = node.ancestor_tags == original.ancestor_tags;
                let score = usize::from(semantic) * 8
                    + usize::from(text) * 6
                    + usize::from(shape) * 3
                    + usize::from(ancestors);
                if !(semantic || text || (shape && ancestors)) {
                    continue;
                }
                let mut reasons = Vec::new();
                if semantic {
                    reasons.push("equal-static-semantics");
                }
                if text {
                    reasons.push("equal-static-copy");
                }
                if shape {
                    reasons.push("equal-child-shape");
                }
                if ancestors {
                    reasons.push("equal-ancestor-shape");
                }
                candidates.push((score, file, node, reasons));
            }
        }
        candidates.sort_by_key(|(score, _, _, _)| std::cmp::Reverse(*score));
        let best = candidates.first();
        if let Some((score, file, node, reasons)) = best
            && candidates
                .get(1)
                .is_none_or(|candidate| candidate.0 < *score)
            && claimed.insert(((*file).clone(), node.index))
        {
            results.push(Ok(Match {
                file: (*file).clone(),
                element: (*node).clone(),
                confidence: if *score >= 6 { "high" } else { "medium" },
                reasons: reasons.clone(),
            }));
        } else {
            results.push(Err(json!({"baselineElement":original.index,"evidence":original.evidence(),"status":"unverified","reason":if best.is_none(){"No content, semantic or supported structural match."}else{"Ambiguous or already claimed source matches."},"candidates":candidates.iter().take(8).map(|(score,file,node,reasons)|json!({"file":file,"elementIndex":node.index,"score":score,"reasons":reasons})).collect::<Vec<_>>()})));
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extraction_and_reordering_match_real_semantics_but_repeated_copy_abstains() {
        let base = inventory("<Box><Text>Alpha</Text><Text>Beta</Text></Box>").unwrap();
        let extracted = vec![
            (
                "Beta.tsx".into(),
                inventory("<Text onClick={go}>Beta</Text>").unwrap(),
            ),
            ("Alpha.tsx".into(), inventory("<Text>Alpha</Text>").unwrap()),
        ];
        let matches = match_elements(&base, &extracted);
        assert!(matches[0].is_err());
        assert_eq!(matches[1].as_ref().unwrap().file, "Alpha.tsx");
        assert_eq!(matches[2].as_ref().unwrap().file, "Beta.tsx");
        let repeated = vec![(
            "Both.tsx".into(),
            inventory("<><Text>Alpha</Text><Text>Alpha</Text></>").unwrap(),
        )];
        assert!(match_elements(&base, &repeated)[1].is_err());
    }
}
