# devup-mcp

Rust-native MCP server that reads Figma designs and generates DevupUI artifacts. One binary acts as a local stdio MCP server and a read-only client of Figma Remote MCP.

저장소는 Cargo workspace이며 `devup-mcp` 실행 crate, OAuth·upstream·snapshot을 담당하는 `devup-mcp-figma`, TSX·theme projection을 담당하는 `devup-mcp-devup-ui`로 구성됩니다. 별도 IR/auth/server crate 없이 제품 설치 단위는 `devup-mcp` 하나입니다.

## 현재 제공 기능

- `devup_figma_auth`: Figma 연결 상태 확인, 브라우저 OAuth 로그인, 로그아웃
- `devup_figma_to_ui`: Figma node 링크를 `@devup-ui/react` TSX로 변환
- `devup_figma_to_json`: Figma 변수와 로컬 스타일을 `devup.json`으로 변환
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

최초 `devup_figma_auth`의 `login` 호출 또는 인증이 필요한 변환 호출에서 시스템 브라우저로 Figma 승인을 완료합니다. 인증 정보는 운영체제 credential store에만 저장되며 `logout`은 해당 정보만 삭제합니다.

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
  "includeDiagnostics": true
}
```

결과에는 `tsx`, import 목록, 사용된 token, source 식별자, 보존한 node 수와 fallback diagnostics가 포함됩니다. Auto Layout은 `Flex`, 일반 container는 `Box`, text는 `Text`로 변환하고 theme binding이 있으면 JSX prop에서 `$token`을 우선 사용합니다.

### Figma → devup.json

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "scope": "file",
  "includeDiagnostics": true
}
```

결과는 `theme.colors`, `theme.typography`, `theme.length`, `theme.shadow`를 포함하는 결정적 JSON 문자열과 counts, completeness를 반환합니다.

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

## Snapshot 의미와 현재 한계

Figma Remote MCP에서는 `JSON_REST_V1` export가 허용되지 않으므로 host object를 그대로 REST JSON으로 만들 수 없습니다. 대신 checked-in property manifest와 runtime prototype/enumerable 탐색을 함께 사용해 모든 읽을 수 있는 data field를 보존합니다. 함수, 순환 node object는 제외하거나 id로 바꾸고, binary asset은 bytes 대신 metadata로 나타내며, 타 plugin private data와 오류를 내는 getter는 읽을 수 없습니다.

현재 private MVP의 남은 한계는 다음과 같습니다.

- 큰 page/file snapshot의 metadata 기반 chunk fan-out은 후속 최적화가 필요합니다.
- 사용되지 않은 외부 Figma library 변수 전체는 Remote MCP가 제공하지 않을 수 있습니다.
- node/page theme scope는 로컬 변수 API의 file-wide 결과를 기반으로 하며 세밀한 사용 범위 필터는 후속 보강 대상입니다.
- vector, mask, image, absolute layout과 일부 effect는 diagnostics를 포함한 제한적 fallback입니다.
- Figma Remote MCP의 `use_figma` tool contract가 바뀌면 live smoke test와 adapter 갱신이 필요합니다.

상세 설계는 [`docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md`](docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md)를 참고하세요.
