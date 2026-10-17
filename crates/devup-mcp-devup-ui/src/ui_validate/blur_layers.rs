use super::{Severity, Violation};
use crate::provenance::attributes::GeneratedAttribute;
use oxc_allocator::Allocator;
use oxc_ast::ast::JSXElement;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::json;
use std::collections::BTreeMap;

fn literal(source: &str) -> Option<&str> {
    let (_, value) = source.split_once('=')?;
    let value = value
        .trim()
        .strip_prefix('{')
        .and_then(|v| v.strip_suffix('}'))
        .unwrap_or(value.trim())
        .trim();
    if matches!(value, "true" | "false" | "0") {
        return Some(value);
    }
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .filter(|s| !s.contains(['"', '\\']))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|v| v.strip_suffix('\''))
                .filter(|s| !s.contains(['\'', '\\']))
        })
}
fn positive_blur(value: &str) -> bool {
    value
        .trim()
        .strip_prefix("blur(")
        .and_then(|v| v.strip_suffix(')'))
        .and_then(|v| v.trim().strip_suffix("px"))
        .and_then(|v| v.parse::<f64>().ok())
        .is_some_and(|n| n.is_finite() && n > 0.0)
}
#[derive(Clone, Copy, Default)]
struct Layer {
    blurred: bool,
    hidden: bool,
    uncertain: bool,
}
fn layers(source: &str, attrs: &[GeneratedAttribute]) -> Vec<Layer> {
    struct Layers<'s> {
        source: &'s str,
        attrs: BTreeMap<usize, Vec<&'s GeneratedAttribute>>,
        stack: Vec<Layer>,
        found: Vec<Layer>,
    }
    impl<'a> Visit<'a> for Layers<'_> {
        fn visit_jsx_element(&mut self, element: &JSXElement<'a>) {
            let ordinal = self.found.len();
            let local = self.attrs.get(&ordinal).map(Vec::as_slice).unwrap_or(&[]);
            let value = |key| {
                local
                    .iter()
                    .find(|a| a.name == key)
                    .and_then(|a| literal(&a.source))
            };
            let parent = self.stack.last().copied().unwrap_or_default();
            let span = element.opening_element.name.span();
            let tag = &self.source[span.start as usize..span.end as usize];
            let primitive = matches!(
                tag,
                "Box" | "Flex" | "VStack" | "Center" | "Grid" | "Image" | "Text"
            );
            let known = primitive
                || (tag.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
                    && !tag.contains(['.', '-']));
            let uncertain = parent.uncertain
                || !known
                || local.iter().any(|a| {
                    matches!(a.name.as_str(), "..." | "style" | "className")
                        || (matches!(
                            a.name.as_str(),
                            "filter" | "display" | "visibility" | "opacity"
                        ) && literal(&a.source).is_none())
                });
            let layer = Layer {
                blurred: parent.blurred
                    || (primitive && value("filter").is_some_and(positive_blur)),
                hidden: parent.hidden
                    || (primitive
                        && (value("display") == Some("none")
                            || value("visibility") == Some("hidden")
                            || value("opacity") == Some("0"))),
                uncertain,
            };
            self.found.push(layer);
            self.stack.push(layer);
            walk::walk_jsx_element(self, element);
            self.stack.pop();
        }
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    let mut visitor = Layers {
        source,
        attrs: attrs.iter().fold(
            BTreeMap::<usize, Vec<&GeneratedAttribute>>::new(),
            |mut map, a| {
                map.entry(a.element).or_default().push(a);
                map
            },
        ),
        stack: vec![],
        found: vec![],
    };
    visitor.visit_program(&parsed.program);
    visitor.found
}
pub(super) fn check(source_text: &str, attrs: &[GeneratedAttribute]) -> Vec<Violation> {
    let layers = layers(source_text, attrs);
    let mut uses = BTreeMap::<&str, Vec<(&GeneratedAttribute, Option<bool>)>>::new();
    for source in attrs
        .iter()
        .filter(|a| a.name == "src" && matches!(a.tag.as_str(), "img" | "Image"))
    {
        let Some(path) = literal(&source.source).filter(|s| !s.is_empty()) else {
            continue;
        };
        let on_element = attrs
            .iter()
            .filter(|a| a.element == source.element)
            .collect::<Vec<_>>();
        let value = |key| {
            on_element
                .iter()
                .find(|a| a.name == key)
                .and_then(|a| literal(&a.source))
        };
        if value("alt") == Some("")
            || value("aria-hidden") == Some("true")
            || value("role") == Some("presentation")
        {
            continue;
        }
        let layer = layers.get(source.element).copied().unwrap_or(Layer {
            uncertain: true,
            ..Default::default()
        });
        if layer.hidden {
            continue;
        }
        // Spread, class/style and expression-valued filters may supply or
        // override the layer. Their absence is never treated as sharp proof.
        let state = if layer.uncertain {
            None
        } else {
            Some(layer.blurred)
        };
        uses.entry(path).or_default().push((source, state));
    }
    uses.into_iter().filter(|(_,uses)|!uses.is_empty()&&uses.iter().all(|(_,state)|*state==Some(true))).map(|(path,uses)| {
        let first=uses[0].0;
        Violation {rule:"blur-only-image",severity:Severity::Warning,byte_range:[first.start,first.end],
            context:json!({"assetPath":path,"elementIndices":uses.iter().map(|(a,_)|a.element).collect::<Vec<_>>(),
                "state":"review-required","evidenceScope":"literal src and statically resolved known-wrapper or same-element CSS filters",
                "evidenceLimit":"Static review cannot establish intended decoration, conditional visibility, external CSS, dynamic/custom ancestor effects or browser pixels; this warning does not prove a missing layer."}).as_object().unwrap().clone(),
            message:"All statically visible literal uses of this image apply positive CSS blur; review whether an intended sharp copy is missing.".into(),
            suggestion:Some("If the design includes a sharp image, render that copy alongside the blur layer; keep deliberate decoration explicitly marked.".into())}
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn count(tsx: &str) -> usize {
        check(
            tsx,
            &crate::provenance::attributes::generated_attributes(tsx),
        )
        .len()
    }
    #[test]
    fn conservative_blur_layer_review() {
        assert_eq!(
            count(r#"<Image src="/photo.webp" filter="blur(8px)" />"#),
            1
        );
        assert_eq!(
            count(
                r#"<><Image src="/photo.webp" filter="blur(8px)"/><Image src="/photo.webp" /></>"#
            ),
            0
        );
        assert_eq!(
            count(r#"<Image src="/photo.webp" filter="blur(0px)" />"#),
            0
        );
        assert_eq!(count(r#"<Image src="/photo.webp" filter={effect} />"#), 0);
        assert_eq!(
            count(r#"<Image src="/photo.webp" filter="blur(8px)" className="photo"/>"#),
            0
        );
        assert_eq!(
            count(r#"<Image src="/photo.webp" filter="blur(8px)" alt=""/>"#),
            0
        );
        assert_eq!(
            count(r#"<Image src="/photo.webp" filter="blur(8px)" display="none"/>"#),
            0
        );
        assert_eq!(
            count(r#"<>{/* <Image src="/photo.webp" filter="blur(8px)"/> */}</>"#),
            0
        );
    }
    #[test]
    fn static_wrapper_blur_requires_a_sharp_copy_outside_the_blurred_subtree() {
        assert_eq!(
            count(r#"<div filter="blur(6px)"><img src="/art.webp" /></div>"#),
            0
        );
        assert_eq!(
            count(r#"<Box filter="blur(6px)"><div><img src="/art.webp" /></div></Box>"#),
            1
        );
        assert_eq!(
            count(
                r#"<Box filter="blur(6px)"><Box overflow="hidden"><Image src="/art.webp" /></Box></Box>"#
            ),
            1
        );
        assert_eq!(
            count(
                r#"<Box filter="blur(6px)"><Image src="/art.webp" /><Image src="/art.webp" /></Box>"#
            ),
            1
        );
        assert_eq!(
            count(
                r#"<><Box filter="blur(6px)"><Image src="/art.webp" /></Box><Image src="/art.webp" /></>"#
            ),
            0
        );
        assert_eq!(
            count(r#"<Box filter={effect}><Image src="/art.webp" /></Box>"#),
            0
        );
        assert_eq!(
            count(r#"<Box className="effect" filter="blur(6px)"><Image src="/art.webp" /></Box>"#),
            0
        );
        assert_eq!(count(r#"<Box><Image src="/art.webp" /></Box>"#), 0);
        assert_eq!(
            count(r#"<CustomWrapper filter="blur(6px)"><Image src="/art.webp" /></CustomWrapper>"#),
            0
        );
    }
}
