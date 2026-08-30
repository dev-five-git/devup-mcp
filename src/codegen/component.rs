use std::collections::{BTreeSet, HashSet};

use crate::figma::{DevupError, Diagnostic, ErrorCode, RawNode, Snapshot};

use super::{layout, style, text};

#[derive(Debug, Clone, Default)]
pub struct CodegenOptions {
    pub component_name: Option<String>,
    pub include_diagnostics: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenOutput {
    pub tsx: String,
    pub imports: Vec<String>,
    pub used_tokens: BTreeSet<String>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn generate_component(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Figma snapshot에서 변환할 node를 찾지 못했습니다.",
            false,
        )
    })?;
    let component_name = options
        .component_name
        .as_deref()
        .map(normalize_component_name)
        .unwrap_or_else(|| normalize_component_name(root.typed_view().name().unwrap_or("")));
    let mut context = Context::default();
    let jsx = render_node(snapshot, root, 2, &mut context, &mut HashSet::new())?;
    let imports = context.imports.iter().cloned().collect::<Vec<_>>();
    let mut tsx = format!(
        "import {{ {} }} from \"@devup-ui/react\";\n\n",
        imports.join(", ")
    );
    tsx.push_str(&format!(
        "export function {component_name}() {{\n  return (\n{jsx}\n  );\n}}\n"
    ));
    Ok(CodegenOutput {
        tsx,
        imports,
        used_tokens: context.used_tokens,
        diagnostics: context.diagnostics,
    })
}

#[derive(Default)]
struct Context {
    imports: BTreeSet<String>,
    used_tokens: BTreeSet<String>,
    diagnostics: Vec<Diagnostic>,
}

fn render_node(
    snapshot: &Snapshot,
    node: &RawNode,
    depth: usize,
    context: &mut Context,
    visiting: &mut HashSet<String>,
) -> Result<String, DevupError> {
    if !visiting.insert(node.id.clone()) {
        return Err(DevupError::new(
            ErrorCode::DevupCodegenFailed,
            "Figma node 트리에 순환 참조가 있습니다.",
            false,
        ));
    }
    add_fallback_diagnostics(node, context);
    let view = node.typed_view();
    let component = if view.node_type() == "TEXT" {
        "Text"
    } else if view.string("layoutMode").is_some_and(|mode| mode != "NONE") {
        "Flex"
    } else {
        "Box"
    };
    context.imports.insert(component.to_owned());

    let mut props = Vec::new();
    layout::push_layout_props(&view, &mut props);
    style::push_style_props(&view, component, &mut props, &mut context.used_tokens);
    text::push_text_props(&view, &mut props);
    let props = if props.is_empty() {
        String::new()
    } else {
        format!(" {}", props.join(" "))
    };
    let indent = "  ".repeat(depth);
    let children = view
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .map(|child| render_node(snapshot, child, depth + 1, context, visiting))
        .collect::<Result<Vec<_>, _>>()?;

    let rendered = if component == "Text" {
        let characters = view
            .string("characters")
            .map(text::escape_jsx_text)
            .unwrap_or_default();
        format!("{indent}<Text{props}>{characters}</Text>")
    } else if children.is_empty() {
        format!("{indent}<{component}{props} />")
    } else {
        format!(
            "{indent}<{component}{props}>\n{}\n{indent}</{component}>",
            children.join("\n")
        )
    };
    visiting.remove(&node.id);
    Ok(rendered)
}

fn add_fallback_diagnostics(node: &RawNode, context: &mut Context) {
    let view = node.typed_view();
    let candidates = [
        (
            view.bool("isMask") == Some(true),
            "DEVUP_CODEGEN_MASK_FALLBACK",
            "Mask는 기본 Box 렌더링으로 보존됩니다.",
        ),
        (
            view.string("layoutPositioning") == Some("ABSOLUTE"),
            "DEVUP_CODEGEN_ABSOLUTE_FALLBACK",
            "절대 배치는 position props로 제한적으로 변환됩니다.",
        ),
        (
            view.value("effects")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|effects| !effects.is_empty()),
            "DEVUP_CODEGEN_EFFECT_FALLBACK",
            "일부 Figma effect는 계산된 CSS로 변환되지 않을 수 있습니다.",
        ),
    ];
    for (enabled, code, message) in candidates {
        if enabled {
            context.diagnostics.push(Diagnostic {
                code: code.to_owned(),
                message: message.to_owned(),
                node_id: Some(node.id.clone()),
            });
        }
    }
}

pub fn normalize_component_name(input: &str) -> String {
    let mut result = String::new();
    let mut segment = String::new();
    for character in input.chars().chain(std::iter::once(' ')) {
        if character.is_alphanumeric() || character == '_' {
            segment.push(character);
            continue;
        }
        if !segment.is_empty() {
            let all_ascii_upper = segment
                .chars()
                .filter(|character| character.is_ascii_alphabetic())
                .all(|character| character.is_ascii_uppercase());
            let mut characters = segment.chars();
            if let Some(first) = characters.next() {
                result.extend(first.to_uppercase());
                for rest in characters {
                    if all_ascii_upper {
                        result.extend(rest.to_lowercase());
                    } else {
                        result.push(rest);
                    }
                }
            }
            segment.clear();
        }
    }
    if result.is_empty() {
        return "FigmaComponent".to_owned();
    }
    if result
        .chars()
        .next()
        .is_some_and(|character| character.is_numeric())
    {
        result.insert(0, '_');
    }
    result
}
