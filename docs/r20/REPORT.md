# R20 — 브라우저 실측 적용 대상 조사

**결론: 현재 확보한 캡처의 잔여 approximated 중 측정으로 판정을 결정할 수
있는 대상은 0건이다. R20-2는 수행하지 않는다. 생성 TSX, 판정, 응답,
provenance 및 기본 export 동작은 변경하지 않는다.**

## 기준과 조사 방법

브리프: `C:/Users/owjs3/orca/devup-mcp-briefs/R20-browser-measurement.md`.
작업 시작 HEAD `50afc95`는 로컬 main의 오래된 0.4.4였다. 원격을 fetch하고
R6~R17 및 두 플랫폼 수정이 통합된 `origin/main` 0.4.5,
`0e01f96d6bde64e15118e1ff7f47cacf19430bd0`으로 fast-forward했다.

조사는 다음 두 집합을 구분한다.

1. 제품 코드에서 `Approximated`를 만들거나 보수적으로 복원하는 모든 경로.
2. 저장된 실제 캡처 및 축소 회귀 스냅샷을 현재 생성기로 재생한 결과.
   합성 테스트의 입력 변조는 실제 캡처의 미해결 건수로 세지 않는다.

브라우저에서 수치를 얻는 것과 그 수치로 원본 대응을 판정하는 것은 다르다.
`측정 가능`은 알려진 원본 기대값과 실제 생성 결과를 비교해 해당 잔여
판정을 결정할 수 있음을 뜻한다. `측정 불가`는 질문 자체가 CSS 측정 대상이
아니거나 필요한 원본 증거가 없음을, `측정해도 판정 불가`는 출력 동작은
측정할 수 있어도 남아 있는 원본 의도/외부 조건을 그 결과로 증명할 수 없음을 뜻한다.

## 제품 코드 발생 경로 전수 목록

| 발생 경로 | 현재 근거 | 브라우저로 해결되지 않는 경우 |
|---|---|---|
| `codegen/component.rs` ABSOLUTE fallback, `provenance/sizing.rs::absolute_component_verification` | width/height/horizontal/vertical 각각 검증, 미해결 축이 있으면 근사 유지 | 원본 치수/제약 읽기 실패, 외부 containing block, 상속 제약 |
| `provenance/absolute_bounds.rs::annotate` | export boundary, rounding, constraintInterpretation을 별도 검증 | render boundary의 선택과 GROUP 의도의 불명확성을 픽셀 일치로 상쇄할 수 없음 |
| `codegen/evidence.rs::placement_contract` | 생성물에 포함되지 않은 외부 호스트 또는 미확정 루트 치수 | 만든 테스트 호스트가 사용자의 실제 호스트라는 증거가 없음 |
| `codegen/component.rs::non_rendering_diagnostic` | visible/render bounds를 오류 없이 수집했는지 | 누락된 원본 paint를 빈 출력의 관측으로 복구할 수 없음 |
| `DEVUP_CODEGEN_TOKEN_NAME_UNRESOLVED` | 변수/스타일의 원본 ID를 이름에 대응하지 못함 | 동일한 현재 색/치수는 토큰 정체성이나 테마 추종의 증거가 아님 |
| `devup-mcp-figma/src/snapshot.rs::Diagnostic::fidelity_impact` | 명시 impact가 없는 ABSOLUTE 또는 미등록 codegen 경고를 보수적으로 근사 처리 | 알 수 없는 경고의 의미를 브라우저 통과만으로 지울 수 없음 |
| `provenance.rs::validate_fidelity` | 기존 VERIFIED ABSOLUTE 또는 NON_RENDERING 주장을 최종 출력에서 다시 검사, 실패 시 근사 복원 | 조작/오래된 라벨을 신뢰하지 않는 R13/R17 보호 경로; 새로운 수집 증거 없이 해제 불가 |

치수 판정의 미해결 이유도 전부 확인했다: `read-error`,
`conflicting-dimension-props`, `intrinsic-layout-unproven`, `non-fixed-sizing`,
`source-dimension-unknown`, `generated-dimension-unproven`,
`parent-width-unknown`, `parent-height-unknown`, `parent-width-unequal`,
`parent-height-unequal`, `parent-padding-or-border`, `containing-block-unproven`.
자산 경계 경로에는 `export-boundary-unproven`, `rounded-dimension-mismatch`가 추가된다.
이 enum/분기들이 존재한다는 사실만으로 실측할 실제 잔여 사례가 존재한다고 세지 않는다.

`MASK_FALLBACK`, `EFFECT_FALLBACK`, `ANIMATION_UNREACHABLE`,
`VARIANT_CHILD_FALLBACK`, 미대응 layout/property는 lossy 계열이다.
정상 자동 텍스트는 R10 이후 impact none이며, 실패한 content-sizing 경로만
lossy다. 기존 R7 문서의 “자동 텍스트는 lossy”를 현재 상태로 오독하면 안 된다.

## 실제 재생 결과와 후보 분류

저장소의 `fixtures` 및 `crates/devup-mcp-devup-ui/tests/fixtures`에서 JSON을
열어 최상위 또는 `snapshot` 안에 nodes/roots가 있는 파일 22개를 전부 선정했다.
여기에 외부 R14 검증의 24노드 전체 snapshot 1개를 추가했다.
각 snapshot의 roots와 SECTION 직계 자식을 중복 제거하고,
`inlineInstances=false/true`, 기본 standalone으로 **62개 출력**을 재생했다.
62개 모두 생성 성공, 생성 오류 0건이다. fixture별 루트, 옵션, impact와
진단 원문은 [inventory.json](inventory.json)에 있다. 서로 겹치는 캡처와 두
출력 모드를 포함한 **관측 건수**이며 독립적인 디자인 결함 수가 아니다.

| 진단/근거 | 관측 수 | 고유 대상과 현재 판정 | 분류 및 이유 |
|---|---:|---|---|
| ABSOLUTE fallback | 4 | 축소/전체 snapshot × inline 2종. `3997:46668`의 horizontal/vertical만 unresolved | **측정해도 판정 불가**. 두 축 모두 GROUP 원본 제약이 아니라 first-child 상속이며 assumed다. 수치 관측으로 상속 정책이 원본 의도인지를 증명할 수 없다. |
| NON_RENDERING_ASSET | 4 | R9 단일 노드와 R2 WQUW-119의 `3997:46703` × 2. `no-render-bounds`, fieldPresent=false | **측정 불가**. 누락된 Figma render bounds를 생성된 숨김/빈 요소 측정으로 복구할 수 없다. 재수집이 필요하다. |
| TOKEN_NAME_UNRESOLVED | 298 | 147개 고유 node/property/resourceId 조합, impact의 기본 경고 분류는 approximated | **측정 불가**. 변수·스타일 이름/정체성은 픽셀 메트릭이 아니다. 모두 리소스 카탈로그 없는 raw/reduced snapshot 입력에서 관측됐다. 실제 앱 export에 298개 결함이 있다는 뜻이 아니다. |
| LAYOUT_ACCOUNTED_FOR | 18 | content-sizing 및 기존 implicit sizing 근거, impact none | 잔여 근사에 포함하지 않는다. 아래 자동 텍스트 설명 참조. |
| PLACEMENT_CONTRACT | 0 (근사) | 이번 standalone 재생에서는 근사 없음. embedded/외부 호스트 조건 분기는 코드와 기존 회귀에서 확인 | **측정해도 판정 불가**(미제공 호스트). 임의로 만든 호스트에서의 일치가 소비 앱의 호스트 조건 충족을 증명하지 않는다. |
| 미등록 경고/최종 재검증 강등 | 0 (현재 재생) | 코드 발생 경로 및 기존 반례 테스트로 확인 | **측정 불가**(미등록 경고 의미), **측정해도 판정 불가**(잘못된 provenance). 브라우저 일치가 source mapping 변조나 원본 수집 오류를 복원하지 않는다. |

알려진 후보를 실제 필드로 채우면 다음과 같다.

| 후보 | 실제 원본/출력 | 판정 가능성 |
|---|---|---|
| `text-auto-size` | 예: `I3997:46130;65:2919` width/height는 R10의 `accounted-for-content-sizing`, impact none, `font-metrics-not-measured` 유지 | **폰트/치수 자체는 측정 가능**, 그러나 현재 approximated를 끊을 대상은 아님. 원본 글꼴 로딩·glyph fallback·실제 devup-ui CSS를 확인한 조건부 픽셀 비교라는 별도 과제로 남긴다. 폰트 미설치를 핑계로 “측정 불가능”으로 분류하지 않았다. |
| `3997:46668` 가로 | GROUP width=433.8122863769531, render width=360, local x=-19, render-relative left=0px. 첫 자식 `3997:46669`의 SCALE 사용 | **측정해도 판정 불가**. 고정 360px/left 0의 반응은 관측 가능하지만 GROUP 자체 SCALE 기대값이 없다. 자식 SCALE을 참이라고 가정한 조건부 불일치를 GROUP의 확정 손실로 보고하지 않는다. |
| `3997:46668` 세로 | parent height=460, render-relative y=314, render height=117.6744155883789 → bottom=28.33px. 첫 자식 MAX 사용 | **측정해도 판정 불가**. 부모 높이에 따른 하단 여백 유지 여부를 관측할 수 있어도 GROUP의 MAX 의도는 미확인이다. |
| 잔여 ABSOLUTE 폭/높이 | 62개 출력에서 근사 폭/높이 0건. R14 모달 `3997:46621` 및 자산 폭/높이는 이미 정적 검증됨 | 현재 **측정으로 해제할 잔여 없음**. 합성 fixture 변조로 새 측정 대상을 만들지 않았다. |
| `boundaryDeltaPixels` | width: 360−433.8122863769531=−73.8122863769531. height: render/source 동일, CSS 117.67px의 serializer 오차 약 0.0044155884px | 선택 경계의 CSS box 치수는 **측정 가능**하나 대응은 이미 verified다. 실제 자산 paint/SVG 합성의 등가성은 이 snapshot의 경계 숫자만으로 **측정해도 판정 불가**. box rect가 paint bounds와 같다고 가정하지 않는다. |

원본 ABSOLUTE 선언의 사전 조사에서는 저장소 22개 snapshot에 33개 노드
관측이 있었다(캡처 중복 포함): CENTER/MIN 11, MAX/MIN 13, MIN/MAX 8,
GROUP 자체 제약 없음 1. 직접 SCALE/STRETCH를 선언한 실제 ABSOLUTE 캡처는
없었다. R9 테스트는 모달 constraints와 width를 **의도적으로 변조**해
SCALE/STRETCH/MAX 및 400px 반례를 만든다. 알려진 직접 제약/기대값을 갖는
이런 사례라면 향후 실제 CSS의 resize 측정으로 조건부 손실을 확인할 수 있다.
하지만 합성 반례를 현재 캡처에 남은 실측 대상인 것처럼 세지 않는다.

전체 snapshot은 다음 파일의 바이트 그대로 복사했다. 원본 저장소는 읽기만 했다.

`C:/Users/owjs3/orca/workspaces/girok-space/wquw-118-fr007-question-trial-send/docs/verify-r14/rawSnapshot.json`

SHA-256: `E57E99A814F26D45B769E656DD0B581D2011B1997F94FEDCAD97BCB8F81B1D67`.
해당 디렉터리 REPORT와 이전 R13 원문 응답도 대조했다. 새로운 Figma 수집이나
설치된 MCP 호출이 아니다. 전체 snapshot에는 카탈로그/자산 바이너리가 없으므로
그 누락으로 발생한 token 경고와 inline=false의 component-reference 손실은
GROUP 축 판정과 분리한다.

## R7-2 전례의 재사용 가능성

`docs/R7-report.md`와 `docs/R7-verification.txt`, 관련 fixture 및
`tests/r7_layout.rs`를 확인했다. 보고된 Edge 관측은 부모 56/기존 55/stretch 56,
부모 80/기존 55/stretch 80이다. 이 라운드에서 다시 측정한 값은 아니다.
저장소의 추적 파일에는 당시 headless 실행 스크립트나 HTML이 없다.
따라서 방법의 재현은 가능하나 기존 스크립트를 그대로 재사용했다고 주장할 수 없다.
R7의 FILL에는 알려진 크기 추종 의도가 있었으며, 그 점이 원본 GROUP 의도나
토큰 정체성이 누락된 문제와 다르다.

별도의 기존 `harness/render`에는 Vite + devup-ui + Playwright Chromium으로
생성 TSX를 렌더하는 경로와 `boxes.mjs`의 getBoundingClientRect/getComputedStyle
관측이 있다. 이 방식은 실제 CSS를 실행한다는 점에서 재사용할 수 있다.
다만 현재 boxes는 정수 반올림, 화면 깊이/텍스트 기준 식별, Windows taskkill
정리이며 sourceMap의 노드/축 판정과 연결되지 않는다. `acquire.py`는 설치된
`~/.cargo/bin/devup-mcp.exe`를 명시하므로 이번 검증에서는 실행하지 않았다.
이 기존 harness의 존재를 신규 opt-in 판정 파이프라인 완성으로 세지 않는다.

## 측정 한계

이번 조사에서 브라우저를 실행하거나 폰트/픽셀을 측정했다고 주장하지 않는다.
향후 측정에서도 폰트 로딩 및 실제 glyph fallback, viewport와 호스트 CSS,
브라우저 엔진 버전을 조건으로 기록해야 한다. 폰트 또는 브라우저가 없으면
미측정/실패로 남기고 정적 판정을 유지해야 한다. 특정 환경의 일치는 모든
환경의 픽셀·반응형 등가성이나 source provenance의 증명이 아니다.

## 재현과 변경 범위

`inventory.rs`는 제품 구현이 아닌 조사 시 사용한 읽기 전용 재생 프로그램이다.
`inputs.txt`는 입력 23개의 목록이다. 저장된 payload는 자기 리소스를,
snapshot/resources wrapper는 wrapper의 리소스를 사용한다. raw snapshot은
추가 카탈로그를 합성하거나 다른 시점의 캡처에서 가져오지 않는다.
따라서 R6의 별도 token-name fixture도 이 raw-only 재생에는 주입하지 않았다.
이 선택은 토큰 경고의 수에 영향을 주며 위 표에 그 한계를 명시했다.

조사를 재현하려면 저장소 루트에서 `inventory.rs`를 임시로
`crates/devup-mcp-devup-ui/examples/r20_inventory.rs`에 복사한 다음 실행한다.

```powershell
$r20Inputs = Get-Content docs/r20/inputs.txt
cargo run -j 2 -p devup-mcp-devup-ui --example r20_inventory -- $r20Inputs
```

저장된 inventory의 fixture 경로에는 실행 당시 Windows 구분자가 남아 있다.
inputs 목록은 다른 OS에서도 사용할 수 있도록 `/`로 정규화했다.
실행 파일은 이번 작업 트리의 target에서 빌드했으며 설치된 MCP는 호출하지 않았다.

측정 가능 잔여가 0건이므로 R20-2 구현이나 제품 버그 수정이 없다.
이에 따라 새 실패 테스트를 인위적으로 만들지 않았다. 기존 실패 테스트 삭제,
assertion 약화, golden 변경 모두 0건이다. 임시 예제 코드는 조사 자료로만
보존하고 crate의 빌드 대상에는 남기지 않는다. R6~R17의 판정·안내,
R13/R17 provenance, R14 widthPreservation/componentSummary,
R11 SECTION 상한은 원본 코드 및 기존 회귀를 그대로 유지한다.

CI의 ubuntu-latest/macos-latest/windows-latest 행렬을 확인했다.
새 브라우저·폰트·프로세스 의존성, 환경변수, 플랫폼별 파일 경로 처리 또는
CI 변경은 없다. Windows에서 직접 검증하며 Ubuntu/macOS 실행은 미확인이다.
기존 harness의 플랫폼 제약을 이번 제품 경로로 가져오지 않았다.

## 검증 결과

- `cargo test --workspace -j 2 --no-fail-fast`: **855 passed / 0 failed / 2 ignored**, exit 0.
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: **13 passed / 0 failed**, exit 0.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: exit 0, 경고 0.
- `cargo fmt --all -- --check`: exit 0. 임시 예제를 문서로 옮긴 최종 상태에서도 통과.
- 자체 `target/debug/devup-mcp.exe --version`: `0.4.5 (0e01f96d6bde-dirty)`.
  `--self-check`: status ok. 임베디드 commit만으로 검증을 주장하지 않고
  [target-identity.json](target-identity.json)에 실제 경로와 SHA-256을 기록했다.
- 해당 target의 `r14_absolute_evidence-6e04826c1505b9df.exe`에서
  `r14_asset_boundary_rounding_and_inherited_constraints_are_separate --exact`
  직접 실행: 1 passed / 0 failed. [local-test.txt](local-test.txt).
- 최초 전체 빌드는 조사를 먼저 실행하도록 순서를 바꾸기 위해 중단했다.
  이후 요구된 전체 명령을 다시 실행한 완결 로그가 [workspace-tests.txt](workspace-tests.txt)다.
  Windows 테스트 링크 중 import library 생성 안내가 `linker_messages` 경고로
  출력됐지만 전체 exit 0이며 별도 clippy는 `-D warnings`로 통과했다.

**자기 target 에서 검증했고 실행된 테스트가 내 것임을 확인했다.**
`CARGO_TARGET_DIR`는 설정하지 않았다. 컴파일하는 cargo 명령에는 모두 `-j 2`를
사용했다. fmt/metadata/clean은 해당 옵션을 받지 않는 명령이다.
최종 전달은 로컬 커밋 후 cargo clean이며 push는 하지 않는다.
