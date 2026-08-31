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
