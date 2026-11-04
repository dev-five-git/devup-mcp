# MCP release delivery

All server crates inherit `[workspace.package].version`. A changepack can therefore
release `Cargo.toml` without returning a `crates/devup-mcp/Cargo.toml` upload entry.
Run 41735774706 reached exactly that state: all compilation checks passed, the
workspace-only 0.20.0 release became public without assets, and binary upload
failed while looking for the crate key. Existing clients only recognize the
canonical `devup-mcp(crates/devup-mcp/Cargo.toml)@<version>` tag.

Changepacks still prepares the version-update PR and synchronized lockfile. Its
own release creation is disabled. After those logs are consumed on main,
`release.py prepare` creates or resumes the canonical MCP draft and emits the
crate-keyed upload URL, pending receipt and frozen source commit. A pre-existing
workspace release is preserved; its notes are reused when recovering that version.
Published canonical releases require all expected assets before being accepted
as complete. A missing canonical 0.20.0 release is recoverable without another
version bump.

Build and bundle jobs retain the current workflow's helper, then check out that
frozen source SHA. This keeps a retried draft's binaries, bundle version and tag
on the same source even when main has moved. Uploads verify SHA-256 and size;
identical retries are no-ops, incomplete empty draft uploads can be retried, and
different uploaded bytes are never overwritten. The final job independently
checks the tag/commit and all three platform binaries plus the MCPB bundle before
publishing and setting latest.

Run the isolated Git/API regression suite with:

```sh
python packaging/test_release.py
```

The suite never contacts GitHub or publishes a release. Live delivery runs only
in the main workflow with its scoped `GITHUB_TOKEN`. Retrying an old failed run
uses its original workflow; recovery needs a new main run containing this fix.
