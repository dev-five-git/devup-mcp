//! `devup_ui_validate`'s `multicolor-mask` rule: a `maskImage` that points at
//! an SVG painted in more than one colour.
//!
//! A CSS mask keeps only the shape of its image and fills it with the one
//! `bg` colour, so a gradient face and a solid outline become one solid blob
//! and whatever the icon drew inside them disappears. The generator no
//! longer emits such a mask, but hand-written code (or a hand "fix" of an
//! `<Image>` icon) can, and nothing in the TSX alone shows it: the colours
//! are in the file. So when `projectRoot` is given, each `maskImage` URL is
//! looked up under the project's public directories and the SVG's own paints
//! are read.
//!
//! Like the rest of the ground-truth tools this reads the files on every
//! call and never caches them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use devup_mcp_devup_ui::ui_validate::{Severity, Violation};
use quick_xml::{Reader, events::BytesStart, events::Event};
use serde_json::{Map, json};

use super::project_root::find_dirs_named;

pub(super) const RULE: &str = "multicolor-mask";

/// SVG files larger than this are not read; no icon is anywhere near it.
const MAX_SVG_BYTES: u64 = 2 * 1024 * 1024;

/// How deep to look for `public` directories, enough for `apps/web/public`.
const PUBLIC_DIR_DEPTH: usize = 4;

/// One `multicolor-mask` warning per `maskImage` URL in `sources` whose SVG,
/// found under a public directory of `project_root`, paints more than one
/// colour. URLs that do not resolve to a file are left alone: the check
/// cannot speak about a file it cannot read.
pub(super) fn violations(sources: &[(&str, &str)], project_root: Option<&str>) -> Vec<Violation> {
    let Some(root) = project_root.map(Path::new).filter(|root| root.is_dir()) else {
        return Vec::new();
    };
    let mut public_dirs: Option<Vec<PathBuf>> = None;
    let mut found = Vec::new();
    for (name, source) in sources {
        for (url, byte_range) in mask_urls(source) {
            let public_dirs = public_dirs.get_or_insert_with(|| public_dirs_of(root));
            let Some(file) = resolve(public_dirs, &url) else {
                continue;
            };
            let Some(svg) = read_svg(&file) else {
                continue;
            };
            let paints = svg_paints(&svg);
            if !paints.is_multicolor() {
                continue;
            }
            let shown = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            let mut context = Map::new();
            context.insert("maskUrl".into(), json!(url));
            context.insert("file".into(), json!(shown));
            context.insert("paints".into(), json!(paints.describe()));
            if !name.is_empty() {
                context.insert("sourceName".into(), json!(name));
            }
            found.push(Violation {
                rule: RULE,
                severity: Severity::Warning,
                byte_range,
                context,
                message: format!(
                    "maskImage points at {shown}, which paints {}. A mask keeps only the shape and fills it with one bg colour, so this icon flattens to a single-colour blob.",
                    paints.summary()
                ),
                suggestion: Some(format!(
                    "Render it as <Image src=\"{url}\" /> with the same size and drop bg/maskImage/maskSize/maskPos/maskRepeat. Keep maskImage only for single-colour icons."
                )),
            });
        }
    }
    found
}

/// Every `url(...)` inside a `maskImage` value, as JSX (`maskImage="..."`,
/// `maskImage={...}`) or object (`maskImage: "..."`), with its byte range.
/// Only root-relative `.svg` URLs are kept, decoded and without query or
/// fragment.
fn mask_urls(source: &str) -> Vec<(String, [usize; 2])> {
    let bytes = source.as_bytes();
    let mut urls = Vec::new();
    let mut from = 0;
    while let Some(offset) = source[from..].find("maskImage") {
        let mut index = from + offset + "maskImage".len();
        from = index;
        index = skip_space(bytes, index);
        if !matches!(bytes.get(index), Some(b'=' | b':')) {
            continue;
        }
        index = skip_space(bytes, index + 1);
        let Some(end) = value_end(bytes, index) else {
            continue;
        };
        let value = &source[index..end];
        let mut cursor = 0;
        while let Some(start) = value[cursor..].find("url(") {
            let open = cursor + start + "url(".len();
            let Some(close) = value[open..].find(')') else {
                break;
            };
            let raw = value[open..open + close]
                .trim()
                .trim_matches(|c| matches!(c, '\'' | '"'));
            cursor = open + close + 1;
            let path = raw.split(['?', '#']).next().unwrap_or(raw);
            if path.starts_with('/')
                && !path.starts_with("//")
                && path.to_ascii_lowercase().ends_with(".svg")
            {
                urls.push((
                    percent_decode(path),
                    [index + open - "url(".len(), index + cursor],
                ));
            }
        }
        from = end;
    }
    urls
}

fn skip_space(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

/// Where a prop or property value that starts at `start` ends: the closing
/// quote of a string, or the brace that balances an expression container.
fn value_end(bytes: &[u8], start: usize) -> Option<usize> {
    match *bytes.get(start)? {
        quote @ (b'"' | b'\'' | b'`') => bytes[start + 1..]
            .iter()
            .position(|byte| *byte == quote)
            .map(|position| start + 1 + position + 1),
        b'{' => {
            let mut depth = 0usize;
            let mut quote = None;
            for (position, byte) in bytes.iter().enumerate().skip(start) {
                match (quote, *byte) {
                    (Some(open), byte) if byte == open => quote = None,
                    (Some(_), _) => {}
                    (None, b'"' | b'\'' | b'`') => quote = Some(*byte),
                    (None, b'{') => depth += 1,
                    (None, b'}') => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(position + 1);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}

fn percent_decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(byte) = path
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            decoded.push(byte);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| path.to_owned())
}

/// `<root>/public` first, then any other `public` directory the project's
/// packages have, as the authoritative scan finds them.
fn public_dirs_of(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let top = root.join("public");
    if top.is_dir() {
        dirs.push(top);
    }
    for dir in find_dirs_named(root, "public", PUBLIC_DIR_DEPTH).0 {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// The file a root-relative URL is served from, refusing any segment that
/// could leave the public directory.
fn resolve(public_dirs: &[PathBuf], url: &str) -> Option<PathBuf> {
    let segments = url.trim_start_matches('/').split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| {
        segment.is_empty() || matches!(*segment, "." | "..") || segment.contains(['\\', ':'])
    }) {
        return None;
    }
    public_dirs.iter().find_map(|dir| {
        let file = segments
            .iter()
            .fold(dir.clone(), |path, segment| path.join(segment));
        file.is_file().then_some(file)
    })
}

fn read_svg(file: &Path) -> Option<String> {
    let length = std::fs::metadata(file).ok()?.len();
    if length > MAX_SVG_BYTES {
        return None;
    }
    std::fs::read_to_string(file).ok()
}

/// The paints an SVG draws with, outside `<clipPath>`, `<mask>` and other
/// definitions that are never painted themselves.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SvgPaints {
    /// Flat colours, normalised so `#FFF` and `white` are one.
    colors: BTreeSet<String>,
    /// `url(#...)` paints: gradients or patterns, more than one colour each.
    references: BTreeSet<String>,
    /// `<image>` elements: pixels a mask would throw away.
    images: usize,
}

impl SvgPaints {
    fn is_multicolor(&self) -> bool {
        self.colors.len() > 1 || !self.references.is_empty() || self.images > 0
    }

    fn describe(&self) -> Vec<String> {
        let mut paints = self.colors.iter().cloned().collect::<Vec<_>>();
        paints.extend(self.references.iter().cloned());
        if self.images > 0 {
            paints.push(format!("{} <image>", self.images));
        }
        paints
    }

    fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.colors.is_empty() {
            parts.push(format!(
                "{} colour(s) ({})",
                self.colors.len(),
                self.colors.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if !self.references.is_empty() {
            parts.push(format!(
                "{} gradient/pattern fill(s)",
                self.references.len()
            ));
        }
        if self.images > 0 {
            parts.push(format!("{} embedded image(s)", self.images));
        }
        parts.join(" and ")
    }
}

/// Elements whose subtree defines something rather than painting it.
const UNPAINTED: &[&str] = &[
    "clippath",
    "mask",
    "defs",
    "lineargradient",
    "radialgradient",
    "pattern",
    "symbol",
    "marker",
    "filter",
    "style",
    "title",
    "desc",
    "metadata",
];

/// Elements that paint their fill and stroke.
const SHAPES: &[&str] = &[
    "path", "rect", "circle", "ellipse", "polygon", "polyline", "line", "text", "use",
];

/// Inherited state for the element being read.
#[derive(Clone, Default)]
struct Inherited {
    fill: Option<String>,
    stroke: Option<String>,
    unpainted: bool,
}

pub(super) fn svg_paints(svg: &str) -> SvgPaints {
    let mut paints = SvgPaints::default();
    let mut reader = Reader::from_str(svg);
    reader.config_mut().check_end_names = false;
    let mut stack = vec![Inherited::default()];
    loop {
        let (element, opens) = match reader.read_event() {
            Ok(Event::Start(element)) => (element, true),
            Ok(Event::Empty(element)) => (element, false),
            Ok(Event::End(_)) => {
                if stack.len() > 1 {
                    stack.pop();
                }
                continue;
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => continue,
        };
        let parent = stack.last().cloned().unwrap_or_default();
        let current = paint_element(&element, &parent, &mut paints);
        if opens {
            stack.push(current);
        }
    }
    paints
}

/// Records what `element` paints and returns what its children inherit.
fn paint_element(
    element: &BytesStart<'_>,
    parent: &Inherited,
    paints: &mut SvgPaints,
) -> Inherited {
    let name = element.local_name().as_ref().to_ascii_lowercase();
    let mut fill = None;
    let mut stroke = None;
    let mut hidden = false;
    let mut fill_hidden = false;
    let mut stroke_hidden = false;
    let mut apply = |key: &str, value: &str| match key {
        "fill" => fill = Some(value.trim().to_owned()),
        "stroke" => stroke = Some(value.trim().to_owned()),
        "opacity" => hidden |= is_zero(value),
        "fill-opacity" => fill_hidden |= is_zero(value),
        "stroke-opacity" => stroke_hidden |= is_zero(value),
        "display" => hidden |= value.trim() == "none",
        "visibility" => hidden |= value.trim() == "hidden",
        _ => {}
    };
    for attribute in element.attributes().with_checks(false).flatten() {
        let key = attribute.key.local_name().as_ref().to_ascii_lowercase();
        let value = quick_xml::escape::unescape(&attribute.value)
            .map_or_else(|_| attribute.value.to_string(), |value| value.into_owned());
        if key == "style" {
            for declaration in value.split(';') {
                if let Some((key, value)) = declaration.split_once(':') {
                    apply(&key.trim().to_ascii_lowercase(), value);
                }
            }
        } else {
            apply(&key, &value);
        }
    }
    let inherit = |own: Option<String>, inherited: &Option<String>| match own {
        Some(value) if value != "inherit" => Some(value),
        _ => inherited.clone(),
    };
    let current = Inherited {
        fill: inherit(fill, &parent.fill),
        stroke: inherit(stroke, &parent.stroke),
        unpainted: parent.unpainted || hidden || UNPAINTED.contains(&name.as_str()),
    };
    if current.unpainted {
        return current;
    }
    if name == "image" {
        paints.images += 1;
    } else if SHAPES.contains(&name.as_str()) {
        // An unset fill is black; a line has no area to fill.
        if name != "line" && !fill_hidden {
            add_paint(paints, current.fill.as_deref().unwrap_or("black"));
        }
        if let Some(stroke) = current.stroke.as_deref().filter(|_| !stroke_hidden) {
            add_paint(paints, stroke);
        }
    }
    current
}

fn is_zero(value: &str) -> bool {
    value
        .trim()
        .trim_end_matches('%')
        .parse::<f64>()
        .is_ok_and(|number| number == 0.0)
}

fn add_paint(paints: &mut SvgPaints, value: &str) {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "" | "none" | "transparent" => {}
        reference if reference.starts_with("url(") => {
            paints.references.insert(reference.to_owned());
        }
        color => {
            paints.colors.insert(normalize_color(color));
        }
    }
}

fn normalize_color(color: &str) -> String {
    match color {
        "black" => return "#000000".to_owned(),
        "white" => return "#ffffff".to_owned(),
        _ => {}
    }
    if let Some(hex) = color.strip_prefix('#')
        && matches!(hex.len(), 3 | 4)
        && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        let mut long = String::from("#");
        for digit in hex.chars() {
            long.push(digit);
            long.push(digit);
        }
        return long;
    }
    color.split_whitespace().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The prime "find" icon as Figma exports it: a gradient face under a
    /// solid outline, clipped by a white rect.
    const FIND: &str = r##"<svg width="64" height="64" viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
<g clip-path="url(#clip0_1_2)">
<path d="M8 8h40v40H8z" fill="url(#paint0_linear_1_2)"/>
<path d="M10 10h36v36H10z" fill="#552AFF" fill-opacity="0.75"/>
<path d="M40 40l12 12" stroke="#552AFF" stroke-opacity="0.75" stroke-width="4"/>
</g>
<defs>
<linearGradient id="paint0_linear_1_2" x1="8" y1="8" x2="48" y2="48" gradientUnits="userSpaceOnUse">
<stop stop-color="#F4F3FF"/>
<stop offset="1" stop-color="#E7E6FF"/>
</linearGradient>
<clipPath id="clip0_1_2">
<rect width="64" height="64" fill="white"/>
</clipPath>
</defs>
</svg>"##;

    #[test]
    fn a_gradient_with_a_solid_outline_is_multicolor() {
        let paints = svg_paints(FIND);
        assert!(paints.is_multicolor(), "{paints:?}");
        assert_eq!(paints.colors, BTreeSet::from(["#552aff".to_owned()]));
        assert_eq!(paints.references.len(), 1);
    }

    #[test]
    fn a_clip_path_rect_does_not_count_as_a_paint() {
        let svg = r##"<svg fill="none" xmlns="http://www.w3.org/2000/svg">
<g clip-path="url(#c)"><path d="M0 0h1v1z" fill="#552AFF"/><path d="M2 2h1v1z" fill="#552aff"/></g>
<defs><clipPath id="c"><rect width="64" height="64" fill="white"/></clipPath>
<mask id="m"><rect width="64" height="64" fill="#fff"/></mask></defs>
</svg>"##;
        let paints = svg_paints(svg);
        assert!(!paints.is_multicolor(), "{paints:?}");
        assert_eq!(paints.colors, BTreeSet::from(["#552aff".to_owned()]));
    }

    #[test]
    fn two_colours_and_inherited_or_default_fills_are_counted() {
        let two =
            r##"<svg><path d="M0 0" fill="#000"/><path d="M1 1" stroke="red" fill="none"/></svg>"##;
        assert!(svg_paints(two).is_multicolor());

        // `<g fill>` reaches its paths, and an unset fill is black.
        let inherited = r##"<svg><g fill="#fff"><path d="M0 0"/></g><path d="M1 1"/></svg>"##;
        assert_eq!(svg_paints(inherited).colors.len(), 2);

        let style = r##"<svg><path style="fill: #111; stroke:#111" d="M0 0"/></svg>"##;
        assert!(!svg_paints(style).is_multicolor());
    }

    #[test]
    fn hidden_paints_and_embedded_images() {
        let hidden = r##"<svg><path d="M0 0" fill="#000"/><path d="M1 1" fill="red" fill-opacity="0"/><path d="M1 1" fill="blue" opacity="0"/></svg>"##;
        assert!(!svg_paints(hidden).is_multicolor());

        let image = r##"<svg><path d="M0 0" fill="#000"/><image href="data:image/png;base64,AA=="/></svg>"##;
        assert!(svg_paints(image).is_multicolor());
    }

    #[test]
    fn mask_urls_are_read_from_jsx_and_object_values() {
        let source = r#"<Box maskImage="url(/icons/find.svg)" />
<Box maskImage={{ a: "url('/icons/%EC%86%8D%EC%84%B1 1=a.svg?v=1')", b: "url(/icons/b.svg)" }[x]} />
const s = css({ maskImage: 'url("/icons/c.svg")' })
<Box maskImage="url(https://cdn.example.com/d.svg)" bg="red" src="/icons/e.svg" />"#;
        let urls = mask_urls(source)
            .into_iter()
            .map(|(url, range)| {
                assert!(source[range[0]..range[1]].starts_with("url("));
                url
            })
            .collect::<Vec<_>>();
        assert_eq!(
            urls,
            [
                "/icons/find.svg",
                "/icons/속성 1=a.svg",
                "/icons/b.svg",
                "/icons/c.svg"
            ]
        );
    }

    #[test]
    fn urls_cannot_leave_the_public_directory() {
        let dirs = vec![PathBuf::from("public")];
        assert_eq!(resolve(&dirs, "/../secret.svg"), None);
        assert_eq!(resolve(&dirs, "/icons//x.svg"), None);
        assert_eq!(resolve(&dirs, "/C:/x.svg"), None);
    }

    #[test]
    fn a_project_file_is_read_and_named() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "mask-guard-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(root.join("public/icons"))?;
        std::fs::write(root.join("public/icons/find.svg"), FIND)?;
        std::fs::write(
            root.join("public/icons/arrow.svg"),
            r##"<svg fill="none"><path d="M0 0" fill="#552AFF"/></svg>"##,
        )?;
        let tsx = r##"<><Box bg="#552AFFBF" maskImage="url(/icons/find.svg)" /><Box bg="$primary" maskImage="url(/icons/arrow.svg)" /><Box maskImage="url(/icons/missing.svg)" /></>"##;
        let root_text = root.to_string_lossy().into_owned();

        let found = violations(&[("Page.tsx", tsx)], Some(&root_text));
        assert_eq!(found.len(), 1, "{found:?}");
        let violation = &found[0];
        assert_eq!(violation.rule, RULE);
        assert_eq!(violation.severity, Severity::Warning);
        assert_eq!(violation.context["maskUrl"], "/icons/find.svg");
        assert_eq!(violation.context["sourceName"], "Page.tsx");
        assert!(
            violation.message.contains("find.svg"),
            "{}",
            violation.message
        );
        assert!(
            violation
                .suggestion
                .as_deref()
                .is_some_and(|suggestion| suggestion.contains("<Image src=\"/icons/find.svg\"")),
            "{violation:?}"
        );

        assert!(violations(&[("", tsx)], None).is_empty());
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }
}
