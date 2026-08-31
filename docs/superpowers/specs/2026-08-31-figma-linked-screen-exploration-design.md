# Figma 링크 기반 화면 탐색과 사용 리소스 수집 설계

## 상태와 검증 기준선

- 작성일: 2026-08-31
- 상태: 사용자 승인
- 구현 저장소: dev-five-git/devup-mcp
- 기준 JavaScript 저장소: dev-five-git/devup-figma-plugin
- 승인된 접근: 명시적 탐색 후 정확한 node 변환(A안)
- 대상 실파일: 85CgSws3o5XsLv7aAwWJyS

WQUW-151의 Jira 링크가 가리키는 3879:35481은 실제 화면이 아니라
"[FR-026] 본연체"라는 4323×97 제목 frame이다. 공식 Figma MCP의 읽기 전용
Plugin API로 확인한 결과, 같은 페이지의 제목 띠 아래에는 녹음, loading,
proofread 및 확인 상태를 나타내는 360px 화면 frame들이 배치되어 있다.

사용자가 Figma Code 패널에서 확인한 전체 proofread 코드는
3879:35518("A : STORY-F-PROOFREAD", 360×740)과 정확히 일치한다. 해당
화면은 h3, bodyXs, bodySmSemibold, caption, h5, body, bodySemibold 등의
text style과 backgroundLight, containerBackground, text, primary, border,
gray300, textLight, background, primaryLight, innerBg, borderBold 등의 color
variable을 실제로 사용한다.

현재 devup_figma_to_ui는 variable/style 수집을 요청하지 않아 위 이름 대신 hex와
개별 font 속성을 출력한다. 또한 live snapshot이 getStyledTextSegments를 호출하지
않아 "[1. 이름]" 같은 부분 스타일의 nested Text typography를 보존하지 못한다.

## 목표

1. Jira, 문서 또는 대화에서 받은 Figma 링크가 제목 띠나 주석을 가리켜도 관련
   화면 후보를 에이전트가 자율적으로 찾을 수 있게 한다.
2. 후보가 여러 개일 때 하나를 조용히 추측하지 않고 전체 후보와 선택 근거를
   반환한다.
3. 선택된 화면을 변환할 때 실제 사용한 로컬 및 원격 variable/style만 정확히
   수집해 DevupUI $token과 typography를 복원한다.
4. mixed text segment의 children, 줄바꿈, 색상 및 typography를 보존한다.
5. direct Figma MCP와 host 공식 Figma MCP handoff가 동일한 Rust collector와
   결과 계약을 사용한다.

## 비목표

- Jira 자연어를 MCP 서버 내부에서 해석하거나 상태로 저장하지 않는다.
- 여러 후보 중 제품 의미상 올바른 화면을 서버가 임의 선택하지 않는다.
- OAuth token, 공식 Figma MCP credential 또는 사용자 계정 정보를 읽거나
  fixture에 기록하지 않는다.
- 전체 페이지 subtree를 한 번에 snapshot하여 응답 한도를 소모하지 않는다.

## 사용자 흐름

~~~text
Jira/Figma 링크
  -> devup_figma_explore
  -> anchor와 공간적으로 연결된 화면 후보 + text preview
  -> 에이전트가 Jira 요구사항과 후보를 대조
  -> 필요한 canonical URL 각각을 devup_figma_to_ui로 변환
  -> 사용된 variable/style이 반영된 DevupUI TSX
~~~

기존 devup_figma_search는 파일 전체에서 이름으로 화면을 찾는 용도로 유지한다.
devup_figma_explore는 이미 주어진 링크를 기준으로 주변 화면 묶음을 찾는 별도
도구다.

## devup_figma_explore 계약

~~~json
{
  "url": "https://www.figma.com/design/<fileKey>/<name>?node-id=<id>",
  "limit": 50,
  "includeTextPreview": true,
  "sourcePolicy": "auto | direct | host"
}
~~~

완료 응답은 다음 정보를 포함한다.

- anchor: 링크 원본 node의 id, name, type, bounds, child count 및 분류
- group: 탐지된 요구사항/화면 묶음의 bounds와 제목
- candidates: canonical URL, node id, name, type, bounds, child count,
  text preview, score 및 모든 selection reason
- truncated: projection 또는 limit 때문에 후보가 잘렸는지 여부
- source와 diagnostics

같은 이름의 후보와 여러 상태 화면을 모두 유지한다. 정렬은 group 안에서 위에서
아래, 왼쪽에서 오른쪽, 마지막으로 node ID 순서로 결정해 source와 실행 순서에
무관하게 동일하다.

## 주변 context projection

Figma Plugin API 접근에 필요한 JavaScript는 데이터를 읽는 projection만 담당하고,
분류와 ranking은 Rust가 담당한다.

projection은 anchor와 ancestor page의 compact metadata, anchor의 가로 band와
교차하는 bounded page sibling, 다음 peer requirement heading 후보, screen
후보마다 제한된 첫 text preview만 반환한다. 전체 node field 또는 전체 페이지
subtree는 읽지 않는다. 반환 node 수와 text 길이는 고정 상한을 적용하고 초과 시
truncated=true를 반환한다.

Rust는 다음 신호로 anchor를 분류한다.

- screen-like: 일반적인 화면 폭/높이, 충분한 높이와 descendants
- heading-like: 낮은 높이, 큰 aspect ratio, 소수 child, 요구사항/설명형 이름
- annotation-like: TEXT/VECTOR 또는 작은 설명 frame
- container-like: SECTION/FRAME 안에 여러 screen-like child

heading-like anchor에서는 가로 band 아래의 screen-like page sibling을 다음 peer
heading 전까지 하나의 group으로 묶는다. WQUW-151에서는 3879:35481을 anchor로
두고 3879:35518을 포함한 관련 상태 화면 전체를 반환한다.

## 리소스 수집 모드

기존 include_variables bool을 의미가 명확한 내부 정책으로 교체한다.

~~~rust
enum ResourceScope {
    None,
    Used,
    File,
}
~~~

- None: search/explore처럼 token 값이 필요 없는 compact projection
- Used: devup_figma_to_ui가 선택 subtree에서 실제 참조하는 리소스만 수집
- File: devup_figma_to_json이 파일의 로컬 variable/style catalog를 수집

### Used 수집

1. node snapshot을 먼저 완성한다.
2. Rust가 snapshot의 모든 JSON field를 재귀적으로 검사한다.
3. 모든 boundVariables 하위 id와 styled segment binding을 variable ID 집합으로
   수집한다.
4. textStyleId, fillStyleId, strokeStyleId, effectStyleId, gridStyleId,
   backgroundStyleId를 type별 style ID 집합으로 수집한다.
5. ID를 정렬·중복 제거하고 bounded batch로 getVariableByIdAsync와
   getStyleByIdAsync를 호출한다.
6. 반환된 실제 id/name/remote/type으로 codegen token map을 구성한다.

이 방식은 전체 파일의 모든 token을 읽지 않으면서도 선택 화면에 필요한 token을
빠짐없이 얻는다. ID 직접 조회이므로 파일 로컬 token뿐 아니라 선택 node에 실제
binding된 library/remote token도 처리한다.

### File 수집

devup_figma_to_json은 기존의 local collection/style catalog를 유지한다. 사용된
원격 리소스를 확인할 수 있는 node/page scope에서는 Used scanner 결과를 합친다.
전체 파일에서 원격 사용 여부를 완전 탐색하지 않은 경우 usedRemoteComplete=false를
유지해 완전성을 과장하지 않는다.

## 실패와 fallback

- ID 조회가 성공하면 이름 기반 $token 또는 typography를 사용한다.
- binding은 있으나 권한, 삭제 또는 API 오류로 리소스를 읽지 못하면 token 이름을
  추측하지 않는다.
- 해당 속성은 snapshot의 resolved hex/font 값으로 fallback하고
  DEVUP_RESOURCE_UNRESOLVED 진단에 node ID, field와 resource ID를 남긴다.
- 일부 batch 실패는 성공한 리소스를 버리지 않지만 completeness는 partial로
  낮춘다.
- Figma version이 수집 도중 달라지면 기존 version guard와 동일하게 재시도 가능한
  충돌로 처리한다.

## styled text segment 수집과 codegen

snapshot.js는 TEXT node마다 다음 필드를 요청한 getStyledTextSegments 결과를
styledTextSegments로 저장한다.

- characters/start/end
- fills/fillStyleId
- fontName/fontSize/fontWeight
- textStyleId
- textDecoration/textCase
- lineHeight/letterSpacing
- listOptions/indentation/hyperlink

segment 필드 목록은 Rust binary에 포함되는 별도 manifest로 관리한다. raw node의
일반 field 수집은 그대로 유지한다.

codegen은 root Text와 nested Text에 동일한 text style map을 전달한다. segment의
textStyleId가 해석되면 typography="<styleName>"을 출력하고 중복되는 raw font
속성은 생략한다. style이 다른 segment만 nested Text로 감싸고 동일한 segment는
기존 텍스트와 줄바꿈 구조를 유지한다.

## 테스트 전략

### Rust 단위 테스트

- heading/screen/annotation 분류, 다음 heading 경계와 공간 group
- 후보 중복 보존과 안정 정렬
- recursive bound variable 및 style reference 추출
- local/remote/missing resource merge와 completeness
- mixed segment의 root/nested typography

### collector 및 MCP 계약 테스트

- direct/host explore 결과 parity
- bounded projection과 truncation
- ToUI가 ResourceScope::Used를 계획하는지 검증
- ToJson이 ResourceScope::File을 유지하는지 검증
- unresolved resource가 raw fallback과 진단을 함께 반환하는지 검증

### 실제 회귀 fixture

공식 Figma MCP에서 읽은 3879:35481 주변 compact projection과 3879:35518
subtree/resource 결과를 비밀정보 없이 JSON fixture로 고정한다. snapshot은 다음을
검증한다.

- explore가 실제 관련 화면 묶음을 반환한다.
- 생성 TSX에 VStack children과 긴 이야기 본문이 존재한다.
- bg="$backgroundLight", color="$primary", typography="h3",
  typography="body"가 존재한다.
- 부분 강조가 typography="bodySemibold" nested Text로 생성된다.

기존 JavaScript plugin 268 fixture corpus와 모든 Rust test를 함께 통과해야 한다.

## 보안과 개인정보

- Figma OAuth credential은 기존 keyring 경계 밖으로 노출하지 않는다.
- 공식 Figma MCP host credential은 계속 devup-mcp가 직접 읽지 않는다.
- fixture에는 access token, 사용자 email/handle, request header와 OAuth callback
  payload를 기록하지 않는다.
- 실제 디자인 텍스트는 회귀에 필요한 최소 subtree만 저장하고 source file key와
  node ID 외 계정 식별정보는 제거한다.
- 모든 Figma 동작은 읽기 전용이다.
