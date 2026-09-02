# Devup MCP Hardening and Delivery Design

## 목적

현재 Figma → DevupUI 변환의 정확성 계약을 다음 단계까지 확장한다.

1. cached artifact가 요청한 asset의 ID뿐 아니라 format과 scale까지 정확히 포함하는지 검증한다.
2. 명시된 `outputPath` 기록을 허용된 workspace 안으로 제한하고, 검증 완료 전에는 어떤 파일도 바꾸지 않는다.
3. 대형 Section에서 전체 subtree를 먼저 내려받지 않고 화면 목록을 찾은 뒤 선택한 여러 Frame만 bounded batch로 수집한다.
4. 큰 TSX, JSON, source map과 binary를 MCP 표준 Resource로 전달해 tool response 상한을 피한다.
5. 생성 TSX의 문법과 의미 보존을 Rust에서 검증하고 실제 렌더 screenshot 비교를 위한 안전한 경계를 제공한다.
6. 이 기능을 추가하면서 1,300줄을 넘은 server router를 acquisition, projection, delivery와 output 책임으로 분리한다.

이 문서는 위 항목을 한 번에 뒤섞어 구현하는 계획이 아니라, 각 단계가 독립적으로 검증되고 배포될 수 있는 통합 아키텍처를 정한다.

## 현재 확인된 공백

### Asset reuse

`CollectionRequest`와 artifact cache key는 `AssetSelection { asset_id, format, scale }` 전체를 보존한다. 그러나 `PendingOperation::Export`는 이를 `asset_ids: Vec<String>`으로 축소하고, artifact 재사용 시 payload에 같은 ID가 있는지만 확인한다. 따라서 `logo`의 PNG 1x artifact를 SVG 4x 요청에 재사용해도 기존 binary가 통과할 수 있다.

### Filesystem output

현재 `write_binary_output`은 사용자 경로의 상위 폴더를 만들고 바로 `std::fs::write`한다. 허용 루트, `..`, junction/symlink 탈출, 여러 출력의 부분 기록, 기존 파일 복구와 crash 시 임시 파일 정리 계약이 없다. strict 검증 전 기록은 이미 막았지만, non-strict 다중 출력의 두 번째 기록이 실패하면 첫 번째 파일은 남는다.

### Section collection

현재 Section export는 Section artifact 전체를 수집한 뒤 그 안에서 후보를 찾고 `selection_required`를 반환한다. WQUW-151 Section처럼 전체 fast envelope가 8 MiB를 넘는 경우 사용자가 Frame 하나만 원해도 큰 subtree 수집과 fallback을 먼저 수행한다.

### Delivery

Tool result는 생성 text와 base64 asset을 inline한다. Artifact cache 자체는 bounded지만 한 번의 MCP response에 들어가는 output 크기 계약은 없다. `rmcp` 3.1.4는 `resources/list`, `resources/templates/list`, `resources/read`, text/blob `ResourceContents`와 Resource Link를 지원하므로 사설 continuation 프로토콜을 추가할 이유가 없다.

### Fidelity

268개 upstream JSON golden과 WQUW-151 fixture는 변환 source의 결정성을 검증하지만 브라우저가 실제 렌더한 pixel까지 증명하지 않는다. 또한 projection quality는 알려진 diagnostic code 세 개를 문자열로 열거하므로 새 fallback diagnostic이 추가되면 quality가 잘못 `exact`가 될 수 있다.

## 설계 원칙과 제약

- 제품 server, collector, codegen과 기본 CI는 계속 순수 Rust/Cargo workspace다.
- 사용자 제공 JavaScript를 실행하지 않는다. Figma에서는 compiled-in read-only script만 실행한다.
- Figma OAuth scope와 official MCP fallback은 넓히지 않는다.
- 기존 tool 이름과 작은 응답의 주요 JSON 필드는 유지한다.
- artifact, projected resource와 screenshot은 memory-only, bounded, TTL 기반이 기본이다.
- design text, token 값, URL, asset ID, OAuth 정보는 로그와 cache 통계에 기록하지 않는다.
- 기존 268개 golden과 WQUW-151 열 Frame fixture는 각 단계의 회귀 게이트다.
- 모든 새 요청 값은 projection과 filesystem mutation 전에 검증한다.

## 검토한 접근과 선택

### 대형 결과 전달

1. **inline 상한만 높인다.** 가장 단순하지만 client/transport 상한을 서버가 통제할 수 없고 base64 팽창도 해결하지 못한다.
2. **사설 continuation tool을 만든다.** 현재 handoff와 유사하지만 MCP client가 별도 호출 규칙을 배워야 하고 표준 Resource와 중복된다.
3. **MCP Resource를 사용한다.** 작은 결과는 inline하고 큰 결과는 manifest와 bounded chunk URI로 읽는다.

3번을 선택한다. 표준 기능이고 artifact TTL과 같은 메모리 수명 모델을 재사용할 수 있다.

### Filesystem 격리

1. 문자열 canonicalize 후 prefix 비교는 생성 전 경로와 symlink 교체 경쟁을 안전하게 막지 못한다.
2. 모든 기록을 제거하면 안전하지만 명시적 `outputPath` workflow를 잃는다.
3. process 시작 시 pre-open한 허용 root capability를 기준으로 상대 경로만 해석하고 staging/commit한다.

3번을 선택한다. `cap-std` 계열 capability API를 사용하고 기본 허용 root는 server 시작 시 canonical current directory 한 곳이다. 반복 가능한 `--allow-write-root <path>`로만 범위를 추가한다.

### 실제 시각 회귀

1. Bun/Playwright harness를 workspace에 넣으면 실제 DevupUI 렌더 fidelity는 가장 높지만 Cargo-only 기본 계약을 깨고 package drift가 다시 생긴다.
2. 구조 비교만 하면 Cargo-only지만 browser layout, font metric과 CSS compiler 차이를 pixel 수준에서 검출하지 못한다.
3. Rust semantic validator와 Rust image comparator를 기본 제공하고, 실제 DevupUI render는 소비자 repo가 명시적으로 제공하는 adapter contract로 연결한다.

3번을 선택한다. devup-mcp는 reference Figma screenshot, generated TSX, semantic report와 pixel comparator를 제공한다. 실제 React/DevupUI bundle 생성은 그 dependencies를 이미 소유한 소비자 프로젝트에서 수행한다. 따라서 기본 Cargo suite는 외부 runtime 없이 완전하게 실행되고, opt-in visual job은 실제 renderer까지 검증한다.

### Server 분해

기능별 새 crate를 매번 추가하면 public type 이동과 dependency cycle이 커진다. 반대로 `server/mod.rs` 안에서 계속 확장하면 router, state machine, projection과 I/O 테스트가 결합된다. 우선 `devup-mcp` crate 내부 module로 server 책임을 분리하고, 재사용성이 입증된 image comparator만 `devup-mcp-visual` crate로 분리한다.

## 단계 A: Asset capture와 안전한 output transaction

### 정확한 asset contract

공개 입력은 기존 `assetRequests`를 유지한다. 내부에서는 축소하지 않고 다음 값 객체를 끝까지 전달한다.

```rust
struct AssetCapture {
    asset_id: String,
    format: AssetFormat,
    scale: u8,
}
```

- `PendingOperation::Export.asset_ids`를 `asset_captures: Vec<AssetCapture>`로 교체한다.
- output path도 asset ID 문자열이 아니라 capture key에 연결한다.
- 같은 요청에서 같은 asset ID를 서로 다른 format/scale로 두 번 요청하는 것은 기존처럼 거절한다. 한 manifest entry와 한 output path가 한 ID에 대응하는 현재 공개 shape를 유지하기 위해서다.
- cached artifact 내부 capability는 정확한 capture set을 가진다.
- 공개 `cache.capabilities`에는 `assetCaptureCount`만 추가하고 asset ID, token 또는 파일명은 노출하지 않는다.
- artifact reuse는 요청 capture 각각에 대해 ID, format, scale이 모두 같은 exported payload가 있는지 projection 전에 검사한다.
- 누락 또는 불일치는 `DEVUP_FIGMA_HANDOFF_INVALID`와 non-sensitive expected/available count만 반환한다. URL 재수집 안내는 유지한다.

`AssetManifestEntry`는 `status=exported`일 때 실제 export format과 scale을 반드시 보존해야 한다. 아직 export하지 않은 `available` entry는 지금처럼 둘 다 `None`일 수 있다. 구버전 또는 잘못된 artifact에 `exported`이면서 format/scale이 없으면 PNG 1x로 추측하지 않고 incompatible artifact로 거절한다.

### Output policy

server configuration에 다음 정책을 둔다.

```rust
struct OutputPolicy {
    roots: Vec<cap_std::fs::Dir>,
    display_roots: Vec<PathBuf>,
}
```

- `devup-mcp` 시작 시 current directory를 한 번 canonicalize하고 capability로 연다.
- `--allow-write-root <path>`는 반복 가능하고 시작 시 존재하는 directory만 허용한다.
- 상대 `outputPath`는 첫 번째 root 기준이다.
- 절대 경로는 정확히 한 허용 root 아래여야 하며 그 root에 상대화한 뒤 capability API로만 연다.
- 빈 경로, directory target, `..`, Windows drive/UNC 전환, alternate data stream, root 밖 symlink/junction은 거절한다.
- 반환 경로는 검증된 display root와 정규화된 상대 경로로 조립하며 credential이나 원본 design 내용을 포함하지 않는다.

### Transaction semantics

모든 text와 binary output은 하나의 `OutputTransaction`으로 처리한다.

1. output 이름, target, 중복 target, parent와 예상 byte 수를 모두 검증한다.
2. strict/quality/asset/base64/resource 검증을 모두 끝낸다.
3. 각 target과 같은 directory에 예측 불가능한 이름의 exclusive temp file을 만든다.
4. 전체 contents를 쓰고 `sync_all`한다.
5. 기존 target은 같은 directory의 private backup 이름으로 이동한다.
6. temp를 target으로 rename한다.
7. 한 commit이 실패하면 이미 바뀐 target을 역순으로 제거하고 backup을 복구한다.
8. 성공 후 backup을 지우고 가능한 플랫폼에서는 parent directory를 sync한다.

개별 파일의 rename은 atomic하다. 여러 directory와 process crash를 가로지르는 완전한 원자성은 일반 filesystem에서 보장하지 않는다. Runtime error는 rollback하지만 전원 중단 후에는 `.devup-tmp-*` 또는 `.devup-bak-*`가 남을 수 있으므로 다음 시작/기록 때 같은 root 안의 stale internal file만 TTL 기준으로 정리한다. 사용자 파일명과 일치하는 임의 파일은 정리하지 않는다.

output path를 지정하지 않으면 filesystem API를 호출하지 않는다. Resource delivery도 디스크를 사용하지 않는다.

## 단계 B: Section discovery와 multi-root acquisition

### 두 단계 흐름

Section URL을 exact Frame처럼 먼저 전부 수집하지 않는다.

```text
Section URL
  -> bounded SectionIndex 수집
  -> frame 선택이 없으면 selection_required
  -> frameIds/allScreens가 있으면 selected root batch 수집
  -> composite artifact
  -> frame별 projection/delivery
```

`SectionIndex`는 Section root의 ID/name/bounds와 screen candidate마다 다음 값만 가진 의도적인 compact projection이다.

- ID, name, type, visible
- absolute bounds와 visual order
- parent/breadcrumb
- direct child count
- subtree node count와 estimated serialized byte count
- screen classification reasons와 canonical URL

Figma sandbox에서는 subtree를 읽어 count/estimate를 계산하지만 response에는 descendant field를 넣지 않는다. 이 discovery projection이 일부 필드만 반환하는 것은 누락이 아니라 명시된 acquisition kind다. 선택한 root의 최종 수집은 기존 exhaustive property manifest와 runtime enumerable 탐색을 그대로 사용한다.

### Batch planner

- 후보는 기존 visual order로 고정한다.
- 요청한 `frameIds`의 유효성·중복을 SectionIndex에서 먼저 확인한다.
- `allScreens`와 `frameIds`의 상호 배타 계약을 유지한다.
- batch는 `estimatedBytes`, node count와 응답 안전 상한을 기준으로 결정적으로 pack한다.
- 한 batch script는 여러 root를 받아 각 root를 독립 envelope chunk로 만들고, 실제 byte 상한에 닿으면 완료된 root 목록과 continuation cursor를 반환한다.
- 추정이 빗나간 oversized root만 기존 cursor collector로 이어 읽는다. 이미 완료된 root를 0부터 다시 읽지 않는다.
- 동시에 실행하는 official MCP call 수는 bounded하고 기본값은 2다. Figma rate limit 신호가 오면 병렬도를 높이지 않는다.
- 정상적으로 상한 안에 든 N개 root는 가능한 최소 batch 수로 수집한다. root마다 무조건 한 번 호출하지 않는다.

### Composite artifact

artifact payload를 다음 두 shape로 일반화한다.

```rust
enum ArtifactPayload {
    Single(Arc<CollectedPayload>),
    Composite(Arc<CompositePayload>),
}

struct CompositePayload {
    target: FigmaTarget,
    index: SectionIndex,
    roots: Vec<CollectedPayload>,
    shared_resources: CollectedResources,
    completeness: CompositeCompleteness,
}
```

- root 순서는 SectionIndex visual order다.
- node ID 중복은 byte-identical이면 한 번만 보존하고 다르면 version/merge conflict로 실패한다.
- variable/style/component/asset resource는 ID와 canonical content hash로 dedupe한다.
- 모든 chunk의 file key와 source version이 같아야 한다. 수집 중 version drift가 있으면 전체 composite를 cache하지 않는다.
- root별 completeness와 전체 aggregate를 모두 제공한다. 한 Frame의 partial 상태가 다른 Frame의 TSX를 제거하지 않는다.
- cache key는 Section ID, 정렬된 선택 root set이 아니라 요청 visual order의 root list, scope, resource scope, asset captures와 source policy를 포함한다.
- cache byte limit은 composite root와 resource bytes 전체를 센다. 상한을 넘으면 frame 선택 축소 또는 explicit output streaming을 안내하고 silently evict/truncate하지 않는다.

선택이 없는 SectionIndex artifact는 design artifact가 아니며 TSX projection에 재사용할 수 없다. 새 internal/public kind `section-index`를 사용한다.

### Compatibility

- exact Frame URL의 정상 single-call path는 바꾸지 않는다.
- 기존에 이미 확보한 full Section `design` artifact는 계속 후보 탐색과 선택 projection에 사용할 수 있다.
- 새 URL 요청만 index-first 경로를 사용한다.
- `frames[]` response shape와 component naming 규칙은 유지한다.

## 단계 C: MCP Resource delivery

### 공개 입력

`devup_figma_export`에 다음 additive 입력을 추가한다.

```json
{
  "delivery": "auto",
  "outputs": ["tsx", "devupJson", "sourceMap", "assetManifest"]
}
```

허용값은 다음과 같다.

- `inline`: 기존 JSON field에 담는다. 안전한 hard limit을 넘으면 명시적 오류를 반환하며 oversized response를 보내지 않는다.
- `resource`: 모든 생성 artifact를 Resource Link와 manifest로 반환한다.
- `auto`: 개별 output 256 KiB 이하이고 inline 전체가 1 MiB 이하이면 기존처럼 inline하고, 그보다 크면 resource로 전환한다.

기본값은 `auto`다. 기존 작은 응답은 byte-compatible field shape를 유지한다. 이전에 transport 실패 위험이 있던 큰 응답만 resource manifest로 바뀐다. 호환 wrapper인 `devup_figma_to_ui`와 `devup_figma_to_json`도 같은 auto 정책을 사용하고 explicit `delivery: inline`을 받을 수 있게 한다.

### Resource store와 URI

projected output은 acquisition artifact entry에 연결된 memory-only resource로 저장한다. 별도 수명이 긴 global blob store를 만들지 않는다.

```text
devup://artifact/{artifactId}/outputs/manifest
devup://artifact/{artifactId}/outputs/{outputName}/manifest
devup://artifact/{artifactId}/outputs/{outputName}/chunks/{index}
devup://artifact/{artifactId}/assets/{opaqueCaptureId}/manifest
devup://artifact/{artifactId}/assets/{opaqueCaptureId}/chunks/{index}
```

- URI에는 file key, node ID, asset ID, component name이나 output path를 넣지 않는다.
- `{artifactId}`는 기존 256-bit random ID다.
- `{opaqueCaptureId}`는 capture를 attach할 때 만든 독립적인 128-bit 이상 random ID이며 원본 asset ID를 복구할 수 없다.
- text와 binary raw chunk는 최대 256 KiB다. blob `ResourceContents`의 base64 팽창은 store byte accounting에 원본과 encoded 크기를 모두 반영한다.
- manifest는 MIME, raw bytes, SHA-256, chunk count/size, quality와 expiry만 가진다.
- chunk read는 hash와 index를 검증하고 전체 output을 복사하지 않는 slice 기반 응답을 만든다.
- artifact가 TTL 만료 또는 LRU eviction되면 관련 resource도 함께 사라지고 `resource not found`가 된다.
- 동일 artifact와 projection option hash의 output은 재사용해 재-projection과 추가 Figma call을 하지 않는다. acquisition `contentHash`는 attached output 때문에 바뀌지 않으며 resource manifest가 별도의 output content hash를 가진다.

`resources/list`는 현재 살아 있는 top-level output manifest만 cursor pagination으로 열거한다. chunk 수가 많아도 chunk URI 전부를 list하지 않는다. `resources/templates/list`는 위 manifest/chunk URI template을 설명한다. subscribe/list-changed는 실제 push use case가 없으므로 광고하지 않는다.

Server capability는 tools와 resources를 함께 광고한다. Tool response에는 Resource Link, manifest URI, expiry, byte 수와 content hash를 넣고 design 원문을 summary에 반복하지 않는다.

### outputPath와 Resource의 관계

- `outputPath`가 있으면 transaction commit 후 path를 반환하고 해당 binary base64는 inline에서 제거한다.
- `delivery=resource`와 `outputPath`를 함께 쓰면 파일 기록과 Resource Link 둘 다 제공한다. 두 결과는 같은 content hash여야 한다.
- strict 실패, capability 실패 또는 transaction preflight 실패 시 Resource도 publish하지 않는다.
- projection과 resource chunk를 먼저 staging하고 quality가 통과한 뒤 artifact entry에 한 번에 attach한다.

## 단계 D: Fidelity, 진단 구조와 시각 회귀

### Structured diagnostic impact

`Diagnostic`에 optional additive field를 추가한다.

```rust
enum FidelityImpact {
    None,
    Approximated,
    Lossy,
    Failed,
}
```

- collector informational diagnostic은 `none`이다.
- absolute layout fallback은 `approximated`다.
- mask/effect와 의미를 잃는 fallback은 `lossy`다.
- projection 자체를 만들 수 없으면 `failed`다.
- 기존 fixture/외부 payload에서 field가 없으면 diagnostic producer domain의 registry가 code를 impact로 변환한다.
- 알 수 없는 warning/error code는 자동으로 `exact`가 되지 않는다. codegen warning은 최소 `approximated`, codegen error는 `failed`; collector warning은 acquisition completeness가 판정한다.

`projection_quality`는 문자열 목록 대신 impact의 최댓값을 계산한다. `includeDiagnostics=false`는 response 표시만 끄고 내부 quality 계산에는 영향을 주지 않는다.

### Rust TSX syntax gate

생성된 모든 TSX는 반환·Resource publish·파일 commit 전에 Rust parser로 다시 읽는다.

- workspace MSRV 1.88을 만족하는 `oxc_parser` 버전을 lockfile에 고정한다.
- TypeScript + JSX source type으로 parse하고 모든 syntax error를 수집한다.
- error가 하나라도 있으면 `DEVUP_CODEGEN_PROJECTION_FAILED`, `FidelityImpact::Failed`다.
- parser error에는 byte range와 정제된 parser message만 넣고 주변 design text는 로그에 남기지 않는다.
- 268개 golden과 WQUW-151 모든 TSX snapshot을 전수 parse한다.

이 gate는 TypeScript type checker가 아니다. DevupUI public prop type drift는 소비자 integration이 검증한다.

### Semantic fidelity report

codegen이 source map과 함께 `ProjectionTrace`를 만든다. validator는 generated TSX AST와 trace를 독립적으로 대조한다.

```text
FidelityReport
  syntax
  nodeCoverage
  textCoverage
  tokenCoverage
  typographyCoverage
  assetCoverage
  layoutCoverage
  diagnosticsByImpact
```

- visible source node가 emitted node, intentional flatten, ignored-with-reason 중 정확히 하나에 속해야 한다.
- text characters와 styled segment 순서/경계가 AST child 순서와 일치해야 한다.
- variable/style binding은 emitted token 또는 explicit resolved fallback provenance가 있어야 한다.
- typography는 style token 또는 font family/size/weight/line-height/letter-spacing fallback을 추적한다.
- image/vector/mask reference는 manifest capture 또는 placeholder diagnostic을 가져야 한다.
- layout property는 direct/normalized/inferred/fallback 중 하나로 source map에 분류한다.

`strict=true`는 syntax success와 모든 requested fidelity coverage 100%, failed/lossy 0을 요구한다. 브라우저 pixel 비교는 환경 의존이므로 server strict의 기본 조건에는 넣지 않는다. opt-in visual job은 별도 threshold를 강제한다.

### Visual reference와 comparator

새 `devup-mcp-visual` crate는 다음만 담당한다.

- PNG decode와 color-space normalize
- reference/actual viewport 크기 검증
- configurable per-channel threshold와 changed-pixel ratio
- anti-aliasing tolerance를 적용한 pixel diff
- opaque diff PNG와 content-free JSON metric report 생성

Figma screenshot은 existing read-only `get_screenshot` acquisition으로 요청하고 artifact-bound binary Resource 또는 confined output transaction으로만 전달한다. screenshot은 cache key나 telemetry에 넣지 않는다.

실제 renderer adapter contract는 다음과 같다.

```text
input: generated TSX/resource manifest, viewport, theme mode, asset directory
output: PNG path, renderer name/version, DevupUI version, font manifest
```

devup-mcp server는 임의 command를 실행하지 않는다. 소비자 프로젝트의 opt-in job이 자체 Bun/Next.js/DevupUI 환경에서 TSX를 build/type-check/render한 뒤 Rust comparator CLI를 호출한다. WQUW-151 대표 Frame은 실제 Figma PNG와 consumer-rendered PNG를 검증하는 live/visual job을 갖고, repository 기본 Cargo CI에는 credential과 screenshot binary를 저장하지 않는다. 결정적 comparator 자체는 synthetic PNG fixtures로 항상 테스트한다.

pixel threshold는 viewport 크기 불일치 0건, changed pixel ratio 0.5% 이하를 초기 기준으로 한다. font 미설치 또는 renderer/version 불일치는 visual failure가 아니라 `environment-invalid`로 분리해 false success를 막는다. threshold는 report에 기록하며 조용히 완화하지 않는다.

## 내부 모듈 경계

`crates/devup-mcp/src/server/mod.rs`는 router와 `ServerHandler`만 남긴다.

```text
server/
  mod.rs          tool router, ServerHandler, shared state wiring
  tools.rs        public request schemas and defaults
  handoff.rs      official MCP continuation sessions only
  artifacts.rs    acquisition/composite artifact + attached resources
  acquisition.rs  URL/artifact planning, Section index and batches
  projection.rs   TSX/theme/source-map/manifest orchestration
  validation.rs   capability, quality, TSX and fidelity gates
  delivery.rs     inline/resource/auto selection and MCP read/list
  output.rs       confined OutputPolicy and OutputTransaction
  quality.rs      typed quality aggregation
```

`devup-mcp-figma`는 Figma URL, collector, snapshot, SectionIndex와 merge까지만 소유한다. `devup-mcp-devup-ui`는 codegen, theme, source map, ProjectionTrace와 syntax/semantic validation을 소유한다. `devup-mcp-visual`은 Figma나 MCP에 의존하지 않고 byte image comparison만 소유한다.

분해는 기능 commit에서 이동하는 코드만 대상으로 하며 동작 없는 대규모 rewrite를 먼저 하지 않는다. 각 이동 후 public tool contract tests가 그대로 통과해야 한다.

## 오류와 상태 계약

- invalid asset reuse: `DEVUP_FIGMA_HANDOFF_INVALID`, upstream call 0, projection 0, write 0
- unsafe output path: `DEVUP_CODEGEN_FAILED`, projection 결과 publish 0, write 0
- Section selection invalid: `DEVUP_FIGMA_NODE_NOT_FOUND`, selected collection 0
- batch version drift/merge conflict: complete artifact를 만들지 않고 retryable consistency error
- inline hard limit 초과: resource/auto 사용 안내가 있는 non-retryable input error
- expired resource: MCP resource not found; URL 또는 artifact reacquire 필요
- TSX parse failure: projection failed, strict/non-strict 모두 깨진 TSX를 반환하거나 기록하지 않음
- visual mismatch: comparator failure report; acquisition/projection artifact를 삭제하지 않음

`status=complete`는 기존 네 quality axis와 semantic fidelity gate를 만족할 때만 가능하다. `selection_required`와 `needs_figma`는 계속 workflow state다.

## 테스트 매트릭스

### Asset와 artifact

- PNG 1x artifact를 PNG 1x로 재사용하면 Figma call 0
- PNG 1x를 SVG 1x, PNG 2x, 다른 ID로 요청하면 projection/write 전에 실패
- failed/pending export는 같은 tuple이어도 complete capture로 인정하지 않음
- 공개 capability JSON에 asset ID가 없음

### Filesystem

- relative path와 허용 root 내부 absolute path 성공
- `..`, sibling root, UNC/drive switch, directory target, duplicate target 거절
- Windows junction과 symlink를 통한 root 탈출 거절
- temp write, backup rename, target rename 각 fault injection에서 원본 복구
- strict/parse/capability 실패 시 생성 파일 0
- text + 여러 asset의 두 번째 commit 실패 시 runtime rollback
- stale internal temp만 정리하고 사용자 파일은 보존

### Section

- 선택 없는 Section은 compact index call 후 `selection_required`; full subtree call 0
- one/many/all selection의 visual order, naming과 중복 검증
- 여러 small root가 한 batch, oversized root만 continuation
- chunk 누락/중복/hash/version drift가 complete로 승격되지 않음
- composite resource dedupe와 root별 completeness
- 기존 full Section artifact reuse 호환
- WQUW-151 10 Frame 전체 text/children/token/typography/stroke snapshot 유지

### Resource delivery

- 256 KiB 경계와 1 MiB total 경계의 inline/auto/resource 결정
- text/blob manifest MIME, size, SHA-256와 chunk round-trip
- list pagination에는 manifest만 나타남
- invalid index, altered hash, expired/evicted artifact not found
- Resource Link response가 design content를 중복 inline하지 않음
- outputPath와 resource bytes hash 일치
- cache byte accounting과 LRU가 attached resources를 포함

### Fidelity와 quality

- 모든 checked-in TSX fixture parse 성공
- 잘못된 attribute/text fragment parse 실패 및 write/publish 0
- 새 approximated/lossy/failed diagnostic이 code 문자열 추가 없이 quality에 반영
- `includeDiagnostics=false`에서도 quality 동일
- theme conflict/unresolved, failed asset, malformed Explore critical field strict matrix
- text segment, token, typography, asset와 layout provenance coverage
- synthetic exact/changed/size-mismatch PNG comparator
- opt-in WQUW-151 Figma/consumer screenshot threshold와 environment-invalid 분리

### 전체 gate

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo insta test --workspace --all-features --check
cargo build --workspace --release
```

각 단계에서 268/268 plugin JSON golden과 WQUW-151 fixture를 다시 실행한다. Snapshot은 자동 accept하지 않고 의도한 diff만 검토한다.

## 구현 및 commit 순서

1. exact asset capture capability와 artifact reuse tests
2. confined `OutputPolicy`와 staged `OutputTransaction`
3. structured fidelity impact와 누락된 strict/quality matrix
4. SectionIndex, batch planner와 composite artifact
5. standard MCP Resource store, read/list/template와 delivery policy
6. Rust TSX parse gate와 semantic fidelity report
7. `devup-mcp-visual` comparator, Figma reference output와 adapter contract
8. server module decomposition 완료, README/changepack/install 문서
9. 전체 Cargo/golden/WQUW/live smoke 검증, focused commits push와 PR 갱신

각 번호는 red test → 최소 구현 → focused verification → commit 순서로 진행한다. 앞 단계가 green이 아니면 뒤 단계로 넘어가지 않는다.

## 보안과 개인정보

- 허용 root는 startup configuration이며 tool input으로 root 자체를 추가할 수 없다.
- temp/backup 이름은 cryptographically unpredictable하고 exclusive create한다.
- output transaction은 Figma/OAuth credential 파일을 특별 취급하는 대신 root confinement로 접근 자체를 막는다.
- resource URI는 process-local random/opaque ID만 사용하고 design 식별자를 포함하지 않는다.
- resources와 screenshots는 artifact TTL/LRU 안에서 memory-only이며 기본적으로 디스크에 남지 않는다.
- Resource read와 cache tracing은 URI hash, byte 수, status만 기록한다.
- Figma 접근은 read-only allowlist를 유지하고 mutation API를 추가하지 않는다.
- visual renderer는 server가 실행하지 않아 command injection과 consumer dependency 권한을 분리한다.
- repository fixture에는 token, OAuth credential, Figma 사용자 정보와 새 screenshot 원본을 추가하지 않는다.

## 호환성과 migration

- tool 이름과 기존 output key는 유지한다.
- `delivery=auto`에서 작은 output은 기존 inline shape다.
- 큰 inline에 의존한 client는 `delivery=inline`을 명시할 수 있지만 hard limit보다 크면 resource 또는 outputPath로 전환해야 한다.
- 현재 arbitrary absolute output path 동작은 의도적으로 폐기한다. server cwd 밖 기록은 `--allow-write-root`로 명시해야 한다.
- cache는 process-local이므로 artifact capability shape 변경에 persistent migration은 없다.
- `Diagnostic.fidelityImpact`, `cache.capabilities.assetCaptureCount`, fidelity/resource metadata는 additive다.

## 완료 기준

- asset artifact reuse에서 ID/format/scale 불일치가 성공할 수 없다.
- 허용 root 밖 기록과 symlink/junction 탈출이 불가능하고 runtime 실패 시 기존 파일을 복구한다.
- 선택 없는 Section은 전체 subtree를 수집하지 않는다.
- WQUW-151 10 Frame을 root별 호출이 아닌 bounded 최소 batch로 수집하고 기존 TSX parity를 유지한다.
- 큰 text/binary output은 표준 MCP Resource로 bounded하게 읽을 수 있다.
- 깨진 TSX와 provenance가 불완전한 strict output은 반환, publish, 기록되지 않는다.
- 기본 workspace와 CI는 Bun/Node 없이 Cargo만으로 build/test된다.
- 실제 consumer renderer를 연결한 opt-in visual job이 Figma reference와 정량 diff를 보고한다.
- 모든 관련 Cargo, snapshot, plugin corpus와 WQUW-151 gate가 통과한다.
- README, changepack, focused commit, push, PR과 설치 검증이 갱신된다.

## 남는 한계

- Figma official MCP 자체의 응답/호출 상한과 catalog 승인 실패는 제거할 수 없으며 direct OAuth와 host fallback 정책을 계속 따른다.
- 여러 directory의 파일을 한 filesystem transaction으로 crash-atomic하게 바꾸는 것은 보장하지 않는다. 정상 runtime failure rollback과 개별 파일 atomic replace까지만 보장한다.
- Cargo-only server만으로 React/DevupUI browser render를 재현할 수 없다. pixel fidelity는 실제 consumer renderer adapter가 연결된 환경에서만 증명된다.
- pixel 결과는 font, browser, OS rasterizer에 민감하므로 renderer/font/version metadata가 다르면 비교 자체를 유효 성공으로 보지 않는다.
