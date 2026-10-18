

fn push_effects(
    view: &TypedNode<'_>,
    component: &str,
    props: &mut Vec<Prop>,
    used_tokens: &mut BTreeSet<String>,
    variable_tokens: &std::collections::BTreeMap<String, String>,
    lengths: Lengths<'_>,
) {
    let Some(effects) = view.value("effects").and_then(Value::as_array) else {
        return;
    };
    let visible = effects
        .iter()
        .filter(|effect| effect.get("visible").and_then(Value::as_bool) != Some(false))
        .collect::<Vec<_>>();
    let shadows = visible
        .iter()
        .filter(|effect| {
            matches!(
                effect.get("type").and_then(Value::as_str),
                Some("DROP_SHADOW" | "INNER_SHADOW")
            )
        })
        .filter_map(|effect| {
            let offset = effect.get("offset")?;
            let x = offset.get("x")?.as_f64()?;
            let y = offset.get("y")?.as_f64()?;
            let radius = effect.get("radius")?.as_f64()?;
            let spread = effect.get("spread").and_then(Value::as_f64).unwrap_or(0.0);
            // A shadow's colour can be bound to a variable, exactly as a fill
            // or a stroke can, and then the token is what the design means:
            // the landing page's cards are `$shadow`, one value the theme can
            // move for dark mode. Written as the resolved `#87878740` they
            // were a colour nothing could reach.
            let color = if let Some(token) = bound_paint_token(effect, variable_tokens) {
                used_tokens.insert(token.clone());
                format!("${token}")
            } else {
                color_from(effect.get("color")?)?
            };
            let inset = if effect.get("type").and_then(Value::as_str) == Some("INNER_SHADOW") {
                "inset "
            } else {
                ""
            };
            let mut length = |field: &str, value: f64| {
                let literal = zero_or_px(value);
                match css_length(&literal).and_then(|number| {
                    lengths.token(
                        [effect.pointer(&format!("/boundVariables/{field}"))],
                        number,
                    )
                }) {
                    Some(token) => {
                        used_tokens.insert(token[1..].to_owned());
                        token
                    }
                    None => literal,
                }
            };
            let (x, y, radius) = (
                length("offsetX", x),
                length("offsetY", y),
                length("radius", radius),
            );
            Some(if component == "Text" {
                format!("{inset}{x} {y} {radius} {color}")
            } else {
                format!(
                    "{inset}{x} {y} {radius} {} {color}",
                    length("spread", spread)
                )
            })
        })
        .collect::<Vec<_>>();
    if !shadows.is_empty() {
        let mut composed = shadows.join(", ");
        if component != "Text"
            && single_outside_stroke(view).is_some()
            && let Some((_, super::component::PropValue::String(stroke))) =
                props.iter().find(|(name, _)| name == "boxShadow")
        {
            composed = format!("{stroke}, {composed}");
        }
        string_prop(
            props,
            if component == "Text" {
                "textShadow"
            } else {
                "boxShadow"
            },
            composed,
        );
    }
    for effect in visible {
        let radius = effect.get("radius").and_then(Value::as_f64).unwrap_or(0.0);
        match effect.get("type").and_then(Value::as_str) {
            Some("LAYER_BLUR") => string_prop(props, "filter", format!("blur({})", px(radius))),
            Some("BACKGROUND_BLUR" | "GLASS") => {
                string_prop(props, "backdropFilter", format!("blur({})", px(radius)))
            }
            Some("NOISE" | "TEXTURE") => {
                string_prop(props, "filter", "contrast(100%) brightness(100%)")
            }
            _ => {}
        }
    }
}

/// Shared literal effect projection for the public CSS recipe. The renderer
/// may replace these literals with bound theme tokens from its own resources.
pub(super) fn image_effect_props(node: &RawNode) -> BTreeMap<String, String> {
    let mut props = Vec::new();
    let mut used = BTreeSet::new();
    let tokens = BTreeMap::new();
    let lengths = BTreeMap::new();
    let view = node.typed_view();
    push_radius(&view, &mut props);
    push_strokes(&view, None, &mut props, &mut used, &tokens);
    push_effects(
        &view,
        "Box",
        &mut props,
        &mut used,
        &tokens,
        Lengths {
            tokens: &lengths,
            viewport: None,
        },
    );
    if let Some(opacity) = view.number("opacity")
        && opacity < 1.0
    {
        string_prop(&mut props, "opacity", format_number(opacity));
    }
    push_blend_mode(&view, &mut props);
    props
        .into_iter()
        .map(|(key, PropValue::String(value))| (key, value))
        .collect()
}

/// Whether every visible effect on this node survives `push_effects` without
/// loss. Mirrors that function case for case; the two must move together.
///
/// `DEVUP_CODEGEN_EFFECT_FALLBACK` used to fire whenever a node merely *had* an
/// effects array. A plain drop shadow is present on nearly every real design,
/// so that permanently pinned `projection` to `lossy` and made `strict: true`
/// unusable, while saying nothing about what was actually lost.
///
/// Deliberately *not* counted as loss: `showShadowBehindNode`. CSS always
/// paints a non-inset `box-shadow` behind the element's box, so the flag only
/// changes rendering behind a translucent fill. Treating it as loss would put
/// essentially every Figma shadow back into `lossy` for a difference that is
/// usually invisible, recreating the problem this guard removes.
pub(super) fn effects_are_exact(view: &TypedNode<'_>) -> bool {
    let Some(effects) = view.value("effects").and_then(Value::as_array) else {
        return true;
    };
    // `push_effects` picks `textShadow` for Text, which has no spread slot.
    // `component.rs` resolves exactly this node type to the `Text` component.
    let is_text = view.node_type() == "TEXT";
    let visible = effects
        .iter()
        .filter(|effect| effect.get("visible").and_then(Value::as_bool) != Some(false))
        .collect::<Vec<_>>();

    // `push_effects` writes `filter` once per effect that maps to it, so two
    // such effects would collide on a single prop and the later one wins.
    let filter_writers = visible
        .iter()
        .filter(|effect| {
            matches!(
                effect.get("type").and_then(Value::as_str),
                Some("LAYER_BLUR" | "NOISE" | "TEXTURE")
            )
        })
        .count();
    if filter_writers > 1 {
        return false;
    }

    visible
        .iter()
        .all(|effect| match effect.get("type").and_then(Value::as_str) {
            Some("DROP_SHADOW" | "INNER_SHADOW") => {
                // Same fields `push_effects` requires before it emits a shadow;
                // if any is missing the effect is dropped on the floor.
                let renders = effect
                    .get("offset")
                    .and_then(|offset| {
                        Some((offset.get("x")?.as_f64()?, offset.get("y")?.as_f64()?))
                    })
                    .is_some()
                    && effect.get("radius").and_then(Value::as_f64).is_some()
                    && effect.get("color").and_then(color_from).is_some();
                // CSS shadows carry no per-shadow blend mode.
                let blend_survives = effect
                    .get("blendMode")
                    .and_then(Value::as_str)
                    .is_none_or(|mode| mode == "NORMAL");
                // `text-shadow` has no spread component.
                let spread_survives =
                    !is_text || effect.get("spread").and_then(Value::as_f64).unwrap_or(0.0) == 0.0;
                renders && blend_survives && spread_survives
            }
            // `push_effects` falls back to `blur(0px)` when the radius is
            // missing or unparseable, which silently fabricates the blur away.
            Some("LAYER_BLUR" | "BACKGROUND_BLUR") => {
                effect.get("radius").and_then(Value::as_f64).is_some()
            }
            // `GLASS` is flattened to a plain backdrop blur, `NOISE`/`TEXTURE`
            // become a no-op filter placeholder, and any other type is silently
            // ignored. All of those are real losses.
            _ => false,
        })
}

fn zero_or_px(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        px(value)
    }
}

pub(super) fn first_solid_color(value: Option<&Value>) -> Option<String> {
    let paint = value?.as_array()?.iter().find(|paint| {
        paint.get("type").and_then(Value::as_str) == Some("SOLID")
            && paint.get("visible").and_then(Value::as_bool) != Some(false)
            && paint.get("opacity").and_then(Value::as_f64) != Some(0.0)
    })?;
    color_from_paint(paint)
}

fn color_from(color: &Value) -> Option<String> {
    let channel = |name: &str| -> Option<u8> {
        Some((color.get(name)?.as_f64()?.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    let alpha = color.get("a").and_then(Value::as_f64).unwrap_or(1.0);
    let mut hex = format!(
        "#{:02X}{:02X}{:02X}",
        channel("r")?,
        channel("g")?,
        channel("b")?
    );
    if alpha < 1.0 {
        hex.push_str(&format!(
            "{:02X}",
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
        ));
    }
    let chars = hex.as_bytes();
    if (hex.len() == 7 || hex.len() == 9)
        && (1..hex.len())
            .step_by(2)
            .all(|index| chars[index] == chars[index + 1])
    {
        let mut short = String::from("#");
        for index in (1..hex.len()).step_by(2) {
            short.push(chars[index] as char);
        }
        Some(short)
    } else {
        Some(hex)
    }
}
