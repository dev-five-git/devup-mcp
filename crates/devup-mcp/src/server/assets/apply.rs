use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Undo {
    mutation: plan::Mutation,
    before_base64: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Journal {
    version: u32,
    plan_id: String,
    project_root: String,
    entries: Vec<Undo>,
}

fn read_optional(workspace: &Workspace<'_>, path: &str) -> Result<Option<Vec<u8>>, DevupError> {
    let target = workspace.target(path)?;
    match std::fs::symlink_metadata(target.display_path()) {
        Ok(_) => target.read_bounded(MAX_FILE_BYTES).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(invalid(error.to_string())),
    }
}
fn current_hash(workspace: &Workspace<'_>, path: &str) -> Result<Option<String>, DevupError> {
    Ok(read_optional(workspace, path)?.map(|bytes| hash(&bytes)))
}
fn verify_after(workspace: &Workspace<'_>, journal: &Journal) -> Result<(), DevupError> {
    for entry in &journal.entries {
        if current_hash(workspace, &entry.mutation.path)? != entry.mutation.after {
            return Err(invalid(format!(
                "Migration target changed after apply; refusing to overwrite it: {}",
                entry.mutation.path
            )));
        }
    }
    Ok(())
}

pub(super) fn run(
    workspace: &Workspace<'_>,
    input: &AssetsInput,
    rollback: bool,
) -> Result<Value, DevupError> {
    let path = input
        .plan_path
        .as_deref()
        .ok_or_else(|| invalid("planPath is required for apply and rollback"))?;
    let plan: plan::Plan = serde_json::from_slice(&workspace.read(path)?)
        .map_err(|_| invalid("Asset migration plan JSON is invalid"))?;
    if plan.version != 1 || plan.id.len() != 64 || !plan.id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("Unsupported asset migration plan"));
    }
    let plan_workspace = Workspace::new(&plan.request.project_root, workspace.policy)?;
    if plan_workspace.root != workspace.root {
        return Err(invalid(
            "Asset migration plan belongs to another projectRoot",
        ));
    }
    let base = format!(".devup/asset-migrations/{}", plan.id);
    let journal_path = format!("{base}/rollback.json");
    let receipt_path = format!("{base}/receipt.json");
    let receipt = read_optional(workspace, &receipt_path)?;
    let state = receipt
        .as_ref()
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .and_then(|value| value["state"].as_str().map(str::to_owned));
    if rollback {
        let journal: Journal = serde_json::from_slice(&workspace.read(&journal_path)?)
            .map_err(|_| invalid("Asset rollback journal is invalid"))?;
        if journal.version != 1
            || journal.plan_id != plan.id
            || journal.project_root != workspace.root.to_string_lossy()
        {
            return Err(invalid(
                "Rollback journal identity does not match this plan",
            ));
        }
        if state.as_deref() == Some("rolled-back") {
            for entry in &journal.entries {
                let retained = !plan.database_changes.is_empty()
                    && matches!(entry.mutation.kind.as_str(), "asset" | "registry");
                let expected = if retained {
                    &entry.mutation.after
                } else {
                    &entry.mutation.before
                };
                if &current_hash(workspace, &entry.mutation.path)? != expected {
                    return Err(invalid(
                        "Files changed after rollback; state cannot be replayed",
                    ));
                }
            }
            return Ok(
                json!({"action":"rollback","state":"already-rolled-back","changed":false,"planId":plan.id}),
            );
        }
        verify_after(workspace, &journal)?;
        if input.dry_run {
            return Ok(
                json!({"action":"rollback","dryRun":true,"planId":plan.id,"fileCount":journal.entries.len(),"liveDatabaseRolledBack":false,"databaseRollbackSql":format!("{base}/rollback.sql")}),
            );
        }
        let mut transaction = OutputTransaction::new();
        for entry in &journal.entries {
            if !plan.database_changes.is_empty()
                && matches!(entry.mutation.kind.as_str(), "asset" | "registry")
            {
                continue;
            }
            let target = workspace.target(&entry.mutation.path)?;
            if let Some(encoded) = &entry.before_base64 {
                let bytes = STANDARD
                    .decode(encoded)
                    .map_err(|_| invalid("Invalid rollback binary"))?;
                if entry.mutation.before.as_deref() != Some(hash(&bytes).as_str()) {
                    return Err(invalid("Rollback source digest mismatch"));
                }
                transaction.stage_registry(
                    format!("restore:{}", entry.mutation.path),
                    target,
                    &bytes,
                    entry.mutation.after.clone(),
                )?;
            } else if let Some(after) = &entry.mutation.after {
                transaction.stage_remove(
                    format!("remove:{}", entry.mutation.path),
                    target,
                    after.clone(),
                )?;
            }
        }
        transaction.stage_registry("receipt",workspace.target(&receipt_path)?,&serde_json::to_vec_pretty(&json!({"version":1,"planId":plan.id,"state":"rolled-back","liveDatabaseRolledBack":false})).unwrap(),receipt.as_ref().map(|bytes|hash(bytes)))?;
        transaction.commit()?;
        return Ok(
            json!({"action":"rollback","state":"rolled-back","planId":plan.id,"fileCount":journal.entries.len(),"retainedCanonicalAssetsAndRegistry":!plan.database_changes.is_empty(),"liveDatabaseRolledBack":false,"databaseRollbackSql":format!("{base}/rollback.sql")}),
        );
    }
    if state.as_deref() == Some("applied") {
        let journal: Journal = serde_json::from_slice(&workspace.read(&journal_path)?)
            .map_err(|_| invalid("Asset rollback journal is invalid"))?;
        verify_after(workspace, &journal)?;
        return Ok(
            json!({"action":"apply","state":"already-applied","changed":false,"planId":plan.id,"liveDatabaseApplied":false}),
        );
    }
    let built = plan::build(workspace, plan.request.clone())?;
    if built.plan.id != plan.id {
        return Err(invalid(
            "Asset inventory, source, registry or database snapshot changed since this plan; create a new plan",
        ));
    }
    if input.dry_run {
        return Ok(
            json!({"action":"apply","dryRun":true,"planId":plan.id,"fileCount":plan.mutations.len(),"localChangesReady":true,"liveDatabaseApplied":false,"databaseChanges":page(&plan.database_changes,input),"databaseChangeCount":plan.database_changes.len(),"retainedAliases":!plan.request.retire_sources}),
        );
    }
    let mut entries = Vec::new();
    for mutation in &plan.mutations {
        let before = read_optional(workspace, &mutation.path)?;
        if before.as_ref().map(|bytes| hash(bytes)) != mutation.before {
            return Err(invalid(
                "Source changed while preparing the asset migration",
            ));
        }
        entries.push(Undo {
            mutation: mutation.clone(),
            before_base64: before.map(|bytes| STANDARD.encode(bytes)),
        });
    }
    let journal = Journal {
        version: 1,
        plan_id: plan.id.clone(),
        project_root: workspace.root.to_string_lossy().into(),
        entries,
    };
    let encoded_journal = serde_json::to_vec_pretty(&journal).unwrap();
    if encoded_journal.len() > MAX_FILE_BYTES {
        return Err(invalid(
            "Rollback journal exceeds the safe bound; split this migration",
        ));
    }
    let mut transaction = OutputTransaction::new();
    let mut ordered = plan.mutations.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|mutation| {
        if mutation.after.is_none() {
            2
        } else if mutation.kind == "asset" {
            0
        } else {
            1
        }
    });
    for mutation in ordered {
        let target = workspace.target(&mutation.path)?;
        if built.deletes.contains(&mutation.path) {
            transaction.stage_remove(
                format!("retire:{}", mutation.path),
                target,
                mutation
                    .before
                    .clone()
                    .ok_or_else(|| invalid("Removal has no source digest"))?,
            )?;
        } else {
            let bytes = built
                .writes
                .get(&mutation.path)
                .ok_or_else(|| invalid("Plan mutation has no generated bytes"))?;
            // Concurrent identical files still have different ownership for rollback.
            transaction.stage_registry(
                format!("write:{}", mutation.path),
                target,
                bytes,
                mutation.before.clone(),
            )?;
        }
    }
    for (path, bytes) in &built.writes {
        if path.starts_with(&format!("{base}/")) {
            transaction.stage_asset(path, workspace.target(path)?, bytes)?;
        }
    }
    transaction.stage_asset(
        "rollbackJournal",
        workspace.target(&journal_path)?,
        &encoded_journal,
    )?;
    transaction.stage_registry(
        "receipt",
        workspace.target(&receipt_path)?,
        &serde_json::to_vec_pretty(
            &json!({"version":1,"planId":plan.id,"state":"applied","liveDatabaseApplied":false}),
        )
        .unwrap(),
        receipt.as_ref().map(|bytes| hash(bytes)),
    )?;
    let output_paths = transaction.commit()?;
    let output_page = output_paths
        .iter()
        .skip(input.response_offset)
        .take(input.response_limit)
        .collect::<BTreeMap<_, _>>();
    Ok(
        json!({"action":"apply","state":"applied","planId":plan.id,"fileCount":plan.mutations.len(),"outputPaths":output_page,"outputPathCount":output_paths.len(),"rollbackJournal":journal_path,"retainedAliases":!plan.request.retire_sources,"dynamicUsageProven":false,"liveDatabaseApplied":false,"databaseChangesPending":!plan.database_changes.is_empty(),"databaseSql":{"migration":format!("{base}/migration.sql"),"verify":format!("{base}/verify.sql"),"rollback":format!("{base}/rollback.sql")}}),
    )
}
