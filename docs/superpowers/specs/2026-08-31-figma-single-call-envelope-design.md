# Figma 단일 호출 무손실 수집과 DevupUI 출력 호환성 설계

## 상태와 기준선

- 작성일: 2026-08-31
- 상태: 사용자 승인
- 승인된 접근: A안 — 단일 호출 binary envelope와 검증 실패 시 기존 수집기 자동 fallback
- 구현 저장소: `dev-five-git/devup-mcp`
- 기준 JavaScript 저장소: `dev-five-git/devup-figma-plugin`
- 기준 upstream commit: `243db650f1d635ab5385546a2a297eae4ea93515`
- 실제 회귀 대상: file `85CgSws3o5XsLv7aAwWJyS`, node `3879:35518`

현재 `devup_figma_to_ui`의 정확한 WQUW-151 화면 수집은 metadata 1회,
약 20개의 text snapshot page, variable 2 batch, style 2 batch로 대략 25개의
공식 Figma tool call을 사용한다. 수집 결과 자체는 children, mixed text,
variable 및 text style 이름을 복원하지만, 응답 크기를 맞추기 위한 cursor와
field truncation 때문에 호출 수가 크고 완전성 판단도 여러 응답에 분산된다.

공식 Figma Plugin API runtime을 읽기 전용으로 조사한 결과
`CompressionStream`, `TextEncoder`, `Blob`, `Response`는 제공되지 않는다.
따라서 브라우저 gzip에 의존하는 설계는 사용할 수 없다. 반면
`figma.io.write`로 반환한 PNG의 custom ancillary chunk는 공식 MCP의
`image/png` content block에서 byte 단위로 보존됨을 실제 marker round-trip으로
확인했다.

## 목표

1. 정확한 node 링크의 전체 subtree와 실제 사용 variable/style을 정상 경로에서
   공식 Figma tool call 1회로 수집한다.
2. 현재 snapshot이 보존하는 모든 public data field, unknown enumerable field,
   getter error, styled text segment와 resource binding을 유지한다.
3. binary transport를 신뢰하기 전에 형식, 크기, CRC, root, node 수 및 resource
   참조를 Rust에서 검증한다.
4. fast path가 지원되지 않거나 단 하나라도 검증에 실패하면 partial 결과를 쓰지
   않고 검증된 legacy cursor collector로 자동 fallback한다.
5. JavaScript 플러그인 복사 결과와 MCP 결과의 의도된 차이를 명시하고 실제 변환
   오류인 mixed individual stroke 누락을 수정한다.
6. synthetic snapshot corpus뿐 아니라 실제 Figma proxy/runtime 차이를 여러 URL의
   read-only live regression으로 보강할 수 있는 입력 계약을 마련한다.

## 비목표

- Figma 파일, node, variable, style 또는 plugin data를 수정하지 않는다.
- 사용자 JavaScript를 upstream으로 전달하지 않는다. 실행 코드는 계속 Rust
  binary에 compile-in된 enum variant로만 선택한다.
- OAuth token, header, callback parameter, 사용자 email/handle을 envelope,
  fixture, diagnostic 또는 log에 기록하지 않는다.
- binary envelope를 downstream 사용자에게 이미지 결과로 노출하지 않는다.
- 모든 978개 upstream JavaScript 테스트를 Rust에서 동일한 테스트 프레임워크로
  재실행한다고 과장하지 않는다.

## 고려한 접근

### A. PNG data envelope + legacy fallback — 채택

한 `use_figma` script가 subtree, styled segments와 사용 resource를 모두 읽고
UTF-8 JSON envelope를 만든다. 1×1 PNG의 반복 가능한 private ancillary chunk에
payload를 넣어 `figma.io.write`로 반환한다. Rust는 image content block을 직접
검증·해제한다.

장점은 일반 화면을 한 번에 무손실 수집하고, 공식 MCP의 text 응답 한도에
의존하지 않는다는 점이다. PNG는 여기서 화면 캡처가 아니라 공식 `io.write`
data 반환 채널의 framing 형식이다.

### B. text page 크기 확대와 resource batch 결합

기존 12KB page를 16KB로 늘리고 variable/style을 한 batch로 합치면 약 25회를
15~17회로 줄일 수 있다. 변경 위험은 낮지만 화면 크기에 비례해 호출 수가 계속
늘고, field truncation 문제를 해결하지 못한다. A안의 최종 fallback 최적화로만
사용한다.

### C. codegen 전용 field projection

현재 TSX에 필요한 field만 반환하면 1~2회 호출도 가능하지만 새 Figma field와
향후 변환 기능을 잃는다. “모든 field를 먼저 보존한다”는 제품 방향과 충돌하므로
채택하지 않는다.

## Fast path 수집 흐름

~~~text
exact Figma URL
  -> Rust URL parser가 fileKey/nodeId 확정
  -> use_figma(FastSnapshotEnvelope) 1회
       -> getNodeByIdAsync(nodeId)
       -> BFS subtree snapshot + styled text segments
       -> snapshot 전체에서 variable/style ID 추출
       -> Promise.all로 exact ID resource 조회
       -> JSON envelope UTF-8 encode
       -> PNG private chunks에 저장
       -> figma.io.write
  -> Rust PNG/envelope 검증
  -> CollectedParts로 정규화
  -> 기존 DevupUI codegen
~~~

정확한 node ID가 이미 있으므로 별도 `get_metadata`는 호출하지 않는다. 존재 여부,
node type과 child completeness는 fast snapshot 자체에서 검증한다. Jira 링크처럼
heading을 가리키면 기존 `devup_figma_explore` 1회 후 선택된 화면의 fast snapshot
1회가 필요하므로 전체 흐름은 2회다.

## Envelope와 PNG framing

envelope schema version 1은 다음 논리 값을 가진다.

~~~json
{
  "schemaVersion": 1,
  "source": {
    "fileKey": "...",
    "rootId": "..."
  },
  "snapshot": {
    "fileKey": "...",
    "version": null,
    "rootIds": ["..."],
    "nodes": [],
    "diagnostics": []
  },
  "resources": {
    "collections": [],
    "variables": [],
    "styles": [],
    "usedRemoteVariables": [],
    "localComplete": false,
    "usedRemoteComplete": true,
    "unresolved": []
  },
  "integrity": {
    "nodeCount": 0,
    "variableRefCount": 0,
    "styleRefCount": 0,
    "utf8Bytes": 0
  }
}
~~~

- UTF-8 encoder는 surrogate pair와 잘못된 lone surrogate를 명시적으로 처리하는
  compile-in JavaScript 구현을 사용한다.
- PNG signature, 고정 1×1 IHDR/IDAT, IEND 사이에 `duVp` private ancillary chunk를
  둔다. 큰 payload는 순서 번호와 총 chunk 수를 가진 여러 chunk로 나눈다.
- 각 PNG chunk CRC32를 Rust가 재계산한다. 누락, 중복, 순서 오류, CRC 오류는 모두
  fast path 실패다.
- Rust는 base64 decode 후 최대 raw PNG와 최대 envelope 크기를 검사한 다음 JSON을
  decode한다. 기본 상한은 기존 handoff 16MiB JSON 한도 안에서 base64 overhead를
  고려해 정한다.
- text content block에는 schema, node/resource 수, payload byte 수와 chunk 수만
  담고 디자인 text나 credential은 반복하지 않는다. Rust는 이 descriptor와 binary
  envelope 값을 교차 검증한다.
- image content block은 수집 내부에서 소비하고 최종 MCP 응답에는 포함하지 않는다.

## Snapshot 완전성과 resource 수집

fast script는 현재 `snapshot.js`의 serializer를 공유하되 정상 fast path에서는
text 응답용 `maxFieldBytes`/`maxNodeBytes` 축소를 적용하지 않는다. function,
symbol, node reference, circular object와 binary value는 기존의 명시적 marker로
표현한다. getter 실패는 `fieldErrors`에 보존한다.

public Plugin API manifest와 runtime enumerable/prototype field를 합쳐 읽는다.
`strokeWeight === figma.mixed`인 frame/rectangle은
`strokeTopWeight`, `strokeRightWeight`, `strokeBottomWeight`,
`strokeLeftWeight`를 명시적으로 읽는다. manifest에 새 public data property가
추가되면 기존 d.ts coverage test가 실패해야 한다.

snapshot을 완성한 뒤 serialized value 전체를 재귀적으로 검사해 variable ID와
style ID를 정렬·중복 제거한다. `getVariableByIdAsync`와 `getStyleByIdAsync`는 하나의
`Promise.all` fan-out에서 실행하고 결과를 같은 envelope에 포함한다. unresolved
ID는 누락시키지 않고 kind, ID와 안전한 error classification을 기록한다.

## 실패와 fallback

다음 조건 중 하나라도 만족하면 fast 결과 전체를 폐기하고 legacy collector를
처음부터 실행한다.

- `use_figma`, `figma.io.write` 또는 image content capability가 없음
- image block 없음, MIME 불일치, base64/PNG/envelope parse 실패
- size limit, chunk sequence, CRC 또는 descriptor 교차 검증 실패
- fileKey/rootId/rootIds 불일치
- declared node 수와 실제 map 수 불일치
- root가 없거나 child ID가 snapshot map에 없음
- resource reference가 결과 또는 unresolved 목록 어디에도 없음
- unsupported schema version

fallback은 기존 cursor semantics를 유지하되 variable/style ID를 가능한 한 하나의
combined resource batch로 합치고 page payload를 안전한 최대치까지 사용한다.
legacy truncation이 발생하면 기존처럼 diagnostic과 partial completeness를 반환하며
완전한 결과로 표시하지 않는다. fast 실패 원인은 secret이나 전체 payload 없이
collection stats/diagnostic으로 남긴다.

## DevupUI 출력 호환성

### 실제 수정 대상

WQUW-151 하단 `Header` node `3879:35564`는 live Plugin API에서 다음 값이다.

- `strokeWeight === figma.mixed`
- `strokeTopWeight = 1`
- `strokeRightWeight = 0`
- `strokeBottomWeight = 0`
- `strokeLeftWeight = 0`

따라서 올바른 출력은 `borderTop="solid 1px $border"`이다. mixed weight를 알 수 없는
일반 `1px border`로 fallback하는 현재 동작은 제거한다. 개별 면 값이 정말
수집되지 않은 경우 모든 면 border를 추측하지 않고 diagnostic과 raw-safe fallback을
사용한다.

### 의도적으로 유지하는 차이

- MCP는 import와 named component wrapper를 포함하고 Figma Code panel은 JSX
  fragment만 반환한다.
- 기본 `standalone` root layout은 Figma frame의 `w="360px"`, `h="740px"`와
  absolute child의 containing block인 `pos="relative"`를 보존한다.
- 새 `embedded` root layout은 선택 root의 고정 width/height/position만 생략해
  Figma Code panel fragment와 동일한 embedding semantics를 제공한다.
- formatter의 indentation과 긴 text line wrapping은 의미 차이로 취급하지 않는다.

기본값은 기존 사용자의 렌더링을 깨지 않는 `standalone`이다.

## JavaScript plugin fixture 감사

현재 upstream `main` SHA와 local pinned SHA는 모두
`243db650f1d635ab5385546a2a297eae4ea93515`다.

- source test file: 54
- upstream passing test: 978
- JSON input/golden snapshot: 268/268
  - codegen 260
  - responsive 5
  - render/snapshot 3
- ledger entry: 978
  - `rust_snapshot` 252 entries가 268개 fixture를 생성
  - `rust_assertion` 550
  - `contract` 137
  - `upstream_runtime_only` 21
  - `out_of_scope_write` 18

즉 upstream의 모든 snapshot-producing fixture는 JSON과 golden으로 수집되어 현재
byte parity를 통과한다. 그러나 모든 978개 JavaScript test가 독립 실행 가능한
Rust fixture로 이식된 것은 아니다. 특히 550개 `rust_assertion` ledger가 가리키는
`compat_fixtures::upstream_ledger_mapping` 이름은 현재 실제 테스트 symbol이 아니므로
“전체 테스트 parity”의 근거로 사용할 수 없다.

이번 작업은 다음을 함께 수정한다.

1. 문서 표현을 “268개 snapshot parity + 978개 test inventory”로 정확히 고친다.
2. ledger의 `rustTest`가 명시적 Rust coverage registry에 존재하는지 검사한다.
3. TSX 출력에 직접 영향을 주는 assertion은 실제 Rust unit/fixture test로 연결한다.
4. upstream runtime 또는 write-only test는 이유가 있는 분류로 유지하되 snapshot
   parity 수치에 포함하지 않는다.

## 추가 URL과 기대 결과 수집

사용자가 제공하는 여러 실제 URL과 Figma Code panel 결과는 synthetic fixture가
잡지 못하는 proxy getter, library instance, remote variable, mixed text, individual
stroke와 대형 subtree 회귀를 검증하므로 가치가 크다. 구현을 시작하기 위한
선행조건은 아니며 다음 형식이면 자동화하기 가장 좋다.

- node가 포함된 Figma URL
- 같은 selection에서 Figma가 복사한 TSX 또는 기대하는 핵심 부분
- `standalone`/`embedded` 중 비교할 root mode
- 비공개 text를 fixture에 보존해도 되는지 여부

각 URL은 read-only로 수집하고 개인정보·credential을 검사한 뒤, 공개 저장소에
넣기 부적절한 실제 문구는 구조를 유지한 synthetic text로 sanitize한다. 원문을
fixture에 커밋할 필요가 없으면 CI secret 기반 opt-in live contract로만 유지한다.

## TDD와 검증 기준

### Binary transport

- valid multi-chunk PNG envelope round-trip
- bad signature, invalid length, missing/duplicate/out-of-order chunk, CRC mismatch
- invalid UTF-8/JSON/schema, oversized image/envelope
- image 없는 text-only 응답이 legacy fallback을 시작함
- fast/legacy가 동일한 `CollectedPayload`와 TSX를 생성함

### Collector

- exact `to_ui` 정상 경로가 metadata/resource 추가 호출 없이 1 call로 완료됨
- heading explore 후 exact 변환이 총 2 call임
- capability/parse/integrity failure가 partial 값을 쓰지 않고 cursor 0부터 fallback함
- fallback variable/style combined batch와 stable ordering
- direct 및 host handoff가 동일한 state machine과 결과를 사용함

### Snapshot/codegen

- mixed individual stroke mock가 `borderTop`만 생성함
- individual stroke 정보가 없을 때 all-side border를 추측하지 않음
- WQUW-151 fixture가 144 node, 13 variable, 11 text style을 유지함
- WQUW-151 TSX가 실제 Figma 복사본의 children/token/typography와 의미상 같고
  `borderTop`을 출력함
- standalone root와 embedded root snapshot

### Compatibility와 전체 검증

- 268/268 upstream JSON golden parity
- 978-entry inventory와 실제 coverage registry 정합성
- crate별 focused test, workspace fmt/clippy/test/build
- live WQUW-151 one-call contract와 automatic fallback smoke test
- changepack, focused commit, push, 기존 PR 업데이트 및 Codex binary 재설치

## 관측성, 보안과 개인정보

최종 결과에 `figmaToolCalls`, `transport`, `fallbackUsed`, `nodeCount`,
`variableCount`, `styleCount`, `rawBytes`, `wireBytes`를 포함한다. 디자인 원문,
variable 값, OAuth token과 account identifier는 stats/log에 넣지 않는다.

PNG/envelope parser는 untrusted upstream input으로 취급해 크기 상한, checked integer
연산, CRC와 schema 검증을 먼저 수행한다. 실패는 panic 없이 stable error 또는
legacy fallback으로 처리한다. 모든 embedded JavaScript는 read-only API만 사용하며
Figma document mutation API가 포함되지 않는지 source contract test로 검사한다.
