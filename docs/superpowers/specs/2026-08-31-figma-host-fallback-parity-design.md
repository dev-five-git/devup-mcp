# devup-mcp Figma 폴백 및 전수 호환성 설계

## 상태와 기준선

- 작성일: 2026-08-31
- 상태: 구현 전 승인 명세
- 대체 문서: `2026-08-30-figma-remote-mcp-design.md`
- 구현 저장소: `dev-five-git/devup-mcp`
- 기준 JavaScript 저장소: `dev-five-git/devup-figma-plugin`
- 기준 JavaScript commit: `243db650f1d635ab5385546a2a297eae4ea93515`
- 기준 테스트 결과: 54개 파일, 978 pass, 0 fail, 268 snapshots, 1,974 assertions
- 제품 언어와 배포 형태: Rust 단일 바이너리 `devup-mcp`

이 명세는 승인된 두 가지 요구를 하나의 완료 조건으로 묶는다.

1. Figma Remote MCP에 직접 연결할 수 있으면 이를 최적 경로로 사용하고,
   Catalog 승인 또는 OAuth/capability 문제로 직접 연결할 수 없으면 MCP host에 이미
   설치된 공식 Figma MCP를 통해 동일 데이터를 받는다.
2. 기존 JavaScript 플러그인의 코드 생성과 `devup.json` 출력 규칙을 Rust로 이식하고,
   원본 테스트 전체를 추적하는 고정 fixture corpus와 `cargo insta` snapshot gate를
   통과한다.

Vespera와 Figma 이외의 Devup 기능은 이번 범위에 포함하지 않는다.

## 사용자 결과

사용자는 `devup-mcp` 하나를 설치하고 Figma 링크를 제공해 다음 결과를 얻는다.

- 선택 노드, 페이지 또는 파일을 탐색한 완전성 정보가 있는 Figma snapshot
- `@devup-ui/react` 기반 React/TypeScript 코드와 컴포넌트 파일
- 파일의 로컬 변수, 실제 사용한 외부 변수 및 스타일을 반영한 `devup.json`
- SVG/PNG asset 목록과 내보낼 수 없는 asset의 진단
- 직접 연결 또는 공식 Figma MCP 중 실제 사용한 source 정보
- 미지원 필드, 접근 실패 getter, 누락 token, 충돌 및 fallback 원인

기본 반환은 MCP 응답의 메모리 내 artifact다. 사용자가 `outputPath`를 명시한 경우에만
검증된 workspace 하위 경로에 원자적으로 파일을 기록한다.

## Cargo workspace와 책임

crate 구성은 유지하되 각 crate의 책임을 확장한다.

```text
devup-mcp/
├─ crates/
│  ├─ devup-mcp/                 # stdio MCP, source 선택, handoff session, export
│  ├─ devup-mcp-figma/           # URL, direct OAuth, upstream, collector, raw snapshot
│  └─ devup-mcp-devup-ui/        # NodeTree, codegen, responsive, theme projection
└─ fixtures/devup-figma-plugin/  # JSON inputs, insta snapshots, manifest/ledger
```

`devup-mcp` 저장소는 Cargo만으로 build/test하며 `package.json`, Bun lockfile, Node/Bun
script 또는 JavaScript test runtime을 두지 않는다. 기존 `devup-figma-plugin`의 inline
test input은 한 번 JSON으로 이전하고, 이후 fixture는 JSON을 단일 진실 공급원으로
관리한다. Figma `use_figma`가 요구하는 고정 읽기 전용 JavaScript 문자열은 Rust
binary에 resource로 포함되어 Figma sandbox에서 실행될 뿐, 로컬 JavaScript runtime
의존성이 아니다.

OAuth는 계속 `devup-mcp-figma` 내부에 둔다. 별도 `devup-auth`, `devup-ir`,
`devup-mcp-server` crate는 만들지 않는다.

## 데이터 source 상태 기계

### 직접 경로

`devup-mcp`는 Figma Remote MCP의 Streamable HTTP endpoint에 직접 연결한다. 유효한
keyring credential이 있으면 discovery, initialize, capability negotiation 후 읽기
도구만 호출한다. 명시적인 `devup_figma_auth { "action": "login" }`만 browser OAuth를
시작한다. 일반 변환 요청의 `auto` 모드는 예기치 않게 browser를 열지 않는다.

직접 경로가 가능한 사용자는 별도 agent 왕복 없이 가장 빠른 결과를 얻는다.

### host 공식 MCP 폴백

MCP 서버는 host가 연결한 다른 sibling MCP의 tool을 직접 호출하거나 그 token을 읽을
수 없다. 따라서 폴백은 다음 handoff protocol로 수행한다.

```text
agent/host ── devup_figma_to_* ──> devup-mcp
                 direct 실패          │
agent/host <── needs_figma + calls ────┘
     │
     ├── 공식 Figma MCP tool 호출
     │
     └── devup_figma_continue(result) ──> 동일 Rust collector/converter
```

`devup-mcp`가 반환하는 call에는 server hint, 공식 tool 이름, 인자 및 Devup이 빌드 시
내장한 고정 읽기 전용 `use_figma` script가 포함된다. agent/host는 이 call을 실행하고
결과를 수정하지 않은 채 continuation tool에 돌려준다. 공식 Figma MCP가 설치되지
않은 host에서는 설치 필요 진단과 재개 가능한 handoff를 반환한다.

### source 정책

모든 변환 도구는 다음 정책을 받는다.

- `auto`: 유효한 direct credential이 있고 capability가 맞으면 direct, 아니면 host handoff
- `direct`: direct만 허용하며 실패를 구조화해 반환
- `host`: direct 시도 없이 즉시 host handoff

다음 오류는 `auto`에서 폴백한다.

- Catalog 미승인 또는 dynamic client registration 거부
- OAuth/capability가 direct client에 제공되지 않음
- direct identity의 permission 거부. host identity가 다를 수 있으므로 한 번 허용

잘못된 URL, 존재하지 않는 node, file version 충돌은 source를 바꿔도 해결되지 않으므로
폴백하지 않는다. rate limit은 동일 사용자 quota일 가능성이 높으므로 자동 우회하지
않고 retry hint를 전달한다.

## MCP 도구 계약

기존 세 도구를 유지하고 검색과 continuation을 추가한다.

### `devup_figma_auth`

```json
{ "action": "status | login | logout" }
```

직접 연결 credential만 관리한다. 공식 Figma MCP의 token을 조회하거나 저장하지 않는다.

### `devup_figma_to_ui`

```json
{
  "url": "https://www.figma.com/design/<fileKey>/<name>?node-id=<id>",
  "scope": "node | page | file",
  "componentName": "OptionalName",
  "sourcePolicy": "auto | direct | host",
  "includeDiagnostics": true,
  "outputPath": "optional/workspace/path"
}
```

### `devup_figma_to_json`

```json
{
  "url": "https://www.figma.com/design/<fileKey>/<name>?node-id=<id>",
  "scope": "node | page | file",
  "sourcePolicy": "auto | direct | host",
  "includeDiagnostics": true,
  "outputPath": "optional/workspace/devup.json"
}
```

### `devup_figma_search`

파일 전체에서 사용자가 말한 화면·캔버스를 이름으로 찾는다. Figma UI 용어와 사용자의
표현이 항상 일치하지 않으므로 기본 검색 대상은 `PAGE`, `SECTION`, `FRAME`,
`COMPONENT_SET`, `COMPONENT`다. `nodeTypes`를 지정하면 제한할 수 있다.

```json
{
  "url": "https://www.figma.com/design/<fileKey>/<name>",
  "query": "A : STORY-F-PROOFREAD",
  "nodeTypes": ["PAGE", "SECTION", "FRAME", "COMPONENT_SET"],
  "match": "normalized",
  "limit": 20,
  "sourcePolicy": "auto | direct | host"
}
```

매칭 우선순위는 원문 exact, Unicode NFC·공백·대소문자 normalized exact, prefix,
contains 순이다. `fuzzy`는 요청에서 명시한 경우에만 적용한다. 결과는 score만 주지 않고
node ID, type, page 이름, 전체 breadcrumb와 해당 node를 가리키는 canonical Figma URL을
반환한다. 동일 이름이 여러 개면 조용히 하나를 선택하지 않는다.

대표 agent 흐름은 다음과 같다.

```text
사용자: "~를 구현하려는데 그런 화면이 있니?"
agent: devup_figma_search → 후보 이름/type/breadcrumb 목록 제시
사용자: "맞아, 그 목록 다 구현해줘"
agent: 이전 응답의 canonical URL마다 devup_figma_to_ui 호출 → 전체 artifact 반환
```

검색 결과 집합을 server session에 저장하거나 자연어 후속 발화를 MCP가 직접 해석하지
않는다. agent가 대화 문맥의 구조화된 검색 결과를 유지하고 정확한 URL을 다시 전달한다.
따라서 여러 후보를 구현하는 경우도 숨은 상태 없이 재현할 수 있다. 첫 구현은 기존
`devup_figma_to_ui`를 후보별로 bounded concurrency 호출하며 별도 batch API는 실제
호출 비용이 문제로 확인되기 전에는 추가하지 않는다.

### `devup_figma_continue`

```json
{
  "sessionId": "opaque-session-id",
  "callId": "opaque-call-id",
  "result": {}
}
```

host call이 더 필요하면 다시 `needs_figma`, 수집이 끝나면 `complete`를 반환한다.

```json
{
  "status": "needs_figma",
  "sessionId": "opaque-session-id",
  "expiresAt": "RFC3339",
  "calls": [
    {
      "callId": "opaque-call-id",
      "server": "figma",
      "tool": "use_figma",
      "arguments": {}
    }
  ],
  "resumeTool": "devup_figma_continue"
}
```

완료 응답은 artifact kind, content 또는 path, SHA-256, diagnostics, completeness,
Figma version 및 실제 source를 포함한다. call ID는 정확히 한 번만 소비하며 중복,
만료 또는 다른 session의 결과는 거부한다.

## handoff session 안전성

- CSPRNG로 session ID와 call ID를 만든다.
- 기본 TTL은 10분이고 완료·취소·만료 시 즉시 메모리에서 제거한다.
- process당 활성 session 8개, call result당 16 MiB, 전체 64 MiB로 제한한다.
- 원본 Figma 결과를 disk나 log에 저장하지 않는다.
- continuation result는 기대한 tool별 schema, file key, node ID 및 version과 대조한다.
- 완료 artifact를 파일로 쓸 때는 workspace canonical path 검증과 임시 파일 rename을 쓴다.

## 완전 수집기

collector는 source와 무관한 계획/병합 계층이다. direct transport와 host handoff는 같은
`CollectionPlan`과 `SnapshotChunk`를 생산하므로 변환 결과가 source에 따라 달라지지
않는다.

1. `get_metadata` 또는 고정 snapshot script로 file/page/node 구조와 version을 얻는다.
2. node 범위는 해당 subtree, page 범위는 page 전체, file 범위는 모든 page를 계획한다.
3. 크기가 큰 Page, Section, Frame은 직계 자식 단위로 재귀 분할한다.
4. bounded concurrency로 chunk를 읽고 node ID와 child order로 결정적으로 병합한다.
5. 모든 chunk의 file key와 version이 같아야 한다. 도중 version 변경은 전체 결과를 폐기한다.
6. 같은 ID의 내용이 충돌하면 추측하지 않고 오류로 반환한다.

이름 검색은 같은 collector가 만든 metadata/snapshot index를 순회한다. source가 direct든
host handoff든 정규화와 정렬 결과가 같아야 하며, 검색을 위해 원본 node data를 별도로
disk cache하지 않는다.

### 실제 JSON 계약 고정 gate

fixture schema를 collector보다 먼저 가정하지 않는다. direct transport, host handoff,
고정 serializer, chunk 병합과 변수/style 수집을 먼저 구현한 뒤 제공된 실제 Figma
file/node로 다음을 검증한다.

- 공식 Figma MCP가 반환하는 tool result envelope와 structured/text content 형태
- serializer가 만드는 node field, `extra`, `fieldErrors`, reference ID와 child order
- 변수 collection/mode/alias와 style의 실제 JSON 표현
- 큰 subtree 분할 전후의 병합 shape와 version 정보
- Rust deserialize → serialize round trip에서 unknown field가 보존되는지

Catalog 승인으로 direct live 연결이 불가능한 환경에서는 host fallback live 결과와 direct
mock contract를 대조한다. direct live 성공을 fixture 설계의 전제 조건으로 삼지 않는다.

live 응답은 메모리에서만 검사하고 실제 이름, 텍스트, 디자인 값이나 전체 payload를
repository에 저장하지 않는다. 검증된 key/type/reference 구조를 사용해 비식별 synthetic
sample을 만들고, 그 sample이 동일 Rust collector/parser를 통과한 뒤에만 fixture
schema version 1을 고정한다.

고정 `use_figma` serializer는 공개된 모든 data property를 우선 raw JSON으로 보존한다.
함수, 순환 참조와 binary는 ID/metadata로 치환하고 읽기 실패 getter는 `fieldErrors`,
typed projection이 모르는 새 값은 `extra`에 둔다. Plugin API typings와 serializer
manifest가 어긋나면 CI를 실패시킨다. 사용자 JavaScript는 절대 실행하지 않는다.

## 변수, 스타일과 devup.json

file 범위에서는 모든 local variable collection, variable, paint/text/effect/grid style을
수집한다. node/page 범위에서는 local 전체와 해당 범위에서 실제 binding된 remote
variable/style을 함께 수집하고 결과에서 scope를 구분한다. alias graph는 cycle을
검출하면서 mode별로 해석한다.

출력은 기존 JavaScript 플러그인의 규칙을 기준으로 한다.

- `theme.colors.<mode>.<token>`
- `theme.typography.<token>`
- `theme.length.<mode>.<token>`
- `theme.shadow.<mode>.<token>`
- breakpoint/mode mapping, theme replication, alias와 treeshaking
- WEB `codeSyntax` 우선 token 이름과 결정적인 fallback 정규화

로컬 전체를 얻지 못하면 `full`로 표시하지 않는다. 완전성은 최소한
`full-local-plus-used-remote`, `used-tokens`, `resolved-values-only`를 구분한다.

## DevupUI 변환 범위

Rust 변환기는 JavaScript `Codegen.buildTree()`의 NodeTree 의미와 render 규칙을 이식한다.

- Box, Flex, Grid, Text, Image와 custom component 선택
- auto layout, inferred layout, free/absolute layout, constraints와 responsive breakpoint
- fill/gradient/image, border, radius, opacity/blend, effect/shadow, transform, overflow
- typography, styled text segment, ellipsis/max line, text stroke/alignment
- local/remote bound variable와 token fallback
- component/instance/component set, variants, boolean/text/instance-swap/native slot
- selector, reactions/keyframes, cursor와 import metadata
- SVG/PNG asset 판정, mask와 component reference/inline mode
- Pure Code, component definition, usage 및 responsive output
- 결정적인 import 정렬, prop 직렬화, formatting과 file naming

원본 JavaScript가 내는 의미 있는 문자열은 줄바꿈을 LF로 정규화한 뒤 byte-identical을
목표로 한다. Rust formatter 취향으로 출력 계약을 임의 변경하지 않는다.

## JSON fixture 및 JavaScript 전수 호환성

### 원칙

모든 변환 입력은 실행 가능한 TypeScript mock이 아니라 순수 JSON fixture다. Rust는 이
JSON을 production과 같은 `serde_json` 경로로 읽고 TSX, `devup.json`, NodeTree 또는
구조화 오류를 생성한다. `cargo insta`는 출력 snapshot gate로 사용하고 전수성은
manifest/ledger가 보장한다. 일반 테스트 실행은 기대값을 자동으로 덮어쓰지 않는다.

```text
fixtures/devup-figma-plugin/
├─ manifest.json                # source SHA, schema, case/snapshot 수, checksum
├─ ledger.json                  # 모든 upstream test ID와 fixture/분류/rationale
├─ cases/
│  ├─ codegen/*.json            # JSON -> TSX/component output
│  ├─ responsive/*.json         # JSON -> responsive TSX
│  ├─ devup-json/*.json         # JSON -> devup.json
│  ├─ snapshot/*.json           # JSON -> merge/normalization result
│  └─ errors/*.json             # JSON -> structured error
└─ snapshots/
   ├─ codegen/*.snap
   ├─ responsive/*.snap
   ├─ devup-json/*.snap
   ├─ snapshot/*.snap
   └─ errors/*.snap
```

모든 test ID는 `source-file + full suite path + parameterized case index`로 안정적으로
식별한다. 이름이 같은 parameterized test도 index와 input checksum으로 구분한다. case
파일명은 사람이 읽을 수 있는 kebab-case이고 JSON의 `id`는 전체 corpus에서 유일하다.

### fixture schema

각 fixture는 자기완결적이며 `schemaVersion`, 전역 고유 `id`, `operation`, 원본 test
provenance, 변환 request와 실제 collector가 반환하는 payload를 가진다. payload 내부
node/variable/style의 구체 JSON 표현은 앞 절의 실제 JSON 계약 고정 gate를 통과한 Rust
serde type에서 생성하며 이 설계 문서가 임의의 예시 값을 선행 정의하지 않는다.

`operation`은 `tsx`, `responsive-tsx`, `devup-json`, `snapshot` 또는 `error` 중 하나다.
expected output은 JSON에 중복 저장하지 않고 같은 `id`의 insta snapshot 한 곳에만 둔다.

fixture의 `snapshot`은 별도 test mock 형식이 아니라 실제 collector의 정규화 출력
schema와 동일하다. TypeScript test에서 함수로 표현했던 값은 이전 시 다음처럼 data로
평탄화한다.

- `getMainComponentAsync()` 결과 → `mainComponentId`
- `getStyledTextSegments()` 결과 → `styledTextSegments`
- `getVariableByIdAsync()` 결과 → `variables`의 ID-indexed data
- `getStyleByIdAsync()` 결과 → `styles`의 ID-indexed data
- getter throw/rejection → 해당 node의 `fieldErrors`
- parent, child, default variant와 node 참조 → node ID
- `figma.mixed` → schema가 정의한 tagged JSON sentinel

JSON에는 함수, Symbol, 순환 object 또는 executable code가 들어갈 수 없다. fixture
loader는 unknown field를 보존하지만 잘못된 참조, 중복 ID와 지원하지 않는 schema
version은 변환 전에 실패시킨다.

### 최초 이전

실제 JSON 계약 고정 gate가 끝난 후 기준 commit의 inline TypeScript 입력을 같은 serde
type의 `cases/**/*.json`으로 이전한다. 기존 268개 JavaScript snapshot은 LF로만
정규화해 동일 ID의 최초 insta golden으로 사용한다.
snapshot이 없던 변환 assertion은 기준 JavaScript 테스트의 실제 기대값을 snapshot으로
옮긴다. 이전 완료 후 모든 변환 fixture는 JSON이 기준이며, Rust 프로젝트에는 변환용
JavaScript exporter나 runner를 남기지 않는다.

원본 54개 파일과 978개 test ID는 ledger에 전부 기록한다. 변환과 관련된 항목은 반드시
fixture ID와 snapshot 또는 Rust assertion에 연결한다. plugin bootstrap, browser iframe,
Figma 쓰기처럼 JSON 변환 pipeline 자체가 아닌 항목만 근거와 대응 안전성 test를 가진
범위 외 분류를 허용한다.

### ledger 분류

원본 978개 각각은 다음 중 정확히 하나여야 한다.

- `rust_snapshot`: Rust 공개 변환 결과가 JavaScript golden과 동일해야 함
- `rust_assertion`: snapshot보다 구조/오류 assertion이 적절한 순수 규칙
- `contract`: MCP, collector, 파일 export 경계의 Rust 계약 테스트
- `out_of_scope_write`: Figma import/변경처럼 읽기 전용 제품이 의도적으로 제공하지 않음
- `upstream_runtime_only`: Figma plugin UI, browser iframe 또는 JS module bootstrap 자체의 동작

마지막 두 분류도 삭제하거나 무시하지 않고 원본 ID, 이유, 대응 Rust 안전성 테스트를
필수로 기록한다. `out_of_scope_write`는 쓰기 기능을 구현했다는 의미가 아니며,
읽기 전용 allowlist가 해당 동작을 불가능하게 한다는 테스트에 연결한다.
`upstream_runtime_only`는 가능한 경우 MCP/파일 export의 동등한 경계 테스트에 연결한다.

ledger에는 분류별 최소 목표 수를 하드코딩하지 않는다. 대신 manifest에 기록한 기준
commit의 upstream test ID 집합과 ledger ID 집합의 완전 일치, 모든 항목의 fixture 또는
rationale/test reference 존재, 중복 없음, referenced file/checksum 존재를 검증한다.
이 규칙으로 불편한 테스트를 숫자 맞추기 없이 제외하는 것을 막는다.

### insta 사용

`devup-mcp-devup-ui`의 compatibility integration test는 `cases/**/*.json`을 재귀적으로
발견하고 manifest 순서와 무관하게 모든 case를 변환한다.

- TSX, component definitions, imports: `insta::assert_snapshot!`
- NodeTree, devup.json, diagnostics: `insta::assert_json_snapshot!`
- case ID를 snapshot name과 상대 경로로 사용해 JSON과 원본 test까지 역추적
- JSON은 object key 정렬, 문자열은 LF만 정규화
- snapshot update는 `cargo insta review` 후 manifest checksum 재생성 필요
- CI는 `cargo insta test`로 `.snap.new` 파일 존재와 unreviewed snapshot을 실패로 처리

테스트는 glob으로 발견한 JSON 집합, manifest case 집합과 snapshot 집합의 일대일 대응을
검증한다. orphan JSON, orphan snapshot, 중복 ID, 잘못된 source test 또는 ledger에서
분류되지 않은 upstream test가 하나라도 있으면 실패한다.

### drift 관리

- manifest에 원본 repository URL, commit SHA, schema version, case count와 checksum 저장
- Cargo integration test가 case ID, source test ID, snapshot과 모든 checksum 검증
- fixture 갱신은 JSON과 insta snapshot diff를 함께 review하고 source commit을 명시적으로 갱신
- `devup-mcp` CI는 network와 JavaScript runtime 없이 committed JSON/snapshot만 실행
- production package와 개발 의존성에 corpus generator/Bun/Node를 포함하지 않음

## 테스트 게이트

### Rust 단위/계약 테스트

- URL, OAuth/PKCE/keyring redaction과 source 오류 분류
- handoff session TTL, 크기/동시성, 중복 call, schema/file/version 검증
- direct와 host input이 같은 `SnapshotChunk`와 최종 artifact를 만드는지 검증
- 전체 node field 보존, 청크 분할/병합, version 충돌
- 이름 검색의 Unicode NFC, 공백/대소문자, exact/prefix/contains 우선순위와 동명 node
- variable/style alias, mode, scope, completeness
- read-only allowlist와 고정 script hash
- outputPath traversal/symlink 방지와 원자적 기록

### 호환성 테스트

- Rust corpus manifest/checksum verifier
- manifest/ledger 전수성 테스트
- 모든 `rust_snapshot`, `rust_assertion`, `contract` case
- 기준 SHA의 원본 268개 snapshot 전부 대응과 `cargo insta test`
- manifest에 기록한 원본 기준 실행 결과 978 pass / 0 fail과 source SHA 검증

기준 SHA의 숫자는 baseline이지 영구 상수는 아니다. SHA를 갱신하면 원본 test discovery
결과와 ledger/corpus를 함께 갱신한다. Rust CI는 원격 source를 실행하지 않지만, 갱신
commit에 기록된 ID가 ledger/fixture/snapshot과 일치하지 않으면 실패한다.

### 통합 및 live 테스트

- mock Streamable HTTP upstream과 mock host continuation의 동일 결과
- 공식 Figma MCP가 설치된 환경의 opt-in host fallback smoke
- direct OAuth가 허용된 환경의 opt-in direct smoke
- 제공된 file `85CgSws3o5XsLv7aAwWJyS`, node `3879:35481`은 원본 디자인
  payload를 repository에 저장하지 않고 구조/완전성만 확인
- 생성 TSX parse/type check와 생성 devup.json schema 검증
- workspace 전체 `fmt`, `clippy -D warnings`, `test`, release build

## 오류와 진단

기존 오류에 다음 안정적인 code를 추가한다.

- `DEVUP_FIGMA_DIRECT_UNAVAILABLE`
- `DEVUP_FIGMA_CATALOG_REJECTED`
- `DEVUP_FIGMA_HOST_REQUIRED`
- `DEVUP_FIGMA_HANDOFF_EXPIRED`
- `DEVUP_FIGMA_HANDOFF_INVALID`
- `DEVUP_FIGMA_RESPONSE_TOO_LARGE`
- `DEVUP_COMPAT_CORPUS_DRIFT`

모든 오류는 retry 가능 여부, source, 안전한 details를 포함한다. token, OAuth code,
verifier, Figma 사용자 정보, 원본 node 내용은 오류와 tracing에서 redact한다.

## 구현 순서와 완료 기준

1. direct source 오류 분류, host handoff session과 `devup_figma_continue`
2. 완전 collector, 고정 serializer, chunk 병합과 node/page/file 수집
3. variables/styles/assets 수집과 direct/host mock 계약
4. 실제 공식 MCP live payload를 메모리에서 검증하고 JSON 계약 고정 gate 통과
5. 검증된 serde type 기반 fixture envelope/schema와 Rust JSON loader/validator
6. 기준 plugin의 inline input을 JSON으로 이전하고 978개 ledger/268개 golden 연결
7. `insta` compatibility runner와 JSON/snapshot 일대일 검증
8. Rust NodeTree/prop/render/devup.json 규칙을 corpus 기반 TDD로 전수 이식
9. JSON fixture 전체 gate 통과. 이 단계 전에는 검색 기능으로 넘어가지 않음
10. `devup_figma_search`와 검색 결과별 `devup_figma_to_ui` 흐름
11. workspace export, 전체 live 검증, lint/test/build, changepack, 문서와 배포

완료는 단순히 Rust test가 green인 상태가 아니다. pinned 원본의 모든 test ID가 ledger에
있고, 모든 읽기/변환 케이스가 Rust snapshot/assertion/contract로 통과하며, 범위 밖
항목도 이유와 읽기 전용 안전성 테스트가 있고, direct와 host 경로가 같은 artifact를
내며, 관련 lint/test/build와 changepack이 통과해야 한다.

## 개인정보, 보안과 남은 위험

- corpus에는 기준 저장소의 합성 mock만 포함하고 실제 사용자 Figma payload는 넣지 않는다.
- direct token은 OS keyring만 사용하며 공식 MCP token에는 접근하지 않는다.
- localhost callback은 명시적 direct login 중에만 loopback에 bind하고 즉시 종료한다.
- 사용자 JavaScript, upstream test code 또는 corpus executable code를 product에서 실행하지 않는다.
- Figma write tool은 direct allowlist와 host handoff 모두에서 생성하지 않는다.
- host가 반환한 result는 신뢰 경계 밖 입력으로 보고 크기/schema/context를 검증한다.

가장 큰 남은 위험은 JavaScript의 Figma mock 함수와 plugin side effect를 손실 없이
정규화된 JSON data로 이전하는 난이도, Remote MCP의 beta contract 변화, 대형 파일의
호출 수와 quota다. 최초 이전에서 함수의 의미를 추측하지 않고 실제 기대값과 대조하며,
직렬화할 수 없는 경계는 ledger에 드러낸다. 대표 fixture가 기존 golden을 재현하지
못하면 codegen 대량 이식을 계속하지 않고 fixture schema를 먼저 수정한다.
