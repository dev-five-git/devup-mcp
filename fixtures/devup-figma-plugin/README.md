# devup-figma-plugin compatibility corpus

이 corpus는 `dev-five-git/devup-figma-plugin` commit `243db650f1d635ab5385546a2a297eae4ea93515`의 테스트 결과를 고정한 회귀 자료입니다.

검증 범위는 다음과 같이 구분합니다.

- `rust_snapshot`: upstream test 252개에서 생성된 JSON input/golden output 268쌍을 `compat_fixtures::upstream_json_goldens`가 byte 단위로 전부 실행합니다.
- `rust_assertion`: upstream assertion inventory 666개를 같은 영역의 실제 Rust 단위·통합 테스트에 연결합니다. import metadata, component usage, responsive provider, asset manifest, Section note, theme scope/conflict와 artifact 재사용을 포함하며 각 항목은 registry의 실행 test를 가리킵니다.
- `upstream_runtime_only`: 38개 항목은 Figma plugin module/codegen handler, iframe, notify 또는 browser download 수명주기에만 의존합니다. 각 항목은 이를 대체하는 MCP tool/error-boundary contract를 가리킵니다.
- `out_of_scope_write`: 22개 항목은 Figma 문서, text style 또는 import 대상을 변경하므로 read-only devup-mcp 범위에서 실행하지 않습니다. 각 항목은 내장 script mutation 비포함 contract를 가리킵니다.
- `not_ported`: 0개입니다. 미분류 항목이 다시 생기면 `compat_manifest`가 실패합니다.

따라서 정확한 표현은 “268/268 snapshot byte parity, 666개 실행 가능한 대표 Rust assertion 연결, 60개 명시적 runtime/write 경계, 978-entry upstream test inventory”입니다. 대표 assertion 연결은 JavaScript assertion을 각각 별도 fixture로 복제했다는 뜻이 아니라 동일 동작 영역의 production-shaped Rust contract에 연결했다는 뜻입니다.

`coverage-registry.json`은 parity/대표 assertion에 쓰는 Rust test의 source file과 함수 이름을 등록합니다. `compat_manifest`는 978개 ledger entry가 등록된 실행 test 또는 근거가 있는 비-parity 분류 중 하나인지, 등록 함수가 실제 `#[test]`/`#[tokio::test]`인지, 268개 fixture와 snapshot이 빠짐없이 snapshot harness에 연결됐는지 검사합니다.

```powershell
cargo test -p devup-mcp-devup-ui --test compat_manifest
cargo test -p devup-mcp-devup-ui --test compat_fixtures
```

`manifest.json`은 원본 54개 test file, 978 pass/0 fail, 268 snapshot이라는 upstream 기준 실행 정보와 536개 case/snapshot 파일의 LF-normalized SHA-256을 보존합니다. `ledger.json`은 이 upstream 결과의 inventory/coverage 분류이며 그 자체가 Rust 실행 결과를 뜻하지 않습니다.

## Codegen Defect Corrections

The reviewed Rust goldens intentionally differ from the pinned plugin output:
multi-child auto-layout containers retain their individual assets; captured roots
do not inherit absolute placement without a collected parent; root FILL sizing
is axis-specific rather than implicitly filling both axes; and typography keeps
real letter-spacing overrides while removing redundant resolved metrics.
The 50 affected snapshot checksums are synchronized in `manifest.json`; original
case inputs and upstream inventory remain unchanged. These are static codegen
regressions, not new browser pixel measurements.

Intrinsic-width text (`textAutoResize=WIDTH_AND_HEIGHT`) now emits
`whiteSpace="pre"` so a narrow flex parent cannot soft-wrap a captured marquee.
The reviewed corpus changes add only that property and formatter whitespace;
source-map snapshots also record the explicit textAutoResize derivation.
Clamped text and list text retain their separate policies. Original inputs and
upstream inventory are unchanged; checksums follow the reviewed snapshots.

The accessibility correction adds image `alt` attributes derived from captured
layer names to 34 reviewed codegen snapshots. Explicitly named semantic regions
and heading layers also emit their HTML tags. The review checks that all other
attribute values and text remain unchanged, allowing formatter line wrapping.
Only the affected snapshot checksums are updated; case inputs, upstream source
identity and test inventory are preserved. These are non-visual HTML corrections,
not additional browser pixel measurements.

## R4 배치 계약 변경

`upstream-codegen-191`, `251`–`254`의 골든은 absolute 자식의 기준 요소를 생성 코드가 소유하도록 갱신했습니다. PAGE/SECTION 루트에 relative와 FIXED 축의 크기를 보존하고, 앞에 그린 absolute 배경에는 루트 stacking context 안의 z-index를 적용합니다. 원본 case JSON은 유지했고 변경한 5개 snapshot의 LF-normalized checksum만 동기화했습니다. 이는 생성 코드 배치 계약의 의도적인 변경이며 신규 브라우저 픽셀 측정 결과를 뜻하지 않습니다.

The carriage-return correction updates only codegen snapshot 253: lone CR
controls render as inline spaces, while CRLF, LF and Unicode line separators
retain their hard breaks. Its two changed JSX lines follow three lone CRs in
unchanged captured strings (including trailing spaces). The rule was checked
against the native Figma location transit PNGs and four single-line source
boxes; this is not a new pixel measurement of the Greetings corpus screen.
Raw case inputs, style properties and the upstream inventory remain unchanged.


### Captured Korean HEIGHT wrapping correction

Six reviewed codegen goldens intentionally replace exactly fourteen
`wordBreak="keep-all"` lines with `wordBreak="normal"`. Every changed source
Text has captured Korean styled segments, `textAutoResize=HEIGHT`, a positive
measured width on a FILL axis, and `textTruncation=DISABLED` without a line cap.
The old keep-all default adds word-boundary restrictions absent from Figma's
captured wrapping. The source-backed Vision correction is protected by
`captured_korean_wrapping.rs`. Separate browser verification of those Vision
cases matched source heights 46/52px instead of the old 69/78px; these older
corpus screens have not acquired new PNG measurements.

| Golden case | Source Text IDs | Changed lines |
| --- | --- | --- |
| 165 | 1:17, 1:18 | 2 |
| 191 | 107:32 | 1 |
| 213 | 109:64 | 1 |
| 223 | 113:10, 113:13, 113:18, 113:25, 113:29, 113:33 | 6 |
| 252 | 284:18751, 284:18753 | 2 |
| 253 | 213:7494, 213:7495 | 2 |

The review verifies that all other output bytes are unchanged after LF
normalization. Original case JSON, upstream source identity and the test
inventory remain untouched; only these six snapshot checksums change.
Intrinsic HUG/no-wrap text, unknown fields, truncation/clamping, explicit
existing word-break settings and theme policies retain their separate guards.
`compat_fixtures` continues to compare all 268 complete outputs byte for byte;
there is no comparator exception or broad text normalization.

### ChondroGen delivery feedback (2026-11-03)

The later explicit Korean-word-boundary requirement supersedes the HEIGHT
wrapping default above: Korean text uses keep-all plus emergency long-token
wrapping unless an explicit user/theme policy says otherwise. Seventeen reviewed
codegen goldens change only this wrapping policy, emergency wrapping, or removal
of one component-usage inspection comment. Source case JSON, upstream identity
and coverage inventory are unchanged. Only the affected snapshot checksums are
updated; complete outputs still compare byte for byte. These changes do not
assert new pixel measurements of every pinned upstream screen.
