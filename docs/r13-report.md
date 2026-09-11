# R13 postprocess provenance 결과

## 근거와 판정

요청된 brief와 다른 저장소의 원문을 먼저 읽었다. girok-space 파일은 수정하지 않았다.

- R13-1: WQUW-118 followup `report.md`의 F13-1과 같은 폴더 `calls.json` 전체 인자·원문 응답. 두 응답의 `content.text`를 JSON으로 풀어 `structuredContent`와 동일함을 확인했다. 두 번째 응답의 51-node `rawSnapshot`을 값 변경 없이 [fixture](../fixtures/r13/f13-1-snapshot.json)로 추출했다. 원문 SHA256·두 호출 인자·서버 build 정보는 [README.json](../fixtures/r13/README.json)에 기록했다.
- R13-2: WQUW-118 `docs/wquw-118-r12/report.md`의 일반 outputPaths 키 무시 관측.
- R13-3: WQUW-119 `docs/wquw-119/r12-report.md`, WQUW-120 `docs/WQUW-120-devup-r12-report.md`의 독립적인 resource 재투영 안내 요구.

118의 판정을 유지했다. TSX와 값은 존재하므로 매핑 누락 자체를 lossy로 분류하지 않는다. `boxShadow="0 4px 32px 0 #46608740"`, `objectFit="none"`, `objectPos="-32px -28px"`를 그대로 검사한다. SVG 자체 효과와 CSS 효과의 브라우저 합성·픽셀 동등성은 측정하지 않았다.

## 구현 전에 조사한 변형 단계

[사전 계획 및 목록](r13-plan.md)을 구현 전에 작성했다. 이번 F13-1의 효과·자산 경계 속성은 실제로 map 생성 전 `push_style_props`에서 만들어진다. 최종 속성과 별개인 기존 역매핑 표가 이 속성들을 빠뜨린 것이 직접 원인이다. 따라서 세 속성만 표에 추가하는 방식으로 끝내지 않았다.

| 단계 / 코드 | map과의 순서 | 처리 |
| --- | --- | --- |
| `codegen/{layout,style,text,animation}.rs`, component asset layout filtering | map 전 | 최종 AST의 모든 속성을 검사. 기존 map에 없으면 실제 생성 단계 재실행 결과와 원본 필드·계산을 대조하여 evidence, 미지원이면 명시적 매핑 누락 진단. |
| `codegen/component.rs` 모듈·함수 wrapper, 들여쓰기, 빈 Box 크기 보강, instance 크기/usage 삽입 | 일반 finalizer 전 | 삽입 이후 전체 AST 검사. 등록되지 않은 속성도 검사 대상이며 조용히 exact가 되지 않음. |
| `codegen/variant.rs` selector/tree 및 조건부 속성 병합 | 일반 finalizer 전 | 표현식 전체와 spread까지 검사. 근거 없는 병합 속성은 진단. |
| `provenance::finalize_tsx` / `strip_markers` | marker 제거와 동시에 map 생성 | 기존 범위 map 유지 후 전체 속성 문자열 확정, sourceMap 또는 단계 evidence 대조. |
| `codegen/responsive.rs`, `server/projection.rs` responsive merge/reset/module wrapper | 일반 finalizer 우회 | breakpoint map은 병합 map으로 취급하지 않음. `mergedMappingVerified=false`를 최종 전달 감사에서 명시적 누락으로 보고. 속성 없는 모듈도 미검증 단계 진단 유지. |
| `server/projection.rs::rewrite_result_asset_references` host-safe 파일명 및 수집·중복제거·public URL 치환 | map과 초기 quality 생성 후 | 코드·map의 generatedProperty·evidence·내부 속성 계약을 함께 치환한 다음 최종 AST 계약 재검사. 기존 R8 범위 무효화/semantic map 처리를 유지. |
| 제외된 자산을 참조하는 TSX withholding | codegen 후 | 기존 명시적 failure와 withheld-output 판정 유지. |
| `apply_delivery`, 파일 staging/commit 및 resource 직렬화 | 최종 치환·감사 후 | 같은 최종 본문 전달. TSX 변형 없음. resource 자산 reference materialization은 manifest/resource metadata 대상. |
| `validation_guidance.rs` 사용자 TSX 교정 preview | 별도 validate 작업 | 생성 export map 파이프라인 밖. 기존 교정 안내 유지. |

## 구조적 계약

- `provenance/attributes.rs`는 속성 이름 whitelist가 아닌 OXC AST 순회로 boolean·표현식·namespace·spread를 포함한 모든 JSX 속성을 열거한다. 주석과 문자열 안의 가짜 JSX는 제외한다.
- 기존 sourceMap은 노드 소유권, 전체 속성 문자열, 속성을 포함하는 범위로 대조한다. 부족한 경우 `property_derivations.rs`가 실제 layout/style/text/animation 생성 함수를 재실행하여 해당 속성 값이 일치하는지 확인한다. 알려진 계산과 입력 필드가 있는 경우에만 evidence로 인정한다. 새 속성이 추가되어도 미등록 상태를 성공으로 처리하지 않는다.
- `projectionEvidence`에는 원본 nodeId, property, output, sourceFields, originalValue, calculation, stage, generatedProperty, elementIndex, 관련 assetId를 제공한다. includeDiagnostics/sourceMap을 끈 호출에서도 추가 근거를 유지한다.
- 서버 전달 경계는 private 속성 계약과 최종 TSX를 다시 비교한다. 새 속성·값 변경·소유 요소 변경·spread·파싱 실패를 잡으며 private ledger는 응답에서 제거한다. 두 코드 출력 요청은 `sourceMap.byOutput`으로 map 충돌을 방지한다.
- 누락은 `DEVUP_CODEGEN_PROPERTY_UNMAPPED`, `fidelityImpact=none`, `mappingComplete=false`. 다른 손실/근사가 없다면 `quality.projection=mapping-incomplete`, status partial, deliverable.isFinal false. 기존 lossy/approximated/failed 우선순위와 accounted-for 분류를 유지한다. strict도 누락을 거절하며 속성·노드별 projectionIssues를 포함한다.
- 미래 후처리 속성 삽입 회귀, 값 변경/요소 이동 회귀와 원문 fixture의 모든 생성 속성 대응 회귀를 고정했다. 일반 텍스트 segment의 미지원 fontSize도 값 보존+명시적 진단을 검증한다.

## 전달 계약

- R13-2: `frame:<nodeId>:tsx|componentTsx|sourceMap` 키를 지원한다. 이번 응답에 실제 존재하는 키는 `outputPathResults.supportedKeys`, 형식은 frameKeyFormat, 미지원/미생성 요청은 diagnostics로 반환한다. 반환 outputPaths에는 성공적으로 commit한 파일만 포함한다. 기존 경로 허용·transaction 정책을 유지하고 프레임 파일 실제 바이트를 검사했다.
- R13-3: explicit/auto resource 응답에 같은 artifact의 `nextAction.tool/arguments`를 제공한다. 일반 화면은 단일 화면·단일 출력이며 컴포넌트 이름 suffix와 rootLayout 등 원래 인자를 보존한다. 로컬 서버 핸들러로 예시를 실제 재투영하여 원래 본문과 동일함을 검사했다. responsive는 원래 breakpoint 선택을 유지한다. assetRequests/paths가 URL에 결합된 경우 원래 resource 인자를 유지한다.
- sizeEstimate는 기존 본문 UTF-8 또는 PNG binary 바이트와 JSON/base64 중복 전송 하한만 계산한다. 새 응답 전체 크기는 실측하지 않았으므로 `isMeasuredReprojection=false`, 한도 초과가 확실하지 않으면 inlineFit unknown. 기존 R12 크기 초과 오류 교정 인자는 유지했다.
- R8의 기존 3화면 inline 크기 회귀를 제한·테스트 변경 없이 유지했다. 새 PROPERTY_EVIDENCE를 diagnostics에 중복 직렬화하지 않고 필요한 필드만 기록한다. 기존 진단은 제거하지 않았다.

## 실패 재현 기록

수정 전에 회귀를 실행하여 실패를 확인했고 실패 테스트를 삭제하거나 조건을 완화하지 않았다.

| 기록 | 수정 전 실패 |
| --- | --- |
| [r13-red-contract.txt](r13-red-contract.txt) | 원문 fixture 최종 속성의 map/evidence 누락 |
| [r13-red-server.txt](r13-red-server.txt) | exact 오판, outputPaths 무응답, resource nextAction 부재, 세 자산 속성 근거 부재: 4 failures |
| [r13-review-red.txt](r13-review-red.txt) | malformed TSX 빈 AST, responsive 선택 유실, PNG 안내 누락: 3 failures |
| [r13-reprojection-red.txt](r13-reprojection-red.txt) | 동일 artifact 재투영의 컴포넌트 이름 suffix 불일치 |
| [r13-final-review-red.txt](r13-final-review-red.txt) | mapping-incomplete 검토 우선순위, 빈 responsive 미검증 단계 누락: 2 failures |
| [r13-strict-red.txt](r13-strict-red.txt) | 일반 codegen 매핑 누락 strict 오류에서 projectionIssues 누락 |

중간 테스트 fixture 두 곳은 원래 생성 quality를 exact로 확인할 수 있게 FIXED sizing을 지정하고, 속성 검사 대상이 없던 빈 responsive에 실제 fill을 넣었다. 기존 검사 조건은 그대로 유지했고 별도 빈 responsive 단계 회귀도 추가했다.

## Golden 변경 사유

- `crates/devup-mcp-devup-ui/tests/snapshots/wquw_151__wquw_151_proofread_diagnostics.snap`: 기존 진단 3개를 그대로 유지하고 누락 속성의 생성 단계 근거 37개를 추가했다. TSX/기존 판정 변경 없음.

## 최종 검증

- `cargo test --workspace -j 2 --no-fail-fast`: **795 passed / 0 failed / 2 ignored**, exit 0. 기준선 779에 R13 테스트 16개 추가. [전체 로그](r13-final-workspace.txt).
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: **12 passed / 0 failed**, exit 0. [로그](r13-js.txt).
- `cargo fmt --all -- --check`: exit 0, 출력 없음.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: **0 warnings / 0 errors**, exit 0. [로그](r13-clippy.txt).
- `git diff --check`: exit 0.

**자기 target에서 검증했고 실행된 테스트가 내 것임을 확인했다.** cargo metadata가 보고한 target은 이 워크트리 내부의 `target`이며 CARGO_TARGET_DIR은 설정하지 않았다. [경로·바이너리 SHA256·수정 시간](r13-local-target.json), [로컬 --version/--self-check 및 테스트 목록](r13-local-executables.txt)을 남겼다. 전체 로그의 실제 실행 파일은 같은 `target/debug/deps/devup_mcp-bbcce4741168dc25.exe`(R13 13개)와 `r13_provenance-f87a5fd4cbac7da4.exe`(R13 3개)다. 빌드 ID는 수정 중인 기반 HEAD를 정확히 표시한 `a66fc287d0b4-dirty`다. 설치된 devup-mcp MCP 툴을 검증에 사용하지 않았다.

독립 코드 리뷰의 malformed AST, responsive 선택, PNG 크기 안내, 검토 우선순위, strict 진단 누락 지적을 모두 실패 재현 후 보완했다. 브라우저 SVG/CSS 합성은 이번 검증 범위가 아니다.
