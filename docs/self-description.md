# What the server says about itself

Two changes to the same surface: the guidance devup-mcp publishes, and the
staleness it reports. Both are about cost and honesty, not features.

## The guidance moved from push to pull

`initialize` published roughly 3,500 bytes of prose: a build-identity sentence
plus twelve numbered rules covering Figma export, sourceMap sizing
measurements, asset placeholders, SECTION batching and delivery. Every client
paid that on every session, including a client that only ever called
`devup_stack_diff` and never touched a design.

Meanwhile `resources/list` returned an empty array. The `resources` capability
was already advertised and implemented; it carried generated outputs and
nothing else.

So the bulk moved into a resource the caller pulls:

| | before | after |
|---|---|---|
| `initialize` instructions | ~3,500 bytes | **951 bytes** |
| `resources/list` | `[]` | `devup://guide/usage` (`text/markdown`) |
| guide body | n/a | 3,501 bytes, read on demand |

What stays in `instructions` is what changes the very next action: what this
server is for, that implementation starts with `devup_figma_export` and takes
`tsx`, how to read the identity on every response, and the guide's URI.
Everything else is one `resources/read` away.

This is a relocation, not a rewrite. Rule numbers are preserved, including the
original list's skip from 8 to 10, and `guide.rs` carries a test asserting that
every original rule number still appears in instructions-plus-guide combined.

Keeping the text compiled into the binary is the other half of the point. A
README describes whatever build the reader is looking at; this ships with the
build that answers the call, so it cannot describe behaviour the running server
does not have. It is the same reason `orca skills get` serves its guide from
the binary rather than from a checked-in file.

## Staleness is reported, never acted on

This repo has paid for "which build am I actually talking to" more than once:
`displayVersion` and `identityGuidance` exist because the answer was expensive,
and R17's report records the tested binary's SHA-256 for the same reason.

`server.updateAvailable` now rides the identity that `tool_result` already
attaches to every response:

```json
"updateAvailable": {
  "current": "0.4.5",
  "state": "unknown",
  "note": "The newest release is not known: the check has not run yet, or it could not reach the network. This is an absent answer, not a failure, and it never delays a call."
}
```

`state` distinguishes four things a boolean would have merged: `disabled`, not
yet run, ran and could not tell (both `unknown`), and `known`. Only `known`
ever carries `isStale`, and `latest` appears only when it is known.

### What makes the constraints structural rather than careful

- **No network on the call path.** `snapshot()` reads a cache and returns; it is
  synchronous and has nothing to await. The only fetch lives in the task
  `spawn()` starts.
- **`--self-check` and `doctor` stay network-free.** `spawn()` is called from
  `get_info` — the `initialize` handler — and not from server construction,
  because `self_check()` builds a server on a thread with no Tokio runtime.
  `spawn()` also returns immediately when `Handle::try_current()` fails, so the
  property holds even if someone later calls it from the wrong place.
- **Failure is an absent answer.** A rate limit, an offline host, a proxy or a
  DNS failure leaves `state: "unknown"`. None of them is something the caller
  asked about.
- **The server never replaces its own binary.** It cannot: the MCP host owns
  this process and its stdio pipes, and the README already documents that a
  replaced binary cannot recover the pipe the host holds. There is no download,
  no replacement and no execution. The wording states a difference rather than
  issuing an instruction.
- **Only a tag naming this crate counts.** Release tags here are per-crate
  (`devup-mcp(crates/devup-mcp/Cargo.toml)@0.4.5`); a `devup-mcp-figma` release
  says nothing about this binary's version, and a tag that cannot be parsed is
  `unknown` rather than a guess.

### Opt-out

`DEVUP_MCP_NO_UPDATE_CHECK` — any value except `0`, `false` or empty disables
the check and reports `state: "disabled"`. Measured:

| value | state |
|---|---|
| `1`, `true` | `disabled` |
| `0`, `false`, empty, unset | `unknown` (check enabled) |

Environment only, deliberately: `parse_cli_args` is being extended
concurrently for the skills commands, and a flag there would have been a merge
conflict for no gain.

Interval is 24 hours with a 20-second startup delay, so a cold session reports
`unknown` and never waits.

## Deliberately not done

Skill installation and upstream `SKILL.md` fetching are **not** here. That work
is in flight separately (`--install-skills` / `--check-skills` and
`src/skills/`), and mirroring another repository's documentation inside this one
would ship a copy that drifts from its owner — one of those upstream URLs
already 404s.

## Verification

Measured on this branch:

```
cargo fmt --all -- --check                                        exit 0
cargo test --workspace                                            1011 passed / 0 failed / 2 ignored (98 suites)
cargo clippy --locked --workspace --all-targets --all-features    exit 0, 0 warnings
cargo insta test --workspace --all-features --check               no snapshots to review
cargo test --locked -p devup-mcp --test stdio_smoke               2 passed
node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs   exit 0
cargo metadata --locked                                           exit 0
```

Base was 994; the 17 added tests are 5 stdio-level contract tests, 3 in
`guide.rs` and 9 in `release_check.rs`.

Five existing tests were updated rather than weakened. Three in
`resource_delivery.rs` asserted the resource list contained only manifests, and
now assert the manifests plus the guide, with the guide last so manifest
positions keep their meaning. Two in `figma_doctor.rs` pinned the entire
`server` identity block as one JSON literal; they now pin the tool's own
payload exactly and assert the identity field by field, which is both stronger
and no longer brittle to an identity that grows.

Exercised against the built binary, not only through tests: instructions came
back at 951 bytes naming the guide URI, `resources/list` returned the guide,
`resources/read` returned its 3,501-byte body, a `devup_project_context` call
carried `updateAvailable` with `state: "unknown"` without delay, every opt-out
value behaved as tabulated, and `--self-check` reported no `updateAvailable`.
