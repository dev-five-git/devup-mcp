use super::*;

#[test]
fn split_infers_owned_exports_assets_and_shared_imports_from_app_evidence() {
    let dir = TestDir::new();
    for app in ["front", "admin"] { std::fs::create_dir_all(dir.path().join(format!("apps/{app}/src/app"))).unwrap(); }
    let mut plan = json!({"facts":{"parts":"front","screens":[
        {"nodeId":"1:1","parts":["Header","PageHero"],"assets":[{"assetId":"hero","path":"/hero.webp"}]},
        {"nodeId":"2:1","parts":["Header","PageHero"],"assets":[{"assetId":"unrelated"}]}
    ]},"features":[{"id":"news","scope":"News","screens":["1:1"]}],"issues":[],"answers":{},"units":[],
    "screenPlan":{"routes":[{"url":"/news/[id]","nodeIds":["1:1"],"app":"apps/front"},{"url":"/other","nodeIds":["2:1"],"app":"apps/admin"}]}});
    let policy = OutputPolicy::from_roots(vec![dir.path().to_path_buf()]).unwrap();
    let mut tx = OutputTransaction::new();
    units::split(&input(dir.path(), "split"), &mut plan, &mut tx, &policy).unwrap();
    let contract = &array(&plan["units"]).iter().find(|unit| unit["id"] == "news-front").unwrap()["contract"];
    for path in ["apps/front/src/app/news/[id]/page.tsx","apps/front/src/components/pages/news/[id]/"] {
        assert!(array(&contract["exports"]).iter().any(|item| item["path"] == path && item["inferred"] == true), "{contract}");
    }
    assert_eq!(contract["assets"][0]["assetId"], "hero");
    assert_eq!(array(&contract["assets"]).len(), 1);
    for path in ["apps/front/src/components/layout/Header.tsx","apps/front/src/components/common/PageHero.tsx"] {
        assert!(array(&contract["imports"]).iter().any(|item| item["path"] == path && item["inferred"] == true), "{contract}");
    }
    assert!(array(&contract["exports"]).iter().chain(array(&contract["assets"])).chain(array(&contract["imports"])).all(|item| item["evidence"].is_string()));
}

#[test]
fn split_inference_honors_empty_explicit_facts_and_single_app() {
    let dir = TestDir::new();
    std::fs::create_dir_all(dir.path().join("src/app")).unwrap();
    let mut plan = json!({"facts":{"parts":"front","news.assets":[],"news.imports":["explicit"]},"features":[{"id":"news","scope":"News","screens":["/news"]}],"issues":[],"answers":{},"units":[],"screenPlan":{"routes":[{"url":"/news"}]}});
    let policy = OutputPolicy::from_roots(vec![dir.path().to_path_buf()]).unwrap();
    let mut tx = OutputTransaction::new();
    units::split(&input(dir.path(), "split"), &mut plan, &mut tx, &policy).unwrap();
    let contract = &array(&plan["units"]).iter().find(|unit| unit["id"] == "news-front").unwrap()["contract"];
    assert_eq!(contract["assets"], json!([]));
    assert_eq!(contract["imports"], json!(["explicit"]));
    assert_eq!(contract["exports"][0]["path"], "src/app/news/page.tsx");
}
