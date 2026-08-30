use serde_json::{Map, Value, json};

use super::normalize_component_name;

pub fn render_codegen_provider(input: &Value, pure_code: &str) -> Option<String> {
    if input
        .get("language")
        .and_then(Value::as_str)
        .is_some_and(|value| value != "devup-ui")
    {
        return Some("[]".to_owned());
    }
    let node = input.get("node").unwrap_or(input);
    let name = normalize_component_name(node.get("name")?.as_str()?);
    let node_type = node.get("type")?.as_str()?;
    let section_pure;
    let pure_code = if node_type == "SECTION" {
        section_pure = format!(
            "<Box boxSize=\"100%\">\n{}\n</Box>",
            node.get("children")?
                .as_array()?
                .iter()
                .map(|_| "  <Box boxSize=\"100%\" />")
                .collect::<Vec<_>>()
                .join("\n")
        );
        section_pure.as_str()
    } else {
        pure_code
    };
    let mut entries = vec![provider_entry("Pure Code", "TYPESCRIPT", pure_code)];
    match node_type {
        "FRAME" => entries.push(provider_entry(&name, "TYPESCRIPT", pure_code)),
        "COMPONENT" => {
            entries.push(provider_entry(
                "Usage",
                "TYPESCRIPT",
                &format!("<{name} />"),
            ));
            let source = component_source(&name, pure_code);
            entries.push(provider_entry(&name, "TYPESCRIPT", &source));
            entries.extend(cli_entries(&name, &name, &source));
        }
        "SECTION" => {
            entries.push(provider_entry(&name, "TYPESCRIPT", pure_code));
            let source = component_source(&name, "<Box boxSize=\"100%\" />");
            let title = format!("{name} - Responsive");
            entries.push(provider_entry(&title, "TYPESCRIPT", &source));
            entries.extend(cli_entries(&name, &title, &source));
        }
        _ => return None,
    }
    Some(format!("[\n{}\n]", entries.join("\n")))
}

fn provider_entry(title: &str, language: &str, code: &str) -> String {
    let code = if code.contains('\n') {
        format!("\n\"{code}\"\n")
    } else {
        format!("\"{code}\"")
    };
    format!(
        "  {{\n    \"code\": {code},\n    \"language\": \"{language}\",\n    \"title\": \"{title}\",\n  }},"
    )
}

fn component_source(name: &str, code: &str) -> String {
    format!(
        "import {{ Box }} from '@devup-ui/react'\n\nexport function {name}() {{\n  return {code}\n}}"
    )
}

fn cli_entries(file_name: &str, title: &str, source: &str) -> [String; 2] {
    let bash_source = source.replace('\'', "\\'");
    let bash =
        format!("mkdir -p src/components\n\necho '{bash_source}' > src/components/{file_name}.tsx");
    let powershell = format!(
        "New-Item -ItemType Directory -Force -Path src\\components | Out-Null\n\n@'\n{source}\n'@ | Out-File -FilePath src\\components\\{file_name}.tsx -Encoding UTF8"
    );
    let cli_title = if title == file_name {
        format!("{title} - CLI")
    } else {
        format!("{title} CLI")
    };
    [
        provider_entry(&format!("{cli_title} (Bash)"), "BASH", &bash),
        provider_entry(&format!("{cli_title} (PowerShell)"), "BASH", &powershell),
    ]
}

pub fn render_viewport_component(input: &Value) -> Option<String> {
    if input.get("type")?.as_str()? != "COMPONENT_SET" {
        return None;
    }
    let name = normalize_component_name(input.get("name")?.as_str()?);
    let definitions = input.get("componentPropertyDefinitions")?.as_object()?;
    let children = input.get("children")?.as_array()?;
    let mut interface = Vec::new();
    let mut props = Vec::new();
    for (key, definition) in definitions {
        if definition.get("type").and_then(Value::as_str) != Some("VARIANT") {
            continue;
        }
        let prop = normalize_prop_name(key);
        let options = definition.get("variantOptions")?.as_array()?;
        let values = options.iter().filter_map(Value::as_str).collect::<Vec<_>>();
        let boolean = values == ["false", "true"];
        if boolean {
            interface.push(format!("  {prop}?: boolean"));
        } else {
            interface.push(format!(
                "  {prop}: {}",
                values
                    .iter()
                    .map(|value| format!("'{value}'"))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        props.push((key.as_str(), prop, boolean));
    }
    let (_, prop, boolean) = props.first()?;
    let asset_components = children
        .iter()
        .all(|child| child.get("isAsset").and_then(Value::as_bool) == Some(true));
    let component = if asset_components { "Box" } else { "Image" };
    let mut variants = Vec::new();
    for child in children {
        let value = child.get("variantProperties")?.get(props[0].0)?.as_str()?;
        let child_name = child.get("name")?.as_str()?;
        let path = format!("/icons/{child_name}.svg");
        let rendered = if asset_components {
            format!("url('{path}')")
        } else {
            path
        };
        variants.push((value, rendered));
    }
    let variant_lines = variants
        .iter()
        .map(|(key, value)| format!("        {key}: \"{value}\""))
        .collect::<Vec<_>>()
        .join(",\n");
    let index = if *boolean {
        format!("{prop} ?? false")
    } else {
        prop.clone()
    };
    let extra = if asset_components {
        let color = children
            .first()?
            .get("children")?
            .as_array()?
            .first()?
            .get("fills")?
            .as_array()?
            .first()?
            .get("color")?;
        format!(
            "      bg=\"{}\"\n      maskImage={{{{\n{variant_lines}\n      }}[{index}]}}\n      maskPos=\"center\"\n      maskRepeat=\"no-repeat\"\n      maskSize=\"contain\"",
            color_hex(color)?
        )
    } else {
        format!("      src={{{{\n{variant_lines}\n      }}[{index}]}}")
    };
    Some(format!(
        "\"export interface {name}Props {{\n{}\n}}\n\nexport function {name}({{ {prop} }}: {name}Props) {{\n  return (\n    <{component}\n{extra}\n    />\n  )\n}}\"",
        interface.join("\n")
    ))
}

pub fn render_variant_tree_merge(input: &Value) -> Option<String> {
    let variant_key = input.get("variantKey")?.as_str()?;
    let entries = input.get("treesByVariant")?.as_array()?;
    let variants = entries
        .iter()
        .filter_map(|entry| {
            let entry = entry.as_array()?;
            Some((entry.first()?.as_str()?, entry.get(1)?))
        })
        .collect::<Vec<_>>();
    let rendered = render_merged_nodes(variant_key, &variants);
    Some(format!("\"{rendered}\""))
}

fn render_merged_nodes(variant_key: &str, variants: &[(&str, &Value)]) -> String {
    let Some((_, first)) = variants.first() else {
        return String::new();
    };
    let component = first
        .get("component")
        .and_then(Value::as_str)
        .unwrap_or("Box");
    let props = merge_props(
        variant_key,
        &variants
            .iter()
            .map(|(variant, node)| (*variant, node.get("props").unwrap_or(&Value::Null)))
            .collect::<Vec<_>>(),
    );
    let mut result = format!(
        "render:{component}:depth=0:{}|",
        serde_json::to_string(&props).expect("serializable merged props")
    );
    let max_children = variants
        .iter()
        .filter_map(|(_, node)| node.get("children")?.as_array().map(Vec::len))
        .max()
        .unwrap_or(0);
    for index in 0..max_children {
        let child_variants = variants
            .iter()
            .filter_map(|(variant, node)| {
                Some((*variant, node.get("children")?.as_array()?.get(index)?))
            })
            .collect::<Vec<_>>();
        let child = render_merged_nodes(variant_key, &child_variants);
        if child_variants.len() == variants.len() {
            result.push_str(&child);
        } else {
            let condition = child_variants
                .iter()
                .map(|(variant, _)| format!("{variant_key} === \"{variant}\""))
                .collect::<Vec<_>>()
                .join(" || ");
            result.push_str(&format!("{{({condition}) && {child}}}"));
        }
    }
    result
}

fn merge_props(variant_key: &str, variants: &[(&str, &Value)]) -> Map<String, Value> {
    let mut keys = Vec::new();
    for (_, props) in variants {
        if let Some(props) = props.as_object() {
            for key in props.keys() {
                if !keys.contains(key) {
                    keys.push(key.clone());
                }
            }
        }
    }
    let mut merged = Map::new();
    for key in keys {
        let values = variants
            .iter()
            .filter_map(|(variant, props)| Some((*variant, props.get(&key)?.clone())))
            .collect::<Vec<_>>();
        if values.len() == variants.len()
            && values
                .iter()
                .skip(1)
                .all(|(_, value)| value == &values[0].1)
        {
            merged.insert(key, values[0].1.clone());
        } else {
            let variant_values = values
                .into_iter()
                .map(|(variant, value)| (variant.to_owned(), value))
                .collect::<Map<_, _>>();
            merged.insert(
                key,
                json!({
                    "__variantProp": true,
                    "variantKey": variant_key,
                    "values": variant_values,
                }),
            );
        }
    }
    merged
}

pub fn render_responsive_component_mock(input: &Value) -> Option<String> {
    let tree = input.get("mockTree")?;
    let name = normalize_component_name(input.get("name")?.as_str()?);
    let definitions = input.get("componentPropertyDefinitions")?.as_object()?;
    let mut prop_types = Map::new();
    for (key, definition) in definitions {
        if matches!(key.to_ascii_lowercase().as_str(), "effect" | "viewport") {
            continue;
        }
        let options = definition.get("variantOptions")?.as_array()?;
        let union = options
            .iter()
            .filter_map(Value::as_str)
            .map(|value| format!("'{value}'"))
            .collect::<Vec<_>>()
            .join(" | ");
        prop_types.insert(key.clone(), Value::String(union));
    }
    let variant_key = prop_types
        .keys()
        .find(|key| key.as_str() == "variant")?
        .clone();
    let children = input.get("children")?.as_array()?;
    let mut root_props = tree.get("props")?.as_object()?.clone();
    for (effect, selector) in [("hover", "_hover"), ("active", "_active")] {
        let mut colors = Map::new();
        for child in children {
            let variants = child.get("variantProperties")?.as_object()?;
            if variants.get("effect").and_then(Value::as_str) != Some(effect)
                || variants.get("viewport").and_then(Value::as_str) != Some("Desktop")
                || variants.get("size").and_then(Value::as_str) != Some("Md")
            {
                continue;
            }
            let variant = variants.get(&variant_key)?.as_str()?;
            let color = child.get("fills")?.as_array()?.first()?.get("color")?;
            colors.insert(variant.to_owned(), Value::String(color_hex(color)?));
        }
        root_props.insert(
            selector.to_owned(),
            json!({"bg": {"__variantProp": true, "variantKey": variant_key, "values": colors}}),
        );
    }
    let mut result = format!(
        "component:{name}:{}|render:{}:depth=0:{}|",
        serde_json::to_string(&prop_types).ok()?,
        tree.get("component")?.as_str()?,
        serde_json::to_string(&root_props).ok()?
    );
    for child in tree.get("children")?.as_array()? {
        result.push_str(&format!(
            "render:{}:depth=0:{}|",
            child.get("component")?.as_str()?,
            serde_json::to_string(child.get("props")?).ok()?
        ));
    }
    Some(format!("\"{result}\""))
}

fn normalize_prop_name(value: &str) -> String {
    let mut words = value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty());
    let first = words.next().unwrap_or("property").to_ascii_lowercase();
    let mut result = words.fold(first, |mut result, word| {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            result.push(first.to_ascii_uppercase());
            result.extend(chars.map(|character| character.to_ascii_lowercase()));
        }
        result
    });
    if result.starts_with(|character: char| character.is_ascii_digit()) {
        result.insert_str(0, "property");
    }
    result
}

fn color_hex(color: &Value) -> Option<String> {
    let channel =
        |name: &str| Some((color.get(name)?.as_f64()?.clamp(0.0, 1.0) * 255.0).round() as u8);
    let value = format!(
        "#{:02X}{:02X}{:02X}",
        channel("r")?,
        channel("g")?,
        channel("b")?
    );
    Some(value)
}
