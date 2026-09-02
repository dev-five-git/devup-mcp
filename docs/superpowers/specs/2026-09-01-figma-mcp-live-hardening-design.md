# Figma MCP Live Hardening Design

## 상태와 범위

- 작성일: 2026-09-01
- 대상 저장소: `dev-five-git/devup-mcp`
- 선행 설계: `2026-08-31-figma-host-fallback-parity-design.md`
- 기준 실사용: Girok WQUW-151, file `85CgSws3o5XsLv7aAwWJyS`
- 범위: Figma MCP 탐색, host handoff, 실행 진단, DevupUI fidelity 판정
- 범위 밖: 기록공간 애플리케이션, Figma 쓰기 기능, 신규 외부 서비스

## 목표

실제 Figma 문서 구조가 fixture 캡처 이후 바뀌더라도 사용자가 전달한 요구사항 제목
또는 중첩 노드 링크에서 관련 화면을 한 번의 공식 Figma 호출로 찾는다. 대용량
`use_figma` 결과를 이어받는 host handoff는 잘못된 continuation 하나 때문에 세션
전체를 잃지 않으며, 만료·중복·알 수 없는 호출을 서로 구분한다. 설치된 stdio 서버가
종료됐을 때는 바이너리와 host 연결 중 어느 경계가 실패했는지 안전하게 진단한다.
생성 결과의 품질 상태는 fallback diagnostic과 모순되지 않아야 한다.

## 확인된 결함

### 중첩 제목 탐색

WQUW-151의 제목 노드 `3879:35481`은 과거 fixture에서는 PAGE 직속이었지만 현재
문서에서는 SECTION `4217:7743`의 자식이다. 현재 Plugin API projection은
`anchorPeer`로 SECTION을 보존하지만 SECTION 하위 screen traversal은
`anchor.type === "SECTION"`일 때만 수행한다. 제목 링크의 실사용 결과는 후보
0개이고 SECTION 링크는 정확한 후보 10개다.

### handoff 세션 유실

`HandoffStore::accept`는 세션을 map에서 제거한 다음 call ID를 검사한다. call ID가
없거나 collector가 결과를 거부하면 세션을 다시 넣지 않고 반환한다. 이후 올바른
continuation도 존재하지 않는 session 오류가 된다. 현재 오류 메시지는 unknown과
already-consumed도 구분하지 않는다.

### transport 종료 진단

설치 바이너리를 직접 시작한 stdio initialize와 tool call은 성공했지만 이미 등록된
connector 호출은 `Transport closed`였다. 프로세스가 시작되기 전의 launch 실패,
initialize 전 종료, 정상 initialize 이후 종료를 서버 밖에서 자동 복구할 수는 없다.
따라서 제품이 제어할 수 있는 경계에서는 시작 정보와 stderr 안전성 계약을 강화하고,
설치 문서와 smoke test가 host 재연결 필요 여부를 판별하게 해야 한다. 서버가 sibling
MCP나 host 프로세스를 임의로 재시작하는 기능은 추가하지 않는다.

### fidelity 상태 불일치

WQUW-151 TSX에는 `DEVUP_CODEGEN_ABSOLUTE_FALLBACK` 두 건이 있으나 개별
`FidelityReport::strict_compatible`은 approximated impact를 허용한다. 상위
`OutputQuality`는 diagnostic을 보고 partial로 판정하므로 두 품질 계약이 서로 다른
답을 낸다.

## 설계

### 1. 탐색 기준 노드 승격

읽기 전용 `explore.js`는 세 노드를 명시적으로 구분한다.

- `requestedAnchor`: 사용자가 링크한 원래 노드
- `scopeAnchor`: 탐색 범위를 결정하는 가장 가까운 SECTION 또는 PAGE 직속 peer
- `page`: 한 번만 활성화하는 소속 PAGE

`requestedAnchor`가 SECTION 내부에 있고 screen 자체가 아니라면 가장 가까운 SECTION을
`scopeAnchor`로 사용한다. SECTION traversal은 `scopeAnchor.type`을 기준으로
실행한다. projection에는 page, scopeAnchor, requestedAnchor, 필요한 ancestor chain과
최상위 screen 후보만 포함한다. 결과의 public anchor와 heading node ID는 원래
`requestedAnchor`를 유지하고, 후보 판정만 scopeAnchor 아래에서 수행한다.

Rust `explore_snapshot`은 원래 anchor의 ancestor chain에 SECTION이 있으면 그 SECTION
하위 screen을 선택한다. SECTION이 projection에 있으나 자식 연결이 축약된 경우에도
`parentId` chain으로 descendant를 판정한다. 기존 PAGE 직속 heading 동작은 유지한다.

완료 조건:

- 현재 WQUW 구조를 재현한 fixture에서 제목 링크로 10개 화면을 찾는다.
- SECTION 링크와 제목 링크의 candidate ID/순서는 동일하다.
- screen 링크는 exact screen 하나만 반환한다.
- projection JSON 상한 14,000자는 유지한다.

### 2. 손실 없는 handoff 상태 전이

`HandoffStore`는 세션을 꺼낸 뒤 검증하는 방식 대신 lock 안에서 먼저 session과 call
상태를 확인한다. 결과 크기 검증 후 pending call을 찾고, collector state 변경이
성공했을 때만 pending call을 consumed로 이동한다.

세션은 다음 call 상태를 유지한다.

- `pending`: 아직 결과를 받지 않음
- `consumed`: 이미 정상 처리됨

짧은 tombstone map은 제거된 session의 종료 이유와 만료 시각만 TTL 동안 보존한다.
원본 결과, Figma 내용, call arguments는 tombstone에 저장하지 않는다.

오류는 다음처럼 구분한다.

- 존재하지 않은 session/call: `DEVUP_FIGMA_HANDOFF_INVALID`, retryable false
- 이미 처리한 call: `DEVUP_FIGMA_HANDOFF_INVALID`, reason `consumed`
- 만료 session: `DEVUP_FIGMA_HANDOFF_EXPIRED`, retryable true
- collector가 거부한 result: 원래 collector 오류를 반환하되 session/call은 pending 유지

유효한 result를 처리할 때 `expires_at`을 현재 시각 + TTL로 연장한다. 단순 `next`
조회는 lease를 무한 연장하지 않는다. session/result/전체 메모리 상한은 기존 값을
유지하고 tombstone에는 별도의 작은 개수 상한을 둔다.

완료 조건:

- 잘못된 call ID 이후 올바른 call ID로 계속할 수 있다.
- malformed result 이후 수정된 result로 계속할 수 있다.
- 동일 call 재전송은 consumed로 판정된다.
- 대용량 3청크 결과의 순차 continuation 동안 lease가 연장된다.
- 만료와 unknown 오류 code/details가 다르다.

### 3. stdio 실행 및 설치 진단

MCP stdout은 계속 protocol frame 전용으로 유지한다. `--version`은 package version과
빌드 식별자를 출력하고, 새 `--self-check` CLI는 다음 로컬 조건만 검사한다.

- 실행 파일을 시작할 수 있음
- credential backend 초기화 가능 여부
- stdout 오염 없이 initialize 가능한 server 구성

`--self-check`는 OAuth를 시작하거나 Figma/network를 호출하지 않는다. 결과는 안전한
JSON이며 secret과 사용자 경로를 포함하지 않는다. README 설치 절차는 바이너리 hash나
버전 변경 후 MCP host 연결 재시작이 필요하다는 점과 `--self-check` 판별 순서를
명시한다.

CI는 child process에 initialize → tools/list → auth status를 보내 정상 종료시키는 stdio
smoke test를 실행한다. 이는 host 자체의 stale transport를 재시작하지 않지만
"바이너리 실패"와 "host가 죽은 이전 transport를 보유"한 상황을 분리한다.

### 4. fidelity 단일 진실 공급원

`FidelityReport::strict_compatible`은 approximated, lossy, failed impact가 모두 0일
때만 true다. 정확한 position props가 생성돼 fallback이 필요 없는 absolute node에는
diagnostic을 만들지 않는다. 판단 기준은 node의 absolute 좌표와 크기를 부모 기준
position props로 모두 표현하고 source-map layout coverage가 이를 확인하는지 여부다.

WQUW-151의 TIP 닫기 버튼과 하단 controller는 좌표·크기·고정 edge가 모두 생성되므로
generic fallback diagnostic을 제거할 수 있는지 테스트로 판정한다. 표현되지 않은
constraint나 transform이 있으면 diagnostic을 유지하고 결과 status는 partial이어야
한다. diagnostic을 숨겨 complete로 만드는 것은 허용하지 않는다.

완료 조건:

- approximated diagnostic이 하나라도 있으면 report와 output status 모두 strict
  incompatible/partial이다.
- 완전히 표현된 absolute node는 exact이며 diagnostic이 없다.
- WQUW snapshot의 status와 diagnostics가 일치한다.

## 테스트 전략

모든 production 변경은 실패하는 회귀 테스트를 먼저 추가한다.

1. 최신 WQUW parent chain을 합성 fixture로 재현하는 Rust explore test와 실제
   `explore.js` behavior test
2. invalid call, malformed result, consumed call, expiry tombstone, lease renewal handoff test
3. `--self-check`와 stdio initialize/tools/auth smoke test
4. absolute layout exact/approximated 분기와 WQUW fidelity snapshot test
5. `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, release build

실제 Figma live 검증은 원본 payload를 저장하지 않고 다음 수치와 ID만 확인한다.

- 제목 링크와 SECTION 링크가 동일한 10개 candidate를 반환
- target `3879:35518`이 144 nodes, 20 variables, 11 styles로 수집
- handoff가 3개 PNG envelope chunk를 모두 소비하고 complete

## 보안과 호환성

- Figma write tool과 사용자 제공 JavaScript는 계속 허용하지 않는다.
- tombstone, self-check, tracing에 원본 디자인이나 credential을 기록하지 않는다.
- 기존 tool input/output 필드는 삭제하거나 이름을 바꾸지 않는다.
- 새 오류 details와 CLI action은 additive change다.
- 기존 direct/host source 정책과 10분 TTL 기본값은 유지한다.

## 범위 제외와 한계

MCP 서버는 이미 종료된 자신과 host 사이의 stdio pipe를 스스로 되살릴 수 없다.
따라서 host process 재시작은 문서화된 운영 단계이며 제품 내부 retry로 가장하지 않는다.
이번 변경은 서버가 살아 있을 때의 state loss를 제거하고, 죽은 경계를 명확히 판별하는
데까지를 완료 조건으로 삼는다.
