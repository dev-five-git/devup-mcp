use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let fixture = Self(std::env::temp_dir().join(format!(
            "devup-ui-context-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        )));
        fixture.write("package.json", "{}");
        fixture
    }
    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    async fn ui(&self, filter: Option<&str>) -> Value {
        run("ui", self.0.to_str(), filter)
            .await
            .expect("ui scope supported")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn ui_client_component_records_exported_names_and_declared_props() {
    let f = Fixture::new();
    f.write("components/Button.tsx", "/* directive comment */ 'use client';\ninterface ButtonProps { label: string; disabled?: boolean; onClick?: (value: string) => void }\nexport function Button({label}: ButtonProps) { return <button>{label}</button> }\nexport { Button as PrimaryAction };");
    let v = f.ui(None).await;
    let c = &v["components"][0];
    assert_eq!(c["path"], "components/Button.tsx");
    assert_eq!(c["exports"], json!(["Button", "PrimaryAction"]));
    assert_eq!(c["client"], true);
    assert_eq!(c["kind"], "project-component");
    assert_eq!(
        c["props"],
        json!([
            {"component":"Button", "name":"label", "optional":false, "type":"string"},
            {"component":"Button", "name":"disabled", "optional":true, "type":"boolean"},
            {"component":"Button", "name":"onClick", "optional":true, "type":"(value: string) => void"}
        ])
    );
}

#[tokio::test]
async fn ui_server_component_ignores_use_client_in_comments_and_strings() {
    let f = Fixture::new();
    f.write("components/Card.tsx", "// 'use client'\nconst message = 'use client'; export default async function Card(props: { title: string }) { return <article>{props.title}</article>; }");
    let v = f.ui(None).await;
    assert_eq!(v["components"][0]["client"], false);
    assert_eq!(v["components"][0]["exports"], json!(["default"]));
    assert_eq!(v["components"][0]["localNames"], json!(["Card"]));
    assert_eq!(v["components"][0]["props"][0]["type"], "string");
}

#[tokio::test]
async fn ui_import_sites_resolve_aliases_barrels_and_transitive_routes() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["./src/*"]}}}"#,
    );
    f.write(
        "src/components/Button.tsx",
        "export const Button = (props: { disabled?: boolean }) => <button />;",
    );
    f.write(
        "src/components/index.ts",
        "export { Button } from './Button';",
    );
    f.write(
        "src/components/pages/account/Account.tsx",
        "import { Button } from '@/components'; export function Account() { return <Button />; }",
    );
    f.write("src/app/(private)/account/page.tsx", "import { Account } from '@/components/pages/account/Account'; export default function Page() { return <Account />; }");
    let v = f.ui(Some("Button")).await;
    let sites = v["components"][0]["importedBy"].as_array().unwrap();
    assert!(
        sites
            .iter()
            .any(|s| s["path"] == "src/components/index.ts" && s["routes"] == json!(["/account"])),
        "{v}"
    );
    assert!(
        sites
            .iter()
            .any(|s| s["path"] == "src/components/pages/account/Account.tsx"
                && s["routes"] == json!(["/account"])),
        "{v}"
    );
}

#[tokio::test]
async fn ui_route_tree_preserves_route_groups_and_page_components() {
    let f = Fixture::new();
    f.write(
        "app/(private)/account/[id]/page.tsx",
        "export default function Page() { return <div /> }",
    );
    f.write(
        "components/pages/account/[id]/Details.tsx",
        "export function Details() { return <div /> }",
    );
    let v = f.ui(None).await;
    assert_eq!(v["routes"][0]["route"], "/account/[id]");
    assert_eq!(
        v["routes"][0]["page"],
        "app/(private)/account/[id]/page.tsx"
    );
    assert_eq!(v["routes"][0]["routeGroups"], json!(["(private)"]));
    assert_eq!(
        v["routes"][0]["components"],
        json!(["components/pages/account/[id]/Details.tsx"])
    );
}

#[tokio::test]
async fn ui_all_remains_opt_in_and_scope_schema_explains_it() {
    let f = Fixture::new();
    f.write(
        "components/Button.tsx",
        "export function Button() { return <button /> }",
    );
    let all = run("all", f.0.to_str(), None).await.unwrap();
    assert!(all.get("ui").is_none());
    assert!(all.get("components").is_none());
    let schema =
        serde_json::to_value(schemars::schema_for!(super::super::ProjectContextInput)).unwrap();
    assert!(
        schema["properties"]["scope"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("ui"))
    );
    assert!(
        schema["properties"]["scope"]["description"]
            .as_str()
            .unwrap()
            .contains("excluded from all")
    );
}

#[tokio::test]
async fn ui_component_cap_is_explicit() {
    let f = Fixture::new();
    for i in 0..210 {
        f.write(
            &format!("components/Item{i:03}.tsx"),
            &format!("export function Item{i}() {{ return <div /> }}"),
        );
    }
    let v = f.ui(None).await;
    assert_eq!(v["components"].as_array().unwrap().len(), 200);
    assert_eq!(v["truncation"]["truncated"], true);
    assert!(
        v["truncation"]["capsHit"]
            .as_array()
            .unwrap()
            .contains(&json!("maxComponents"))
    );
    assert_eq!(v["totalComponents"], 210);
}

#[tokio::test]
async fn ui_total_byte_cap_is_explicit_and_bounds_serialized_response() {
    let f = Fixture::new();
    let long_type = format!("'{}'", "x".repeat(140_000));
    f.write(
        "components/Huge.tsx",
        &format!("export function Huge(props: {{ value: {long_type} }}) {{ return <div /> }}"),
    );
    let v = f.ui(None).await;
    assert!(serde_json::to_vec(&v).unwrap().len() <= 131_072);
    assert!(
        v["truncation"]["capsHit"]
            .as_array()
            .unwrap()
            .contains(&json!("maxBytes")),
        "{v}"
    );
}

#[tokio::test]
async fn ui_excludes_all_checkout_kinds_and_reports_exact_reasons() {
    let f = Fixture::new();
    f.write(
        "components/Real.tsx",
        "export function Real() { return <div /> }",
    );
    for dir in [
        ".worktrees/stale",
        ".worktree/stale",
        ".git-worktrees/stale",
        "copies/stale",
    ] {
        f.write(
            &format!("{dir}/components/Ghost.tsx"),
            "invalid stale source",
        );
    }
    f.write("copies/stale/.git", "gitdir: elsewhere");
    for dir in [
        "node_modules",
        "target",
        "dist",
        "build",
        "out",
        ".next",
        ".turbo",
        ".nuxt",
        ".venv",
        "venv",
        "__pycache__",
        ".cache",
        "coverage",
        ".git",
    ] {
        f.write(&format!("{dir}/Ghost.tsx"), "invalid excluded source");
    }
    let v = f.ui(None).await;
    assert_eq!(v["components"].as_array().unwrap().len(), 1);
    let excluded = v["excludedPaths"].as_array().unwrap();
    assert_eq!(excluded.len(), 4);
    for e in excluded {
        assert_eq!(
            e["reason"],
            if e["path"].as_str().unwrap().starts_with("copies/") {
                "nested-git-checkout"
            } else {
                "nested-checkout-directory"
            }
        );
        assert!(
            e["path"]
                .as_str()
                .unwrap()
                .ends_with("components/Ghost.tsx")
        );
    }
}

#[tokio::test]
async fn ui_filter_is_literal_case_sensitive_substring() {
    let f = Fixture::new();
    f.write(
        "components/Primary.tsx",
        "export function Primary() { return <div /> }",
    );
    assert_eq!(
        f.ui(Some("Primary")).await["components"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for filter in ["primary", ".*"] {
        assert!(
            f.ui(Some(filter)).await["components"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn ui_devup_reexports_are_distinguished_from_project_components() {
    let f = Fixture::new();
    f.write("components/Actions.ts", "export { Button as PrimaryAction } from '@devup-ui/components'; export { Box } from '@devup-ui/react';");
    let v = f.ui(None).await;
    let c = &v["components"][0];
    assert_eq!(c["kind"], "devup-ui-reexport");
    assert_eq!(c["exports"], json!(["Box", "PrimaryAction"]));
    assert_eq!(
        c["reexports"][0],
        json!({"exported":"PrimaryAction","imported":"Button","source":"@devup-ui/components"})
    );
}

#[tokio::test]
async fn ui_multi_app_authority_is_package_local() {
    let f = Fixture::new();
    for app in ["admin", "front"] {
        f.write(&format!("apps/{app}/package.json"), "{}");
        f.write(
            &format!("apps/{app}/src/components/Card.tsx"),
            "export function Card() { return <div /> }",
        );
        f.write(
            &format!("apps/{app}/src/app/page.tsx"),
            "export default function Page() { return <div /> }",
        );
    }
    let v = f.ui(Some("Card")).await;
    for c in v["components"].as_array().unwrap() {
        assert_eq!(c["authority"], "package-local");
        assert!(["apps/admin", "apps/front"].contains(&c["appliesTo"].as_str().unwrap()));
    }
    assert!(v["authorityNote"].as_str().unwrap().contains("appliesTo"));
}

#[tokio::test]
async fn ui_unresolved_props_and_parse_errors_are_explicit() {
    let f = Fixture::new();
    f.write("components/External.tsx", "import type { Props } from './types'; export function External(props: Props) { return <div /> }");
    f.write("components/Broken.tsx", "export function Broken( {");
    let v = f.ui(None).await;
    assert_eq!(v["components"][0]["propsStatus"], "unresolved");
    assert_eq!(v["diagnostics"][0]["path"], "components/Broken.tsx");
    assert_eq!(v["diagnostics"][0]["kind"], "parse-error");
    assert_eq!(v["diagnostics"][0]["state"], "unparsed");
    assert!(!v["diagnostics"][0]["reason"].as_str().unwrap().is_empty());
    assert_eq!(v["unparsedFiles"], 1);
    assert_eq!(v["componentCountExact"], false);
}

#[tokio::test]
async fn ui_wrapped_components_and_local_intersection_props_are_evidence() {
    let f = Fixture::new();
    f.write("components/Action.tsx", "import { memo, forwardRef } from 'react'; type Base = { label: string }; type Props = Base & { busy?: boolean }; export const Action = memo((props: Props) => <button />); export const Input = forwardRef<HTMLInputElement, { value: string }>((props, ref) => <input ref={ref} />);");
    let v = f.ui(None).await;
    let c = &v["components"][0];
    assert_eq!(c["exports"], json!(["Action", "Input"]));
    assert!(c["props"].as_array().unwrap().contains(
        &json!({"component":"Action", "name":"busy", "optional":true, "type":"boolean"})
    ));
    assert!(c["props"].as_array().unwrap().contains(
        &json!({"component":"Input", "name":"value", "optional":false, "type":"string"})
    ));
}

#[tokio::test]
async fn ui_barrel_imports_do_not_invent_usage_of_other_exports() {
    let f = Fixture::new();
    f.write("components/A.tsx", "export function A() { return <div /> }");
    f.write("components/B.tsx", "export function B() { return <div /> }");
    f.write(
        "components/index.ts",
        "export { A } from './A'; export { B } from './B';",
    );
    f.write(
        "app/page.tsx",
        "import { A } from '../components'; export default function Page() { return <A /> }",
    );
    let v = f.ui(Some("components/B.tsx")).await;
    let sites = v["components"][0]["importedBy"].as_array().unwrap();
    assert!(sites.iter().all(|s| s["path"] != "app/page.tsx"), "{v}");
}

#[tokio::test]
async fn ui_default_class_component_props_are_not_silently_omitted() {
    let f = Fixture::new();
    f.write("components/Card.tsx", "import React from 'react'; export default class Card extends React.Component<{ title?: string }> { render() { return <div /> } }");
    let v = f.ui(None).await;
    assert_eq!(v["components"][0]["exports"], json!(["default"]));
    assert_eq!(
        v["components"][0]["props"][0],
        json!({"component":"Card","name":"title","optional":true,"type":"string"})
    );
}

#[tokio::test]
async fn ui_type_only_imports_do_not_claim_component_usage() {
    let f = Fixture::new();
    f.write(
        "components/Card.tsx",
        "export function Card() { return <div /> }",
    );
    f.write("app/page.tsx", "import { type Card } from '../components/Card'; export default function Page() { return <div /> }");
    let v = f.ui(Some("Card")).await;
    assert!(
        v["components"][0]["importedBy"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{v}"
    );
}

#[tokio::test]
async fn ui_jsonc_aliases_resolve_without_reading_excluded_sources() {
    let f = Fixture::new();
    f.write(
        "tsconfig.json",
        "{ // next config\n \"compilerOptions\": { \"paths\": { \"@/*\": [\"./src/*\",], \"stale/*\": [\"./.worktrees/stale/*\"] }, }, }",
    );
    f.write(
        "src/components/Card.tsx",
        "export function Card() { return <div /> }",
    );
    f.write(".worktrees/stale/Ghost.tsx", "not valid TSX");
    f.write("src/app/page.tsx", "import {Card} from '@/components/Card'; import {Ghost} from 'stale/Ghost'; export default function Page() { return <Card /> }");
    let v = f.ui(Some("Card")).await;
    assert_eq!(v["components"][0]["importedBy"][0]["routes"], json!(["/"]));
    assert_eq!(v["unparsedFiles"], 0);
    assert_eq!(v["diagnostics"][0]["kind"], "import-unresolved");
    assert_eq!(v["diagnostics"][0]["source"], "stale/Ghost");
    assert_eq!(
        v["excludedPaths"][0],
        json!({"path":".worktrees/stale/Ghost.tsx","reason":"nested-checkout-directory"})
    );
}

#[tokio::test]
async fn ui_export_star_does_not_forward_default_exports() {
    let f = Fixture::new();
    f.write(
        "components/Card.tsx",
        "export default function Card() { return <div /> }",
    );
    f.write("components/index.ts", "export * from './Card';");
    f.write(
        "app/page.tsx",
        "import Card from '../components'; export default function Page() { return <Card /> }",
    );
    let v = f.ui(Some("components/Card.tsx")).await;
    assert!(
        v["components"][0]["importedBy"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["path"] != "app/page.tsx"),
        "{v}"
    );
}

#[tokio::test]
async fn ui_unterminated_jsonc_is_reported_without_panicking() {
    let f = Fixture::new();
    f.write("tsconfig.json", "/*");
    f.write(
        "components/Card.tsx",
        "export function Card() { return <div /> }",
    );
    let v = f.ui(None).await;
    assert_eq!(v["diagnostics"][0]["kind"], "config-unresolved");
}

#[tokio::test]
async fn ui_generic_props_are_not_claimed_fully_resolved() {
    let f = Fixture::new();
    f.write("components/Card.tsx", "type Props<T = string> = { title: T }; export function Card(props: Props) { return <div /> }");
    let v = f.ui(None).await;
    assert_eq!(v["components"][0]["propsStatus"], "unresolved");
    assert_eq!(v["components"][0]["props"][0]["type"], "T");
}

#[tokio::test]
async fn ui_alias_exported_component_keeps_local_props() {
    let f = Fixture::new();
    f.write("components/Actions.tsx", "function action(props: { label: string }) { return <button /> } export { action as Button };");
    let v = f.ui(None).await;
    assert_eq!(v["components"][0]["exports"], json!(["Button"]));
    assert_eq!(v["components"][0]["localNames"], json!(["action"]));
    assert_eq!(v["components"][0]["props"][0]["name"], "label");
}

#[tokio::test]
async fn ui_importing_a_value_helper_does_not_claim_component_usage() {
    let f = Fixture::new();
    f.write(
        "components/Card.tsx",
        "export function Card() { return <div /> } export function format() { return 'label' }",
    );
    f.write("app/page.tsx", "import { format } from '../components/Card'; export default function Page() { return <div>{format()}</div> }");
    let v = f.ui(Some("Card")).await;
    assert!(
        v["components"][0]["importedBy"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{v}"
    );
}

#[tokio::test]
async fn ui_layouts_outside_app_do_not_acquire_route_membership() {
    let f = Fixture::new();
    f.write(
        "components/Card.tsx",
        "export function Card() { return <div /> }",
    );
    f.write("layout.tsx", "import {Card} from './components/Card'; export default function Layout() { return <Card /> }");
    f.write(
        "app/page.tsx",
        "export default function Page() { return <div /> }",
    );
    let v = f.ui(Some("Card")).await;
    assert_eq!(v["components"][0]["importedBy"][0]["path"], "layout.tsx");
    assert_eq!(v["components"][0]["importedBy"][0]["routes"], json!([]));
}

#[tokio::test]
async fn ui_relative_project_root_resolves_the_same_import_evidence() {
    let f = Fixture(PathBuf::from(".").join(format!(".devup-ui-relative-{}", std::process::id())));
    f.write("package.json", "{}");
    f.write(
        "components/Card.tsx",
        "export function Card() { return <div /> }",
    );
    f.write("app/page.tsx", "import {Card} from '../components/Card'; export default function Page() { return <Card /> }");
    let relative = f.ui(Some("Card")).await;
    let absolute = run(
        "ui",
        dunce::canonicalize(&f.0).unwrap().to_str(),
        Some("Card"),
    )
    .await
    .unwrap();
    assert_eq!(
        relative["components"][0]["importedBy"],
        absolute["components"][0]["importedBy"]
    );
}
