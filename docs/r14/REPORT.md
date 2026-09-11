# R14 — ABSOLUTE 폭 및 자산 경계

기준 commit: `009491afd0892e8350c9d64357cfc09253caaa7c` (`integration/r6`).
작업 브리프와 WQUW-120 REPORT/06-poll, WQUW-119 r12-report,
WQUW-118 REPORT C-OBS-1/016 응답 원문을 읽었다. 다른 저장소는 읽기만 했다.

## 유형 1: 확정 containing block과 백분율 폭

생성 코드는 유지한다. `layoutSizingHorizontal=FIXED`인 ABSOLUTE 자식의
원본 폭, 부모 원본 폭, 생성 부모의 확정 px 폭이 같으면 `100%`의 대응을
검증한다. 새 경로는 자식이 auto-layout 컴포넌트인지에 의존하지 않는다.
실제 생성 부모가 직계 positioned containing block인지 확인하며 기존
부모 padding/border 및 크기 override 배제 조건도 유지한다.

`components.width`와 `appliedValue.widthPreservation`에 원본 sizing/폭,
부모 생성 폭, resolvedPixels, sourceFields, 계산식과 실패 이유를 제공한다.
폭을 360으로 하드코딩하지 않는다. 280/375/411.25/768/1280px 및 불일치,
부모 FILL/필드 오류, 자식 FILL/필드 오류, 생성 부모 px 변조를 검사한다.
높이·제약의 다른 미검증 항목이 있으면 전체는 계속 partially verified다.

## 유형 2: 3997:46668

| 항목 | 계산·근거 | 판정 |
|---|---|---|
| 폭 | export 정책이 absoluteRenderBounds.width=360을 선택. layout width=433.8122863769531과의 차이 -73.8122863769531은 경계 차이다. | 선택한 export 치수의 대응 verified |
| 높이 | render.height=117.6744155883789 → 117.67px, 오차 약 0.0044155884px | 선택한 export 치수의 대응 verified |
| horizontal | render.x 19263 - parent.bounds.x 19263 = left 0px. 첫 자식 3997:46669의 SCALE을 사용한다. | approximated: GROUP 자체 제약 및 SCALE 반응형 대응 미증명 |
| vertical | render.y 16871 - parent.bounds.y 16557 = 314; 460 - 314 - 117.6744155883789 = 28.3255844116211 → bottom 28.33px | approximated: 자식 MAX를 GROUP 의도로 상속하는 대응 미증명 |

`boundary`에는 선택 필드, 원본 경계, 경계 차이, sourceFields와 계산식을
담는다. `rounding`은 serializer 식 `round(value*100)/100`과 실제 생성값을
대조하고 허용 오차 `0.005px` 및 부동소수점 여유 `1e-12px`를 별도 표기한다.
117.67px를 117.66px로 변조하면 반올림과 치수 검증을 모두 거부한다.

`constraintInterpretation`은 직접 제약/첫 자식 상속/기본 MIN의 출처와
실제 사용값을 분리한다. null과 미수집 모두 상속 또는 기본값은 assumed다.
계산의 성공은 제약 의도의 증명이 아니다. 자산 바이너리나 브라우저/SVG
합성을 새로 측정했다고 주장하지 않는다. fixture의 축소·재구성 범위는
`fixtures/r14/README.md`에 명시했다.

## 축별 응답 및 이전 계약

`componentSummary.verified/unresolved`는 height/width/horizontal/vertical의
분해 결과를 직접 제공한다. 기존 components, resolutionConditions,
heightPreservation, evidenceLimit 및 R6~R13 안내를 보존한다.
R13 provenance 계약 테스트 파일은 수정하지 않는다.

R8 테스트의 FIXED 모달 diagnostic code는 이제 VERIFIED를 요구하지만
기존 높이, padding, CENTER/MIN, FILL/HUG 및 sourceMap assertion은 유지한다.
R9는 같은 폭/다른 폭을 각각 CENTER/SCALE/STRETCH/MAX로 검사한다. MAX는
폭이 증명된 경우에만 기존 margin proof가 성립하고, 다른 폭과 SCALE/STRETCH는
계속 근사다. 기존 실패 검사를 삭제하거나 허용 범위를 느슨하게 하지 않는다.

## 실패 재현 및 검증 기록

- 제품 코드 수정 전 신규 통합 회귀 6개: 0 passed / 6 failed. 기존 width는
  approximated였고 boundary/constraintInterpretation/widthPreservation이 없었다.
- 제품 코드 수정 전 변조 검사 2개: 0 passed / 2 failed. round 상태 없음,
  원본 모달 width approximated를 각각 확인했다.
- 첫 수정 후 통합 6개·변조 2개 통과. 수치 assertion 두 곳은 같은 360을
  JSON 정수 대신 생성 API의 f64(360.0)로 대조하도록 바로잡았다.
- 독립 리뷰가 constraint 표시 불일치와 MAX 계산의 parent dimension 읽기
  오류 누락을 지적했다. 별도 회귀와 export bounds 미선택 대조군으로 검증한다.

- 리뷰 재현: 새 로컬 target 실행 파일에서 5 passed / 4 failed를 확인했다
  (`review-red.txt`). 제약 표시, MAX 부모 오류, export bounds 미선택의 세
  제품 결함을 수정했다. 일반 폭 fixture는 작은 모달에서 기존 child overflow
  정책까지 바뀌는 문제를 분리하기 위해 최소 parent/child로 교체했다.
  280/375/411.25/768/1280px 모두 같은 100% 기대값을 유지했다.
- 첫 전체 실행은 기존 R9 layoutMode 읽기 오류 guard가 새 폭 경로에서
  우회되는 것을 검출했다. 제품 guard를 복원했고 해당 테스트는 변경하지 않았다.
- 수정 후 집중 검증: R14 통합 9 passed, R8 4 passed, R9 16 passed.
  남은 1개 실패는 아래 진단 golden의 새 근거 필드였다.
- R2 서버 응답 테스트에서 실측 모달의 TSX는 이제 exact가 된다. 기대값을
  바꾸고 projectionEvidence의 실제 width=100%/verified를 필수 검사했다.
  기존 원본·padding 검사는 issues와 evidence 양쪽으로 유지하고,
  componentTsx의 lossy 및 component-reference 5개 assertion은 유지했다.

## Golden 변경 (1개)

- `wquw_151__wquw_151_proofread_diagnostics.snap`: 기존 판정과 TSX를 유지하면서 두 ABSOLUTE 진단에 widthPreservation·componentSummary·폭 계산 근거와 구체적인 백분율 검증 이유를 추가했다.

## 검증 환경과 명령

모든 컴파일은 `-j 2`로 실행했고 CARGO_TARGET_DIR를 설정하지 않았다.
`cargo metadata --no-deps --format-version 1`의 target_directory는
`C:/Users/owjs3/orca/workspaces/devup-mcp/r14-absolute-width-asset-bounds/target`이다.
설치된 devup-mcp MCP 도구는 호출하지 않았다. 이 경로의 `devup-mcp.exe`
`--version`은 `0.4.4 (009491afd089-dirty)`, `--self-check`는 status=ok였다.
임베디드 commit만으로 새 코드를 주장하지 않고, local target 실행 경로와
R14 신규 테스트 이름 및 실제 실행 결과로 소유를 확인한다.

- 첫 전체 테스트: 799 passed / 7 failed / 2 ignored (`workspace-before-corrections.txt`).
- 최종 clippy: `cargo clippy --workspace --all-targets -j 2 -- -D warnings`, exit 0, 경고 0 (`clippy-final.txt`). 첫 lint의 items_after_test_module은 테스트 모듈을 끝으로 옮겨 수정했다. 억제/allow를 추가하지 않았다.
- fmt: `cargo fmt --all -- --check`, exit 0.
- JS: `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`, 12 passed / 0 failed (`js-final.txt`).
- 서버 응답 대조: r2_production_layout_evidence_is_actionable_per_output, 1 passed (`server-response.txt`).

- 최종 전체: `cargo test --workspace -j 2 --no-fail-fast`, exit 0,
  **806 passed / 0 failed / 2 ignored** (`workspace-final.txt`). 기준선 795에서
  R14 통합 9개와 변조 검사 2개가 추가됐다. R13 계약 테스트를 포함한 기존
  검사가 모두 통과했다. 생성 TSX golden 변경은 0개다.
- 최종 fmt 및 로컬 `--self-check`를 다시 실행해 exit 0을 확인했다.
- Windows cargo test의 MSVC 라이브러리 생성 stdout은 `linker_messages`
  경고로 출력됐다. 숨기거나 억제하지 않았으며 테스트 실패와 구분한다.
  `clippy -- -D warnings`의 최종 경고는 0개다.

**자기 target 에서 검증했고 실행된 테스트가 내 것임을 확인했다.**
`target-identity.json`에 target 경로, CARGO_TARGET_DIR 미설정, 직접 실행한
테스트 바이너리의 `--list`, SHA-256, R14 11개 실행 성공 로그, 소스 해시를
보존했다. 테스트 바이너리 SHA-256:
`19eaa88dfe595519d11ed896df7f25766e8fd990d682757e6e17f61a52ba80ab`.

커밋에 이 보고서와 검증 근거를 포함한다. 커밋 직후 metadata로 target 경로가
현재 워크트리 안인지 재확인하고 `cargo clean`으로 반납한다. push하지 않는다.
