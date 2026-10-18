//! Which framework runs each app in a project: Next App Router on the `next`
//! CLI, or on vinext.
//!
//! vinext runs Next.js App Router on Vite, so an app built on it has a
//! `vite.config.ts`, no `next.config.*` and often no `next` dependency at all,
//! while its `layout.tsx` still imports `from 'next'`. Asking for a `next`
//! dependency missed every such app, which is DevFive's default
//! (`dev-five-git/service-template`). Each signal is reported as evidence, so
//! a caller can see why an app was or was not recognised.
//!
//! bun workspaces (`"workspaces": ["apps/*"]`) are followed, so a project
//! root may be the repository or one app inside it.

use std::path::Path;

use serde_json::{Value, json};

/// One app: where it is, what runs it, and why that was decided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct App {
    /// Relative to the project root; `.` for the root itself.
    pub dir: String,
    /// `vinext`, `next`, or `unknown` when only an `app/` directory says it
    /// is an App Router app.
    pub framework: &'static str,
    pub evidence: Vec<String>,
    /// `src/app` or `app` under `dir`, when either exists.
    pub app_router: Option<String>,
    /// The `name` in its package.json.
    pub name: Option<String>,
}

impl App {
    pub fn json(&self) -> Value {
        json!({"dir": self.dir, "name": self.name, "framework": self.framework,
            "evidence": self.evidence, "appRouter": self.app_router})
    }

    /// `dir` joined with a relative path, without a leading `./`.
    pub fn path(&self, relative: &str) -> String {
        if self.dir == "." {
            relative.to_owned()
        } else {
            format!("{}/{relative}", self.dir)
        }
    }
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Workspace directories a `package.json` declares: `["apps/*"]`, or
/// `{"packages": [...]}`. A trailing `/*` lists that directory; anything else
/// is taken as one path.
fn workspace_dirs(root: &Path, package: &Value) -> Vec<String> {
    let patterns = package["workspaces"]
        .as_array()
        .or_else(|| package["workspaces"]["packages"].as_array())
        .cloned()
        .unwrap_or_default();
    let mut dirs = Vec::new();
    for pattern in patterns.iter().filter_map(Value::as_str) {
        let pattern = pattern.trim_start_matches("./").trim_end_matches('/');
        if let Some(parent) = pattern.strip_suffix("/*") {
            let Ok(entries) = std::fs::read_dir(root.join(parent)) else {
                continue;
            };
            let mut found = entries
                .flatten()
                .filter(|entry| entry.path().join("package.json").is_file())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .map(|name| format!("{parent}/{name}"))
                .collect::<Vec<_>>();
            found.sort();
            dirs.extend(found);
        } else if !pattern.contains('*') && root.join(pattern).join("package.json").is_file() {
            dirs.push(pattern.to_owned());
        }
    }
    dirs
}

/// What runs the app in `dir`, from its package.json and vite config.
fn classify(root: &Path, dir: &str) -> App {
    let base = if dir == "." {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    let package = read_json(&base.join("package.json")).unwrap_or(Value::Null);
    let mut vinext = Vec::new();
    let mut next = Vec::new();
    for section in ["dependencies", "devDependencies"] {
        if let Some(version) = package[section]["vinext"].as_str() {
            vinext.push(format!("vinext {version} in {dir}/package.json {section}"));
        }
        if let Some(version) = package[section]["next"].as_str() {
            next.push(format!("next {version} in {dir}/package.json {section}"));
        }
    }
    if let Some(scripts) = package["scripts"].as_object() {
        let names = scripts
            .iter()
            .filter(|(_, command)| {
                command
                    .as_str()
                    .is_some_and(|command| command.split_whitespace().any(|word| word == "vinext"))
            })
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        if !names.is_empty() {
            vinext.push(format!(
                "vinext in {dir}/package.json scripts ({})",
                names.join(", ")
            ));
        }
    }
    for file in [
        "vite.config.ts",
        "vite.config.mts",
        "vite.config.js",
        "vite.config.mjs",
    ] {
        if let Ok(source) = std::fs::read_to_string(base.join(file))
            && (source.contains("from 'vinext'") || source.contains("from \"vinext\""))
        {
            vinext.push(format!("{dir}/{file} imports vinext"));
        }
    }
    let app_router = ["src/app", "app"]
        .into_iter()
        .find(|candidate| base.join(candidate).is_dir())
        .map(str::to_owned);
    let (framework, evidence) = if !vinext.is_empty() {
        ("vinext", [vinext, next].concat())
    } else if !next.is_empty() {
        ("next", next)
    } else {
        (
            "unknown",
            app_router
                .iter()
                .map(|found| format!("{dir}/{found} exists, but nothing names a framework"))
                .collect(),
        )
    };
    App {
        dir: dir.to_owned(),
        framework,
        evidence,
        app_router,
        name: package["name"].as_str().map(str::to_owned),
    }
}

/// Every app in the project that a framework or an App Router directory
/// marks: the root itself and each bun workspace.
pub(super) fn detect(root: &Path) -> Vec<App> {
    let package = read_json(&root.join("package.json")).unwrap_or(Value::Null);
    std::iter::once(".".to_owned())
        .chain(workspace_dirs(root, &package))
        .map(|dir| classify(root, &dir))
        .filter(|app| app.framework != "unknown" || app.app_router.is_some())
        .collect()
}

/// The `frameworks` block `devup_project_context` adds to a ui or routes
/// inventory.
pub(super) fn report(root: &Path) -> Value {
    let apps = detect(root);
    let kinds = apps
        .iter()
        .map(|app| app.framework)
        .collect::<std::collections::BTreeSet<_>>();
    json!({
        "framework": if kinds.len() == 1 { json!(kinds.iter().next()) } else { Value::Null },
        "apps": apps.iter().map(App::json).collect::<Vec<_>>(),
        "note": include_str!("text/framework-report-6fa8f9df.md"),
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(crate) fn service_template(root: &Path) {
        let write = |path: &str, text: &str| {
            let target = root.join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, text).unwrap();
        };
        write(
            "package.json",
            r#"{"workspaces": ["apps/*"], "scripts": {"build": "bun run -F ./apps/front build"}}"#,
        );
        write(
            "apps/front/package.json",
            r#"{"name": "front", "scripts": {"dev": "vinext dev --port 3000", "build": "vinext build"},
                "dependencies": {"react": "^19", "react-server-dom-webpack": "19.3.0", "vinext": "1.0.0"}}"#,
        );
        write(
            "apps/front/vite.config.ts",
            "import vinext from 'vinext'\nexport default { plugins: [vinext({ react: { compiler: true } })] }\n",
        );
        write(
            "apps/front/src/app/layout.tsx",
            "import type { Metadata } from 'next'\nexport const metadata: Metadata = { title: 'Devfive' }\nexport default function RootLayout({ children }) { return <html lang=\"ko\"><body>{children}</body></html> }\n",
        );
        write(
            "apps/admin/package.json",
            r#"{"name": "admin", "dependencies": {"next": "16.1.0"}}"#,
        );
        write(
            "apps/admin/src/app/page.tsx",
            "export default function AdminPage() { return null }\n",
        );
    }

    #[test]
    fn vinext_is_an_app_router_app_without_a_next_dependency() {
        let root = std::env::temp_dir().join(format!("devup-framework-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        service_template(&root);
        let apps = detect(&root);
        let summary = apps
            .iter()
            .map(|app| (app.dir.as_str(), app.framework, app.app_router.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            [
                ("apps/admin", "next", Some("src/app")),
                ("apps/front", "vinext", Some("src/app"))
            ]
        );
        let front = &apps[1];
        assert!(
            front
                .evidence
                .iter()
                .any(|why| why.contains("vinext 1.0.0"))
        );
        assert!(
            front
                .evidence
                .iter()
                .any(|why| why.contains("scripts (dev, build)"))
        );
        assert!(
            front
                .evidence
                .iter()
                .any(|why| why.contains("vite.config.ts imports vinext"))
        );
        assert_eq!(report(&root)["framework"], Value::Null);
        assert_eq!(front.path("src/app"), "apps/front/src/app");
        std::fs::remove_dir_all(root).ok();
    }
}
