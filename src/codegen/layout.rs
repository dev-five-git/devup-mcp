use crate::figma::TypedNode;

pub(super) fn push_layout_props(view: &TypedNode<'_>, props: &mut Vec<String>) {
    push_px(view, props, "width", "w");
    push_px(view, props, "height", "h");
    if let Some(mode) = view.string("layoutMode") {
        match mode {
            "HORIZONTAL" => props.push("flexDir=\"row\"".to_owned()),
            "VERTICAL" => props.push("flexDir=\"column\"".to_owned()),
            _ => {}
        }
    }
    push_px(view, props, "itemSpacing", "gap");

    let padding = ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"]
        .map(|field| view.number(field));
    if let [Some(top), Some(right), Some(bottom), Some(left)] = padding {
        if top == right && right == bottom && bottom == left {
            props.push(format!("p=\"{}\"", px(top)));
        } else {
            props.push(format!("pt=\"{}\"", px(top)));
            props.push(format!("pr=\"{}\"", px(right)));
            props.push(format!("pb=\"{}\"", px(bottom)));
            props.push(format!("pl=\"{}\"", px(left)));
        }
    }
}

fn push_px(view: &TypedNode<'_>, props: &mut Vec<String>, field: &str, prop: &str) {
    if let Some(value) = view.number(field) {
        props.push(format!("{prop}=\"{}\"", px(value)));
    }
}

pub(super) fn px(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}px")
    } else {
        format!("{value}px")
    }
}
