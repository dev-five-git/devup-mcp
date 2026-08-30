# devup-mcp

Rust-native MCP server that reads Figma designs and generates DevupUI artifacts. One binary acts as a local stdio MCP server and a read-only client of Figma Remote MCP.

저장소는 Cargo workspace이며 `devup-mcp` 실행 crate, OAuth·upstream·snapshot을 담당하는 `devup-mcp-figma`, TSX·theme projection을 담당하는 `devup-mcp-devup-ui`로 구성됩니다. 별도 IR/auth/server crate 없이 제품 설치 단위는 `devup-mcp` 하나입니다.

## 현재 제공 기능

- `devup_figma_auth`: Figma 연결 상태 확인, 브라우저 OAuth 로그인, 로그아웃
- `devup_figma_to_ui`: Figma node 링크를 `@devup-ui/react` TSX로 변환
- `devup_figma_to_json`: Figma 변수와 로컬 스타일을 `devup.json`으로 변환
- `devup_figma_continue`: host가 실행한 공식 Figma MCP read 결과로 중단된 변환을 재개
- Figma Plugin API의 readable data property를 raw JSON으로 보존하고, 알려지지 않은 runtime field는 `extra`, 실패한 getter는 `fieldErrors`로 유지

Figma PAT, 사용자가 만든 OAuth app, 내장 client secret은 필요하지 않습니다. Figma Remote MCP의 OAuth discovery, Dynamic Client Registration, PKCE S256과 일시적인 `127.0.0.1` callback을 사용합니다.

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
  "scope": "node"
}
```

결과에는 `tsx`, import 목록, 사용된 token, source 식별자, 보존한 node 수와 fallback diagnostics가 포함됩니다. Auto Layout은 `Flex`, 일반 container는 `Box`, text는 `Text`로 변환하고 theme binding이 있으면 JSX prop에서 `$token`을 우선 사용합니다.

### Figma → devup.json

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "scope": "file",
  "includeDiagnostics": true,
  "sourcePolicy": "auto"
}
```

결과는 `theme.colors`, `theme.typography`, `theme.length`, `theme.shadow`를 포함하는 결정적 JSON 문자열과 counts, completeness를 반환합니다.

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
- test fixture는 합성 데이터만 사용합니다.

### 실제 Figma JSON contract gate

`crates/devup-mcp/tests/live_figma_contract.rs`는 기본적으로 ignore됩니다. `DEVUP_MCP_LIVE_FIGMA=1`을 설정하고 공식 MCP의 `get_metadata`, 내장 node snapshot, variable/style catalog, resource batch 결과를 호출 순서대로 stdin에 전달하면 실제 payload를 디스크에 쓰거나 출력하지 않고 serde round-trip, 요청 context, node 존재, 변수/style parser를 검증하고 값이 제거된 `PayloadStructure`만 출력합니다.

실제 확인된 공식 metadata는 XML text content envelope이며, local 변수/style은 catalog 후 resource 단위로 수집합니다. resource 단위 분할은 exhaustive style payload가 MCP text 출력 한도를 넘지 않게 하며, 공식 MCP seat/plan quota가 소진되면 live gate는 rate-limit 오류로 중단되고 committed offline fixture test에는 영향을 주지 않습니다.

## Snapshot 의미와 현재 한계

Figma Remote MCP에서는 `JSON_REST_V1` export가 허용되지 않으므로 host object를 그대로 REST JSON으로 만들 수 없습니다. 대신 checked-in property manifest와 runtime prototype/enumerable 탐색을 함께 사용해 모든 읽을 수 있는 data field를 보존합니다. 함수, 순환 node object는 제외하거나 id로 바꾸고, binary asset은 bytes 대신 metadata로 나타내며, 타 plugin private data와 오류를 내는 getter는 읽을 수 없습니다.

현재 private MVP의 남은 한계는 다음과 같습니다.

- 공식 `get_metadata`의 page/file 전체 탐색은 node XML과 다른 top-level page 목록 envelope를 사용하므로 추가 live 검증이 필요합니다.
- 사용되지 않은 외부 Figma library 변수 전체는 Remote MCP가 제공하지 않을 수 있습니다.
- node/page theme scope는 로컬 변수 API의 file-wide 결과를 기반으로 하며 세밀한 사용 범위 필터는 후속 보강 대상입니다.
- vector, mask, image, absolute layout과 일부 effect는 diagnostics를 포함한 제한적 fallback입니다.
- Figma Remote MCP의 `use_figma` tool contract가 바뀌면 live smoke test와 adapter 갱신이 필요합니다.

상세 설계는 [`docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md`](docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md)를 참고하세요.
