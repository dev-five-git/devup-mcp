# R8 검증 보고

## R8-1: 공개 오프셋 제거와 의미 기반 sourceMap

WQUW-120의 보존 응답을 직접 읽어 모달 `3997:46621`의 node `[7251,9033)`, type `[7254,7257)`, `resolution=exact`를 확인했다. 반환 TSX는 JS 문자 8309개 / UTF-8 9081바이트였다. type 범위를 문자로 읽으면 공백 세 개, 바이트로 읽으면 `k>\n`이었다. 실제 `<Box` 시작은 문자 6633 / 바이트 7279여서 어느 해석으로도 맞지 않았다.

코드 경로를 추적하면 생성기가 만든 코드와 map 뒤에 서버가 자산 파일명을 호스트용 경로로 바꾸는 단계가 있다. 이때 기존 범위는 갱신되지 않았다. 감독 변경 지시에 따라 오프셋을 수선하지 않고 공개 sourceMap v2에서 제거했다. 렌더러 검증에 필요한 내부 위치는 직렬화하지 않는다. 노드만 가리키던 항목도 제거하고 `nodeId`, 원본 `property`, `generatedProperty`, `resolution`을 보존한다. 테마의 JSON pointer 및 리소스 식별 정보는 유지한다.

`exact`는 원본 필드와 생성 속성의 매핑을 확인했다는 뜻이다. 픽셀 동일성이나 생성 코드 위치를 보증하지 않는다. 생성 속성을 확인할 수 없는 항목에는 `unverified-property-mapping`을 쓴다. 다음 네 근거는 오프셋 없이 모두 표현되고 각각 회귀 테스트로 확인됐다.

| resolution | 생성 근거 |
|---|---|
| verified-layout-sizing | `h="740px"`, 본문의 `flex="1"` |
| accounted-for-implicit-flex-stretch | `implicit:align-self:stretch` |
| accounted-for-implicit-flex-grow | `flex="1"` |
| restored-hug-after-mask-child-folding | `aspectRatio` 속성 |

노드 코드의 발췌는 기존 `projectionIssues.generatedSource`가 담당한다. 최종 자산 경로 치환을 TSX뿐 아니라 generatedProperty/generatedSource에도 적용해 반환 코드와 발췌가 일치하도록 보강했다. 한글 모달과 한글 자산 이름을 포함한 테스트가 이 계약을 검증한다. 기존 공개 오프셋 테스트는 속성 매핑 검증으로 전환했고 내부 fidelity 검증을 고의로 깨뜨리는 테스트는 유지했다.

실제 WQUW-120 캡처를 자체 생성기에 넣고 compact JSON UTF-8 크기를 측정했다. 기존 sourceMap 55,865바이트에서 48,832바이트로 **7,033바이트 / 12.6% 감소**했다. R8-2 반영 TSX는 9,099바이트이며 sourceMap은 **5.37배(약 5.4배)**다. 작아졌지만 여전히 큰 출력이다. 도구 설명과 README에 이 측정 대상·수치를 명시하고 줄 역추적 설명은 제거했다.

## R8-2: ABSOLUTE FIXED 높이

R6의 기존 고정 높이 보존 경로에서 derived padding이 있다는 이유로 ABSOLUTE 높이를 버리던 예외를 제거했다. FIXED `740`은 `h="740px"`로 보존하고 FILL/HUG는 픽셀 높이로 고정하지 않는다. CENTER/MIN 배치, translate, padding을 유지하는 회귀 테스트가 통과했다. 기존 ABSOLUTE 제한 진단은 유지하며 `heightPreservation`에 원본 sizing/height, 보존 상태와 근거를 추가했다. sourceMap의 height 매핑도 검증했다.

## R8-3: 장기 수집의 진행 상태와 측정

자산이 없는 코드 export도 기존 작업 실행 경로를 사용한다. 최초 응답은 약 1초 안에 `exportJob.jobId`와 상태를 반환하고 이후 상태 조회/재개를 지원한다. 호출별 프레임/root IDs, pagination, 경과 시간과 projection-and-write 단계를 기록한다. 기존 assetJob 필드는 호환성을 위해 남긴다.

시간 안내는 출력 단위당 5–20초의 계획 추정이며 보장 시간이 아니다. 3프레임/9단위는 45–180초로 안내하고 upstream 지연 시 프레임별 분할을 권한다. 실제 upstream 시도에만 90초 제한을 걸어 rate-limit 대기와 기존 3회 재시도를 방해하지 않는다. 활성 작업 최대 8개와 완료 결과 캐시 최대 16개를 분리했다. 완료 결과는 5분, 작업 checkpoint는 프로세스 내 30분 보존이다.

로컬 저장 캡처 투영은 모달 약 64.5ms, 별도 3프레임/9단위 약 242.3ms였다. 후자는 저장돼 있던 `46315/46715/46333`이며 당시 타임아웃 대상 `46761/46799/46845`와 다르다. 당시 오류 전문에는 수집 단계별 시간이 없어 과거 300초의 원인은 확정할 수 없다. 자체 target 실행 파일로 원본 119 요청을 추가 실행한 결과는 아래 검증 기록에 남긴다.

## R8-4: 만료 복구 값

최대 64개의 작은 복구 기록에 canonical URL과 선택을 보존한다. 만료/퇴출된 항목은 실제 URL과 선택을 `nextAction`에 넣고, 기록이 없는 항목은 `unrecoverable`/unknown으로 구분한다. 모르는 원인을 서버 재시작이라고 단정하지 않는다. branch URL의 `/branch/{file}/{branch}` 및 선택 round-trip도 테스트했다.

## R8-5: 스크롤 캡처

118의 raw payload에 overflowDirection은 없었고 기존 plugin manifest에도 누락돼 있었다. 캡처 누락이 확인됐으며 생성기 역시 이 값을 처리하지 않았다. manifest와 fast/fallback 캡처, 생성기 모두 수정했다. 실제 캡처 JavaScript를 실행해 값, NONE, 원본 필드 없음(null), getter 실패(fieldErrors)를 검증한다. 과거 캡처의 키 부재는 새 캡처의 명시적 null과 구별된다.

`clipsContent=true`는 스크롤 여부가 아니다. VERTICAL은 `overflowY="auto"`, HORIZONTAL은 `overflowX="auto"`를 쓰며 클립할 때 반대 축을 hidden으로 한다. 양방향은 `overflow="auto"`, NONE의 클립 프레임은 기존 hidden이다. 원본 정보 없이 스크롤 의도를 만들어 넣지 않는다.

## 회귀 및 golden

수정 전에 공개 generatedRange 잔존, ABSOLUTE 높이 누락, 세로 스크롤 hidden, 실제 capture 스크롤 누락, 만료 URL 누락, 느린 코드 export의 3초 블로킹을 각각 실패로 확인했다. 리뷰에서 발견한 완료 작업의 용량 소모, branch URL, 비SECTION 자산 경로도 실패 테스트를 추가한 뒤 고쳤다. 재시도 테스트의 기존 3회 시도 검증을 유지했다.

golden 변경은 TSX 6개와 sourceMap 1개다. 각 TSX의 의미 변경은 ABSOLUTE FIXED 높이뿐이다.

| snapshot | 변경 이유 |
|---|---|
| wquw_151_frames__wquw_151_frame_3879_35569 | FIXED overlay 1개의 h=740 보존 |
| wquw_151_frames__wquw_151_frame_3879_35652 | FIXED overlay 1개의 h=740 보존 |
| wquw_151_frames__wquw_151_frame_3879_35729 | FIXED overlay 1개의 h=740 보존 |
| wquw_151_frames__wquw_151_frame_3879_35887 | FIXED overlay 2개의 h=740 보존 |
| wquw_151_frames__wquw_151_frame_3879_35973 | FIXED overlay 2개의 h=740 보존 |
| wquw_151_frames__wquw_151_frame_3879_36059 | FIXED overlay 1개의 h=740 보존 |
| wquw_151__wquw_151_proofread_source_map | v2 의미 매핑 전환: 범위/노드 전용 항목 제거, generatedProperty 추가 |

외부 기록공간 파일은 읽기만 했다. 설치된 MCP 도구는 검증에 사용하지 않았다. `CARGO_TARGET_DIR`을 설정하지 않았으며 이 워크트리의 `target/debug/deps/devup_mcp-e7c3ff09bf3cef75.exe` 경로를 테스트 안에서 출력해 확인했다. 자기 target에서 검증했고 실행된 테스트가 내 것임을 확인했다.

## 최종 검증 기록

- `cargo test --workspace -j 2 --no-fail-fast`: **739 passed / 0 failed / 2 ignored** (기준선 대비 12개 추가).
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: exit 0, 경고 0.
- `cargo fmt --all -- --check`: exit 0.
- `git diff --check`: exit 0, `.snap.new` 없음.
- 독립 읽기 전용 리뷰에서 지적된 세 항목을 회귀 테스트로 고친 후 재검토 완료.

자체 `target/debug/devup-mcp.exe`의 stdio MCP에 119의 원본 인자를 그대로 전달했다. 응답 identity는 `0.4.4`, `buildId=b34b69e19ddf-dirty`였다. 첫 응답 **1.016초**, 관측 **200.016초**, 완료된 snapshot 호출 **32개**이며 마지막 상태는 running/snapshot이었다. 검증기가 200초 관측 후 자기 프로세스를 종료했으므로 이 실호출의 최종 완료를 주장하지 않는다. [측정 JSON](r8-live-measurement.json)에 단계별 기록을 보존했다.

긴 구간은 `46799` offset 17의 **64,382ms**, `46761` offset 39의 **52,764ms**, `46845` offset 47의 **53,958ms**였다. 기록 시간은 호출 전 pacer·재시도·upstream 획득을 포함한다. 기존 pacer의 기본값은 분당 8회이고 32회의 페이지 수집에는 여러 분이 걸릴 수 있다. 따라서 frame-output units만으로 완료 시간을 보장할 수 없으며, 현재 실측의 지연 구간은 페이지 수집/대기다. 과거 300초 사건의 정확한 원인이나 각 구간의 순수 Figma 실행 시간은 이 기록만으로 확정하지 않는다.

커밋 후 이 워크트리의 `cargo clean`으로 target을 반납한다. push는 하지 않는다.
