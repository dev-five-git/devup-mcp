# R10 — ID 공간과 내용 기반 크기

## 근거와 결정

- 읽기 전용으로 WQUW-119의 `r9-report.md`, `r9-lossy-verification.json`, WQUW-118의 `wquw-118-devup-r9-report.md`를 확인했다. 다른 저장소는 수정하지 않았다.
- 실측 `I3997:46130;65:2919`는 `textAutoResize=WIDTH_AND_HEIGHT`, 측정값 16×18이고 실제 응답의 Text에는 고정 폭·높이가 없다. `codegen/layout.rs`도 FIXED/FIXED Text의 WIDTH_AND_HEIGHT에서 두 축, HEIGHT에서 높이를 의도적으로 생략한다. 내용 기반 크기 지시는 보존됐고 브라우저 픽셀 등가성은 미측정이다.
- R6-2/R7-1의 `account_for_sizing` → source map resolution → `validate_fidelity` 재검증 경로를 확장했다. R9-1처럼 semantic accounting과 미측정 정보를 분리한다. `accounted-for-content-sizing` 문자열만 신뢰하지 않고 원본 모드·필드 읽기 오류·실제 Text 및 크기 속성을 다시 확인한다. NONE/TRUNCATE의 누락, 알 수 없는 요소, 읽기 실패는 인정하지 않는다.
- R10-2는 **진단에 screenId 제공**을 선택했다. frameIds의 화면 선택 의미와 범위를 유지하면서 바로 쓸 수 있는 소속 ID를 제공한다. 자손을 조용히 화면으로 바꾸지 않는다. 화면 소속을 알 수 없는 전역/외부 진단은 `screenId:null`과 이유를 제공한다.
- 실사용 SECTION 인덱스는 자손의 geometry를 버리는 압축 응답이다. 이미 순회한 트리에서 ID 소속만 `nodeScreenIds`에 보존하여 fresh collection과 cached index 모두에서 교정할 수 있게 했다. 전체 snapshot에서는 parentId/childrenIds로 소속을 구하며 순환을 차단한다.
- R10-3은 자손, 소속 화면 없음, 인덱스 내 부재, 잘린 인덱스에서 미확인을 `selectionIssues`로 구분한다. 모두 교정 가능할 때만 SECTION URL 및 중복을 제거한 전체 화면 선택을 `nextAction.arguments`에 제공한다. 정상 요청 화면도 유지한다.
- R10-4는 종료 job의 nextAction에 교정 인자를 전달하거나 null 및 이유를 반환한다. running의 status와 paused의 resume는 유지한다.
- R10-5는 isFinal=false의 resource 안내를 검토가 필요한 available TSX로 표현한다.

## 먼저 확인한 실패

- `r10_content_sizing_accounts_for_intent_and_keeps_pixel_uncertainty`: source map에 content-sizing resolution이 없어 실패했다. 같은 target의 수정 후 테스트 2개가 통과했고 NONE/TRUNCATE·알 수 없는 요소·필드 오류의 손실 판정을 확인했다.
- SECTION script 동작 테스트: `3997:46703 → 3997:46690` 연결이 undefined라 실패한 뒤 ID 소속 보존으로 통과했다.
- 직접 실행한 `target/debug/deps/devup_mcp-e7c3ff09bf3cef75.exe r10_ --nocapture`: R10-2 screenId=null, R10-3 교정 인자=null, R10-4 종료 job의 status nextAction, R10-5 nonfinal의 final TSX 문구로 4개 모두 실패했다.
- opt-out 회귀는 근거 2건이 0건으로 반환되어 실패했다. 기존 `projectionEvidence` 필터를 확장해 픽셀 미측정 정보도 항상 전달한다.
- 속성 없는 `<Text>hello</Text>`는 태그 확인의 공백 조건 때문에 실패했다. 정확한 Text 태그인 경우 속성 유무에 의존하지 않도록 고쳤다.

## 기존 테스트 및 golden

- Golden 스냅샷은 변경하지 않았다.
- `r7_auto_text_explicitly_reports_unverified_font_metrics`: unverified와 font-metrics-not-measured 검증은 유지하고, R10의 의미 보존 판정에 맞게 impact=None 및 content-sizing resolution/code 검증을 추가했다.
- `r2_production_layout_evidence_is_actionable_per_output`: 자동 텍스트 lossy 기대 2건을 0건으로 바꾸되, 삭제가 아니라 projectionEvidence 2건으로 남는지 검증한다. component-reference 40건과 실제 손실 검증은 유지한다.
- `manifest_covers_readers`: nodeScreenIds는 Figma 노드 속성이 아니라 section_index.js가 계산하는 메타데이터이므로 기존 NOT_NODE_FIELDS 분류에 근거를 달아 추가했다. 실제 JS 실행 회귀가 이 필드의 수집을 검증한다.

## 최종 검증

- `cargo test --workspace -j 2 --no-fail-fast`: **766 passed / 0 failed / 2 ignored** (기준선 757에서 R10 테스트 9개 추가).
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: **7 passed / 0 failed**.
- `cargo fmt --all -- --check`, `git diff --check`: 통과.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: 통과, 진단 0건.

설치된 devup-mcp MCP 도구를 호출하지 않았다. CARGO_TARGET_DIR을 설정하지 않았으며 build/test/clippy의 jobs는 모두 2다. 실행한 바이너리는 `C:/Users/owjs3/orca/workspaces/devup-mcp/r10-id-space-content-sizing/target/debug/devup-mcp.exe`이고 `--version`은 `devup-mcp 0.4.4 (b24ff8bcb50d-dirty)`였다(커밋 전 수정된 소스 빌드).

전체 로그에서 `target/debug/deps/devup_mcp-bbcce4741168dc25.exe`, `r10_content_sizing-5290240f0a4162fa.exe` 및 SECTION 테스트의 신규 R10 함수 9개가 실제 실행·통과한 것을 확인했다. **자기 target에서 검증했고 실행된 테스트가 내 것임을 확인했다.** MSVC의 lib/exp 생성 stdout은 linker_messages 경고로 출력되었지만 테스트 실패는 없다.

브라우저 폰트 픽셀 치수를 새로 측정했다고 주장하지 않는다. 해당 불확실성은 응답에 남겨 둔다. 커밋 후 `cargo clean`으로 이 워크트리의 target을 반납하며 push하지 않는다.
