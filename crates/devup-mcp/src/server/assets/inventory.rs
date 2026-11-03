use super::*;

const SKIP: &[&str] = &[
    ".git",
    ".devup",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".turbo",
    "coverage",
    ".worktrees",
    ".worktree",
];
const SOURCE: &[&str] = &[
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "json", "rs", "sql", "css", "scss", "html", "md",
    "toml", "yaml", "yml",
];

fn files(
    workspace: &Workspace<'_>,
    start: &Path,
    out: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Value>,
    public: bool,
) -> Result<(), DevupError> {
    let relative = workspace.relative(start)?;
    if start
        .components()
        .count()
        .saturating_sub(workspace.root.components().count())
        > 64
    {
        return Err(invalid(
            "Asset inventory depth exceeds 64; narrow the selected scope",
        ));
    }
    if !relative.is_empty() {
        workspace.target(&relative)?;
    }
    let metadata = std::fs::symlink_metadata(start)
        .map_err(|e| invalid(format!("Cannot inspect asset inventory path: {e}")))?;
    if metadata.file_type().is_symlink() {
        diagnostics.push(json!({"path":relative,"reason":"symlink-not-followed"}));
        return Ok(());
    }
    if metadata.is_file() {
        out.insert(start.into());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(invalid("Inventory requires regular files and directories"));
    }
    let mut entries = std::fs::read_dir(start)
        .map_err(|e| invalid(format!("Cannot read asset inventory directory: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| invalid(e.to_string()))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        if out.len() >= MAX_FILES {
            return Err(invalid(
                "Asset inventory file bound exceeded; select smaller publicRoots/referencePaths",
            ));
        }
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| invalid(e.to_string()))?;
        if !public && kind.is_dir() && SKIP.contains(&entry.file_name().to_string_lossy().as_ref())
        {
            continue;
        }
        if kind.is_dir() && path.join(".git").exists() {
            diagnostics.push(
                json!({"path":workspace.relative(&path)?,"reason":"nested-checkout-not-scanned"}),
            );
            continue;
        }
        files(workspace, &path, out, diagnostics, public)?;
    }
    Ok(())
}

pub(super) fn scan(
    workspace: &Workspace<'_>,
    input: &AssetsInput,
) -> Result<Inventory, DevupError> {
    let mut inventory = Inventory {
        public_roots: Vec::new(),
        assets: Vec::new(),
        references: Vec::new(),
        dynamic_references: Vec::new(),
        file_hashes: BTreeMap::new(),
        database_snapshots: Vec::new(),
        diagnostics: Vec::new(),
        complete: true,
    };
    let roots = if input.public_roots.is_empty() {
        super::super::mask_guard::public_dirs_of(&workspace.root)
            .iter()
            .map(|p| workspace.relative(p))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        input.public_roots.clone()
    };
    let mut total = 0usize;
    let mut public_files = BTreeSet::new();
    for root in roots {
        let path = workspace
            .target(&format!("{root}/.asset-probe"))?
            .display_path()
            .parent()
            .unwrap()
            .to_path_buf();
        let mut selected = BTreeSet::new();
        files(
            workspace,
            &path,
            &mut selected,
            &mut inventory.diagnostics,
            true,
        )?;
        for file in selected {
            if !public_files.insert(file.clone()) {
                return Err(invalid("Overlapping publicRoots are ambiguous"));
            }
            let relative = workspace.relative(&file)?;
            let bytes = workspace.read(&relative)?;
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| invalid("Asset inventory byte bound exceeded"))?;
            if total > MAX_TOTAL_BYTES {
                return Err(invalid(
                    "Asset inventory byte bound exceeded; narrow the scope",
                ));
            }
            let url = super::super::validation::public_asset_url(&path, &file)?;
            inventory.assets.push(PublicAsset {
                public_root: root.clone(),
                url: url.clone(),
                path: relative,
                sha256: hash(&bytes),
                bytes: bytes.len(),
                semantic_name: semantic(&url),
            });
        }
        inventory.public_roots.push(root);
    }
    let mut sources = BTreeSet::new();
    if input.reference_paths.is_empty() {
        files(
            workspace,
            &workspace.root,
            &mut sources,
            &mut inventory.diagnostics,
            false,
        )?;
    } else {
        for relative in &input.reference_paths {
            files(
                workspace,
                workspace.target(relative)?.display_path(),
                &mut sources,
                &mut inventory.diagnostics,
                false,
            )?;
        }
    }
    for path in sources {
        if public_files.contains(&path)
            || inventory
                .public_roots
                .iter()
                .any(|root| path.starts_with(workspace.root.join(root)))
        {
            continue;
        }
        let relative = workspace.relative(&path)?;
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if !SOURCE.contains(&extension) {
            continue;
        }
        if input
            .database_references
            .iter()
            .any(|db| db.snapshot_path == relative)
        {
            continue;
        }
        let bytes = workspace.read(&relative)?;
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("Asset inventory byte bound exceeded"))?;
        if total > MAX_TOTAL_BYTES {
            return Err(invalid(
                "Asset inventory byte bound exceeded; narrow the scope",
            ));
        }
        let source = std::str::from_utf8(&bytes)
            .map_err(|_| invalid(format!("Source file is not UTF-8: {relative}")))?;
        let (references, dynamic, diagnostics) = references::scan(&relative, source);
        inventory.references.extend(references);
        inventory.dynamic_references.extend(dynamic);
        inventory.diagnostics.extend(diagnostics);
        inventory.file_hashes.insert(relative, hash(&bytes));
    }
    for descriptor in &input.database_references {
        let bytes = workspace.read(&descriptor.snapshot_path)?;
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("Asset inventory byte bound exceeded"))?;
        if total > MAX_TOTAL_BYTES {
            return Err(invalid(
                "Asset inventory byte bound exceeded by database snapshots",
            ));
        }
        inventory
            .file_hashes
            .insert(descriptor.snapshot_path.clone(), hash(&bytes));
        inventory
            .database_snapshots
            .push(database::inventory(descriptor, &bytes)?);
    }
    inventory.public_roots.sort();
    inventory.assets.sort_by(|a, b| a.path.cmp(&b.path));
    inventory
        .references
        .sort_by(|a, b| (&a.file, a.start).cmp(&(&b.file, b.start)));
    inventory.complete = inventory.diagnostics.is_empty();
    Ok(inventory)
}

pub(super) fn duplicates(inventory: &Inventory) -> Vec<Value> {
    let mut groups = BTreeMap::<&str, Vec<&PublicAsset>>::new();
    for asset in &inventory.assets {
        groups.entry(&asset.sha256).or_default().push(asset);
    }
    groups.into_iter().filter(|(_,assets)|assets.len()>1).map(|(sha256,assets)|json!({"sha256":sha256,"assets":assets.iter().take(20).collect::<Vec<_>>(),"assetCount":assets.len(),"membersPaged":assets.len()>20,"equivalence":"exact-bytes","automaticMerge":false,"allMembers":"Read inventory pages to retrieve every member path"})).collect()
}
