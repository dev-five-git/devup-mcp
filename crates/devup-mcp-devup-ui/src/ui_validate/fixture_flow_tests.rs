use super::*;

fn file(path: &str, content: &str) -> SourceFile {
    SourceFile {
        path: path.into(),
        content: content.into(),
    }
}

fn rules(files: &[SourceFile], source: &str) -> Vec<Violation> {
    findings(files, "src/components/pages/NewsPage.tsx", source)
}

fn adapter_files() -> Vec<SourceFile> {
    vec![
        file(
            "src/utils/pageContent.ts",
            r#"
import type { PageKey } from './publicData';
import { getRawPageContent } from './publicData';
const pageSchemas = { companyIntro: { outline: [{ label: 'string', value: 'string' }] } } as const;
function decode(schema: unknown, value: unknown) {
  if (typeof schema === 'string') return value;
  if (Array.isArray(schema)) return value.map(item => decode(schema[0], item));
  const record = value as Record<string,unknown>;
  return Object.fromEntries(Object.entries(schema).map(([key,field]) => [key,decode(field,record[key])]));
}
export function decodePageContent(pageKey: PageKey, value: unknown) { return decode(pageSchemas[pageKey], value); }
export async function getPageContent(pageKey: PageKey) { return decodePageContent(pageKey, await getRawPageContent(pageKey)); }
export type PageContentByKey = typeof pageSchemas;
"#,
        ),
        file(
            "src/utils/publicData.ts",
            r#"
import {client} from '../api';
export type PageKey = 'companyIntro';
export async function getRawPageContent(pageKey: PageKey) {
  const result = await client.get('/page-contents/{pageKey}', {params:{pageKey}});
  return result.data.content;
}
"#,
        ),
    ]
}

#[test]
fn schema_decoder_returns_api_values_without_importing_fixture_content() {
    assert!(rules(&adapter_files(), "import {getPageContent} from '@/utils/pageContent'; export async function Page(){const data=await getPageContent('companyIntro'); return <main>{data.outline.map(row=><p>{row.value}</p>)}</main>}").is_empty());
}

#[test]
fn importing_actual_fixture_still_fails_beside_api_and_schema_exports() {
    let mut files = adapter_files();
    files[0].content.push_str("export const fixtures = [{id:1,title:'Fake'}]; export function getFixture(){return fixtures.slice();}");
    for binding in ["fixtures", "getFixture"] {
        let source = format!(
            "import {{getPageContent,{binding} as rows}} from '@/utils/pageContent'; const view=<main>{{rows}}</main>;"
        );
        let found = rules(&files, &source);
        assert_eq!(found.len(), 1, "{binding}");
        assert_eq!(found[0].context["fixtureBinding"], binding);
    }
}

#[test]
fn type_only_unused_and_test_imports_do_not_become_ui_data_reads() {
    let files = vec![file(
        "src/utils/records.ts",
        "export const records=[{id:1,title:'Fixture'}]; export type Shape=typeof records;",
    )];
    for source in [
        "import type {records} from '@/utils/records'; type T=typeof records; const view=<main/>;",
        "import {type records} from '@/utils/records'; type T=typeof records; const view=<main/>;",
        "import {records} from '@/utils/records'; type T=typeof records; const view=<main/>;",
        "import {records} from '@/utils/records'; const view=<main/>;",
    ] {
        assert!(rules(&files, source).is_empty(), "{source}");
    }
    assert!(
        findings(
            &files,
            "src/__tests__/records.test.ts",
            "import {records} from '@/utils/records'; expect(records).toHaveLength(1);"
        )
        .is_empty()
    );
}

#[test]
fn namespace_default_barrel_aliases_and_returned_fallbacks_follow_the_selected_binding() {
    let files = vec![
        file(
            "src/utils/records.ts",
            "const rows=[{id:1,title:'Fixture'}]; export {rows as records}; export default rows; export async function live(){return fetch('/api')} export async function fallback(){return fetch('/api').catch(()=>rows)}",
        ),
        file(
            "src/utils/barrel.ts",
            "export {records as items} from './records';",
        ),
    ];
    for source in [
        "import data from '@/utils/records'; const view=<main>{data.length}</main>;",
        "import {items as data} from '@/utils/barrel'; const view=<main>{data.length}</main>;",
        "import * as data from '@/utils/records'; const view=<main>{data.records.length}</main>;",
        "import * as data from '@/utils/records'; const view=<main>{data['records'].length}</main>;",
        "import {fallback} from '@/utils/records'; const view=<main>{fallback()}</main>;",
    ] {
        assert_eq!(rules(&files, source).len(), 1, "{source}");
    }
    assert!(
        rules(
            &files,
            "import * as data from '@/utils/records'; const view=<main>{data.live()}</main>;"
        )
        .is_empty()
    );
}

#[test]
fn block_scopes_do_not_hide_a_global_fixture_or_poison_an_api_local() {
    let files = vec![file(
        "src/utils/records.ts",
        r#"
const rows=[{id:1,title:'Fixture'}];
export async function live(){const rows=await fetch('/api'); if(true){const rows=[{id:2,title:'Unused'}]; console.log(rows)} return rows;}
export function fake(){if(true){const rows=fetch('/api'); console.log(rows)} return rows;}
export const spread=[...rows];
"#,
    )];
    assert!(
        rules(
            &files,
            "import {live} from '@/utils/records'; const view=<main>{live()}</main>;"
        )
        .is_empty()
    );
    for binding in ["fake", "spread"] {
        assert_eq!(rules(&files,&format!("import {{{binding} as data}} from '@/utils/records'; const view=<main>{{data}}</main>;")).len(),1);
    }
}

#[test]
fn app_router_metadata_configuration_is_not_ui_fixture_data() {
    let files = vec![file(
        "src/utils/pageMetadata.ts",
        r#"
export const SITE_OPEN_GRAPH={images:[{url:'/opengraph-image.png',width:1440,height:5953}]};
export function createPageMetadata(title:string){return {title,openGraph:SITE_OPEN_GRAPH};}
"#,
    )];
    for source in [
        "import {createPageMetadata} from '@/utils/pageMetadata'; export const metadata=createPageMetadata('Home'); export default function Page(){return <main/>}",
        "import {SITE_OPEN_GRAPH} from '@/utils/pageMetadata'; export const metadata={openGraph:SITE_OPEN_GRAPH}; export default function Layout(){return <main/>}",
        "import {SITE_OPEN_GRAPH} from '@/utils/pageMetadata'; export async function generateMetadata(){return {openGraph:SITE_OPEN_GRAPH}} export default function Page(){return <main/>}",
    ] {
        assert!(
            findings(&files, "src/app/page.tsx", source).is_empty(),
            "{source}"
        );
    }
    let mixed = "import {SITE_OPEN_GRAPH} from '@/utils/pageMetadata'; export const metadata={openGraph:SITE_OPEN_GRAPH}; export default function Page(){return <main>{SITE_OPEN_GRAPH.images.map(image=><p>{image.url}</p>)}</main>}";
    assert_eq!(findings(&files, "src/app/page.tsx", mixed).len(), 1);
    let ordinary = "import {SITE_OPEN_GRAPH} from '@/utils/pageMetadata'; export const metadata=SITE_OPEN_GRAPH; const view=<main>{metadata.images.length}</main>;";
    assert_eq!(
        rules(&files, ordinary).len(),
        1,
        "Only recognized App Router metadata exports have metadata semantics"
    );
}

#[test]
fn unrelated_nested_fixture_returns_and_shadowed_names_do_not_poison_a_real_loader() {
    let files = vec![file(
        "src/utils/records.ts",
        r#"
const rows=[{id:1,title:'Fixture'}];
export async function live(){const rows=await fetch('/api'); function unused(){return [{id:2,title:'Unused'}]} return rows;}
export function fake(){const local=rows; return local.map(row=>row)}
"#,
    )];
    assert!(
        rules(
            &files,
            "import {live} from '@/utils/records'; const view=<main>{live()}</main>;"
        )
        .is_empty()
    );
    assert_eq!(
        rules(
            &files,
            "import {fake} from '@/utils/records'; const view=<main>{fake()}</main>;"
        )
        .len(),
        1
    );
}
