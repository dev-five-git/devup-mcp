# R12 validator and size guidance

## Scope and evidence

- Brief: `C:/Users/owjs3/orca/devup-mcp-briefs/R12-validator-and-size-guidance.md`.
- Read-only references: WQUW-118 `docs/wquw-118-devup-r11-report.md`, `docs/wquw-118-devup-r11-calls.json` (last call contains the complete original TSX), and WQUW-119 `docs/wquw-119/r11-report.md` in the two repositories named in the brief.
- Base: `integration/r6`, commit `1b61eb6160424845fab56adf4dfdae9759bc00f8`.
- The three reported attributes are `Text as="input" maxLength={50}`, `{50}`, and `{500}`. The failing byte ranges are `[59371,59380]`, `[61944,61953]`, `[81513,81522]`.
- Read installed `@devup-ui/react` 1.0.41 `dist/components/Text.d.ts` and `dist/types/props/index.d.ts`: default `Text` uses `span`; polymorphic `as: T` merges `React.ComponentProps<NoInfer<T>>`. HTML attributes come from `@types/react` 19.2.17.
- `check-types.cjs` uses an in-memory source file and the installed TypeScript compiler; it writes nothing to the reference repository. `type-evidence.json` records valid input/textarea attributes and rejected default-span/button/typo cases.

## Implementation sequence

1. Add failing tests for intrinsic `as`, strict unknown-prop boundaries, failed-validation guidance, and raw/serialized inline overflow.
2. Resolve literal `as` before checking any attribute; use a catalog derived from React's actual HTML types. Preserve existing style/token/severity checks and conservative handling of unresolved render targets.
3. Reuse export's `recoveryState`, reason, and `nextAction.tool`/`arguments` shape. Offer a corrected TSX only for unambiguous exact-token replacements that pass validation at the original strictness; otherwise explain why a source edit needs caller judgment.
4. Measure output bytes and serialized MCP response bytes. At the export projection boundary preserve the artifact and projection arguments, set resource delivery, and keep sourceMap with its generated output when splitting.
5. Run all workspace tests with `-j 2`, JS behavior tests, clippy with `-j 2`, fmt, and the original validator call against this workspace's own binary. Review all changes, commit, then run `cargo clean`. Do not push or use installed MCP tools.

## Validation

- Red: `red-tests.txt` records failures in all three requested areas before implementation (polymorphic `maxLength`, missing validator recovery, missing size/reprojection details). The strict unknown-prop boundary passed on the old implementation and remains enforced.
- Original-input red/green: the same 90,859-byte TSX from the last WQUW-118 call was replayed over stdio to this worktree's `target/debug/devup-mcp.exe`. Before: `ok=false`, 3 errors / 16 warnings / 396 info. After: `ok=true`, 0 errors / 16 warnings / 396 info. Both content and structuredContent agree. See `red-original-validator.json`, `green-original-validator.json` and the reproducible replay driver.
- Additional red/green: `red-asset-split.txt` catches invalid splits that separate assetPublicRoot from assetRequests. Such coupled requests now retain the complete resource retry and explain why no optional split is offered.
- Additional red/green: `red-wire-identity.txt` caught a 182-byte undercount (1,200,124 vs actual 1,200,306 bytes) caused by omitting the server identity from serialization. Wire measurement now uses the same `tool_result` assembly as the response.
- Recovery tests deserialize all supplied arguments and execute the actual export handler against the same retained artifact. Auth/upstream calls panic in this test, proving recovery uses no Figma calls. The error envelope is checked for identical content and structuredContent. sourceMap split requests include a generated output.
- Validator recovery tests preserve `strict` and `projectRoot`, validate the complete corrected TSX, preserve Korean text/byte offsets, and reject ambiguous-token or mixed-error automatic corrections.
- `cargo test --workspace -j 2 --no-fail-fast`: **779 passed / 0 failed / 2 ignored** (baseline 770 + 9 R12 tests).
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: **12 passed / 0 failed**.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: exit 0, no warnings/errors.
- `cargo fmt --all -- --check`: exit 0, no diff.
- Existing R10/R11 tests pass. No classification or diagnostic suppression was changed.
- **Golden changes: none.** No failing test was removed or weakened.
- Windows test linking prints linker stdout notices; these are not clippy findings or test failures.
- `target-proof.json` records the workspace-local Cargo target, executable paths and SHA-256 hashes. No `CARGO_TARGET_DIR` override was set. Installed devup-mcp MCP tools were not used. Reference repositories were read only.
- 자기 target 에서 검증했고 실행된 테스트가 내 것임을 확인했다.

## Contract and limits

HTML prop names are generated from 119 React intrinsic HTML elements in 47 attribute groups. The existing style/non-style checks remain intact; the rendered tag only adds its verified HTML props. Literal strings and string expression containers are resolved before attribute traversal. Dynamic/custom targets and later spreads cannot establish a tag, so they do not widen accepted HTML props.

`recoveryState=available` carries `nextAction.tool` and a complete `arguments` object. Validator automatic corrections are limited to unique exact-token warning replacements that pass the original validation settings. `unrecoverable` describes automatic correction, with an explicit source-edit reason; it does not claim the user's code is unfixable.

Inline overflow details report raw payload bytes per output, total payload bytes, full serialized MCP result bytes, limit and excess. Export recovery preserves the artifact and projection settings with `delivery=resource`; optional splits retain sourceMap dependencies. Resource memory/retention limits still apply and do not receive misleading inline retry guidance.
