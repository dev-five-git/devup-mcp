//! `runtime-value` against devup-ui's documented same-file constants.
use super::validate_devup_ui_tsx;

fn runtime_values(tsx: &str) -> Vec<String> {
    validate_devup_ui_tsx(tsx, None, false)
        .violations
        .into_iter()
        .filter(|violation| violation.rule == "runtime-value")
        .map(|violation| violation.context["snippet"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// The devup-ui skill documents `css({ animationName: spin })` with `spin` a
/// `keyframes()` const of the same file. It was reported as `runtime-value`.
#[test]
fn a_same_file_keyframes_const_is_a_build_time_value() {
    for tsx in [
        r#"import { css, keyframes } from '@devup-ui/react'
           const spin = keyframes({ from: { transform: 'rotate(0)' }, to: { transform: 'rotate(360deg)' } })
           const cls = css({ animationName: spin })"#,
        r#"import { globalCss, keyframes } from '@devup-ui/react'
           export const rise = keyframes({ from: { opacity: 0 }, to: { opacity: 1 } })
           globalCss({ '.reveal': { animationName: rise, animation: `${rise} 0.45s ease-out both` } })"#,
        r#"import * as Devup from '@devup-ui/react'
           const fade = Devup.keyframes({ from: { opacity: 0 }, to: { opacity: 1 } })
           const cls = Devup.css({ animationName: fade })"#,
        // Declared after its use: the build reads the module, not the line order.
        r#"const cls = css({ selectors: { '&:hover': { animationName: pulse } } })
           const pulse = keyframes({ '50%': { opacity: 0.5 } })"#,
    ] {
        assert_eq!(runtime_values(tsx), Vec::<String>::new(), "{tsx}");
    }
}

#[test]
fn same_file_literal_constants_and_what_is_built_from_them_are_static() {
    for tsx in [
        r#"const SIZE = 4
           const DOUBLE = SIZE * 2
           const cls = css({ w: DOUBLE, h: `${SIZE}px`, m: -SIZE })"#,
        r#"const card = css({ p: 4 })
           const hover = { color: 'red' }
           const cls = css({ _hover: hover, nested: { ...hover }, className: card })"#,
        r#"const steps = ['0px', '8px']
           const cls = css({ gap: steps })"#,
    ] {
        assert_eq!(runtime_values(tsx), Vec::<String>::new(), "{tsx}");
    }
}

/// Everything the file cannot prove constant is still a runtime value.
#[test]
fn imports_parameters_and_runtime_results_stay_runtime_values() {
    for (tsx, snippet) in [
        // An imported keyframes name is not known at build time.
        (
            r#"import { spin } from './animations'
               const cls = css({ animationName: spin })"#,
            "spin",
        ),
        // A parameter shadows the module constant.
        (
            r#"const spin = keyframes({ to: { opacity: 1 } })
               export function Card({ spin }) { return <div className={css({ animationName: spin })} /> }"#,
            "spin",
        ),
        (
            r#"let name = keyframes({ to: { opacity: 1 } })
               const cls = css({ animationName: name })"#,
            "name",
        ),
        (
            r#"const v = getValue()
               const cls = css({ width: v })"#,
            "v",
        ),
        (
            r#"const size = window.innerWidth
               const cls = css({ width: `${size}px` })"#,
            "`${size}px`",
        ),
        // Not declared anywhere in this file.
        (r#"const cls = keyframes({ from: { opacity: start } })"#, "{ opacity: start }"),
        // Declared inside a function, so possibly computed per call.
        (
            r#"export function Card(props) {
                 const local = props.name
                 return <div className={css({ animationName: local })} />
               }"#,
            "local",
        ),
        // A constant built from a runtime value is one too.
        (
            r#"const base = Math.random()
               const scaled = base * 2
               const cls = css({ opacity: scaled })"#,
            "scaled",
        ),
    ] {
        assert_eq!(runtime_values(tsx), vec![snippet.to_owned()], "{tsx}");
    }
}

/// devup-ui stops treating an object as a constant once visible code changes it.
#[test]
fn an_object_constant_the_file_changes_is_not_static() {
    for tsx in [
        r#"const hover = { color: 'red' }
           hover.color = 'blue'
           const cls = css({ _hover: hover })"#,
        r#"const sizes = ['0px']
           sizes.push('8px')
           const cls = css({ gap: sizes })"#,
        r#"const hover = { color: 'red' }
           Object.assign(hover, extra)
           const cls = css({ _hover: hover })"#,
        r#"const hover = { color: 'red' }
           delete hover.color
           const cls = css({ _hover: hover })"#,
    ] {
        assert_eq!(runtime_values(tsx).len(), 1, "{tsx}");
    }
}
