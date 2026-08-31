# devup-mcp

Rust-native MCP server that reads Figma designs and generates DevupUI artifacts. One binary acts as a local stdio MCP server and a read-only client of Figma Remote MCP.

저장소는 Cargo workspace이며 `devup-mcp` 실행 crate, OAuth·upstream·snapshot을 담당하는 `devup-mcp-figma`, TSX·theme projection을 담당하는 `devup-mcp-devup-ui`로 구성됩니다. 별도 IR/auth/server crate 없이 제품 설치 단위는 `devup-mcp` 하나입니다.

## 현재 제공 기능

- `devup_figma_auth`: Figma 연결 상태 확인, 브라우저 OAuth 로그인, 로그아웃
- `devup_figma_to_ui`: Figma node 링크를 `@devup-ui/react` TSX로 변환
- `devup_figma_to_json`: Figma 변수와 로컬 스타일을 `devup.json`으로 변환
- `devup_figma_search`: 파일 전체의 page, section, frame, component를 이름으로 탐색
- `devup_figma_explore`: 링크된 요구사항/라벨 주변의 실제 화면 후보를 공간 순서로 탐색
- `devup_figma_continue`: host가 실행한 공식 Figma MCP read 결과로 중단된 변환을 재개
- Figma Plugin API의 readable data property를 raw JSON으로 보존하고, 알려지지 않은 runtime field는 `extra`, 실패한 getter는 `fieldErrors`로 유지

host handoff 경로에는 Figma PAT, 사용자가 만든 OAuth app, 내장 client secret이 필요하지 않습니다. direct 경로는 Figma Remote MCP의 OAuth discovery, Dynamic Client Registration, PKCE S256과 일시적인 `127.0.0.1` callback을 구현하지만, Figma는 현재 MCP Catalog에 승인된 client의 registration만 허용합니다. private build에서는 이미 인증된 공식 Figma MCP를 사용하는 `auto` 또는 `host`가 기본 경로입니다.

## 빌드와 설치

Rust 1.88 이상이 필요합니다.

```bash
cargo install --git https://github.com/dev-five-git/devup-mcp.git --branch owjs3901/figma-remote-mcp devup-mcp
```

소스에서 검증하려면 다음을 실행합니다.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --release
```

## MCP 설정

stdio MCP를 지원하는 클라이언트에 다음과 같이 등록합니다.

```json
{
  "mcpServers": {
    "devup-mcp": {
      "command": "devup-mcp"
    }
  }
}
```

시스템 브라우저는 `devup_figma_auth`의 `login`을 명시적으로 호출할 때만 열립니다. 일반 변환의 기본 `auto` 정책은 direct credential이 없거나 Catalog/capability가 허용되지 않으면 브라우저를 열지 않고 공식 Figma MCP handoff를 반환합니다. 인증 정보는 운영체제 credential store에만 저장되며 `logout`은 해당 정보만 삭제합니다.

### 인증

```json
{ "action": "status" }
```

`action`은 `status`, `login`, `logout` 중 하나입니다.

### Figma → DevupUI

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "componentName": "OptionalComponentName",
  "includeDiagnostics": true,
  "sourcePolicy": "auto",
  "scope": "node",
  "outputPath": "optional/path/Component.tsx"
}
```

결과에는 `tsx`, import 목록, 사용된 token, source 식별자, 보존한 node 수와 fallback diagnostics가 포함됩니다. Auto Layout은 `Flex`, 일반 container는 `Box`, text는 `Text`로 변환하고 theme binding이 있으면 JSX prop에서 `$token`을 우선 사용합니다. standalone 결과에서는 Figma instance의 실제 자식 상태를 펼쳐 정의되지 않은 component 참조를 만들지 않습니다.

### Figma → devup.json

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "scope": "file",
  "includeDiagnostics": true,
  "sourcePolicy": "auto",
  "outputPath": "optional/path/devup.json"
}
```

결과는 `theme.colors`, `theme.typography`, `theme.length`, `theme.shadow`를 포함하는 결정적 JSON 문자열과 counts, completeness를 반환합니다.

`outputPath`를 생략하면 결과를 메모리와 MCP 응답에만 유지합니다. 명시하면 생성된 TSX 또는 `devup.json`만 해당 경로에 기록하고 실제 절대 경로를 응답합니다.

### Figma 이름 검색

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>",
  "query": "A : STORY-F-PROOFREAD",
  "nodeTypes": ["PAGE", "SECTION", "FRAME", "COMPONENT_SET"],
  "match": "normalized",
  "limit": 20,
  "sourcePolicy": "auto"
}
```

검색은 먼저 read-only Plugin API로 실제 `figma.root.children` page catalog를 얻고, page마다 한 번씩 전환하는 작은 query projection을 병렬 실행합니다. 전체 page snapshot을 응답하지 않으므로 큰 파일에서도 공식 MCP text 상한을 피합니다. 결과는 원문 exact, Unicode NFC·공백·대소문자를 정규화한 exact, prefix, contains 순으로 정렬하고 `match: "fuzzy"`일 때만 오타 허용 검색을 추가하며, node ID, type, page, 전체 breadcrumb와 후속 `devup_figma_to_ui`에 그대로 전달할 canonical URL을 포함합니다.

### 링크 주변 화면 탐색

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "limit": 50,
  "includeTextPreview": true,
  "sourcePolicy": "auto"
}
```

요구사항 제목이나 설명 node 링크가 실제 구현 화면이 아닐 때 `devup_figma_explore`를 먼저 호출합니다. anchor와 같은 공간 묶음의 frame/component 후보를 시각 순서와 canonical URL로 반환하며, 다음 요구사항 제목에서 탐색 범위를 끝냅니다. 원하는 후보의 canonical URL을 `devup_figma_to_ui`에 넘겨 정확한 화면만 변환합니다.

탐색과 검색은 변수 catalog를 수집하지 않습니다. 정확한 UI 변환 단계에서 선택 subtree의 `boundVariables`와 paint/text/effect/grid style ID를 재귀적으로 스캔하고, 실제 사용된 ID만 공식 Figma API로 조회합니다. `devup_figma_to_json`만 file 전체 로컬 catalog를 수집합니다.

`sourcePolicy`는 `auto`, `direct`, `host` 중 하나입니다. `needs_figma` 응답의 read-only call을 host의 공식 Figma MCP에서 실행한 뒤 원본 result를 `devup_figma_continue`의 `sessionId`, `callId`, `result`로 전달하면 동일한 Rust collector가 이어서 처리합니다. session은 메모리에만 최대 10분 유지되며 완료·오류·만료 시 제거됩니다.

완전성 등급은 다음과 같습니다.

- `full-local-plus-used-remote`: 로컬 전체와 사용된 외부 token을 모두 확인
- `used-tokens`: 확보한 token만 변환했으며 외부 전체를 보장하지 않음
- `resolved-values-only`: 의미 있는 token binding 없이 계산값만 확보

## 읽기 전용·개인정보 보호

- upstream 호출은 `get_metadata`, `get_variable_defs`, `get_design_context`, `get_code_connect_map`, `get_screenshot`과 내장된 read-only `use_figma` script로 닫혀 있습니다.
- 사용자 입력 JavaScript를 받지 않으며 Figma write tool을 호출하지 않습니다.
- stdout에는 MCP frame만 출력하고 trace는 stderr로 보냅니다.
- access token, refresh token, OAuth code, PKCE verifier는 Debug, trace와 MCP error에 포함하지 않습니다.
- Figma snapshot과 screenshot을 기본적으로 디스크에 저장하지 않습니다.
- 호환성 fixture는 고정한 JavaScript 플러그인의 268개 synthetic 입력입니다. 별도의 WQUW-151 회귀 fixture는 공식 MCP에서 read-only로 수집한 디자인 node/텍스트/token 이름만 포함하며 OAuth token, header, callback parameter, 사용자 계정·email은 포함하지 않습니다.

### 플러그인 호환성 corpus

`fixtures/devup-figma-plugin`은 `dev-five-git/devup-figma-plugin`의 고정 commit `243db650f1d635ab5385546a2a297eae4ea93515`에서 수집한 54개 test file과 978개 passing-test inventory를 추적합니다. 그중 upstream test 252개가 만든 JSON/golden 268쌍은 Rust serde/codegen 경로에서 byte parity를 전부 실행하고, 나머지는 550개 대표 Rust assertion 연결, 137개 미이식, 21개 plugin-runtime 전용, 18개 read-only 범위 밖 write 동작으로 명시적으로 구분합니다. 즉 268/268 snapshot parity는 검증되지만 978개 JavaScript test가 각각 Rust parity test로 포팅됐다는 뜻은 아닙니다. manifest는 LF로 정규화한 fixture와 snapshot 536개 파일의 SHA-256을 검증하고, coverage registry는 ledger가 실제 Rust test symbol 또는 근거가 있는 비-parity 분류만 참조하도록 강제합니다. 상세 분류와 실행 방법은 [`fixtures/devup-figma-plugin/README.md`](fixtures/devup-figma-plugin/README.md)를 참고하세요.

### 실제 Figma JSON contract gate

`crates/devup-mcp/tests/live_figma_contract.rs`는 기본적으로 ignore됩니다. `DEVUP_MCP_LIVE_FIGMA=1`을 설정하고 공식 MCP의 `get_metadata`, 내장 node snapshot, variable/style catalog, resource batch 결과를 호출 순서대로 stdin에 전달하면 실제 payload를 디스크에 쓰거나 출력하지 않고 serde round-trip, 요청 context, node 존재, 변수/style parser를 검증하고 값이 제거된 `PayloadStructure`만 출력합니다.

실제 확인된 공식 metadata는 XML text content envelope이며, local 변수/style은 catalog 후 resource 단위로 수집합니다. style의 `consumers`처럼 단일 field가 공식 MCP의 약 20,500자 text 상한을 넘을 수 있으므로, base field와 320개 단위의 compact consumer relation을 분리해 읽고 Rust에서 원래 exhaustive JSON shape로 재조립합니다. node snapshot도 byte budget과 cursor를 사용해 같은 상한 아래에서 자동 재개합니다. range의 누락·중복이나 수집 중 목록 변경은 성공으로 숨기지 않고 오류로 처리합니다.

2026-08-31 실제 파일 검증에서는 13개 page 전체 검색으로 `[FR-026] 본연체` (`3879:35481`)를 찾고, 주변의 화면 후보 10개를 탐색해 `A : STORY-F-PROOFREAD` (`3879:35518`)를 정확한 대상으로 선택했습니다. 이 화면의 144개 node, 사용된 변수 13개와 text style 11개를 공식 read-only MCP에서 수집해 instance children, mixed typography, nested `[1. 이름]`, token binding을 Rust snapshot test로 고정했습니다. 같은 파일의 전체 theme export는 공식 read-only 호출 89개를 통해 collection 1개, variable 49개, style 37개, mode 2개를 수집해 42,794자 `devup.json`을 생성했으며 diagnostics는 0개였습니다.

## Snapshot 의미와 현재 한계

Figma Remote MCP에서는 `JSON_REST_V1` export가 허용되지 않으므로 host object를 그대로 REST JSON으로 만들 수 없습니다. 대신 checked-in property manifest와 runtime prototype/enumerable 탐색을 함께 사용해 모든 발견한 data field의 key와 읽기 결과를 보존합니다. 함수, 순환 node object는 제외하거나 id로 바꾸고, binary asset은 bytes 대신 metadata로 나타내며, 타 plugin private data와 오류를 내는 getter는 읽을 수 없습니다. 단일 값이 byte budget을 넘으면 key를 없애지 않고 `{ "$truncated": ..., "byteLength": ... }`와 `DEVUP_FIELD_VALUE_TRUNCATED`를 남기며, `characters`, styled segment, resource binding처럼 UI 변환에 필요한 값은 우선 보존합니다.

현재 private MVP의 남은 한계는 다음과 같습니다.

- 공식 `get_metadata`의 file-level page 목록은 실제 page 전체보다 적게 반환될 수 있습니다. 이름 검색은 Plugin API page catalog와 per-page projection으로 우회하며 실제 13개 page 파일에서 검증했습니다.
- 매우 큰 computed field(예: vector `fillGeometry`)는 현재 값 전체 대신 명시적인 byte-length marker로 보존됩니다. 모든 대용량 field 값을 lossless하게 export하는 기능은 후속 wire-format 개선 대상입니다.
- exhaustive node 변환은 공식 MCP text 상한에 맞춰 여러 cursor call이 필요하므로 subtree 크기에 따라 시간이 늘어날 수 있습니다.
- direct OAuth registration은 Figma MCP Catalog 승인이 없는 private client에서 거절됩니다. `auto`/`host` fallback은 host가 인증한 공식 Figma MCP로 실제 검증했습니다.
- 사용되지 않은 외부 Figma library 변수 전체는 Remote MCP가 제공하지 않을 수 있습니다.
- node/page theme scope는 로컬 변수 API의 file-wide 결과를 기반으로 하며 세밀한 사용 범위 필터는 후속 보강 대상입니다.
- vector, mask, image, absolute layout과 일부 effect는 diagnostics를 포함한 제한적 fallback입니다.
- Figma Remote MCP의 `use_figma` tool contract가 바뀌면 live smoke test와 adapter 갱신이 필요합니다.

상세 설계는 [`docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md`](docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md)를 참고하세요.
