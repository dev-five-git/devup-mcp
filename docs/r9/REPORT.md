# R9 approximation precision

## 근거와 재현

- base: `54c6a12c48af10ddc4264b6df3100aeb9abdb1e7` (`integration/r6`, R6+R7+R8).
- 지정된 다른 저장소의 보고서와 JSON은 읽기만 했다. 원본 SHA256은 `input-evidence.json`.
- WQUW-120 fresh JSON의 server.commit은 `54c6a12c48af`. 원본 모달 3997:46621은 CENTER/MIN, FIXED 360×740, 로컬 x/y=0. 생성 h=740px와 raw-fallback height 매핑을 확인했다.
- 지정된 WQUW-118 폴더의 STOP-REPORT/reader.json은 `b34b69e19ddf`를 기록한다. 이 파일을 fresh R8 캡처라고 바꾸어 보고하지 않는다. 진단 originalValue에는 absoluteRenderBounds:null이지만 **rawSnapshot에는 키가 없다**. 따라서 이 구형 캡처의 비렌더를 확인된 것으로 승인하지 않는다.
- `fixtures/r9/non-rendering-node.json`은 118 rawSnapshot에서 3997:46703의 필드를 변경하지 않고 추출한 fixture다. fileKey와 단일 root/node, 빈 snapshot diagnostics만 유지했다. 테스트가 별도로 null을 삽입한 경우만 성공적으로 명시적 null이 수집된 상태를 모델링한다.
- `red.txt`: 구현 전에 새 회귀 8개 모두 실패. 테스트 fixture의 필수 diagnostics 누락을 먼저 교정한 뒤, 실제 동작 차이로 실패한 실행을 기록했다.

## 변경

1. 비렌더 진단의 `verification`에 판정 필드, 키 존재, 원본 값, 읽기 오류를 기록한다. 확인된 visible=false/null만 accounted-for 및 impact none. 키 부재/읽기 실패는 unverified 및 approximated.
2. ABSOLUTE 진단을 height/width/horizontal/vertical로 구분한다. 원본 FIXED 수치와 생성 px를 비교하며, CENTER/MIN은 생성된 즉시 부모·position·offset·translate·원본 중심 관계를 검사한다. 미검증 MAX/STRETCH/SCALE과 percentage/intrinsic sizing에는 항목별 해소 조건을 남긴다. 원본 모달은 height preserved, horizontal/vertical verified, width approximated이다.
3. README 및 서버 사용 안내에 sourceMap 기본 필드와 선택적 variableId/styleId/assetId를 문서화했다. 실제 수치와 같은 px 매핑은 verified-explicit-dimension으로 표시한다. raw-fallback은 raw 값으로 생성했다는 출처 구분이며 곧바로 부정확성을 뜻하지 않는다.
4. 기존 R8 heightPreservation 및 내부 sizing 검증을 유지한다. 공개 sourceMap v2 오프셋은 되살리지 않는다. generated geometry는 브라우저 픽셀/반응형 화면 전체 검증이 아니다.

## 검증

- 추가 실패 재현: `red-capture.txt` (수집기 null 생략), `red-boundaries.txt` (숨긴 루트/coverage), `red-revalidation.txt` (CSS 변경 후 오래된 진단), `red-legacy.txt` (기존 exact 경로의 진단 생략), `red-border.txt` (부모 개별 테두리), `red-sizing-guards.txt` (layoutMode 읽기 오류), `red-parent-mapping.txt` (부모 px 매핑 변조).
- 반례 fixture의 전제 오류는 원본과 실제 diagnostics를 확인해 수정했다. 118 원본은 키 부재를 확인하는 assertion을 추가하고, 명시적 null을 주입한 별도 분기를 유지했다. CSS 변조 반례는 다른 token 진단의 개수를 0으로 가정하지 않고 변경 전보다 정확히 1개 증가하는지를 검증한다.
- 빠른 수집기에서 `absoluteRenderBounds`를 null-sensitive 목록에 추가했다. 기존 upstream 계약 테스트는 maxWidth/maxHeight를 그대로 요구하면서 absoluteRenderBounds까지 요구하도록 강화했다. 실제 JS 실행 테스트가 null/키 부재/throw/유효 bounds를 양쪽 수집 경로에서 확인한다.
- 모든 생성 ABSOLUTE 노드에 항목별 증거를 기록해 과거 exact 경로의 우회를 제거했다. MAX는 크기와 right/bottom 여백을 검증할 때만 해소한다. HUG는 대응 auto-layout의 intrinsic sizing 의도를 검증한다. percentage 검증은 원본/생성 auto-layout, 같은 명시적 FIXED 부모 크기 및 override 부재를 요구한다.
- CENTER는 중심 오프셋 관계의 검증이며 부모가 커질 때 x가 그대로라는 주장이 아니다. 별도 부모 dimension mapping을 숫자로 다시 검증한다. `verified-explicit-dimension`은 생략 규칙 때문에 coverage 의무에서 사라지지 않는다.
- 별도 읽기 전용 리뷰 지적사항을 실패 테스트로 재현하고 보완했다. `green.txt`에 회귀 및 수집기 계약 검증 기록이 있다.
- Golden별 변경 이유: [golden-changes.md](golden-changes.md). TSX golden 변경 없음.

최종 workspace/clippy/fmt 및 자체 target 결과는 아래와 `verification.json`에 기록했다.

- `red-fill-dimension.txt`: verified px 라벨이 기존 FILL asset 크기 검증을 우회하는 반례를 먼저 실패시킨 뒤, 수치 일치 검증과 기존 dimension 검증을 모두 요구하도록 수정했다.
- 기존 `r4_export_exposes_placement_contract_without_diagnostics` coverage 기대값은 114에서 117로 강화했다. 기존 114개에 새로 재검증하는 루트 width/height와 모달 height 3개가 추가되었다. 기존 TSX·placement·lossy 검증은 유지했다.

- `red-response-evidence.txt`: 진단 opt-out에서 확인된 비렌더 근거가 사라짐을 실패로 재현했다. `projectionEvidence`를 최상위 및 프레임 응답에 추가해 includeDiagnostics=false에서도 근거를 제공하며, 미해결 issue와 분리했다.

## 최종 결과

- `cargo test --workspace -j 2 --no-fail-fast`: **757 passed / 0 failed / 2 ignored** (기준선 739 + R9 회귀 18). `workspace-final.txt`.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: exit 0, 경고 0. `clippy.txt`.
- `cargo fmt --all -- --check`: exit 0. `fmt.txt`.
- 자기 target에서 검증했고 실행된 테스트가 내 것임을 확인했다. `verification.json`에 자체 실행 파일 절대 경로·SHA256·R9 테스트 목록·해당 바이너리에서 직접 재실행한 테스트 결과를 저장했다.
- 자체 서버 바이너리 버전은 `devup-mcp 0.4.4 (54c6a12c48af-dirty)`였다. 커밋 전 수정 코드로 빌드한 결과이며, 설치된 MCP 서버의 버전으로 검증을 대체하지 않았다. CARGO_TARGET_DIR 미설정도 확인했다.
- 설치된 devup-mcp MCP 도구를 검증에 사용하지 않았다. 새 Figma 호출 없이 원본 fixture와 자체 생성기/수집기/서버 투영 테스트를 실행했다.
- 커밋 후 `cargo clean`으로 자체 target을 반납한다. push는 수행하지 않는다.
