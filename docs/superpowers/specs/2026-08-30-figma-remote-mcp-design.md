# devup-mcp Figma Remote MCP 설계

## 상태

- 작성일: 2026-08-30
- 범위: Figma 읽기 기능 1차 완성
- 구현 언어: Rust
- 제품명, 실행 파일, Cargo package: `devup-mcp`
- 저장소: `dev-five-git/devup-mcp`
- 범위 외: Vespera 및 Figma 이외의 Devup 기능

## 목표

사용자가 `devup-mcp` 하나를 MCP 클라이언트에 설치한 뒤 최초 한 번 Figma OAuth를 승인하면, 이후에는 Figma 링크만으로 다음 결과를 얻는다.

1. 선택한 Figma 노드를 DevupUI React/TypeScript 코드로 변환한다.
2. 노드, 페이지 또는 파일 범위의 Figma 변수와 스타일을 `devup.json`으로 변환한다.
3. 사용자에게 Figma OAuth 앱 생성, PAT 발급 또는 client secret 배포를 요구하지 않는다.
4. Figma 파일은 읽기 전용으로 다룬다.
5. Figma Desktop 실행 여부와 관계없이 동작한다.

## 핵심 결정

### Figma Remote MCP를 upstream으로 사용한다

`devup-mcp`는 로컬에서 stdio MCP 서버로 실행되는 동시에 Figma Remote MCP의 Streamable HTTP 클라이언트 역할을 한다.

```text
Codex / Orca / Claude Code
        │ stdio MCP
        ▼
     devup-mcp
        │ Streamable HTTP MCP + OAuth
        ▼
https://mcp.figma.com/mcp
        │
        ▼
      Figma
```

직접 Figma REST API를 주 데이터 소스로 사용하지 않는다. 이 결정은 다음 문제를 없앤다.

- 사용자가 PAT를 발급하고 전달하는 과정
- DevFive가 고정 Figma client secret을 앱에 내장하는 문제
- 별도 token exchange 서비스를 운영하는 문제
- REST Variables API의 Enterprise 플랜 제한
- Figma Desktop과 현재 열린 파일에 의존하는 문제

### 단일 Rust package로 시작한다

별도의 `devup-figma`, `devup-auth`, `devup-ir`, `devup-mcp-server` crate를 만들지 않는다. 현재 공개 단위와 실행 단위는 하나이므로 `devup-mcp` package 내부 모듈로 경계를 유지한다.

```text
devup-mcp/
├─ Cargo.toml
├─ src/
│  ├─ main.rs
│  ├─ lib.rs
│  ├─ server/
│  │  ├─ mod.rs
│  │  └─ tools.rs
│  ├─ figma/
│  │  ├─ mod.rs
│  │  ├─ url.rs
│  │  ├─ upstream.rs
│  │  ├─ oauth.rs
│  │  ├─ credentials.rs
│  │  ├─ snapshot.rs
│  │  └─ errors.rs
│  ├─ codegen/
│  │  ├─ mod.rs
│  │  ├─ component.rs
│  │  ├─ layout.rs
│  │  ├─ style.rs
│  │  └─ text.rs
│  └─ theme/
│     ├─ mod.rs
│     ├─ tokens.rs
│     └─ devup_json.rs
└─ tests/
   ├─ fixtures/
   └─ integration/
```

모듈 간 타입이 독립적으로 재사용되거나 빌드 feature 분리가 실제로 필요해질 때만 crate로 추출한다.

## OAuth 설계

Figma Remote MCP는 표준 OAuth discovery와 동적 클라이언트 등록을 제공한다.

- Protected Resource Metadata: `https://mcp.figma.com/.well-known/oauth-protected-resource/mcp`
- Authorization Server Metadata: `https://mcp.figma.com/.well-known/oauth-authorization-server`
- Authorization endpoint: `https://www.figma.com/oauth/mcp`
- Token endpoint: `https://api.figma.com/v1/oauth/token`
- Dynamic registration endpoint: `https://api.figma.com/v1/oauth/mcp/register`
- Scope: `mcp:connect`
- PKCE: `S256`

인증 순서는 다음과 같다.

1. loopback listener를 `127.0.0.1`의 사용 가능한 포트에 바인딩한다.
2. 정확한 callback URI로 OAuth client를 동적 등록한다.
3. `state`, PKCE verifier 및 challenge를 CSPRNG로 생성한다.
4. 시스템 기본 브라우저에서 authorization endpoint를 연다.
5. localhost callback에서 code와 state를 받고 state를 constant-time 비교한다.
6. code와 verifier로 토큰을 교환한다.
7. refresh token, access token 및 동적 등록 credential을 OS Credential Manager에 저장한다.
8. access token 만료 전에 refresh하고, refresh 실패 시 사용자에게 재인증 절차를 반환한다.

callback HTTP 서버는 인증 중에만 실행하며 성공, 실패 또는 timeout 이후 즉시 종료한다. 토큰과 verifier, client secret은 stdout, stderr, tracing field 또는 MCP 오류 본문에 기록하지 않는다.

## Downstream MCP 도구

### `devup_figma_auth`

Figma 연결 상태를 확인하거나 명시적으로 로그인·로그아웃한다.

```json
{
  "action": "status | login | logout"
}
```

로그인은 브라우저 OAuth를 시작한다. 변환 도구가 인증되지 않은 상태에서 호출되면 동일한 인증 흐름을 자동으로 시작할 수 있다.

### `devup_figma_to_ui`

```json
{
  "url": "https://www.figma.com/design/<fileKey>/<name>?node-id=<id>",
  "componentName": "OptionalName",
  "includeDiagnostics": true
}
```

반환값:

- DevupUI TSX
- 필요한 `@devup-ui` import
- 사용된 Figma 변수 및 스타일
- 지원하지 않는 속성 또는 fallback 목록
- source file key, node id 및 Figma version 정보가 제공될 경우 version

### `devup_figma_to_json`

```json
{
  "url": "https://www.figma.com/design/<fileKey>/<name>?node-id=<id>",
  "scope": "node | page | file",
  "includeDiagnostics": true
}
```

반환값:

- DevupUI가 소비할 수 있는 `devup.json` 문자열
- 수집한 페이지, 노드, 변수, 스타일 수
- 완전성 등급
- 충돌 및 미해결 alias 경고

`devup.json`은 현재 DevupUI schema에 맞춰 다음 항목을 생성한다.

- `theme.colors.<mode>.<token>`
- `theme.typography.<token>`
- `theme.length.<mode>.<token>`
- `theme.shadow.<mode>.<token>`

Figma mode 이름은 안정적인 JSON key로 정규화하되 원래 이름을 diagnostics에 유지한다. WEB `codeSyntax`가 존재하면 토큰 이름의 우선 소스로 사용하고, 없으면 Figma 변수 이름을 결정적으로 정규화한다.

## Upstream 도구 사용

`devup-mcp`는 Figma Remote MCP의 읽기 관련 도구만 allowlist한다.

- `get_metadata`: 파일의 페이지와 큰 노드의 구조 탐색
- `get_variable_defs`: 선택 범위에서 사용된 로컬·외부 변수 및 스타일 조회
- `get_design_context`: snapshot만으로 부족한 컴포넌트 의미와 Code Connect 보조 정보
- `get_code_connect_map`: 연결된 코드 컴포넌트 확인
- `get_screenshot`: 선택적 시각 검증 자료
- `use_figma`: 고정된 읽기 전용 전체 snapshot script 실행

`use_figma`, `generate_figma_design`, `upload_assets`, `add_code_connect_map` 등 쓰기 가능 도구를 일반적으로 노출하지 않는다. `use_figma`는 예외적으로 호출하지만 Devup이 빌드 시 내장한 읽기 전용 script template만 실행한다.

## Figma 전체 데이터 snapshot

실제 검증 결과 Remote `use_figma`에서는 Plugin API typings에 존재하는 `node.exportAsync({ format: "JSON_REST_V1" })`가 `JSON_REST_V1 export format is not supported in this context`로 거부됐다. 따라서 REST JSON을 그대로 요청하지 않는다.

대신 고정된 읽기 전용 Plugin API script가 Figma Plugin API에서 공개되고 읽을 수 있는 디자인 필드를 손실 최소화 snapshot으로 직렬화한다. 현재 코드생성이 사용하지 않는 필드도 버리지 않고 raw JSON field로 보존한다.

snapshot은 Figma Plugin API typings에서 node type과 mixin별 데이터 property manifest를 생성해 다음 데이터를 포함한다.

- 공통: id, type, name, visible, opacity, rotation
- 트리: parent id, child ids 및 순서
- 크기와 위치: x, y, width, height, constraints
- Auto Layout: direction, wrap, alignment, gap, padding, sizing
- Paint: solid, gradient, image reference, opacity, blend mode
- Border와 effect: stroke, radius, shadow, blur
- Text: characters, styled segments, font family, size, weight, line height, letter spacing, alignment
- Component: instance, main component, variant properties, Code Connect key
- Variable: bound variable ids, explicit mode ids, resolved current value
- 확장 데이터: Dev resources, annotations, measurements, reactions, export settings
- 미지원 신규 데이터: `extra` map과 `fieldErrors`에 원본 값 또는 읽기 실패 정보

Rust는 node의 모든 field를 `serde_json::Map<String, Value>`로 먼저 보존하고, DevupUI codegen에서 필요한 property만 typed view로 해석한다. 새 Figma field를 Rust codegen이 아직 이해하지 못하더라도 snapshot에서는 소실되지 않는다.

전체 snapshot에서도 다음 항목은 JSON으로 직접 표현하지 않는다.

- 함수와 메서드
- 순환 참조 객체: `parent`는 `parentId`, node 참조는 node id로 치환
- 원본 이미지·동영상 bytes: asset id와 metadata를 보존하고 별도 asset 도구로 조회
- 접근할 수 없는 타 plugin의 private data
- 읽기 자체가 오류를 일으키는 getter: 오류 문자열을 `fieldErrors`에 기록

공식 Plugin API typings가 갱신됐는데 serializer manifest에 없는 공개 data property가 생기면 CI contract test를 실패시킨다. runtime에서 열거 가능한 미지의 property는 `extra`에 보존한다.

임의 JavaScript를 사용자 입력으로 받지 않는다. file key와 node id는 파싱·검증한 후 JSON literal로만 script에 삽입한다.

### 크기 제한과 청크 처리

Figma MCP 응답과 대형 선택 영역의 한계를 피하기 위해 다음 순서를 사용한다.

1. `get_metadata`로 대상 구조를 얻는다.
2. 작은 노드는 한 번에 전체 snapshot을 수집한다.
3. 예상 크기가 큰 Frame, Section 또는 Page는 직계 자식 단위로 분할한다.
4. bounded concurrency로 snapshot tool call을 실행한다.
5. Rust에서 node id와 child order를 기준으로 재조립한다.
6. 동일 파일 version 내 결과만 병합한다. version이 달라지면 일관성 오류로 재시도를 요청한다.

페이지 또는 파일 전체 수집은 페이지당 한 번만 page context를 설정하고 페이지 호출을 병렬 fan-out한다.

## 변수와 `devup.json` 완전성

### 로컬 변수

Plugin API의 `getLocalVariableCollectionsAsync`와 `getLocalVariablesAsync`를 collection 단위로 청크 호출한다. 따라서 파일에 정의됐지만 노드에 사용되지 않은 로컬 변수와 mode도 수집 대상이다.

### 외부 라이브러리 변수

모든 노드의 variable binding을 수집한 뒤 `get_variable_defs`와 variable id 조회를 사용한다. 파일에서 실제 사용한 외부 변수는 이름과 현재 값을 보존한다. 구독만 했지만 어느 노드에도 사용하지 않은 외부 라이브러리 변수 전체는 수집을 보장하지 않는다.

완전성 등급:

- `full-local-plus-used-remote`: 모든 로컬 변수와 실제 사용된 외부 변수
- `used-tokens`: 선택 범위에서 사용된 변수만 확보
- `resolved-values-only`: 토큰 의미를 찾지 못해 계산값만 확보

Devup은 누락된 정보를 추측해 토큰을 만들지 않고 diagnostics에 명시한다.

## DevupUI 코드 생성

기존 `devup-figma-plugin`의 검증된 변환 규칙을 참고하되 JavaScript runtime 의존성 없이 Rust로 이식한다.

1. Auto Layout을 `Flex`, 일반 컨테이너를 `Box`, 텍스트를 `Text`로 매핑한다.
2. theme token을 찾으면 `$token`을 우선 사용한다.
3. token을 찾지 못하면 계산된 CSS 값을 사용한다.
4. component 이름은 유효한 TypeScript identifier로 정규화한다.
5. 절대 배치, mask, vector, image 및 미지원 effect는 명시적 fallback과 diagnostics를 남긴다.
6. 결과는 formatter를 거쳐 결정적인 문자열로 만든다.

`get_design_context`가 반환한 React/Tailwind 코드를 다시 파싱해 주 데이터로 사용하지 않는다. snapshot만으로 부족한 의미 정보와 Code Connect 보조 자료로만 사용한다.

## 오류 모델

오류는 안정적인 code와 사용자용 한국어/영어 message, retry 가능 여부 및 안전한 details를 가진다.

- `DEVUP_AUTH_REQUIRED`
- `DEVUP_AUTH_CALLBACK_TIMEOUT`
- `DEVUP_AUTH_STATE_MISMATCH`
- `DEVUP_FIGMA_PERMISSION_DENIED`
- `DEVUP_FIGMA_RATE_LIMITED`
- `DEVUP_FIGMA_NODE_NOT_FOUND`
- `DEVUP_FIGMA_UNSUPPORTED_FILE`
- `DEVUP_FIGMA_RESPONSE_TOO_LARGE`
- `DEVUP_FIGMA_VERSION_CHANGED`
- `DEVUP_SNAPSHOT_UNSUPPORTED`
- `DEVUP_CODEGEN_FAILED`
- `DEVUP_THEME_CONFLICT`

401은 refresh 후 한 번만 재시도한다. 429는 upstream retry hint가 있을 때만 bounded retry한다. snapshot 오류는 더 작은 subtree로 한 단계 분할한 뒤 한 번 재시도하고, 반복 실패하면 해당 node id와 field 오류를 diagnostics에 남긴다.

## 보안과 개인정보

- downstream은 stdio만 사용하며 stdout에는 MCP frame 외 데이터를 쓰지 않는다.
- tracing은 stderr로만 보내고 token, OAuth code, verifier, secret, Figma 사용자 이메일을 redact한다.
- credential은 OS keyring에 저장하고 평문 설정 파일 fallback을 제공하지 않는다.
- localhost callback은 loopback에만 bind한다.
- OAuth state와 PKCE를 필수 검증한다.
- Figma URL host, file type, file key 및 node id를 엄격히 검증한다.
- upstream `use_figma`에는 고정된 읽기 전용 script만 전달한다.
- Figma 원본 디자인 JSON과 스크린샷을 기본적으로 디스크에 저장하지 않는다.
- 테스트 fixture는 합성·비식별 데이터만 저장한다.
- logout은 credential만 삭제하며 Figma 파일이나 프로젝트 파일을 변경하지 않는다.

## 테스트 전략

### 단위 테스트

- Figma URL parser 및 branch URL 처리
- OAuth discovery, DCR, PKCE, state, refresh 및 timeout
- credential redaction
- 전체 snapshot JSON deserialize, unknown field 보존과 청크 재조립
- Plugin API typings와 serializer property manifest의 누락 검출
- 색상, 길이, typography, shadow 및 mode의 `devup.json` 매핑
- component identifier 정규화
- Auto Layout, text, fills, effects 및 token fallback 코드생성
- 충돌과 미지원 속성 diagnostics

### 계약 테스트

- mock Streamable HTTP MCP server로 initialize, tools/list, tools/call 검증
- OAuth metadata와 dynamic registration fixture 검증
- upstream allowlist 밖의 write tool이 호출되지 않는지 검증
- 대형 응답 분할과 version 변경 검증

### 통합 테스트

- Figma OAuth는 수동 opt-in live test로 분리한다.
- 제공된 파일 `85CgSws3o5XsLv7aAwWJyS`, node `3879:35481`을 smoke fixture로 사용하되 반환 디자인 데이터는 저장하지 않는다.
- 반환 TSX를 parser/formatter로 검증한다.
- 생성 `devup.json`을 DevupUI schema/parser로 검증한다.
- stdio MCP client로 실제 도구 목록과 호출 결과를 검증한다.

## 배포

- Rust 단일 바이너리로 배포한다.
- Windows, macOS, Linux를 지원한다.
- GitHub Actions에서 fmt, clippy `-D warnings`, test 및 release build를 실행한다.
- 플랫폼별 keyring backend를 사용한다.
- Figma Remote MCP endpoint는 기본값으로 고정하되 테스트에서만 주입 가능하게 한다.

## 단계별 구현

1. Rust package, CI, tracing 및 stdio MCP skeleton
2. upstream Streamable HTTP MCP client와 OAuth discovery/DCR/PKCE/keyring
3. URL parser와 Figma read-only tool allowlist
4. 전체 snapshot protocol, typings 기반 property manifest와 청크 재조립
5. `devup_figma_to_ui` Rust codegen
6. 변수 수집과 `devup_figma_to_json`
7. 실제 Figma OAuth 및 제공 노드 통합 검증
8. 설치 문서, changelog 및 release packaging

## 남은 위험

- Figma Remote MCP의 도구 목록과 응답 shape는 발전 중이므로 tool contract fixture와 capability negotiation이 필요하다.
- generic Rust MCP client에서 `use_figma`가 동일하게 노출되는지는 실제 OAuth 연결 후 재검증해야 한다.
- Figma MCP 또는 `use_figma`의 응답 크기 제한은 대형 파일에서 많은 호출을 유발할 수 있다.
- 외부 라이브러리의 사용되지 않은 변수 전체는 수집하지 못할 수 있다.
- 기존 TypeScript plugin과 완전히 같은 코드 출력을 내려면 변환 규칙의 fixture parity 작업이 필요하다.

이 위험은 정확하지 않은 결과를 조용히 생성하지 않고 capability, completeness 및 diagnostics로 사용자에게 노출한다.
