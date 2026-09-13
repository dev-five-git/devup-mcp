# Devup Bridge

devup-mcp 가 Figma 를 읽을 때 공식 MCP 의 요금 한도를 쓰지 않게 해 주는 플러그인입니다.

## 왜 필요한가

devup-mcp 의 수집은 읽기 스크립트를 공식 MCP 의 `use_figma` 로 보내는 방식이고,
한도가 걸리는 곳이 바로 그 도구입니다. 화면 하나를 받는 데 snapshot 만 십수 회가
들어가므로 한도는 금방 바닥납니다.

그 스크립트들이 건드리는 것은 문서화된 Plugin API 뿐입니다.

```
getNodeByIdAsync   getStyleByIdAsync   variables   root   fileKey
getLocalPaintStylesAsync / TextStylesAsync / EffectStylesAsync / GridStylesAsync
```

전부 평범한 플러그인이 쓸 수 있는 것들이라, 우리가 돌리는 플러그인이 같은 일을
대신할 수 있습니다. 그 경로에는 한도가 없습니다.

## 설치

빌드할 필요 없습니다. `dist/` 가 저장소에 들어 있습니다.

Figma 데스크톱 앱에서 **Plugins → Development → Import plugin from manifest** 를
고르고 `plugin/manifest.json` 을 선택하면 끝입니다.

브라우저판에서는 개발 플러그인을 불러올 수 없으므로 데스크톱 앱이 필요합니다.

### 플러그인 소스를 고쳤다면

```bash
cd plugin
npm install
npm run build   # dist/ 를 다시 만든다 — 함께 커밋해야 한다
```

`dist/` 는 의도적으로 커밋합니다. 받는 사람이 Node 없이 곧장 import 할 수 있게
하려는 것이고, 그 대가로 원본과 어긋날 위험이 생기므로 CI 가 매번 다시 빌드해
`git diff --exit-code -- dist` 로 대조합니다. 빌드는 재현 가능합니다 — 같은 입력에서
같은 바이트가 나옵니다. 어긋난 채로는 병합되지 않습니다.

## 쓰는 법

읽으려는 파일을 열고 플러그인을 실행한 뒤, 창을 **열어 둔 채로** devup-mcp 를
쓰면 됩니다. 창을 닫으면 연결이 끊기고 그 파일의 읽기는 다시 공식 MCP 로 갑니다.

창에 표시되는 점이 상태입니다.

| 표시 | 뜻 |
|---|---|
| 초록 | devup-mcp 에 연결됨. 이 파일의 읽기는 한도를 쓰지 않습니다 |
| 빨강 | devup-mcp 를 찾지 못함. 2초마다 다시 시도합니다 |

devup-mcp 쪽은 아무 설정도 필요 없습니다. 플러그인이 붙어 있으면 그 파일의
스크립트 읽기를 브리지로 보내고, 안 붙어 있으면 호출마다 곧장 공식 MCP 로
넘어갑니다. 켜 두어서 잃는 것은 없습니다.

## 알아 둘 것

**포트를 바꾸려면 세 곳을 함께 고쳐야 합니다.** Figma 는 플러그인이 접속할 수 있는
주소를 manifest 에 미리 적어 두게 하며, 이 목록은 실행 중에 바뀌지 않습니다.
그래서 `DEVUP_FIGMA_BRIDGE_PORT` 만 바꾸면 devup-mcp 는 새 포트에서 기다리는데
플러그인은 여전히 1993 을 두드리게 되고, **아무 오류 없이 공식 MCP 로 폴백**합니다.
아끼려던 한도가 그대로 나가므로 눈치채기 어렵습니다.

바꿔야 한다면 `manifest.json` 의 `allowedDomains`, `src/code.ts` 의 `PORT`,
그리고 환경 변수를 모두 같은 값으로 맞춘 뒤 플러그인을 다시 빌드·설치하십시오.

끄려면 `DEVUP_FIGMA_BRIDGE_PORT=off` 를 주면 됩니다.

**읽기 전용입니다.** 실행되는 스크립트는 devup-mcp 의 읽기 스크립트에서 생성되며
문서를 바꾸는 호출을 포함하지 않습니다. 데이터는 같은 기기의 devup-mcp 로만
나갑니다(`127.0.0.1`).

**한 기기에서 devup-mcp 를 여러 개 띄우면** 먼저 뜬 쪽이 포트를 잡고, 나머지는
브리지 없이 공식 MCP 로 동작합니다. 오류가 아니라 정상 동작입니다.

**`ws://127.0.0.1` 은 쓸 수 없습니다.** Figma 는 `allowedDomains` 에 그 주소를 적으면
"유효한 URL 이 아니다"라며 **매니페스트 자체를 거부**해 플러그인이 실행되지 않습니다.
`localhost` 로만 적어야 하고, UI 도 같은 이름으로 접속합니다. 실제 설치에서 확인한
동작입니다.

**파일을 하나만 열어 두십시오.** 플러그인은 붙을 때 `figma.fileKey` 로 자기 파일을
알리는데, 이 값이 **비어 오는 경우가 있습니다**(Dev Mode 에서 확인). 그때도 연결은
등록되지만, 어느 파일인지 모르므로 **혼자 붙어 있을 때만** 읽기를 맡습니다. 키 없는
플러그인이 둘 이상이면 어느 쪽이 대상 파일인지 가릴 수 없어 공식 MCP 로 넘어갑니다 —
엉뚱한 파일을 읽어 주는 것보다 낫기 때문입니다.

## 스크립트는 복사본이 아닙니다

`src/generated/` 는 `npm run build` 가 devup-mcp 의 원본에서 만들어 냅니다.

```
crates/devup-mcp-figma/src/scripts/*.js   ← 원본 (공식 MCP 경로도 이것을 씀)
        ↓ plugin/scripts/gen-scripts.mjs
plugin/src/generated/scripts.js           ← 중간 산출물 (커밋하지 않음)
        ↓ rspack + scripts/inline-ui.mjs
plugin/dist/{code.js,ui.html}             ← 최종 번들 (커밋함, CI 가 대조)
```

두 경로가 같은 소스를 쓰므로 봉투가 갈라질 수 없습니다. 원본의 플레이스홀더는
함수 인자로 바뀌고, 치환되지 않은 것이 남으면 빌드가 실패합니다.

`__DEVUP_SNAPSHOT_CURSOR__` 만 예외로 그대로 남습니다. 값이 아니라 Rust 디코더와
공유하는 마커 노드 id 라서, 치환하면 페이지네이션이 조용히 깨집니다.
