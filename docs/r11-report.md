# R11 — SECTION 인덱스 회귀 수정

## 실파일에서 확인한 원인

- 두 현장 보고서를 읽기 전용으로 확인했다. 119 보고서의 실제 경로는 `docs/wquw-119/r10-report.md`였다. 다른 저장소는 수정하지 않았다.
- 이 워크트리의 `target/debug/devup-mcp.exe`로 인증 상태를 확인하고, 문제의 SECTION `4279:7806`을 직접 호출했다. 설치된 devup-mcp MCP 도구는 사용하지 않았다.
- 비교 실험: 수정된 오류 상세 진단은 유지한 채 SECTION JavaScript만 `git show 129c85fde42a:crates/devup-mcp-figma/src/scripts/section_index.js`의 R10 원본으로 바꾸어 `cargo build -j 2 -p devup-mcp`로 빌드했다. 같은 SECTION을 호출하자 `category=truncated-text`, `path=$.content[0].text`, `observed.textBytes=20500`, `truncationMarker=true`가 반환됐다. 실제 upstream의 `// truncated to 20kb` 접미사를 확인했다. 비교 후 수정 스크립트를 복구했다.
- 따라서 재현된 실파일 실패 원인은 **응답 크기 초과 → JSON 절단 → snapshot 추출 실패**다. `received`는 유효한 snapshot 수신을 뜻하지 않는다. 스키마가 정상인 작은 응답은 파싱되므로 새로운 스키마 계약 불일치가 원인은 아니다.
- 부모 접근 반복은 별도의 성능 회귀다. 실제 plugin timeout이 원인이었다고 주장하지 않는다.

## 실패를 먼저 확인한 테스트

- 18,201노드, 깊이 200, 화면 1개의 실제 JavaScript 실행: R10 응답 **551,804바이트**, 부모 getter **1,847,100회**. 측정된 upstream 20,480바이트 절단을 적용하면 JSON.parse가 실패했다. 크기 및 부모 접근 상한 테스트 2개 모두 RED를 확인했다. 수정 후 동일 fixture는 **789바이트**, 자손 부모 getter **0회**, 유지 노드 2개와 subtreeNodeCount=18,201을 확인했다.
- 선택 노드만 소속을 반환하는 테스트: R10이 전체 자손 맵을 반환하여 실패했다. 과대 메타데이터를 upstream에 넘기기 전에 명시적으로 거절하는 테스트도 실패했다.
- 40개 화면 메뉴 테스트: 19,777바이트로 안전 상한을 넘어 실패했다. 후보 목록을 보존하면서 선택적인 텍스트 미리보기를 줄인 뒤 통과했다.
- Rust parser 테스트: 오류 details.category가 null이라 실패했다. 절단, 유효하지 않은 JSON, 스키마 불일치, upstream 오류, snapshot 부재를 구분하며 SECRET 표식이 상세 진단에 노출되지 않는지 검증한다.
- collector 테스트: 선택 ID가 SectionIndex 호출에 전달되지 않아 rootIds=None으로 실패했다. 캐시에 없는 ID의 재조회도 검증한다.
- 실제 MCP 통신 경계 테스트: tools/call이 구조화된 tool error 대신 JSON-RPC Err를 반환하여 실패했다. content와 structuredContent의 동일 오류 객체를 검증한다.

## 수정

- SECTION은 선택 요청에 등장한 ID만 nodeScreenIds에 담는다. 이미 수행하는 BFS에서 부모 ID를 기록하고 소속을 전달하므로 각 자손마다 plugin parent 사슬을 다시 읽지 않는다.
- export 진단의 screenId는 기존 서버의 snapshot parentId/childrenIds 경로로 계속 계산한다. 캐시에서 처음 요청받은 자손 ID는 새 SECTION 인덱스를 조회하여 selectionIssues와 교정할 화면 ID를 제공한다. frameIds의 의미를 바꾸거나 선택을 조용히 교정하지 않는다.
- JSON 전체의 UTF-8 크기를 19KiB로 제한한다. 초과하면 선택적인 미리보기부터 비워 화면 후보 목록을 유지한다. 미리보기 없이도 넘치는 비정상적으로 큰 메타데이터는 DEVUP_SECTION_INDEX_TOO_LARGE로 명시적으로 거절한다. 후보를 조용히 누락하지 않는다.
- snapshot 추출 실패에는 upstreamType, hasStructuredContent, contentCount, category, path, observed, expectedSchema, 검사 제한 정보를 넣는다. 원문·디자인 값·임의 키·serde 오류 문자열을 복사하지 않는다.
- 도구 실행 실패는 isError=true와 구조화된 error(code/message/retryable/details/rpcCode), server 식별자를 content와 structuredContent에 동일하게 제공한다. 알 수 없는 도구와 resources/read 같은 프로토콜 오류는 기존 JSON-RPC 오류를 유지한다.
- JavaScript 행동 테스트를 cargo test에도 연결해 Rust 테스트만 실행할 때 대형 SECTION 검증이 빠지지 않게 했다.

## 기존 테스트와 golden

- Golden 변경 없음.
- R10 소속 테스트는 기존 `3997:46703 → 3997:46690` 검증을 유지하고 조회할 ID를 입력으로 지정했다. 전체 자손 전송 대신 요청 ID 조회로 바뀐 계약을 반영한 것이다.
- 기존 부정 테스트의 code, 메시지, retryable, 복구 안내 검증은 유지했다. 테스트 클라이언트 helper만 isError=true인 structuredContent를 실패로 읽도록 변경했다. stdio 테스트는 artifact 오류의 새 tool-result 위치에서 동일한 build identity를 검사한다.
- R10-1, R10-3, R10-4, R10-5의 판정·안내를 제거하거나 약화하지 않았다.

## 최종 검증

- `cargo test --workspace -j 2 --no-fail-fast`: **770 passed / 0 failed / 2 ignored**. 기준선 766에 R11 Rust 테스트 4개를 추가했다.
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: **12 passed / 0 failed**. 기존 7개와 신규 5개 모두 유지했다. cargo suite 안에서도 같은 JavaScript 파일이 실행된다.
- `cargo fmt --all -- --check`: 통과.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: 통과, warning/error 0건.
- `git diff --check`: 통과. Rust test 링크 과정의 MSVC lib/exp 생성 안내는 linker_messages 경고로 표시되지만 테스트 실패는 없다.
- 최종 전체 실행 로그에서 다음 자체 target 바이너리와 신규 함수의 실행·통과를 확인했다: `target/debug/deps/upstream_error_surfacing-7f7cffa49f13ec99.exe`, `target/debug/deps/section-a7e6afaa26260d8c.exe`, `target/debug/deps/snapshot-d85f713f8faa3fcc.exe`.
- 실제 실행한 서버의 절대 경로: `C:/Users/owjs3/orca/workspaces/devup-mcp/r11-section-index-regression/target/debug/devup-mcp.exe`. `--version`: `devup-mcp 0.4.4 (129c85fde42a-dirty)` — 커밋 전 수정 소스 빌드다.

**자기 target 에서 검증했고 실행된 테스트가 내 것임을 확인했다.** CARGO_TARGET_DIR은 설정하지 않았고 환경에도 없음을 확인했다. build/test/clippy는 모두 `-j 2`로 실행했다.

## 수정본 실파일 확인

- 동일 SECTION `4279:7806`이 `selection_required`, isError=false로 정상 응답했다.
- 두 문제 화면을 frameIds=[3997:46690, 3997:46129]로 함께 export: **state=complete**, upstream 호출 17건, 130,673ms. 각각 TSX **8,187 / 10,053 UTF-8 바이트**를 반환했다.
- `3997:46703`의 `DEVUP_CODEGEN_NON_RENDERING_ASSET` 진단에 **screenId=3997:46690**이 포함됐다. R10-2의 목적을 실제 화면에서도 유지했다.
- 별도 자손 선택 요청 frameIds=[3997:46703]: **DEVUP_FIGMA_NODE_NOT_FOUND**, selectionIssues.reason=descendant-of-screen, screenId=3997:46690, nextAction.arguments.frameIds=[3997:46690]을 확인했다. 실패도 structuredContent.error로 반환됐다.
- 위 검증은 자체 빌드 stdio 서버의 직접 연결로 수행했다. Figma 파일은 읽기만 했다. 설치된 devup-mcp MCP 도구는 호출하지 않았다.
- 실제 디자인 원문/생성 TSX/인증정보는 이 커밋에 추가하지 않는다. 임시 검증 응답과 로그는 이 워크트리 target에만 보관했으며, 커밋 후 cargo clean으로 정리한다. push하지 않는다.
