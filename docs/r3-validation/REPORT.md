# R3 검증 기록

## 결과와 범위

- 기준 main: `8fe4c6d` (워크스페이스 버전 0.4.2).
- 전체 테스트: **652 passed / 0 failed / 2 ignored**. 기준선 641개에서 R3 회귀 11개 추가.
- Clippy `-D warnings`: exit 0. 포맷 검사: exit 0.
- 워크스페이스 빌드: exit 0. 커밋 후 target 정리 기록은 clean.log로 보존한다.
- 설치된 devup-mcp MCP는 호출하지 않았다. Figma upstream은 테스트 fixture/가짜 upstream으로 대체하고 실제 서버·수집기·코드 생성기·출력 트랜잭션을 실행했다.

**자기 target 에서 검증했고 실행된 테스트가 내 것임을 확인했다.**

실행 파일: `C:\Users\owjs3\orca\workspaces\devup-mcp\r3-asset-batch-placement\target\debug\deps\composite_export-7affd3449f5d32bc.exe`

SHA-256: `9e80585393586844c9226ad7be0c160bdce66f896f92e5e8fc2afa24617f4cdd`

`build-identity.json`의 Cargo metadata는 현재 워크트리의 target 경로를 가리키며 `CARGO_TARGET_DIR`은 미설정이었다. `final-tests.log`에 실행 경로와 R3 테스트 성공이 있고, `test-binary-proof.json`과 `test-binary-list.log`에 같은 실행 파일의 해시와 테스트 목록이 있다. 배포된 0.4.2 바이너리의 결과를 재사용하지 않았다.

## 명령 및 로그

| 명령 | 로그 / 종료 코드 |
|---|---|
| `cargo test --workspace -j 2 --no-fail-fast` | `final-tests.log`, `test-exit.json` |
| `cargo clippy --workspace --all-targets -j 2 -- -D warnings` | `clippy.log`, `clippy-exit.json` |
| `cargo fmt --all -- --check` | `fmt.log`, `fmt-exit.json` |
| `cargo build --workspace -j 2` | `build.log`, `build-exit.json` |
| 커밋 후 `cargo clean` | `clean.log` |

로그는 이 디렉터리에 있어 target 정리 후에도 남는다. 로그 사본은 `C:/Users/owjs3/orca/devup-mcp-briefs/R3-validation/`에도 보존한다.

## 회귀 검증

- 자산 16개가 upstream 호출 **0회** 상태에서 거절되고, format/scale/outputPath를 유지한 분할 인자가 제공된다.
- 같은 16개 자산을 권장 3개씩 나누면 16개 모두 성공한다.
- 일시적 실패 및 90초 read-budget 타임아웃 후 첫 자산/스냅샷을 반복하지 않고 미완료 요청만 재개한다.
- 호출자 취소 후 동일 인자로 작업을 찾고 재개한다.
- 출력 설정이 다른 작업은 서로의 일시정지에 갇히지 않으며, 두 resource manifest가 모두 계속 읽힌다.
- resource 응답에서도 자산 수집/파일 완료를 구분하고, 충돌 경로는 실제 파일 해시가 일치하는 자산만 written으로 보고한다.
- 절대 위치 진단은 부모·자식 원본 좌표/크기/constraints/bounds/transform을 제공한다. 기존 WQUW-118/120 fixture로 `(0,0)`, `360×740`, `CENTER`, 카드 x=20, derived padding top=185.5/232.5를 검증했다.
- responsiveTsx는 별도 outputResults/fidelity를 제공한다. breakpoint 원본 tsx의 fidelity이며 mergedMappingVerified=false임을 명시한다.

## WQUW-120 원본 관찰 판정

실제 0.4.2 원본에는 이미 details.output과 tsx/componentTsx별 fidelity가 있다. 40개 layout issue는 전부 componentTsx/component-reference이고, tsx는 layout 104/104, lossy=0이다. Header/Alert로 감춰지는 관찰은 맞지만, 출력 식별자가 없거나 그 손실이 tsx 자체 fidelity에 합산된다는 설명은 이 응답에는 맞지 않는다. 전체 quality는 생성된 projection들의 보수적 집계이며, 자동 생성된 responsive output도 포함할 수 있다. R3는 기존 분리를 유지하고 빠져 있던 responsive 결과를 보강했다.

## 제한과 검토

작업 체크포인트는 서버 프로세스 안에서 최대 30분, 완료 결과는 5분(전체 30분 한도 이내), 최대 8개 작업이다. 서버 재시작 후 복구는 지원하지 않는다. resume은 실제 browser 픽셀 일치를 입증하지 않으며 배치 근사의 등급을 임의로 exact로 올리지 않았다.

별도 코드 리뷰에서 수집 소유권, resource URI 교체, 파일 충돌 상태를 점검하고 수정했다. 최종 재검토에 남은 차단 문제는 없었다. 최초 전체 검증의 1건 실패는 originalValue 문자열을 기대하던 이전 계약 테스트였으며, 원본 구조/transform 검증으로 갱신 후 전체 0 failed를 확인했다.
