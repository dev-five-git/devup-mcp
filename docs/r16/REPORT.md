# R16 — resolution labels and verdict scope

2026-09-11. 브리프와 WQUW-120/119/118 실측 보고서를 먼저 읽고, 현재 worktree에서 응답 설명력만 변경했다. 다른 저장소는 읽기만 했다.

## 변경과 보존 범위

- R16-1: `resolutionSemantics`로 매핑 방식과 ABSOLUTE component 검증 상태를 구분한다. raw-fallback 값은 유지한다. Section은 사용된 라벨의 공통 사전을 상위 응답에 두고 frame sourceMap이 `/resolutionSemantics`를 참조한다. 단일 SourceMap은 전체 사전을 포함한다.
- R16-2: 실패한 치수 증명의 `blockedBy`를 추가했다. 부모 폭 불명/불일치, 읽기 오류, 비FIXED sizing, dimension 충돌은 각각 다른 이유값이다. widthPreservation은 같은 객체를 유지하며, 자산은 최종 render-boundary 증명에 맞는 이유를 반환한다.
- R16-3: `verdictScope`가 전체/화면별 판정 원인과 nodeId·screenId·output·미해결 component를 요약한다. width/height 검증을 horizontal/vertical 검증으로 확대하지 않는다. includeDiagnostics=false와 resource delivery도 포함한다.

생성 TSX 로직과 기존 승격 조건은 변경하지 않았다. R13 provenance, R14 widthPreservation/componentSummary, R15 근거 문구와 R6–R15 판정·안내를 유지했다. 자세한 필드 의미와 strict 거절/selection/in-progress 제외 범위는 [응답 계약](response-contract.md)에 기록했다.

## RED → GREEN

최초 신규 16개 테스트가 해당 구현 전에 설명 필드 누락으로 실패했다. 필수 5개 차단 조건은 독립 테스트에서 각각 다른 기대 이유값으로 실패했다. 부모/자식 읽기 오류와 비FIXED sizing, padding, containing block, embedded 및 자산 경계도 포함한다.

리뷰에서 발견한 frame별 요청 출력 실패 누락도 테스트를 먼저 추가해 재현했다. 이 추가 테스트의 초기 공통 fixture에는 FIXED sizing이 없어 성공 형제도 lossy였다. assertion은 그대로 유지하고 성공 형제의 sizing을 명시한 뒤 실제 requested-output 원인 누락으로 RED를 확인했다. acquisition 근거의 response scope 누락도 RED로 확인하고 보완했다. [RED 근거](red-evidence.txt)

추가 메타데이터 때문에 기존 R8 3화면/9출력 inline 테스트가 처음에는 1,065,859 bytes로 1 MiB 한도를 17,283 bytes 초과했다. 기존 테스트·한도·진단은 그대로 유지하고 새 사전의 공유/사용 라벨 선별과 새 메타데이터의 반복 문구 정리로 해결했다. 원래 R8 inline 테스트가 다시 통과했다. exact 및 approximated resource 응답의 모든 projectionIssue가 요약에 남는 회귀도 추가했다. 최종 신규 테스트는 18개다.

별도 읽기 전용 리뷰에서 분류 조건/TSX 보존, frame 실패 참조, 공유 사전의 다중 출력·resource 유지도 확인했다. 리뷰어는 Cargo 검사를 대신 실행하지 않았다.

## 검증

- `cargo test --workspace -j 2 --no-fail-fast`: **829 passed / 0 failed / 2 ignored**, exit 0. 제공 기준선 811개에 신규 18개를 추가했다. [전체 테스트 요약](test-summary.txt)
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: **12 passed / 0 failed**, exit 0.
- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: **warnings 0 / errors 0**, exit 0. [검사 원문 요약](checks.txt), [검증 결과](verification.json)

공식 Figma 입력을 stdin으로 요구하는 기존 ignored 2개는 실행하지 않았다. Rust test 빌드의 MSVC linker stdout 안내는 컴파일러가 warning으로 표시했지만 테스트 실패는 없었다.

## 자기 target 증명

**자기 target 에서 검증했고 실행된 테스트가 내 것임을 확인했다.**

Cargo metadata의 workspace_root/target_directory가 현재 worktree와 그 아래 `target`임을 확인했다. `CARGO_TARGET_DIR`은 설정하지 않았다. 전체 테스트 로그에 나타난 자체 target의 실행 파일 3개를 절대 경로로 직접 실행해 신규 R16 테스트 **18 passed / 0 failed**를 다시 확인했다. 경로·SHA-256·명령·종료 코드: [target identity](target-identity.json), [직접 실행 원문](target-tests.txt). 실행 파일은 커밋 전 이 worktree의 변경 소스로 빌드한 것이다.

설치된 devup-mcp MCP 도구로 검증하지 않았다. 이 검증은 저장된 fixture와 로컬 실행에 관한 것이다. Figma 재수집, 브라우저 픽셀, 반응형 동등성, SVG/CSS 합성을 새로 측정하지 않았다.

## Golden 변경 이유 — 각 파일 한 줄

- `wquw_151__wquw_151_proofread_diagnostics.snap`: 성공한 치수 증명과 widthPreservation에 `blockedBy: null` 6곳을 추가했다.
- `wquw_151__wquw_151_proofread_source_map.snap`: 매핑 방식과 검증 상태의 관계를 설명하는 `resolutionSemantics` 사전을 추가했다.

추가 필드만 제거하면 두 golden의 기존 JSON과 완전히 같다. sourceMap entries/version, 진단의 기존 판정·근거 값은 동일하다. 생성 TSX golden 12개는 변경하지 않았고 관련 snapshot 테스트도 통과했다. [golden 비교와 해시](golden-audit.json)

커밋은 현재 작업 브랜치에 로컬로 남기고, 이후 cargo clean으로 target을 반납한다. push하지 않는다.
