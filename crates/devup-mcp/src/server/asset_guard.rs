//! `devup_ui_validate`'s `asset-missing` rule: generated code that points at
//! an asset file the project does not have.
//!
//! An export without `assetPublicRoot` keeps placeholder asset paths, and an
//! asset request that failed or was never made leaves the reference in the
//! TSX with no file behind it. Nothing in the source shows that: the page
//! compiles, and the browser draws an empty box or a broken-image icon where
//! the design had a picture. So when `projectRoot` is given, every
//! root-relative asset URL (`src`, and any `url(...)` in a style value) is
//! looked up under the project's public directories, and one that resolves
//! to no file is an error.
//!
//! Like the rest of the ground-truth tools this reads the disk on every call
//! and never caches.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use devup_mcp_devup_ui::ui_validate::{Severity, Violation};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::{Map, json};

use super::mask_guard::{percent_decode, public_dirs_of};

pub(super) const RULE: &str = "asset-missing";
pub(super) const COVERAGE_RULE: &str = "section-coverage";

/// Paths may be renamed/deduplicated or percent-encoded at delivery. Keep
/// the original node census, but freeze URLs only after those rewrites.
pub(super) fn refresh_export_asset_urls(result: &mut serde_json::Value) {
    if let Some(object) = result.as_object_mut() {
        for field in ["tsx", "componentTsx"] {
            if let Some(source) = object.get(field).and_then(serde_json::Value::as_str) {
                let urls = asset_urls(source)
                    .into_iter()
                    .map(|(url, _)| url)
                    .collect::<BTreeSet<_>>();
                if let Some(contract) = object
                    .get_mut("outputResults")
                    .and_then(|outputs| outputs.get_mut(field))
                    .and_then(|output| output.get_mut("sectionCoverage"))
                    .and_then(serde_json::Value::as_object_mut)
                {
                    contract.insert("assetUrls".into(), json!(urls));
                }
            }
        }
        if let Some(frames) = object
            .get_mut("frames")
            .and_then(serde_json::Value::as_array_mut)
        {
            for frame in frames {
                refresh_export_asset_urls(frame);
            }
        }
    }
}

pub(super) fn asset_coverage_violations(
    sources: &[(&str, &str)],
    expected: &[String],
) -> Vec<Violation> {
    let present = sources
        .iter()
        .flat_map(|(_, source)| asset_urls(source).into_iter().map(|(url, _)| url))
        .collect::<BTreeSet<_>>();
    expected.iter().collect::<BTreeSet<_>>().into_iter().filter(|url| !present.contains(url.as_str())).map(|url| Violation {
        rule: COVERAGE_RULE,
        severity: Severity::Error,
        byte_range: [0, 0],
        context: Map::from_iter([("assetUrl".into(), json!(url))]),
        message: format!("Asset reference {url} from the original export is missing from the supplied code."),
        suggestion: Some("Restore the generated image, mask or background reference from the original export.".into()),
    }).collect()
}

/// Compare against the original export, not the edited code's source map.
/// Parse JSX attributes so comments and string literals cannot satisfy coverage.
pub(super) fn coverage_violations(sources: &[(&str, &str)], expected: &[String]) -> Vec<Violation> {
    let mut present = BTreeSet::new();
    for (_, source) in sources {
        for attr in devup_mcp_devup_ui::provenance::attributes::generated_attributes(source) {
            if attr.name != "data-figma-node-id" {
                continue;
            }
            let Some((_, value)) = attr.source.split_once('=') else {
                continue;
            };
            let value = value
                .trim()
                .trim_start_matches('{')
                .trim_end_matches('}')
                .trim();
            if let Some(quote @ ('\'' | '"')) = value.chars().next()
                && value.ends_with(quote)
            {
                present.insert(value[1..value.len() - 1].to_owned());
            }
        }
    }
    expected.iter().collect::<BTreeSet<_>>().into_iter()
        .filter(|id| !present.contains(id.as_str()))
        .map(|id| Violation {
            rule: COVERAGE_RULE,
            severity: Severity::Error,
            byte_range: [0, 0],
            context: Map::from_iter([("nodeId".into(), json!(id))]),
            message: format!("Captured section or asset {id} is missing from the supplied JSX identity coverage."),
            suggestion: Some("Restore the generated section/image and its data-figma-node-id from the original export. Static identity coverage does not verify runtime visibility.".into()),
        }).collect()
}

/// Expected visible sections and assets come from the captured tree, even if
/// generation omitted them. Asset boundaries deliberately hide their internals.
pub(super) fn export_coverage(
    snapshot: &devup_mcp_figma::Snapshot,
    root: &str,
    output: &mut devup_mcp_devup_ui::codegen::CodegenOutput,
    inline_instances: bool,
    annotate: bool,
) -> serde_json::Value {
    let mut scoped = snapshot.clone();
    scoped.roots = vec![root.into()];
    let manifest = devup_mcp_figma::discover_asset_manifest(&scoped);
    let assets = manifest
        .assets
        .iter()
        .filter(|a| a.error_code.as_deref() != Some("DEVUP_ASSET_NODE_HIDDEN"))
        .map(|a| a.node_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut wanted = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut pending = vec![(root, 0)];
    while let Some((id, depth)) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(id) else {
            continue;
        };
        let view = node.typed_view();
        if view.bool("visible") == Some(false)
            || view.number("opacity").is_some_and(|opacity| opacity <= 0.0)
        {
            continue;
        }
        if manifest
            .assets
            .iter()
            .any(|a| a.node_id == id && a.error_code.as_deref() == Some("DEVUP_ASSET_NODE_HIDDEN"))
        {
            continue;
        }
        if depth <= 1 || node.node_type == "SECTION" || assets.contains(id) {
            wanted.insert(id.to_owned());
        }
        // Leaf assets (including a flattened wrapper) represent all their children.
        let flattened = manifest
            .assets
            .iter()
            .any(|a| a.node_id == id && a.field == "node");
        if flattened || (!inline_instances && node.node_type == "INSTANCE") {
            continue;
        }
        pending.extend(view.child_ids().map(|child| (child, depth + 1)));
    }
    if annotate {
        devup_mcp_devup_ui::codegen::node_ids::annotate_node_ids(output, &wanted);
        // Identity attributes are derived from captured IDs. Record that
        // derivation so the final attribute audit can verify them too.
        for attr in devup_mcp_devup_ui::provenance::attributes::generated_attributes(&output.tsx) {
            if attr.name != "data-figma-node-id" {
                continue;
            }
            let Some((_, value)) = attr.source.split_once('=') else {
                continue;
            };
            let id = value.trim().trim_matches('"');
            output
                .source_map
                .entries
                .push(devup_mcp_devup_ui::provenance::ProvenanceEntry {
                    generated_range: Some(devup_mcp_devup_ui::provenance::GeneratedRange {
                        start: attr.start,
                        end: attr.end,
                    }),
                    calculation: None,
                    generated_property: Some(attr.source.clone()),
                    json_pointer: None,
                    node_id: Some(id.into()),
                    property: Some("id".into()),
                    variable_id: None,
                    style_id: None,
                    asset_id: None,
                    resolution: "exact".into(),
                    element_index: Some(attr.element),
                });
        }
    }
    let node_ids = wanted.into_iter().collect::<Vec<_>>();
    let mapped = output
        .source_map
        .entries
        .iter()
        .filter(|e| e.element_index.is_some())
        .filter_map(|e| e.node_id.as_ref())
        .collect::<BTreeSet<_>>();
    for id in node_ids.iter().filter(|id| !mapped.contains(id)) {
        output.diagnostics.push(devup_mcp_figma::Diagnostic {
            code: "DEVUP_CODEGEN_SECTION_COVERAGE".into(),
            node_id: Some(id.clone()),
            message: format!("Captured section or asset {id} has no generated JSX element."),
            fidelity_impact: Some(devup_mcp_figma::FidelityImpact::Lossy),
            details: Some(json!({"stage":"projection","classification":"omitted-content","originalValue":{"nodeId":id},"appliedValue":null,"appliedValueReason":"No generated JSX element maps to the captured node."})),
            ..Default::default()
        });
    }
    for asset in manifest
        .assets
        .iter()
        .filter(|asset| node_ids.contains(&asset.node_id))
    {
        if output.source_map.entries.iter().any(|entry| {
            entry.asset_id.as_deref() == Some(&asset.asset_id) && entry.element_index.is_some()
        }) {
            continue;
        }
        output.diagnostics.push(devup_mcp_figma::Diagnostic {
            code: "DEVUP_CODEGEN_SECTION_COVERAGE".into(),
            node_id: Some(asset.node_id.clone()),
            property: Some("assetReference".into()),
            message: format!(
                "Captured asset {} has no generated asset reference.",
                asset.asset_id
            ),
            fidelity_impact: Some(devup_mcp_figma::FidelityImpact::Lossy),
            details: Some(json!({"stage":"projection","classification":"omitted-content","originalValue":{"assetId":asset.asset_id,"nodeId":asset.node_id},"appliedValue":null,"appliedValueReason":"No generated asset reference maps to the captured asset."})),
            ..Default::default()
        });
    }
    if annotate {
        let asset_urls = asset_urls(&output.tsx)
            .into_iter()
            .map(|(url, _)| url)
            .collect::<BTreeSet<_>>();
        json!({"nodeIds":node_ids,"assetUrls":asset_urls})
    } else {
        serde_json::Value::Null
    }
}

/// Extensions treated as files a page serves from `public`.
const ASSET_EXTENSIONS: &[&str] = &[
    "svg", "png", "jpg", "jpeg", "webp", "gif", "avif", "ico", "bmp", "woff", "woff2", "ttf",
    "otf", "mp4", "webm", "mp3", "pdf",
];

/// One `asset-missing` error per distinct root-relative asset URL in each
/// source that no public directory of `project_root` holds. A project with
/// no public directory at all is still checked: every asset is then missing.
pub(super) fn violations(sources: &[(&str, &str)], project_root: Option<&str>) -> Vec<Violation> {
    let Some(root) = project_root.map(Path::new).filter(|root| root.is_dir()) else {
        return Vec::new();
    };
    let mut public_dirs: Option<Vec<PathBuf>> = None;
    let mut found = Vec::new();
    for (name, source) in sources {
        let mut reported = BTreeSet::new();
        for (url, byte_range) in asset_urls(source) {
            if !reported.insert(url.clone()) {
                continue;
            }
            let dirs = public_dirs.get_or_insert_with(|| public_dirs_of(root));
            if served(dirs, &url) {
                continue;
            }
            let looked_in = dirs
                .iter()
                .map(|dir| {
                    dir.strip_prefix(root)
                        .unwrap_or(dir)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect::<Vec<_>>();
            let mut context = Map::new();
            context.insert("assetUrl".into(), json!(url));
            context.insert("publicDirs".into(), json!(looked_in));
            context.insert("nextAction".into(), json!({
                "tool": "devup_figma_export",
                "how": "Use the original Figma URL and the discovered icon nodeId in assetRequests; set outputPath under public/ to serve this assetUrl. Do not invent node IDs or replace the missing icon with a placeholder.",
                "assetUrl": url,
                "requiredInputs": ["url", "assetRequests[].nodeId", "assetRequests[].outputPath"]
            }));
            if !name.is_empty() {
                context.insert("sourceName".into(), json!(name));
            }
            found.push(Violation {
                rule: RULE,
                severity: Severity::Error,
                byte_range,
                context,
                message: if looked_in.is_empty() {
                    format!("{url} is referenced, but the project has no public directory, so no file serves it; the browser renders an empty or broken image.")
                } else {
                    format!("{url} is referenced, but no file at that path exists under {}; the browser renders an empty or broken image.", looked_in.join(", "))
                },
                suggestion: Some(
                    "Acquire the asset with devup_figma_export assetRequests (outputPath under public/, or assetPublicRoot so references are rewritten), or point the reference at the file that exists. Never leave a placeholder path in shipped code.".into(),
                ),
            });
        }
    }
    found
}

/// Whether a root-relative URL is a file under one of `dirs`. A URL with a
/// segment that could leave the directory is never served.
fn served(dirs: &[PathBuf], url: &str) -> bool {
    let segments = url.trim_start_matches('/').split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| {
        segment.is_empty() || matches!(*segment, "." | "..") || segment.contains(['\\', ':'])
    }) {
        return false;
    }
    dirs.iter().any(|dir| {
        segments
            .iter()
            .fold(dir.clone(), |path, segment| path.join(segment))
            .is_file()
    })
}

/// Root-relative asset URLs with their byte ranges: the quoted value of a
/// `src` prop or property, and the inside of every `url(...)`.
fn asset_urls(source: &str) -> Vec<(String, [usize; 2])> {
    #[derive(Default)]
    struct Literals {
        src: Vec<[usize; 2]>,
        strings: Vec<[usize; 2]>,
    }
    impl<'a> Visit<'a> for Literals {
        fn visit_jsx_attribute(&mut self, attr: &JSXAttribute<'a>) {
            if attr
                .name
                .as_identifier()
                .is_some_and(|name| name.name == "src")
            {
                let span = match &attr.value {
                    Some(JSXAttributeValue::StringLiteral(s)) => Some(s.span),
                    Some(JSXAttributeValue::ExpressionContainer(c)) => match &c.expression {
                        JSXExpression::StringLiteral(s) => Some(s.span),
                        JSXExpression::TemplateLiteral(t) if t.expressions.is_empty() => {
                            Some(t.span)
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(span) = span {
                    self.src.push([span.start as usize, span.end as usize]);
                }
            }
            walk::walk_jsx_attribute(self, attr);
        }
        fn visit_object_property(&mut self, prop: &ObjectProperty<'a>) {
            if !prop.computed
                && prop.key.static_name().as_deref() == Some("src")
                && matches!(
                    &prop.value,
                    Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
                )
            {
                let span = prop.value.span();
                self.src.push([span.start as usize, span.end as usize]);
            }
            walk::walk_object_property(self, prop);
        }
        fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
            self.strings
                .push([literal.span.start as usize, literal.span.end as usize]);
        }
        fn visit_template_literal(&mut self, literal: &TemplateLiteral<'a>) {
            if literal.expressions.is_empty() {
                self.strings
                    .push([literal.span.start as usize, literal.span.end as usize]);
            }
            walk::walk_template_literal(self, literal);
        }
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    let mut literals = Literals::default();
    literals.visit_program(&parsed.program);
    let mut urls = Vec::new();
    for span in literals.src {
        if let Some(raw) = source.get(span[0] + 1..span[1].saturating_sub(1))
            && let Some(url) = asset_path(raw)
        {
            urls.push((url, span));
        }
    }
    let mut cursor = 0;
    while let Some(offset) = source[cursor..].find("url(") {
        let open = cursor + offset + "url(".len();
        let Some(close) = source[open..].find(')') else {
            break;
        };
        let raw = source[open..open + close]
            .trim()
            .trim_matches(|c| matches!(c, '\'' | '"'));
        if let Some(url) = asset_path(raw)
            && literals
                .strings
                .iter()
                .any(|span| span[0] <= open && open + close < span[1])
        {
            urls.push((url, [open - "url(".len(), open + close + 1]));
        }
        cursor = open + close + 1;
    }
    urls.sort_by_key(|(_, range)| range[0]);
    urls
}

/// The decoded path of a root-relative asset URL, without query or fragment.
/// Template interpolation, protocol-relative and absolute URLs are not files
/// of this project and are left alone.
fn asset_path(raw: &str) -> Option<String> {
    if raw.contains("${") {
        return None;
    }
    let path = raw.split(['?', '#']).next().unwrap_or(raw);
    if !path.starts_with('/') || path.starts_with("//") {
        return None;
    }
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    ASSET_EXTENSIONS
        .contains(&extension.as_str())
        .then(|| percent_decode(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_uses_jsx_identity_across_files_not_comments_or_strings() {
        let expected = vec![
            "section".into(),
            "picture".into(),
            "absent".into(),
            "absent".into(),
        ];
        let found = coverage_violations(
            &[
                ("page.tsx", r#"<Box data-figma-node-id="section" />"#),
                (
                    "image.tsx",
                    r#"<Image data-figma-node-id={'picture'} />; /* <Box data-figma-node-id="absent" /> */ const example = '<Box data-figma-node-id="absent" />';"#,
                ),
            ],
            &expected,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].context["nodeId"], "absent");
        assert_eq!(found[0].severity, Severity::Error);
    }

    #[test]
    fn export_reports_omitted_sections_and_images_without_requiring_diagnostics() {
        use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
        let snapshot = serde_json::from_value(json!({
            "fileKey":"coverage", "roots":["root"], "diagnostics":[], "nodes": {
                "root":{"id":"root","type":"FRAME","fields":{"layoutMode":"VERTICAL","childrenIds":["section","hidden"]}},
                "section":{"id":"section","type":"FRAME","fields":{"layoutMode":"VERTICAL","childrenIds":["picture"]}},
                "picture":{"id":"picture","type":"RECTANGLE","fields":{"width":100,"height":100,"fills":[{"type":"IMAGE","imageHash":"photo"}]}},
                "hidden":{"id":"hidden","type":"FRAME","fields":{"visible":false,"childrenIds":["hiddenPicture"]}},
                "hiddenPicture":{"id":"hiddenPicture","type":"RECTANGLE","fields":{"fills":[{"type":"IMAGE","imageHash":"hidden"}]}}
            }
        })).unwrap();
        let mut output = generate_component(&snapshot, "root", &CodegenOptions::default()).unwrap();
        let contract = export_coverage(&snapshot, "root", &mut output, true, true);
        let attributes = devup_mcp_devup_ui::provenance::attributes::attribute_contract(&output);
        let unmapped = devup_mcp_devup_ui::provenance::attributes::uncovered_attributes(
            &output.tsx,
            &attributes,
        );
        assert!(
            !unmapped
                .iter()
                .any(|attr| attr["property"] == "data-figma-node-id"),
            "{unmapped:?}"
        );
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_SECTION_COVERAGE"),
            "{:?}",
            output.diagnostics
        );
        let ids: Vec<String> = serde_json::from_value(contract["nodeIds"].clone()).unwrap();
        assert!(ids.contains(&"section".into()), "{contract}");
        assert!(ids.contains(&"picture".into()), "{contract}");
        assert!(!ids.contains(&"hiddenPicture".into()));
        assert!(
            coverage_violations(&[("", &output.tsx)], &ids).is_empty(),
            "{}",
            output.tsx
        );
        // Simulate a generator silently dropping the two nodes from its map.
        output
            .source_map
            .entries
            .retain(|e| !matches!(e.node_id.as_deref(), Some("section" | "picture")));
        export_coverage(&snapshot, "root", &mut output, true, false);
        let missing = output
            .diagnostics
            .iter()
            .filter(|d| d.code == "DEVUP_CODEGEN_SECTION_COVERAGE")
            .collect::<Vec<_>>();
        assert_eq!(
            missing.iter().filter(|d| d.property.is_none()).count(),
            2,
            "{missing:?}"
        );
        assert!(
            missing
                .iter()
                .any(|d| d.property.as_deref() == Some("assetReference")),
            "{missing:?}"
        );
        assert!(
            missing
                .iter()
                .all(|d| d.fidelity_impact == Some(devup_mcp_figma::FidelityImpact::Lossy))
        );
    }

    #[test]
    fn src_props_and_style_urls_are_read() {
        let source = r#"const page = <><Image src="/images/hero.webp" />
<Box bg="url(/images/bg.png)" maskImage="url('/icons/a%20b.svg?v=2')" />
<Image src={"/icons/c.svg"} imgSrc="/x.png" srcSet="/y.png 2x" />
<Image src="https://cdn.example.com/d.png" />
<Image src={`/icons/${name}.svg`} />
<a href="/about">About</a></>;
const s = { src: '/fonts/a.woff2' }"#;
        let urls = asset_urls(source)
            .into_iter()
            .map(|(url, _)| url)
            .collect::<Vec<_>>();
        assert_eq!(
            urls,
            [
                "/images/hero.webp",
                "/images/bg.png",
                "/icons/a b.svg",
                "/icons/c.svg",
                "/fonts/a.woff2"
            ]
        );
    }

    #[test]
    fn comments_examples_and_dynamic_sources_are_not_asset_references() {
        let source = r#"// <Image src="/comment.png" />
const example = '<Image src="/example.png" />';
/* const style = { bg: "url(/comment.svg)" }; */
<Image src={"/prefix.png" + name} />;
<Image src={`/dynamic/${name}.png`} />;"#;
        assert!(asset_urls(source).is_empty(), "{:?}", asset_urls(source));
    }

    #[test]
    fn an_image_removed_from_an_existing_node_fails_reference_coverage() {
        let expected = vec!["/picture.png".into(), "/mask.svg".into()];
        let source =
            r#"<><Box data-figma-node-id="picture"/><Box maskImage="url(/mask.svg)" /></>"#;
        let found = asset_coverage_violations(&[("", source)], &expected);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].context["assetUrl"], "/picture.png");
    }

    #[test]
    fn delivery_refreshes_renamed_asset_urls_for_each_output_and_frame() {
        let mut result = json!({"frames":[{
            "tsx":"<Image src=\"/photos/final%20name.webp\" />",
            "componentTsx":"<Box maskImage=\"url(/icons/final.svg)\" />",
            "outputResults":{
                "tsx":{"sectionCoverage":{"nodeIds":["photo"],"assetUrls":["/images/placeholder.png"]}},
                "componentTsx":{"sectionCoverage":{"nodeIds":["icon"],"assetUrls":[]}}
            }
        }]});
        refresh_export_asset_urls(&mut result);
        let output = &result["frames"][0]["outputResults"];
        assert_eq!(
            output["tsx"]["sectionCoverage"]["assetUrls"],
            json!(["/photos/final name.webp"])
        );
        assert_eq!(
            output["componentTsx"]["sectionCoverage"]["assetUrls"],
            json!(["/icons/final.svg"])
        );
        assert_eq!(
            output["tsx"]["sectionCoverage"]["nodeIds"],
            json!(["photo"])
        );
    }

    #[test]
    fn traversal_is_never_served() {
        let dirs = vec![PathBuf::from("public")];
        assert!(!served(&dirs, "/../Cargo.toml"));
        assert!(!served(&dirs, "/a//b.png"));
        assert!(!served(&dirs, "/C:/x.png"));
    }

    #[test]
    fn a_missing_file_is_an_error_and_a_present_one_is_not() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "asset-guard-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(root.join("apps/web/public/images"))?;
        std::fs::write(root.join("apps/web/public/images/hero.webp"), b"RIFF")?;
        let tsx = r#"<><Image src="/images/hero.webp" /><Image src="/images/missing.png" /><Box bg="url(/images/missing.png)" /></>"#;
        let root_text = root.to_string_lossy().into_owned();

        let found = violations(&[("Page.tsx", tsx)], Some(&root_text));
        assert_eq!(found.len(), 1, "{found:?}");
        let violation = &found[0];
        assert_eq!(violation.rule, RULE);
        assert_eq!(violation.severity, Severity::Error);
        assert_eq!(violation.context["assetUrl"], "/images/missing.png");
        assert_eq!(violation.context["sourceName"], "Page.tsx");
        assert_eq!(violation.context["publicDirs"][0], "apps/web/public");
        assert_eq!(
            &tsx[violation.byte_range[0]..violation.byte_range[1]],
            "\"/images/missing.png\""
        );

        assert!(violations(&[("", tsx)], None).is_empty());
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn a_project_without_public_reports_every_asset() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "asset-guard-empty-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root)?;
        let found = violations(
            &[("", r#"<Image src="/a.png" />"#)],
            Some(&root.to_string_lossy()),
        );
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("no public directory"));
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }
}
