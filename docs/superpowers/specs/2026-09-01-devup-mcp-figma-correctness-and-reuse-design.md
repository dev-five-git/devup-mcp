# devup-mcp Figma 정확성·재사용 강화 설계

## 상태

- 승인일: 2026-09-01
- 대상 branch: `owjs3901/figma-remote-mcp`
- 대상 PR: <https://github.com/dev-five-git/devup-mcp/pull/1>
- 선행 설계:
  - `2026-08-31-figma-host-fallback-parity-design.md`
  - `2026-08-31-figma-linked-screen-exploration-design.md`
  - `2026-08-31-figma-single-call-envelope-design.md`

이 문서는 기존 Figma Remote MCP 구현을 대체하지 않는다. 이미 구현된 Rust workspace,
read-only direct/host 수집, exact-node 단일 호출, JSON fixture parity와 화면 탐색 위에서
정확성 계약, 재사용, 진단과 남은 corpus 이전을 강화한다.

## 현재 기준선

현재 workspace는 다음 3개 crate로 구성된다.

- `devup-mcp`: stdio MCP server, source orchestration, host handoff와 공개 tool
- `devup-mcp-figma`: URL, OAuth, upstream MCP, read-only script, snapshot과 resource 수집
- `devup-mcp-devup-ui`: DevupUI TSX와 `devup.json` projection

설계 승인 시점의 검증 결과는 다음과 같다.

- `cargo test --workspace`: 116개 통과, 인증 입력이 필요한 opt-in live test 1개 제외
- exact-node fast path: 공식 `use_figma` 호출 1회
- WQUW-151 proofread fixture: 144개 node, 변수 참조 20개, text style 11개
- pinned plugin corpus: 54개 test file, 978개 test ID, 268개 golden snapshot
- Rust golden parity: 268/268
- ledger: `rust_snapshot` 252, `rust_assertion` 550, `not_ported` 137,
  `upstream_runtime_only` 21, `out_of_scope_write` 18

## 문제 정의

기존 구현은 정상 exact-node 흐름에서는 충분히 빠르고 WQUW-151의 핵심 TSX를 재현한다.
그러나 다음 항목은 아직 성공 응답의 의미와 장기적인 Figma 변경 대응을 약하게 만든다.

1. `ThemeScope`가 실제 variable/style filtering에 적용되지 않는다.
2. 동일 token key의 서로 다른 variable 후보가 결정적으로 충돌 해결되지 않는다.
3. UI, theme, raw snapshot을 연속 요청하면 같은 Figma 데이터를 다시 수집할 수 있다.
4. legacy snapshot 병합 후 전체 child graph 완전성을 별도 감사하지 않는다.
5. field truncation과 unsupported fallback이 `status: complete`와 명확히 분리되지 않는다.
6. Section 링크에서 내부 화면을 찾은 다음 다중 frame을 내보내는 계약이 분리돼 있다.
7. 생성 TSX의 prop이 어느 Figma node/field/resource에서 왔는지 추적하기 어렵다.
8. 매우 큰 computed field는 전체 값 대신 truncation marker로 끝날 수 있다.
9. upstream corpus의 137개 test ID가 `not_ported` 상태다.

## 핵심 결정

### 기존 3개 crate 구조를 유지한다

새 `devup-ir`, `devup-auth`, `devup-cache`, `devup-mcp-server` crate를 만들지 않는다.
원본 보존과 완전성은 Figma snapshot 계약이고, projection은 DevupUI의 책임이며, 수집
artifact의 수명과 MCP 재사용은 server의 책임이다.

### 수집과 projection을 분리한다

모든 변환은 검증된 하나의 acquisition artifact를 입력으로 사용한다.

```text
Figma URL
  -> Target classification
  -> Read-only acquisition
  -> RawSnapshot + ResourceSnapshot
  -> Completeness audit
  -> In-memory artifact
  -> TSX / devup.json / raw JSON / source map projections
```

동일 artifact에서 여러 projection을 만들 때 Figma를 다시 호출하지 않는다.

### 정보 손실을 조용히 성공으로 처리하지 않는다

완전성 상태는 다음 3개로 고정한다.

- `complete`: 요청한 범위의 node graph와 필수 projection field가 모두 보존됨
- `partial`: 사용 가능한 결과를 생성했지만 누락, truncation 또는 unsupported fallback 존재
- `failed`: 안전한 artifact를 생성할 수 없거나 strict 요구를 만족하지 못함

기본 모드는 복구 가능한 결과를 `partial`로 반환한다. `strict: true`이면 partial도 안정적인
오류 code와 completeness report를 반환한다. 어떤 모드에서도 누락을 `complete`로 숨기지 않는다.

## 데이터 계약

### AcquisitionArtifact

server는 수집이 끝나면 다음 논리 구조를 등록한다. 실제 공개 Rust type 이름은 구현 계획에서
테스트와 함께 확정하되 필드 의미는 이 계약을 따른다.

```text
AcquisitionArtifact
  artifact_id
  target
  source
  transport
  source_version
  schema_hash
  content_hash
  snapshot
  resources
  completeness
  diagnostics
  stats
```

`artifact_id`는 credential이나 Figma 사용자 정보를 포함하지 않는 opaque ID다.
`content_hash`는 canonical snapshot/resource bytes에서 계산하며 응답에는 필요할 때만 노출한다.

### CompletenessReport

완전성 감사 결과에는 최소한 다음 항목이 들어간다.

- requested root 수
- preserved/reachable/orphan node 수
- declared/exported child 수
- missing child ID와 parent node ID
- parent-child 역참조 불일치
- duplicate/conflicting node 수
- field error 및 truncated field 수
- unresolved variable/style/asset 수
- graph, fields, resources 각각의 상태

root에서 `childrenIds`를 순서대로 순회하며 모든 child가 존재하는지 확인한다. child의
`parentId`가 수집 범위 안에서 확인 가능하면 역참조도 검증한다. 숨겨진 node와 instance의
실제 children도 일반 child와 동일하게 보존한다.

`childCount`가 제공되면 `childrenIds` 수와 비교한다. Figma API가 해당 필드를 제공하지 않는
경우 count를 추측하지 않고 `not-observed`로 기록한다.

### Diagnostic

기존 `code`, `message`, `nodeId`를 유지하면서 다음 필드를 additive하게 추가한다.

- `severity`: `info`, `warning`, `error`
- `property`: 관련 Figma field 또는 생성 prop
- `resourceKind`, `resourceId`
- `fallback`: 적용한 fallback 종류
- `recoverable`
- `details`: 원문 디자인 값과 secret을 포함하지 않는 구조화 정보

token, OAuth code, verifier, 사용자 정보와 실제 text 내용은 diagnostic/tracing에 넣지 않는다.

## 변수, style과 theme 충돌

### 결정적 후보 처리

variable은 `HashMap::values()` 순회 결과로 projection하지 않는다. 모든 후보를 안정적으로
정렬하고 token/mode별 후보 집합을 만든다.

후보 값이 같으면 하나로 병합하고 provenance에 모든 source ID를 남긴다. 값이 다르면 다음
우선순위로 winner를 선택한다.

1. 명시적인 `codeSyntax.WEB` token
2. local variable
3. 실제 node에서 사용된 remote variable
4. 정규화된 Figma variable name
5. collection name, variable name, variable ID의 lexical order

마지막 lexical order는 의미 우선순위가 모두 같은 경우의 결정성만 보장한다.

충돌은 `DEVUP_THEME_TOKEN_CONFLICT` warning으로 반환하고 모든 후보의 collection, mode,
raw name, resource ID와 값 hash를 결과 metadata에 보존한다. devup.json에는 winner 하나만
기록해 기존 schema를 깨지 않는다. 전체 원본 후보는 acquisition artifact에서 손실 없이
유지된다.

### alias와 fallback

- alias는 variable ID와 mode ID 쌍으로 순환을 감지한다.
- alias 대상 mode가 없으면 collection default mode를 안전하게 확인한다.
- 실제 resolved value가 있으면 color/length로 projection한다.
- node-bound raw paint 또는 실제 text/style 값이 있으면 TSX에 hex/px fallback을 사용할 수 있다.
- 존재하지 않는 token 이름이나 값을 추측해서 만들지 않는다.
- collection/mode가 없는 후보는 중단하지 않고 `unresolvedVariables`와 diagnostic에 남긴다.

### scope

- `node`: 선택 subtree에서 실제 참조된 variable/style만 포함
- `page`: page subtree에서 실제 참조된 항목과 필요한 alias dependency만 포함
- `file`: 수집 가능한 local 전체와 실제 사용된 remote variable 포함

응답의 completeness가 해당 범위를 증명하지 못하면 `full-local-plus-used-remote`를 반환하지
않는다.

## 호출 최적화와 artifact cache

### 복수 projection

하나의 공개 export 흐름에서 다음 output을 복수 선택할 수 있게 한다.

- `tsx`
- `devupJson`
- `rawSnapshot`
- `sourceMap`
- `assetManifest`

기존 `devup_figma_to_ui`와 `devup_figma_to_json`은 호환 wrapper로 유지한다.

### fast 수집

- exact node의 subtree와 used resources는 현재처럼 정상 경로 1회 호출을 유지한다.
- full theme는 크기 제한 안에서 variable collection, variable, style을 하나의 read-only
  `use_figma` envelope로 수집하는 fast path를 추가한다.
- response가 제한을 넘거나 contract가 달라지면 같은 source의 cursor/batch legacy 경로를
  0부터 원자적으로 재시작한다.
- direct source가 catalog/auth/capability 경계에서 실패하면 `auto` 정책이 host official MCP
  handoff로 전환한다.
- rate limit, node not found, version conflict는 다른 source로 무조건 재시도하지 않는다.

### cache 정책

cache는 `devup-mcp` process 내부에만 둔다.

- request alias key: source policy, file key, node ID, scope와 acquisition option
- canonical key: source, file key, source version, schema hash와 content hash
- bounded LRU + TTL
- 전체/항목별 byte 상한
- single-flight로 같은 동시 요청 병합
- `refresh: true`로 강제 재수집
- process 종료 시 자동 폐기
- credential, screenshot과 binary asset bytes는 cache key나 stats에 포함하지 않음

source version을 새 API 호출 없이 확인할 방법은 없으므로 TTL 안의 reuse는 명시적인 freshness
정책이다. 응답에는 `cacheHit`, `artifactId`, `acquiredAt`, `expiresAt`과 call count를 포함한다.

## Section, 검색과 다중 Frame export

링크 target을 `file`, `page`, `section`, `screen-frame`, `component`, `other`로 분류한다.

Section 또는 대형 container가 입력되면 단일 거대 TSX를 기본 생성하지 않는다. 먼저 하위
screen candidate를 visual order로 반환한다.

각 candidate는 다음을 포함한다.

- node ID, name, type
- bounds, child count, visible state
- parent/breadcrumb
- screen classification 근거
- canonical URL

export는 다음 선택을 받는다.

- 정확한 단일 frame ID
- frame ID 목록
- `allScreens: true`

기존 `search`는 파일 전체 이름 탐색에 사용하고, `explore`는 링크 주변/내부 화면 선택에
사용한다. 공개 tool은 대상 분류와 artifact reuse를 additive하게 보강하며 기존 agent 흐름을
깨지 않는다.

## 대형 field와 asset

### 대형 field

inline 안전 상한을 넘는 field를 즉시 버리지 않고 descriptor를 반환한다.

```text
LargeValueDescriptor
  node_id
  field
  byte_length
  content_hash
  cursor
```

collector는 descriptor가 있는 field를 bounded continuation으로 읽어 content-addressed chunk로
재조립한다. 청크 누락, 중복, hash 불일치는 전체 field를 partial로 표시하며 complete로
승격하지 않는다.

Figma runtime이 getter를 거절하거나 private plugin data처럼 읽을 권한이 없는 값은
`unsupported-by-upstream`으로 구분한다. 접근 가능한 값의 단순 크기 문제와 권한 문제를 같은
truncation으로 취급하지 않는다.

### asset

- image hash, export setting, vector/network reference와 source node를 asset manifest에 보존한다.
- read-only `exportAsync`가 허용되는 source에서는 요청된 asset만 SVG/PNG로 내보낸다.
- binary는 MCP 응답에 무제한 inline하지 않고 명시적 output path 또는 bounded resource로 제공한다.
- asset export 실패가 일반 layout/text subtree를 제거하지 않는다.
- 누락 asset은 node/property별 diagnostic과 placeholder provenance를 가진다.

## Source map

기본 TSX에는 `data-figma-node`나 debug comment를 강제로 삽입하지 않는다. runtime DOM과 기존
golden 출력을 바꾸지 않는 별도 sidecar source map을 생성한다.

각 entry는 다음을 연결한다.

- 생성 artifact와 UTF-8 byte range 또는 JSON pointer
- Figma node ID
- 입력 field 목록
- variable/style/asset ID
- direct mapping, normalized mapping, inferred layout 또는 fallback 종류

formatter 이후 range가 달라지지 않도록 codegen fragment가 source span을 함께 조립하고 최종
렌더러가 offset을 확정한다.

## 테스트 전략

### TDD 원칙

각 변경은 실패하는 작은 contract/snapshot test를 먼저 추가하고 구현 후 기존 전체 suite를
재실행한다. snapshot은 자동 accept하지 않으며 의도한 출력 변경만 검토한다.

### WQUW-151 실제 계약

- Section `4217:7743` 내부 frame 목록과 visual order
- proofread frame `3879:35518`의 144-node graph
- 모든 paragraph text와 nested `[1. 이름]`
- 20개 variable 참조, 11개 typography style
- individual footer top stroke
- standalone/embedded root layout
- TSX + used-token theme composite export의 정상 1-call 수집
- 같은 artifact 재사용 시 추가 upstream call 0회

실제 fixture에는 token, OAuth credential, Figma 사용자 정보 또는 계정 식별자를 넣지 않는다.
디자인 원문을 repository에 보존하는 fixture는 현재 repository visibility와 사용 권한을 확인한
범위에 한한다.

### plugin corpus

137개 `not_ported`를 모두 재분류한다.

- 읽기/변환 의미가 있으면 JSON fixture, Rust assertion 또는 contract test로 이전
- 실제 plugin process lifecycle만 의미가 있으면 근거와 가장 가까운 Rust boundary test 연결
- mutation 동작만 `out_of_scope_write` 유지
- 최종 ledger에서 `not_ported`는 0이어야 함

기존 268개 golden은 output compatibility gate로 계속 byte parity를 유지한다.

### 필수 명령

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo insta pending-snapshots
cargo build --workspace --release
```

live contract는 인증된 official host MCP 결과를 secret 출력 없이 검증한다. CI 비용 문제로
원격 workflow가 비활성 또는 실패하더라도 로컬 검증 결과와 CI 상태를 분리해 보고한다.

## 보안과 개인정보

- Figma 기능은 계속 read-only allowlist와 compiled-in script만 사용한다.
- 사용자 제공 JavaScript나 fixture code를 product에서 실행하지 않는다.
- direct OAuth credential은 OS keyring에만 저장한다.
- localhost callback은 명시적인 login 중에만 `127.0.0.1`에 bind하고 종료한다.
- host official MCP credential에는 접근하거나 복사하지 않는다.
- snapshot과 screenshot은 기본적으로 디스크에 저장하지 않는다.
- artifact cache는 메모리 전용, bounded, TTL 기반이다.
- diagnostics, stats와 tracing은 디자인 text와 인증 정보를 포함하지 않는다.

## 구현 순서

1. completeness/status/diagnostic 계약과 graph auditor
2. 결정적 theme resolver, non-fatal conflict와 실제 scope filtering
3. acquisition artifact, composite export와 bounded cache
4. Section target classification과 다중 frame export
5. large field continuation과 asset manifest/export
6. TSX/devup.json source map
7. WQUW-151 Section/10-frame 실제 회귀 fixture와 visual 보조 검증
8. 137개 `not_ported` 제거
9. 전체 fmt/clippy/test/insta/release/live 검증
10. changepack, focused commits, push, PR #1 갱신과 Codex binary 재설치

각 단계는 독립적인 focused commit으로 남기고 이전 단계의 contract가 green인 상태에서만 다음
단계로 진행한다.

## 완료 기준

- 누락 child 또는 필수 field가 있는 결과는 `complete`가 아니다.
- theme/token 충돌은 결정적이며 변환 전체를 중단하지 않는다.
- node/page/file theme scope가 실제 참조 범위를 반영한다.
- exact-node composite export의 정상 upstream 호출 수는 1회다.
- 동일 artifact의 후속 projection은 추가 upstream 호출 0회다.
- Section 링크에서 내부 화면 목록과 선택/batch export가 가능하다.
- TSX와 devup.json의 주요 출력에 Figma provenance를 역추적할 수 있다.
- 접근 가능한 대형 field를 단순 byte 상한 때문에 조용히 버리지 않는다.
- plugin corpus ledger의 `not_ported`가 0이다.
- WQUW-151의 text, children, color token, typography와 stroke 회귀가 모두 통과한다.
- 모든 관련 로컬 lint/test/build와 snapshot 검증이 통과한다.
- changepack, commit SHA, PR URL, 설치 버전, 보안 검토와 남은 upstream 제약을 최종 보고한다.

## 남은 외부 제약

- private OAuth client는 Figma MCP Catalog 승인 전 direct registration이 거절될 수 있다.
- 사용되지 않은 외부 library variable 전체는 official MCP가 제공하지 않을 수 있다.
- Figma runtime이 공개하지 않는 private data와 오류를 내는 getter는 읽을 수 없다.
- official `use_figma` contract가 바뀌면 adapter와 live contract 갱신이 필요하다.
- 매우 큰 파일은 정확성을 유지하기 위해 단일 호출보다 cursor continuation이 필요할 수 있다.

이 제약은 호출 수를 줄이기 위해 결과를 생략하는 근거로 사용하지 않는다. 빠른 경로가 안전
상한을 넘으면 느리더라도 완전한 legacy/continuation 경로로 전환하고 그 사실을 stats와
diagnostics에 명시한다.
