use super::*;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Mutation {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub kind: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Plan {
    pub version: u32,
    pub id: String,
    pub request: AssetsInput,
    pub inventory: Inventory,
    pub mutations: Vec<Mutation>,
    pub database_changes: Vec<Value>,
    pub retirement_allowed: bool,
}
pub(super) struct Built {
    pub plan: Plan,
    pub writes: BTreeMap<String, Vec<u8>>,
    pub deletes: BTreeSet<String>,
}

fn planned_path(id: &str) -> String {
    format!(".devup/asset-migrations/{id}/plan.json")
}
fn existing(workspace: &Workspace<'_>, path: &str) -> Result<Option<Vec<u8>>, DevupError> {
    let target = workspace.target(path)?;
    match std::fs::symlink_metadata(target.display_path()) {
        Ok(_) => target.read_bounded(MAX_FILE_BYTES).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(invalid(error.to_string())),
    }
}
pub(super) fn build(
    workspace: &Workspace<'_>,
    mut request: AssetsInput,
) -> Result<Built, DevupError> {
    request.action = AssetsAction::Plan;
    request.plan_path = None;
    request.dry_run = true;
    request.response_limit = 100;
    request.response_offset = 0;
    if request.renames.is_empty() {
        return Err(invalid(
            "A migration plan requires explicit renames; duplicate suggestions are never applied automatically",
        ));
    }
    if request.renames.len() > 512 {
        return Err(invalid(
            "At most 512 explicit asset renames are supported per plan",
        ));
    }
    let inventory = inventory::scan(workspace, &request)?;
    if !inventory.complete {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "Asset inventory has unresolved syntax or scan gaps; review diagnostics before planning",
            false,
            json!({"diagnostics":inventory.diagnostics}),
        ));
    }
    let mut writes = BTreeMap::new();
    let mut deletes = BTreeSet::new();
    let mut urls = BTreeMap::new();
    let mut sources = BTreeSet::new();
    for rename in &request.renames {
        let root = rename
            .public_root
            .as_ref()
            .or_else(|| (inventory.public_roots.len() == 1).then(|| &inventory.public_roots[0]))
            .ok_or_else(|| {
                invalid("Each rename requires publicRoot when multiple public roots exist")
            })?;
        if !inventory.public_roots.contains(root) {
            return Err(invalid("Rename publicRoot is not part of this inventory"));
        }
        if !semantic(&rename.to) {
            return Err(invalid(
                "Destination asset name must describe its content or role",
            ));
        }
        let from = url_path(&rename.from)?;
        let to = url_path(&rename.to)?;
        let extension = |path: &str| path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        if extension(&from) != extension(&to)
            && !matches!(
                (extension(&from).as_deref(), extension(&to).as_deref()),
                (Some("jpg"), Some("jpeg")) | (Some("jpeg"), Some("jpg"))
            )
        {
            return Err(invalid(
                "URL migration preserves asset encoding; format conversion requires a separate export",
            ));
        }
        let source = format!("{root}/{from}");
        let target = format!("{root}/{to}");
        if source == target {
            return Err(invalid("Asset rename source and target must differ"));
        }
        if !sources.insert(source.clone()) {
            return Err(invalid(
                "A source asset cannot have multiple rename targets",
            ));
        }
        if inventory
            .assets
            .iter()
            .filter(|a| a.url == rename.from)
            .count()
            > 1
        {
            return Err(invalid(
                "The same public URL exists in multiple applications; narrow publicRoots and referencePaths before migrating it",
            ));
        }
        let bytes = workspace.read(&source)?;
        if let Some(previous) = urls.insert(rename.from.clone(), rename.to.clone())
            && previous != rename.to
        {
            return Err(invalid("Ambiguous URL rename"));
        }
        if let Some(existing) = existing(workspace, &target)?
            && existing != bytes
        {
            return Err(invalid(
                "A destination contains different asset bytes; it will not be overwritten",
            ));
        }
        if let Some(existing) = writes.insert(target.clone(), bytes.clone())
            && existing != bytes
        {
            return Err(invalid(
                "Different asset bytes cannot share a canonical target",
            ));
        }
        deletes.insert(source);
    }
    // Disallow cascades and swap cycles: URL updates are simultaneous and public aliases stay stable.
    if request
        .renames
        .iter()
        .any(|rename| urls.contains_key(&rename.to))
    {
        return Err(invalid(
            "Rename chains/cycles require separate reviewed plans",
        ));
    }
    let mut grouped = BTreeMap::<String, Vec<&Reference>>::new();
    let referenced = inventory
        .references
        .iter()
        .map(|r| r.url.as_str())
        .chain(
            inventory
                .database_snapshots
                .iter()
                .flat_map(|snapshot| snapshot["references"].as_array().into_iter().flatten())
                .filter_map(|row| row["url"].as_str()),
        )
        .collect::<Vec<_>>();
    for url in referenced {
        if let Some(at) = url.find(['?', '#'])
            && let Some(to) = urls.get(&url[..at]).cloned()
        {
            urls.insert(url.into(), format!("{to}{}", &url[at..]));
        }
    }
    for reference in &inventory.references {
        if urls.contains_key(&reference.url) {
            grouped
                .entry(reference.file.clone())
                .or_default()
                .push(reference);
        }
    }
    for (file, mut references) in grouped {
        let bytes = workspace.read(&file)?;
        let mut source = String::from_utf8(bytes)
            .map_err(|_| invalid("Asset reference source must be UTF-8"))?;
        references.sort_by_key(|r| std::cmp::Reverse(r.start));
        for reference in references {
            source.replace_range(
                reference.start..reference.end,
                &references::replacement(reference, &urls[&reference.url]),
            );
        }
        writes.insert(file, source.into_bytes());
    }
    let (sql, database_changes) =
        database::scripts(workspace, &request.database_references, &urls)?;
    let retirement_allowed = request.public_roots.len() == 1
        && !request.reference_paths.is_empty()
        && inventory.dynamic_references.is_empty()
        && database_changes.is_empty();
    if request.retire_sources && !retirement_allowed {
        return Err(invalid(
            "Source retirement requires an explicit single-public-root static reference scope with no dynamic references or pending database SQL. Retain aliases and resolve those obligations first.",
        ));
    }
    if !request.retire_sources {
        deletes.clear();
    }
    // Preserve registry provenance while rewriting its exact public paths. Missing inventories are created.
    let registry_path = "docs/public-assets.json";
    let before = existing(workspace, registry_path)?;
    let mut registry = if let Some(bytes) = &before {
        serde_json::from_slice::<Value>(bytes)
            .map_err(|_| invalid("Existing public asset registry is invalid JSON"))?
    } else {
        json!({"version":1,"assets":[]})
    };
    if !registry["assets"].is_array() {
        return Err(invalid("Public asset registry assets must be an array"));
    }
    for asset in registry["assets"].as_array_mut().unwrap() {
        if let Some(url) = asset["path"].as_str()
            && let Some(to) = urls.get(url)
        {
            asset["path"] = json!(to);
        }
    }
    for asset in &inventory.assets {
        let path = urls.get(&asset.url).unwrap_or(&asset.url);
        if !registry["assets"].as_array().unwrap().iter().any(|a| {
            a["path"] == *path
                && (a["publicRoot"] == asset.public_root || a.get("publicRoot").is_none())
        }) {
            registry["assets"].as_array_mut().unwrap().push(json!({"path":path,"publicRoot":asset.public_root,"sha256":asset.sha256,"bytes":asset.bytes}));
        }
        if !request.retire_sources
            && urls.contains_key(&asset.url)
            && !registry["assets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == asset.url)
        {
            registry["assets"].as_array_mut().unwrap().push(json!({"path":asset.url,"publicRoot":asset.public_root,"sha256":asset.sha256,"bytes":asset.bytes,"compatibilityAliasFor":path,"retirement":"pending runtime/DB reference review"}));
        }
    }
    writes.insert(
        registry_path.into(),
        serde_json::to_vec_pretty(&registry).unwrap(),
    );
    // Update owned per-app canonical sidecars too; their schema/provenance remains unchanged.
    for root in &inventory.public_roots {
        if let Some(parent) = Path::new(root).parent() {
            let sidecar = parent
                .join(".devup/public-assets.json")
                .to_string_lossy()
                .replace('\\', "/");
            if let Some(bytes) = existing(workspace, &sidecar)? {
                let mut value: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| invalid("Canonical asset sidecar is invalid JSON"))?;
                for record in value["assets"].as_array_mut().into_iter().flatten() {
                    if let Some(url) = record["path"].as_str()
                        && let Some(to) = urls.get(url)
                    {
                        record["path"] = json!(to);
                    }
                }
                writes.insert(sidecar, serde_json::to_vec_pretty(&value).unwrap());
            }
        }
    }
    let mut mutations = Vec::new();
    for (path, bytes) in &writes {
        let before = existing(workspace, path)?.map(|bytes| hash(&bytes));
        let after = hash(bytes);
        if before.as_deref() != Some(after.as_str()) {
            mutations.push(Mutation {
                path: path.clone(),
                before,
                after: Some(after),
                kind: if path.ends_with("public-assets.json") {
                    "registry"
                } else if inventory.assets.iter().any(|a| &a.path == path)
                    || request
                        .renames
                        .iter()
                        .any(|r| path.ends_with(&url_path(&r.to).unwrap_or_default()))
                {
                    "asset"
                } else {
                    "reference-or-registry"
                }
                .into(),
            });
        }
    }
    for path in &deletes {
        mutations.push(Mutation {
            path: path.clone(),
            before: Some(hash(&workspace.read(path)?)),
            after: None,
            kind: "retire-source".into(),
        });
    }
    mutations.sort_by(|a, b| a.path.cmp(&b.path));
    let mut plan = Plan {
        version: 1,
        id: String::new(),
        request,
        inventory,
        mutations,
        database_changes,
        retirement_allowed,
    };
    plan.id = hash(&serde_json::to_vec(&plan).unwrap());
    for (name, bytes) in sql {
        writes.insert(format!(".devup/asset-migrations/{}/{name}", plan.id), bytes);
    }
    Ok(Built {
        plan,
        writes,
        deletes,
    })
}

pub(super) fn run(workspace: &Workspace<'_>, input: AssetsInput) -> Result<Value, DevupError> {
    let persist = !input.dry_run;
    let built = build(workspace, input.clone())?;
    let path = planned_path(&built.plan.id);
    if persist {
        let mut transaction = OutputTransaction::new();
        transaction.stage_asset(
            "assetPlan",
            workspace.target(&path)?,
            &serde_json::to_vec_pretty(&built.plan).unwrap(),
        )?;
        transaction.commit()?;
    }
    bounded_page(&input, |input| {
        let has_more = built
            .plan
            .mutations
            .len()
            .max(built.plan.database_changes.len())
            > input.response_offset.saturating_add(input.response_limit);
        let summary = json!({"version":built.plan.version,"id":built.plan.id,"inventory":inventory_reply(&built.plan.inventory,input),"mutations":page(&built.plan.mutations,input),"mutationCount":built.plan.mutations.len(),"databaseChanges":page(&built.plan.database_changes,input),"databaseChangeCount":built.plan.database_changes.len(),"retirementAllowed":built.plan.retirement_allowed,"paging":{"offset":input.response_offset,"limit":input.response_limit,"hasMore":has_more}});
        json!({"action":"plan","dryRun":!persist,"planPath":path,"planPersisted":persist,"plan":summary,"dynamicUsageProven":false,"liveDatabaseVerified":false,"sqlExecution":"separate PostgreSQL transaction; not executed by this tool","nextAction":{"tool":"devup_assets","arguments":{"action":"apply","projectRoot":workspace.root,"planPath":path,"dryRun":true}}})
    })
}
