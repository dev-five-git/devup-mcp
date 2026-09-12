# devup-mcp

Rust-native MCP server that reads Figma designs and generates DevupUI artifacts. One binary acts as a local stdio MCP server and a read-only client of Figma Remote MCP.

저장소는 Cargo workspace이며 `devup-mcp` 실행 crate, OAuth·upstream·snapshot을 담당하는 `devup-mcp-figma`, TSX·theme projection을 담당하는 `devup-mcp-devup-ui`, PNG 비교 library/CLI인 `devup-mcp-visual`로 구성됩니다. 별도 IR/auth/server crate 없이 MCP 제품 설치 단위는 `devup-mcp` 하나입니다.

## 도구

Figma 쪽 4개, 프로젝트 쪽 5개, 모두 9개입니다.

- `devup_figma_export`: Figma를 한 번 수집해 요청한 `outputs`만 투영합니다. TSX가 산출물이고, `componentTsx`·`responsiveTsx`·`devup.json`·source map·asset manifest·reference PNG를 같은 수집에서 함께 얻거나, `cache.artifactId`로 재수집 없이 추가 투영할 수 있습니다. raw snapshot·raw payload는 구현이 아니라 진단에 쓰는 것이라 `debug: true`로만 열립니다.
- `devup_figma_search`: page, section, frame, component를 이름으로 탐색. URL에 `node-id`가 있으면 **그 노드와 그 아래로 범위를 좁히고**, 없으면 파일 전체를 검색합니다. 둘 중 무엇을 했는지는 응답의 `scope`가 알려줍니다
- `devup_figma_explore`: 링크된 요구사항/라벨 주변의 실제 화면 후보를 공간 순서로 탐색
- `devup_figma_auth`: 연결 상태 확인, 브라우저 OAuth 로그인, 로그아웃, 사전 등록 자격증명 주입(`configure`), 연결 실패 원인을 실측해 보고하는 `doctor`
- `devup_project_context`: 프로젝트의 실제 `devup.json` 토큰, `openapi.json` 엔드포인트, Vespertide 모델을 읽음. 중첩 체크아웃과 빌드 산출물 디렉터리는 스캔에서 제외하고 무엇을 제외했는지 보고
- `devup_ui_validate`: 생성한 TSX를 프로젝트의 실제 `devup.json`에 대조해 검증. `ok`는 개수가 아니라 심각도로 판정
- `devup_stack_diff`: DB 모델부터 생성된 API 클라이언트까지의 층간 드리프트 탐지. 모든 발견은 명시적 `confidence`를 가짐
- `devup_visual_compare`: 소비자가 만든 `actual.png`를 reference PNG 하나와 비교. **스스로 렌더하지 않고 브라우저를 띄우지도 명령을 실행하지도 않습니다** — 경로는 allowlist 안이어야 합니다. 기본 threshold는 0.005(0.5%). renderer 환경 manifest가 없거나 불완전하거나 유효하지 않으면 `visual.passed`가 true여도 판정은 `inconclusive`입니다
- `devup_feature_trace`: 명시적 anchor(`routePath`, `figmaNodeId`/`artifactId`, `operationId` 또는 `apiPath`+`method`, `componentPath`, `tableName`)에서 출발해 화면→API→라우트→모델→컬럼 슬라이스와 acceptance matrix를 읽어냄. **요구사항 산문만 주면 거절합니다.** 각 hop은 근거가 있거나 `UNVERIFIED`로 표시되고, 생성 산출물 소유권·UI 재사용 순위·잘림 상한을 함께 보고합니다

`devup_figma_to_ui`와 `devup_figma_to_json`은 각각 `devup_figma_export`에 `outputs: ["tsx"]`, `outputs: ["devupJson"]`을 넘긴 것과 같아서 제거했습니다. 도구가 셋이면 모든 클라이언트가 세 개의 스키마를 컨텍스트에 싣고도 어느 것을 부를지 매번 판단해야 했습니다.

devup-mcp는 Figma Plugin API의 readable data property를 raw JSON으로 보존하고, 알려지지 않은 runtime field는 `extra`, 실패한 getter는 `fieldErrors`로 유지합니다.

### 응답에 무엇이 들어오는가

`devup_figma_export`는 **요청한 `outputs`가 만드는 키만** 추가합니다. `outputs`를 생략했을 때의 스키마 기본값은 **`["tsx"]`** 하나뿐이며, 아래 표는 무엇을 더 요청할 수 있는지에 대한 **레퍼런스**이지 한 번에 전부 요청하라는 목록이 아닙니다.

`devupJson`은 기본값에서 빠졌습니다. **프로젝트에 이미 `devup.json`이 있으면 코드가 맞춰야 할 대상은 그 파일**이지 Figma에서 새로 뽑은 이름 집합이 아니고, 그 파일을 읽는 도구는 `devup_project_context`입니다. `devupJson`은 프로젝트에 아직 `devup.json`이 없거나, 그 파일이 정의하지 않은 토큰을 새로 들일 때만 함께 요청하세요.

| output | 추가되는 키 |
|---|---|
| `tsx` | `tsx` |
| `componentTsx` | `componentTsx` |
| `responsiveTsx` | `responsiveTsx`, `responsiveSlots`, (표현 불가한 값이 있으면) `responsiveUnrepresented` |
| `devupJson` | `devupJson`, `themeCounts`, `themeCompleteness`, `conflicts`, `unresolvedVariables` |
| `sourceMap` / `assetManifest` / `referencePng` | 같은 이름의 키 |
| `rawSnapshot` / `rawPayload` | 같은 이름의 키 — **`debug: true` 필요** |

그 밖에 항상 붙는 것은 `status`, `quality`, `completeness`, `cache`, `collection`, `source`, `targetKind`, `failures`, `assetSummary`, `outputPaths`뿐입니다.

조건이 맞을 때만 붙는 키는 다음과 같습니다.

| 키 | 나오는 조건 |
|---|---|
| `fidelity` / `completenessReport` | 결과가 exact/complete가 **아닐 때**, 또는 `includeDiagnostics: true`일 때 |
| `deliverable` | `status`가 `complete`이고 TSX를 만들었을 때 |
| `warnings` | 숨김 노드처럼 실패가 아닌 자산 문제가 있을 때 |
| `frames` | Section에서 여러 화면을 한 번에 export했을 때 |
| `selection` / `nextAction` | `status`가 `selection_required`일 때 |

`fidelity`와 `completenessReport`를 기본에서 뺀 이유는 깨끗한 결과에서 `quality`가 이미 한 말을 되풀이할 뿐이기 때문이고, 그만큼(측정값 797 B) 매 응답이 가벼워집니다.

#### `assetSummary` — 자산이 "없음"인지 "못 받음"인지

자산은 없는 것과 못 받은 것이 전혀 다른 상황인데 예전에는 둘 다 빈 목록으로 보였습니다. 지금은 `assetSummary`가 구분해 줍니다.

- `status`: `none`(자산이 없다) · `unknown`(스냅샷이 불완전해 판단 불가) · `not-collected` · `partial` · `collected`
- `discovery`: `complete` | `incomplete`, `discoveryReason`: `index-only` | `snapshot-incomplete`
- `discoveredCount` / `collectedCount` / `manifestIncluded`
- `unavailable[]`: 항목마다 `assetId`, `nodeId`, `errorCode`와 **`reason`** — `hidden-node`(숨김 노드) · `export-failed`(export 실패) · `not-requested`(애초에 요청하지 않음) · `capture-not-in-artifact`(요청했지만 이 artifact엔 그 format/scale이 없음)

숨김 노드 실패는 `failures[]`가 아니라 `warnings[]`로 갑니다. 구현을 막는 실패가 아니기 때문입니다.

`rawSnapshot`과 `rawPayload`는 수집한 디자인을 raw로 담은 것이라 `debug: true` 없이는 거절됩니다. 화면을 구현하는 데는 필요 없습니다 — 실제 캡처 10개 화면에서 tsx가 node·text·typography·asset·layout 기대치를 100% 담고 있습니다. 쓰는 자리는 하나입니다: **화면이 이상해 보일 때 생성기 탓인지 디자인이 원래 그런지 판정하는 것.** 그때는 디자인을 코드 옆에 놓고 읽어야 하고, 그게 이 플래그입니다.

에러는 호출 자체가 잘못된 경우(`DEVUP_INVALID_INPUT`, 없는 node/파일, 만료·부적합한 `artifactId` 등) JSON-RPC `-32602 INVALID_PARAMS`로, 그 밖의 실패는 `-32603 INTERNAL_ERROR`로 옵니다. 인자를 고쳐 다시 부를 일인지 멈추고 보고할 일인지를 메시지를 파싱하지 않고 구분할 수 있습니다. 정확한 `code`와 `retryable`은 예전처럼 `data`에 그대로 실립니다.

devup-mcp는 Figma Remote MCP에 직접 붙습니다 — OAuth discovery, Dynamic Client Registration, PKCE S256, 일시적인 `127.0.0.1` callback을 구현합니다. Figma는 MCP Catalog에 승인된 client의 registration만 허용하므로 등록은 allowlist에 있는 `client_name`으로 이루어집니다(기본값 `Codex`). Figma PAT나 사용자가 만든 OAuth app은 필요하지 않습니다.

## 빌드와 설치

### MCP Bundle (`.mcpb`) — 툴체인 없이 한 번에 설치

릴리스마다 `devup-mcp-<version>.mcpb` 파일 하나가 함께 올라갑니다. 이 하나에 Linux x86_64, Windows x86_64, macOS universal 바이너리가 **모두** 들어 있고, `manifest.json`의 `server.mcp_config.platform_overrides`가 실행 시점에 호스트의 운영체제에 맞는 바이너리를 고릅니다. 운영체제별로 어떤 파일을 받아야 하는지 고를 필요가 없고, Rust 툴체인도 Node 런타임도 `cargo install`도 필요하지 않습니다.

1. [Releases](https://github.com/dev-five-git/devup-mcp/releases)에서 `devup-mcp-<version>.mcpb`를 받습니다.
2. `.mcpb`를 지원하는 호스트(예: Claude for macOS/Windows)에서 파일을 엽니다.
3. 설치 대화상자의 **Workspace directory**에 코드를 생성할 프로젝트 디렉터리를 지정합니다. 이 디렉터리가 devup-mcp가 파일을 쓸 수 있는 **유일한** 위치이며, `..`·다른 drive·symlink로 그 밖을 가리키는 경로는 기록 전에 거절됩니다. 여러 root가 필요하면 아래 stdio 설정으로 `--allow-write-root`를 반복해 등록하세요.

`.mcpb`는 그냥 zip이므로 `unzip -l`로 내용을 확인할 수 있고, 안의 바이너리는 같은 릴리스에 따로 올라가는 것과 같은 파일입니다. CI는 pack 직후 아카이브를 다시 읽어 Unix 바이너리에 실행 비트가 남아 있는지 확인하고, 없으면 릴리스를 게시하지 않습니다. 그럼에도 macOS에서 설치 직후 서버가 `EACCES`로 실패한다면 호스트가 압축을 풀면서 권한을 지운 경우이며([mcpb#294](https://github.com/modelcontextprotocol/mcpb/issues/294)), 그때는 같은 릴리스의 `devup-mcp-macos-universal` 바이너리를 직접 받아 아래 stdio 설정으로 등록하면 됩니다.

### 소스에서 빌드

Rust 1.98 이상이 필요합니다. compile-in Figma 탐색 행동 fixture를 직접 실행하려면 CI와 동일한 Node.js 24가 필요하며 제품 binary에는 Node가 필요하지 않습니다.

```bash
cargo install --git https://github.com/dev-five-git/devup-mcp.git --branch owjs3901/figma-remote-mcp devup-mcp
```

설치 또는 binary 교체 후에는 먼저 로컬 진단을 실행합니다.

```bash
devup-mcp --version
devup-mcp --self-check
```

`--version`은 package version과 build ID를, `--self-check`는 network/OAuth 없이 binary,
credential backend 초기화와 server 구성을 안전한 JSON으로 확인합니다. 둘 다 성공하지만
등록된 connector가 `Transport closed`를 반환하면 MCP host가 교체 전 process의 종료된
stdio pipe를 보유한 상태이므로 host의 MCP 연결을 재시작하거나 다시 등록해야 합니다.
새로 실행된 server가 host가 보유한 이전 pipe를 스스로 복구할 수는 없습니다.

서버 시작 시 OAuth 준비가 실패하면 **프로세스가 죽지 않고 진단 가능한 오류로 나옵니다.** HTTP 클라이언트 초기화가 실패하면 `DEVUP_FIGMA_DIRECT_UNAVAILABLE`로 `Cannot start Figma OAuth: HTTP client initialization failed. Check TLS configuration and system certificate initialization, then restart devup-mcp.`와 함께 원인 체인을 붙여 돌려줍니다. endpoint URL이 잘못됐으면 `DEVUP_INVALID_INPUT`입니다. 예전에는 이 경로가 panic이라 무엇이 잘못됐는지 알 수 없었습니다 — TLS나 시스템 인증서 설정을 먼저 확인하세요.

소스에서 검증하려면 다음을 실행합니다.

```bash
cargo fmt --all -- --check
node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test -p devup-mcp --test stdio_smoke
cargo insta test --workspace --all-features --check
cargo build --workspace --release
```

## MCP 설정

stdio MCP를 지원하는 클라이언트에 다음과 같이 등록합니다.

```json
{
  "mcpServers": {
    "devup-mcp": {
      "command": "devup-mcp",
      "args": ["--allow-write-root", "/absolute/path/to/workspace"]
    }
  }
}
```

시스템 브라우저는 `devup_figma_auth`의 `login`을 명시적으로 호출할 때만 열립니다. 변환 도구는 자격증명이 없으면 브라우저를 열지 않고 로그인이 필요하다는 오류를 반환합니다. 인증 정보는 운영체제 credential store에만 저장되며 `logout`은 해당 정보만 삭제합니다.

### 인증

```json
{ "action": "status" }
```

`action`은 `status`, `login`, `logout`, `configure`, `doctor` 중 하나이며, 스키마가 이 목록을 그대로 게시합니다. `status`/`login`/`logout`의 응답 형태는 항상 `{ "status": "connected" | "disconnected" }`입니다. Figma에 붙지 못하는 이유를 알고 싶으면 `doctor`를 호출하세요.

```json
{ "action": "doctor" }
```

```json
{
  "status": "disconnected",
  "paths": {
    "direct": {
      "available": false,
      "credentialSource": "none",
      "credentialSourceNote": "Where the OAuth *client registration* credential came from ...",
      "tokenState": "absent",
      "callbackPort": { "port": null, "free": null },
      "registrationClientName": { "value": "Codex", "isDefault": true },
      "reason": "저장된 자격증명 없음. ..."
    }
  },
  "clientSetup": { "constraints": { ... }, "opencode": { ... }, "claudeCode": "...", "codex": "..." }
}
```

`doctor`는 네트워크 호출을 전혀 하지 않습니다. 세 필드는 **서로 다른 것**을 말하므로 함께 읽어야 합니다.

- `credentialSource` — **client 등록 자격증명**(`client_id`/`client_secret`)이 어디서 왔는지. `cli-arg`, `env`, `credential-store`, `none` 중 하나입니다.
- `tokenState` — **사용자의 access token** 상태. `valid`, `expired`, `absent` 중 하나입니다.
- `available` — 지금 direct 경로를 쓸 수 있는지.

`credentialSource: "none"`은 "사전 등록된 client가 주입되지 않았다"는 뜻일 뿐이며, 그 경우 로그인은 `registrationClientName`의 이름으로 동적 등록합니다. **그렇게 로그인한 정상 세션은 `credentialSource: "none"`과 `tokenState: "valid"`를 동시에 보입니다.** 예전에는 이 조합이 모순처럼 보였는데, 지금은 `credentialSourceNote`가 응답 안에서 직접 설명합니다.

`callbackPort`는 `--figma-callback-port`를 지정했을 때만 실측한 `port`/`free`를 담고, `registrationClientName`은 DCR이 보낼 이름(`value`)과 그것이 기본값인지(`isDefault`)를 알려줍니다. 자세한 제약은 아래 "Figma 연결 설정" 절을 참고하세요.

### direct 경로에 사전 등록된 client 자격증명 주입하기

Figma MCP Catalog에 승인된 client(예: 직접 waitlist로 등록해 발급받은 client)의 `client_id`/`client_secret`을 이미 가지고 있다면, devup-mcp에 다음 세 가지 방법 중 하나로 주입해 Dynamic Client Registration을 완전히 건너뛸 수 있습니다. 우선순위는 시작 인자 > 환경변수 > `configure`로 저장한 값입니다.

- **시작 인자**: `devup-mcp --figma-client-id <id> --figma-client-secret <secret>`
- **환경변수**: `DEVUP_FIGMA_CLIENT_ID`, `DEVUP_FIGMA_CLIENT_SECRET`
- **도구**: `devup_figma_auth { "action": "configure", "clientId": "...", "clientSecret": "..." }` — OS credential store(시작 인자/환경변수와는 별도 항목)에 저장되어 프로세스를 재시작해도 유지됩니다.

자격증명이 해석되면 `devup_figma_auth { "action": "login" }`은 registration 엔드포인트를 전혀 호출하지 않고 바로 authorization_code + PKCE 흐름으로 진입합니다. 자격증명이 없으면 DCR을 시도하고, 403이면 그대로 보고합니다. DCR 요청의 `client_name` 기본값은 `"Codex"`입니다(`DEFAULT_CLIENT_NAME`). allowlist는 이름을 정확히 일치시켜 판정하고 `"devup-mcp"`는 거기에 없으므로, 그 이름으로 보내면 등록이 403으로 거절되어 direct 경로 자체가 성립하지 않습니다. 이 등록은 Figma에게 devup-mcp가 아니라 Codex로 기록됩니다. 본인 client가 카탈로그에 승인되면 `--figma-client-name` 또는 `DEVUP_FIGMA_CLIENT_NAME`으로 그 이름을 넘기세요. `client_secret`은 로그, 에러, MCP 응답, `doctor` 출력 어디에도 노출되지 않으며 `doctor`는 `credentialSource`로 존재 여부만 보고합니다.

## Figma 연결 설정

devup-mcp가 Figma에 붙는 경로는 하나입니다 — **원격 OAuth (`direct`)**. `devup_figma_auth { action: "login" }`으로 브라우저 인증. Figma MCP Catalog에 승인된 client만 등록할 수 있습니다.
현재 사용 가능한지는 `devup_figma_auth { action: "doctor" }`로 확인하세요.

Figma 데스크톱 앱의 로컬 Dev Mode MCP(`http://127.0.0.1:3845/mcp`)는 세 번째 경로로 안내했으나 제거했습니다. 읽기 도구 6개(`get_design_context`, `get_variable_defs`, `get_screenshot`, `get_motion_context`, `get_metadata`, `get_figjam`)만 제공하고 그중에 `use_figma`가 없습니다. devup-mcp의 수집은 snapshot·explore·section index·theme 모두 `use_figma`로 스크립트를 실행하므로 로컬에서는 실행할 도구 자체가 없습니다. 도구들이 `fileKey`를 받지 않고 데스크톱 앱에 열려 있는 파일만 가리키는 것도 같은 이유로 맞지 않습니다. "OAuth 없이 바로 쓸 수 있다"는 안내는 확신에 차서 틀린 안내였고, 믿은 쪽이 한 턴을 버린 뒤에야 알게 됩니다.

### 원격 OAuth 등록 제약 (실측)

Figma Remote MCP 등록 엔드포인트는 `POST https://api.figma.com/v1/oauth/mcp/register`입니다. 요청 본문의 `client_name`은 정확히 일치하는 allowlist로만 승인됩니다.

| client_name | 결과 |
|---|---|
| `Codex` | 200 (client_id + client_secret 발급) |
| `Claude Code` | **403** (2026-09-06 실측; 이전 표에는 200으로 적혀 있었음) |
| `OpenCode` | 403 |
| `opencode` | 403 |
| `Cursor` | 403 |
| `VS Code` | 403 |

403 응답 본문은 JSON이 아니라 평문 `Forbidden`입니다. 그래서 많은 클라이언트가 `Invalid OAuth error response ... Raw body: Forbidden`으로 파싱까지 깨집니다. `X-Figma-Plugin-Bundle` 헤더나 User-Agent를 바꿔도 결과는 바뀌지 않습니다. 신규 client 등록은 waitlist를 통해서만 가능합니다: <https://www.figma.com/mcp-catalog/>.

`redirect_uri`도 형태가 고정되어 있습니다.

| redirect_uri | 결과 |
|---|---|
| `http://127.0.0.1:<port>/callback` | 200 |
| `http://127.0.0.1:<port>/mcp/oauth/callback` | 400 |
| `http://localhost:<port>/mcp/oauth/callback` | 400 |

경로는 정확히 `/callback`이어야 하고 호스트는 `127.0.0.1`이어야 합니다 (`localhost` 불가). Figma PAT(`figd_...`)는 `Authorization: Bearer`, `X-Figma-Token` 어느 방식으로도 원격 MCP에서 지원되지 않습니다.

### 숨은 함정 — 콜백 포트 점유

로컬 OAuth 콜백이 쓰는 포트를 OS나 보안 소프트웨어(예: 사내 보안 에이전트)가 이미 점유하고 있으면, 브라우저는 리다이렉트에 "성공"한 것처럼 보이지만 그 요청은 다른 프로세스로 전달됩니다. 클라이언트는 **아무 에러 없이** `Waiting for authorization...` 상태로 영원히 남습니다. 로그인이 멈춘 것처럼 보이면 가장 먼저 콜백 포트를 다른 프로세스가 쓰고 있지 않은지 확인하세요.

기본값은 OS가 매번 빈 임시 포트를 골라주므로(`0`) 이 충돌을 피합니다. 사전 등록한 client의 `redirect_uri`가 고정 포트로 등록되어 있어 특정 포트를 고정해야 한다면 `devup-mcp --figma-callback-port <port>`를 지정하세요. 이 경우 devup-mcp는 그 포트가 이미 사용 중이면 **연결을 기다리지 않고** `DEVUP_FIGMA_CALLBACK_PORT_IN_USE` 오류를 즉시 반환합니다. `devup_figma_auth { "action": "doctor" }`의 `paths.direct.callbackPort.free`에서도 지정한 포트가 실제로 비어 있는지 실측한 값을 확인할 수 있습니다.

### opencode에서 direct 경로 미리 설정하기

Dynamic Client Registration을 건너뛰려면 `mcp.<name>.oauth`에 이미 발급받은 `clientId`/`clientSecret`을 직접 지정합니다.

```json
{
  "mcp": {
    "figma": {
      "type": "remote",
      "url": "https://mcp.figma.com/mcp",
      "oauth": {
        "clientId": "<allowlist된 client_name으로 등록해 발급받은 client_id>",
        "clientSecret": "<allowlist된 client_name으로 등록해 발급받은 client_secret>",
        "scope": "mcp:connect",
        "callbackPort": 19876,
        "redirectUri": "http://127.0.0.1:19876/callback"
      }
    }
  }
}
```

Codex는 allowlist에 있어 별도 설정 없이 등록할 수 있습니다. `Claude Code`는 한때
200이었으나 2026-09-06 실측에서 403으로 거절됐습니다 — allowlist는 Figma가 바꿀 수
있으며, 위 표는 측정 시점의 기록입니다.

devup-mcp는 **직접 경로만** 씁니다. 호스트(Codex)에 등록된 공식 Figma MCP를 빌리는
우회 경로는 만들지 않습니다 — devup-mcp가 스스로 `Codex`로 등록해 Figma 원격 MCP에
붙고, 수집에 필요한 `use_figma`를 그 연결로 직접 부릅니다. 호스트에 Figma MCP를
따로 설정할 필요가 없고, 설정돼 있어도 devup-mcp는 그것을 쓰지 않습니다.

```bash
claude mcp add --transport http figma https://mcp.figma.com/mcp
codex mcp add figma --url https://mcp.figma.com/mcp
```

### Figma → DevupUI

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "outputs": ["tsx"],
  "componentName": "OptionalComponentName",
  "rootLayout": "standalone",
  "scope": "node",
  "outputPaths": { "tsx": "optional/path/Component.tsx" }
}
```

결과에는 `tsx`와 함께 `status`, `quality`, `cache`, `collection`, `source`가 포함됩니다. import 목록과 사용 token은 별도 키로 보내지 않습니다 — 각각 TSX의 첫 줄과 본문의 `$token`이 이미 같은 내용을 담고 있어, 응답에 두 번 싣는 만큼이 그대로 낭비였습니다. Auto Layout은 `Flex`, 일반 container는 `Box`, text는 `Text`로 변환하고 theme binding이 있으면 JSX prop에서 `$token`을 우선 사용합니다. 변수 token은 비어 있지 않은 Figma `codeSyntax.WEB`을 우선하고, 없으면 변수 경로의 마지막 이름을 정규화합니다. 따라서 TSX의 `$token`, `devup.json` key와 source map이 같은 이름을 사용합니다. `rootLayout` 기본값인 `standalone`은 선택한 root의 크기·위치 제약까지 포함하고 Figma instance의 실제 자식 상태를 펼쳐 정의되지 않은 component 참조를 만들지 않습니다. 이미 레이아웃을 소유한 React 부모 안에 삽입할 때는 `rootLayout: "embedded"`로 root의 외부 크기·위치 제약만 생략합니다.

### Figma → devup.json

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "outputs": ["devupJson"],
  "scope": "file",
  "outputPaths": { "devupJson": "optional/path/devup.json" }
}
```

결과는 `theme.colors`, `theme.typography`, `theme.length`, `theme.shadow`를 포함하는 결정적 JSON 문자열과 counts, completeness를 반환합니다.

`outputPath`를 생략하면 결과를 메모리와 MCP 응답에만 유지합니다. 명시하면 생성된 TSX 또는 `devup.json`만 해당 경로에 기록하고 실제 절대 경로를 응답합니다. 기본 허용 write root는 `devup-mcp` process 시작 당시의 current directory 하나이며, 그 밖의 workspace는 반복 가능한 `--allow-write-root <directory>` 시작 인자로만 추가할 수 있습니다. Tool 입력으로 root 자체를 넓힐 수 없고 `..`, 다른 drive/UNC, alternate data stream, symlink/junction을 통한 root 탈출은 기록 전에 거절됩니다.

여러 text/asset output은 모두 검증한 뒤 같은 directory의 exclusive 임시 파일에 staging하고 한 transaction으로 교체합니다. 정상 runtime 오류에서는 이미 교체한 파일을 역순으로 되돌리고 기존 파일을 복구합니다. 개별 rename은 atomic하지만 여러 directory와 process crash를 가로지르는 완전한 원자성은 일반 filesystem 특성상 보장하지 않습니다.

### 통합 수집과 다중 출력

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "outputs": ["tsx"],
  "scope": "node",
  "strict": true,
  "refresh": false,
  "delivery": "auto"
}
```

위 `outputs`는 스키마 기본값과 같은 **권장 호출**입니다. 읽을 output만 요청하세요 — 위의 [응답에 무엇이 들어오는가](#응답에-무엇이-들어오는가) 표가 **전체 목록 레퍼런스**이고, 그 표를 그대로 한 배열에 옮겨 적으라는 뜻이 아닙니다. 특히 `rawSnapshot`/`rawPayload`를 `debug: true` 없이 `outputs`에 넣은 호출은 투영 전에 `-32602`로 **거절됩니다**. 실제로 이 절의 예시가 한때 여덟 output을 전부 나열하면서 `debug`는 빠뜨리고 있었고, 그대로 복사하면 실행되지 않는 호출이었습니다.

### 한 번에 얼마나 요청할 것인가

`devup_figma_export`는 **작은 선택**을 위한 도구입니다.

- 한 호출에 **프레임 1~3개를 권장**하고, **프레임 6개·(프레임 × output) 12단위**까지만 허용합니다.
- 상한을 넘는 선택은 화면 수집을 시작하기 전에 거절하므로, `frameIds`를 배치로 나눠 부르세요.
- 계획용 어림값은 **프레임당 15~60초**입니다. 보장이 아니라 어림이며, 페이지네이션·복잡도·스로틀링이 겹치면 넘어갑니다. 클라이언트는 보통 300초에서 타임아웃합니다.
- 프레임별 투영이 일부 실패해도 **성공한 프레임의 산출물은 유지**됩니다.

이미 노드 ID를 알고 있다면(브리프에 적혀 있거나 앞선 호출이 알려줬다면) `devup_figma_explore`를 건너뛰고 `frameIds`에 바로 넘기세요. 갖고 있는 ID를 다시 찾으려고 탐색하는 것은 Figma 호출 하나를 그냥 쓰는 일입니다. 탐색은 **Section 링크밖에 없을 때** 쓰는 도구입니다.

`devup_figma_export`는 동일한 node/resource acquisition에서 여러 projection을 생성합니다. 응답의 `cache.artifactId`를 다음 요청의 `artifactId`로 넘기면 Figma를 다시 호출하지 않고 다른 output을 만들 수 있습니다. `artifactId`는 `url`·`refresh`와 동시에 쓸 수 없고, 함께 보내면 `DEVUP_FIGMA_HANDOFF_INVALID`로 거절됩니다. URL 요청은 같은 process 안에서 10분 TTL, 최대 8개/항목당 32 MiB/전체 128 MiB인 memory-only LRU cache를 재사용하며, `refresh: true`는 완료 cache뿐 아니라 진행 중 요청 공유도 우회해 URL을 새로 수집합니다. 동일 acquisition의 선행 작업이 취소되더라도 닫힌 in-flight 표식을 다음 요청이 원자적으로 제거하고 다시 수집하므로 같은 key가 process 수명 동안 오염되지 않습니다. `cache`에는 `reuseKind`, `ageSeconds`, `remainingTtlSeconds`, `avoidedFigmaToolCalls`, 원 수집의 `originCollection`이 포함되고, 응답 최상위 `collection`은 현재 요청이 실제로 실행한 호출만 집계합니다. `cache.capabilities`는 artifact의 `kind`(`design`, `theme-only`, `search`, `explore`, `section-index`), `collectionScope`, `resourceScope`, `referencePng` 보유 여부, redacted `assetCaptureCount`와 `sectionSelection`만 공개합니다. **`sectionSelection`이 마지막으로 검증된 Section 선택(`frameIds` 또는 `allScreens`)을 보존하므로**, artifact를 재사용할 때 `frameIds`를 다시 보내지 않아도 선택이 유지됩니다. 예전에는 `artifactId`만 넘기면 선택이 사라져 후보 목록 전체가 다시 돌아왔습니다. 내부 artifact는 asset ID·format·scale 전체를 보존하고 세 값이 정확히 같은 capture만 추가 Figma 호출 없이 재사용합니다. 재사용 요청이 이 범위를 넘으면 `DEVUP_FIGMA_HANDOFF_INVALID`로 투영과 파일 기록 전에 거절합니다. 예를 들어 node/used-resource artifact로 file 전체 `devupJson`을 만들거나 screenshot을 수집하지 않은 artifact로 `referencePng`를 만들 수 없습니다. credential, screenshot과 asset binary는 cache key나 통계에 포함하지 않고, process가 끝나면 cache도 사라집니다.

`delivery`는 `auto | inline | resource`입니다. `auto`는 JSON escape, base64와 structured/text 이중 표현을 포함한 실제 MCP wire 크기를 계산해 개별 256 KiB·합계 1 MiB 이하만 inline으로 반환하고, 그보다 큰 결과는 native MCP `ResourceLink`와 `devup://artifact/...` URI로 바꿉니다. 링크 URI는 JSON manifest를 가리키므로 link MIME은 `application/json`이고 payload MIME·길이·SHA-256은 `payload*` metadata로 분리합니다. **manifest는 `chunkUris`로 내용 청크를 직접 가리킵니다** — 링크만 따라가면 manifest에서 막히고 청크 URI 형식을 따로 알아내 손으로 조립해야 했던 문제가 없어졌습니다. 두 URI 형식은 `resources/templates/list`에도 그대로 게시됩니다.

```
devup://artifact/{artifactId}/outputs/{outputId}/manifest
devup://artifact/{artifactId}/outputs/{outputId}/chunks/{index}
``` `resource`는 크기와 무관하게 TSX/JSON/PNG를 bounded chunk resource로 제공하며, binary chunk는 base64 MCP blob입니다. asset manifest는 binary를 내장하지 않고 각 asset의 독립 resource URI·MIME·길이·SHA-256을 참조하므로 `resources/read`로 원본 bytes를 정확히 재구성할 수 있습니다. 같은 artifact와 정규화한 projection은 content hash가 같은 resource를 재사용합니다. 파일 출력과 새 resource publication을 함께 요청하면 resource 조회를 reservation 동안 차단한 하나의 transaction으로 다루며, 파일 commit이 전부 성공한 뒤에만 resource와 LRU 변경을 공개합니다. 실패하면 원래 파일을 fingerprint로 검증해 복원하고 복원 불능 backup 경로를 구조화해 보고합니다. 현재 transaction이 만든 temp는 정상 종료·rollback에서 직접 제거하지만, 소유권을 증명할 수 없는 pre-existing temp나 crash·rollback recovery backup은 자동 삭제하지 않습니다.

`referencePng`는 선택했을 때만 공식 read-only `get_screenshot`을 정확히 한 번 추가 호출합니다. 공식 도구는 기본으로 PNG의 URL과 curl 안내를 text로만 돌려주고 긴 변을 1024px로 줄이므로, `enableBase64Response: true`와 `maxDimension: 8192`로 호출해 node 원래 크기의 PNG를 inline으로 받습니다. 결과의 image block은 정확히 하나여야 하며(곁의 text block은 읽지 않음), JSON/text에 숨긴 image나 다중 image는 거절합니다. 16 MiB compressed, 8192px, 64 MiB decoded 상한 안에서 PNG 전체를 실제 decode한 뒤 byte length와 SHA-256을 확인해 artifact에 보존하며, 단일 링크 node에만 적용됩니다. 그래서 `referencePng`를 `frameIds` 또는 `allScreens: true`와 함께 요청하면 수집 전에 거절되고, Section의 여러 Frame은 먼저 반환된 canonical URL별로 하나씩 수집해야 합니다. PNG bytes는 log·통계·cache key에 포함되지 않으며 `outputPaths.referencePng`를 명시하지 않으면 디스크에 기록하지 않습니다.

모든 완료 응답에는 다음처럼 요청한 산출물별 `quality`가 포함됩니다.

```json
{
  "status": "complete",
  "quality": {
    "acquisition": "complete",
    "projection": "exact",
    "theme": "not-requested",
    "assets": "not-requested"
  }
}
```

`acquisition`은 `complete | expected-projection | partial | failed`, `projection`은 `exact | approximated | lossy | failed | not-requested`, `theme`은 `complete | conflicted | unresolved | not-requested`, `assets`는 `complete | partial | failed | not-requested`입니다. 검색·탐색의 의도적인 얕은 graph는 `expected-projection`으로 정상 완료하지만, 포함된 field의 실패나 truncation은 `partial`입니다. mask/effect fallback은 `lossy`, absolute layout fallback은 `approximated`이며 `includeDiagnostics: false`여도 품질 판정에는 반영됩니다. 최상위 `status`는 `complete | partial | failed | selection_required` 중 하나입니다. 요청한 모든 축이 정확·완전하고 **`failures[]`가 비어 있을 때만** `complete`입니다. 축 하나라도 어긋나거나 실패가 하나라도 집계되면 `partial`이고, 수집이나 투영 자체가 죽으면 `failed`, Section 링크에서 아직 화면을 고르지 않았으면 `selection_required`입니다.

`failures[]`에는 노드 단위 투영 실패, 자산 export 실패, Section의 프레임 단위 실패가 **모두 함께** 집계됩니다. 예전처럼 최상위가 `complete`인데 안에서 자산이 실패해 있는 상태는 나오지 않습니다.

`strict: true`는 모든 요청 축이 exact/complete가 아니면 quality와 `completenessReport`를 담은 오류로 거절합니다.

모든 공개 TSX generator는 반환 전에 Rust의 고정된 TypeScript+JSX parser를 통과합니다. parser 오류는 디자인 원문을 노출하지 않고 byte range와 오류 category만 반환합니다. 응답의 `fidelity`는 생성된 mapping 수가 아니라 수집한 source snapshot에서 독립적으로 계산한 node/text segment/variable/style/asset/layout 기대 집합을 분모로 사용하고, 내부 validator가 각 항목을 `emitted | flattened | ignored` 중 정확히 하나로 검증했는지와 축별 coverage·typed impact count를 담습니다. component set, non-default variant selector와 inline instance도 최종 변환 후 source identity별 provenance를 다시 만들며, 반복된 동일 text segment는 하나의 mapping을 재사용하지 않고 occurrence별로 소비하고 multiline·중첩 text와 asset identity를 검증합니다. 알 수 없는 codegen warning/error도 각각 최소 `approximated`/`failed`로 보수적으로 판정하며, `strict`는 syntax, source-derived trace coverage, lossy/failed impact를 함께 검사합니다.

브라우저 시각 회귀는 MCP 서버가 임의 명령을 실행하지 않고 소비자 repository가 실제 font/asset/DevupUI 환경으로 `actual.png`를 만든 뒤 순수 Rust `devup-mcp-visual`로 비교합니다. renderer pinning, 기본 0.5% threshold, diff PNG와 개인정보 취급 계약은 [`docs/visual-renderer-contract.md`](docs/visual-renderer-contract.md)에 있습니다.

### 렌더링 하네스 — 생성 코드를 Figma가 그린 PNG와 비교

생성 코드를 플러그인의 답안과 줄 단위로 맞춰보면 둘이 일치한다는 것까지는 알 수 있지만, **둘 중 어느 쪽도 Figma가 그리는 그림과 같은지는 말해주지 못합니다.** `harness/render`는 그 질문에 답합니다 — 각 화면을 devup-ui로 빌드해 프레임 크기 그대로 열고, Figma가 같은 프레임을 렌더한 PNG와 픽셀 비교합니다.

```bash
cd harness/render && npm install
python scripts/acquire.py            # 모듈·테마·에셋·기준 PNG를 실행 중인 devup-mcp에서 가져옴
node scripts/render.mjs              # 빌드·캡처·비교, 화면별 임계값 초과 시 exit 1
```

`acquire.py`는 Figma 호출을 `fixtures/local-call-bank`에 적립하므로 재실행은 이미 지불한 만큼 무료입니다. 화면마다 **자기 테마를 node scope로** 받습니다 — 파일 하나에 여러 브랜드 컬렉션이 섞이면 `primary` 같은 토큰이 서로 덮어써서, 공지 화면이 Figma가 파랑으로 그리는 자리를 보라색으로 그렸습니다. devup-ui는 테마를 빌드 시점에 굽기 때문에 화면들은 필요한 테마별로 묶여 그룹마다 한 번씩 빌드됩니다. 리셋은 생성 코드가 전제하는 `@devup-ui/reset-css` 그대로입니다.

`thresholds.json`이 화면별로 Figma와 벌어져도 되는 최대치를 들고 있습니다. 초과하면 실패하고, 밑돌면 그렇다고 알려줍니다(= 수치를 조일 차례). 측정값:

| 화면 | 1920 | 992 / 768 | 360 / 390 |
|---|---|---|---|
| popup | **0.83%** | 2.19% | 3.59% |
| popup (플러그인 답안) | 21.06% | 7.14% | 12.02% |
| notice | **2.33%** | 4.19% | 8.29% |
| about | 4.54% | 6.94% | 11.26% |
| report **1.87%** · grid 2.96% · keyframes 6.71% | | | |

차이가 **어디** 있는지는 보조 도구가 답합니다 — `bands.mjs`(가장 많이 어긋난 구간), `drift.mjs`(단순 이동인지 실제 차이인지), `crop.mjs`(구간을 기준/캡처 나란히), `boxes.mjs`(DOM 상자를 Figma 좌표와 대조), `elements.mjs`(그림이 실제로 몇 픽셀로 나왔는지). 긴 화면을 통째로 줄인 스크린샷은 아무것도 보여주지 않습니다.

`text-check.mjs`는 픽셀이 아니라 **글자**를 봅니다. 생성기는 텍스트 노드의 `characters`를 JSX에 쓰는데, JSX는 공백에 자기 규칙이 있습니다 — 한 문장이 소스 두 줄로 나뉘면 사이에 공백 하나가 들어갑니다. 디자인에 그 공백이 없으면 화면은 디자인에 없는 단어를 찍고, 문단은 Figma가 끊지 않는 자리에서 감깁니다. JSX를 읽어 무엇이 그려질지 추론하는 건 그 규칙을 다시 구현하는 일이고, 그렇게 넘겨짚으면 없는 결함을 만들어냅니다 — 그래서 **브라우저가 실제로 찍은 글자**를 `characters`와 대조합니다. 현재 245개 텍스트 중 3개(같은 문단의 세 폭)가 디자인대로 찍히지 않습니다.

캡처·테마·에셋·빌드 산출물은 커밋하지 않습니다(`harness/render/.gitignore`). 이 하네스가 찾아낸 결함은 테마 스코프, 컨테이너가 칠하는 그림의 매니페스트 누락, 잘린 fill의 crop 행렬, 파일시스템이 못 받는 레이어 이름, 폭마다 크기가 다른 사진의 파일 공유, 투명도 0 노드의 export 거부, 그리고 positioned child 너머로 CSS가 못 미치는 높이입니다.

Section 링크에서 TSX를 요청하면 먼저 내부 screen frame 후보와 canonical URL을 `selection_required`로 반환합니다. `frameIds`로 검토한 frame만 고르거나 `allScreens: true`로 모든 화면을 시각 순서대로 batch export할 수 있으며 두 옵션은 동시에 사용할 수 없습니다. `sourceMap`은 `nodeId`·원본 `property`·`generatedProperty`·`resolution`으로 필드→생성 속성을 설명하는 sidecar입니다. TSX 문자/바이트 오프셋은 제공하지 않습니다. 생성 코드 발췌는 진단의 `generatedSource`를 확인합니다. devup.json은 JSON pointer를 사용합니다. `assetManifest`는 image hash/vector/export provenance를 항상 열거하고, `assetRequests`로 명시한 항목만 권장 1–3개, 최대 6개·scale 1~4 범위에서 read-only SVG/PNG export합니다. `assetRequests`를 쓰는 호출은 `outputs`에 `assetManifest`가 함께 있어야 하며, 빠뜨리면 거절됩니다. `outputPath`를 지정하면 binary를 해당 파일로 디코딩하고 응답의 base64를 제거하며, 생략하면 후속 소비를 위해 base64가 memory-only artifact와 해당 MCP 응답에 남을 수 있습니다.

자산 6개 초과 요청은 인증·수집·파일 쓰기 전에 거절하며 `recommendedBatchSize`, `maxAssetCount`, `recommendedAssetRequests`, `remainingAssetRequests`로 분할을 안내합니다. 느린 자산 호출은 1초 대기 후 `assetJob.jobId`, 단계별 호출 기록, 자산별 수집/파일 상태를 먼저 반환합니다. `{"jobId":"..."}`로 조회하고, `paused`이면 `{"jobId":"...","jobAction":"resume"}`로 미완료 호출만 재개합니다. 동일 인자를 다시 보내면 유실된 최초 응답의 작업을 찾습니다. 클라이언트 타임아웃은 작업을 취소하지 않습니다. 체크포인트는 **같은 서버 프로세스에서 최대 30분**, 완료 결과는 5분 보존하며 서버 재시작 후에는 복구되지 않습니다. 개별 upstream 호출은 90초에 일시정지합니다. 자산 `exported`는 검증된 바이트 수집이며, `fileState=written`은 실제 파일 바이트까지 확인한 결과입니다. 상세 계약은 [R3 자산 작업과 근거](docs/r3-asset-jobs-and-evidence.md)를 참조하세요.

**`assetManifest`는 목록일 뿐 파일을 쓰지 않습니다.** 파일을 얻으려면 `assetRequests`로 다시 부르되, 그 호출은 **`artifactId`가 아니라 원래 `url`로** 해야 합니다. 자산 캡처 없이 수집된 artifact는 자산 요청을 처리할 수 없기 때문입니다. 그 조합으로 부르면 무엇이 없는지와 어떻게 복구하는지를 담아 거절합니다:

> `This artifact was collected without the requested asset captures. Remove artifactId and call again with the original url and assetRequests.`

기본값에서는 manifest의 `path`와 TSX의 자산 참조가 **자리표시자로 남습니다.** 이것이 기본 동작이고, `assetPublicRoot`는 **옵트인**입니다.

실제 경로로 바꾸려면 같은 호출에서 `assetPublicRoot`와 `assetRequests.outputPath`를 함께 주세요. 규칙은 이렇습니다.

- `assetPublicRoot`는 URL `/`로 서빙되는 **이미 존재하는 절대 로컬 디렉터리**여야 합니다. 상대 경로이거나 존재하지 않으면 호출자 오류입니다.
- 같은 호출에 `assetRequests.outputPath`가 **최소 한 개** 있어야 합니다. 저장된 output 매핑은 artifact에 보존되지 않으므로 `artifactId`로 재사용해도 이어지지 않고, 이 지정은 호출마다 해야 합니다.
- `outputPath`가 `assetPublicRoot` 밖이거나 허용 write root를 벗어나면 거절됩니다.

조건이 맞으면 그 루트 아래에 쓰인 파일이 manifest와 **모든 TSX output**에서 percent-encoding된 상대 URL로 바뀝니다.

내보낸 바이트가 **동일하면** 하나의 canonical `path`/`outputPath`를 공유하고, 그 canonical 경로는 응답에 실려 돌아오므로 **반환된 경로를 그대로 확인해 쓰면 됩니다.**

중복 제거의 판단 기준은 좁습니다.

- **디코드한 export 바이트의 SHA-256**으로만 판단합니다. 레이어 이름이나 검증되지 않은 `imageHash`로는 합치지 않습니다.
- 먼저 **무결성을 검증**합니다. 자산이 신고한 `byteLength`·`sha256`이 실제 디코드 결과와 다르면 합치기 전에 거절합니다.
- **내용과 포맷이 모두 같을 때만** 공유합니다. 판정 키는 (내용 해시, 확장자, MIME)이라 같은 그림의 SVG와 PNG는 각각 남습니다.
- **manifest의 노드별 항목은 그대로 유지됩니다.** 합쳐지는 것은 파일 경로이지 목록의 줄이 아니므로, 어느 노드가 어느 자산을 쓰는지는 계속 보입니다.

`assetNamesPerNode` 기본값은 여전히 `true`입니다.

asset의 파일 이름은 기본적으로 **레이어 이름**입니다 — 플러그인이 그렇게 짓기 때문입니다. 그래서 디자이너가 같은 이름을 준 노드들은 파일 하나를 공유합니다. 같은 그림이면 맞지만 아니면 손실입니다. 한 화면에서 여덟 노드가 `Logo.svg` 하나를 주장하는데 실제로는 서로 다른 그림 다섯 개였고, 폭마다 그려진 사진은 마지막으로 export된 폭의 파일만 남아 다른 폭에서는 상자와 크기가 어긋난 채 늘어납니다(파일이 상자와 같은 크기이면 `object-fit`이 무엇이든 결과가 같으므로, 플러그인에서는 이 문제가 드러나지 않습니다).

`assetNamesPerNode`는 각 asset을 **그 노드**의 이름으로 지어(`Logo-422-6921.svg`, `Frame 269-422-3392.png`) 둘을 함께 없앱니다. **기본값은 `true`입니다** — 렌더링해 보면 이쪽이 Figma가 그리는 그림에 가깝고(공지 화면 992폭 6.48% → 4.19%, about 992폭 10.79% → 6.94%), 플러그인 golden 268개는 그대로 통과합니다. golden은 `CodegenOptions`를 직접 쓰고 그 **라이브러리 기본값은 여전히 플러그인과 동일**하기 때문입니다. 플러그인과 byte 단위로 같은 이름이 필요하면 `assetNamesPerNode: false`로 끄십시오.

끈 상태에서 서로 다른 그림이 한 파일을 계속 주장하면 첫 번째만 기록하고 나머지는 `DEVUP_ASSET_NAME_SHARED` diagnostic으로 보고합니다 — 조용히 덮어쓰지 않습니다.

레이어 이름이 파일시스템이 받지 못하는 이름일 때(`ic:round-arrow-left`처럼 콜론이 든 이름은 Windows가 만들지 못합니다) 전달 시점에 생성 코드와 manifest를 **함께** 개명해 둘이 어긋나지 않게 합니다. 생성기 자체는 플러그인의 이름을 그대로 쓰므로 golden parity는 유지됩니다.

개명 규칙은 이렇습니다. 영숫자·`-`·`_`가 아닌 문자를 `-`로 바꾸고 앞뒤 `-`를 떼며, Windows 예약어(`CON`, `NUL`, `COM1`…)이거나 비ASCII가 있었거나 이름이 바뀌었거나 120자를 넘으면 원래 이름의 SHA-256 앞 12자리를 접미사로 붙여 서로 다른 이름이 한 파일로 합쳐지지 않게 합니다. 확장자는 ASCII 영숫자 10자까지만 남깁니다.

```
/icons/ic:round-arrow-left.svg  ->  /icons/ic-round-arrow-left-b041d24e668d.svg
```

그래서 공백이나 한글이 든 레이어 이름도 코드와 manifest 양쪽에서 같은 ASCII 경로가 됩니다.

Section 링크는 전체 subtree를 직접 변환하지 않습니다. `selection_required.nextAction`에 따라 후보를 확인한 뒤 `frameIds` 또는 `allScreens: true`로 화면별 export를 계속하며, 일부 화면 수집이 실패하면 성공한 화면은 유지하고 실패한 node는 `failures`에 보고합니다. `nextAction`은 `why`·`how`·`doNot`과 함께 첫 후보를 고르는 **그대로 실행 가능한** `example` 호출을 담습니다.

여러 화면을 한 번에 받으면 `frames[]`의 각 항목이 서로 같은 구조인지 알려줍니다. 생성 구조가 앞선 프레임과 같으면 그 프레임에 `structurallyIdenticalTo`(같은 구조인 첫 프레임의 node ID), `differsOnly`(예: `["assetReferences"]`), `comparedOutputs`가 붙습니다. 두 화면이 정말 다른지 확인하려고 TSX를 직접 대조할 필요가 없습니다.

화면 후보 판정은 `explore`, export의 `selection`, `allScreens`가 **같은 규칙 하나**를 씁니다. 셋이 서로 다른 "화면" 정의를 갖던 문제가 사라져, `allScreens`가 배너·주석 프레임을 화면으로 만들어내지 않습니다.

SECTION은 **2단계**입니다. 1단계로 `status: "selection_required"`와 선택 목록을 받고, 2단계로 `nextAction.example`을 실행해 고른 화면을 export합니다. 한 번에 Section 전체를 수집하려 하지 마세요.

SECTION 기본 응답은 화면 아티팩트가 아닌 선택 목록입니다. `selection.status`와 `selection.count`로 목록 조회 상태와 후보 수를 알 수 있으며, 최상위 `status: "selection_required"`는 아직 화면을 선택해야 한다는 뜻입니다.

목록은 **두 갈래**로 나뉩니다.

- `selection.candidates[]` — 자동으로 화면이라고 판정한 후보입니다. `node.name`, `node.nodeType`, `node.textPreview`, `canonicalUrl`로 서로 구분합니다. **`allScreens: true`는 메뉴의 모든 항목이 아니라 바로 이 자동 화면 후보를 한꺼번에 고르는 것**입니다.
- `selection.explicitCandidates[]` — 자동 판정이 화면으로 보지 않은 것들입니다. 작은 사례, 주석·텍스트, 그리고 화면 치수로 측정되지 않는 **긴 페이지** 같은 것이 여기 들어옵니다. `allScreens`는 이들을 고르지 않으므로, 필요하면 `frameIds`나 그 `canonicalUrl`로 **명시적으로** 선택하세요.

긴 페이지는 높이·비율 어느 쪽으로도 화면으로 측정되지 않습니다. 그렇다고 그 안을 뜯어 조각들만 후보로 내놓으면 정작 페이지 자체는 목록에 없게 되므로, 페이지는 `explicitCandidates`에 통째로 남기고 그 **안에 있는 자동 화면 후보도 함께** 보존합니다. 둘 중 무엇을 고를지는 호출자가 정합니다. 미리보기는 보이는 텍스트만 모아 최대 120자, 전체 최대 2KB로 제한하므로 비어 있거나 짧아도 실제 화면 내용이 없다는 뜻은 아닙니다. `nextAction.example`에는 첫 후보를 선택하는 `devup_figma_export` 호출 예시가 들어 있습니다. 예시의 `frameIds`를 검토한 후보 ID로 바꿔 호출하면 선택한 화면만 수집합니다.

### 한 화면의 여러 폭 — 반응형 모듈

Section 안의 frame이 `mobile` / `tablet` / `desktop`처럼 **breakpoint 이름**을 가지면, 그 frame 하나를 요청해도 같은 이름 규칙의 형제 frame이 함께 수집됩니다(Section 자체는 수집 범위 밖이며, 그 이름은 각 frame의 `parentName`으로 전달됩니다). 이때 `tsx`나 `responsiveTsx`를 요청하면 결과에 `responsiveTsx`가 추가됩니다 — 세 폭을 하나의 트리로 접고 폭마다 다른 값을 devup-ui 반응형 배열 `[mobile, sm, tablet, lg, pc]`로 쓴 모듈입니다. 각 폭이 놓이는 slot은 frame **이름이 아니라 폭**으로 정해집니다(`≤480 / ≤768 / ≤992 / ≤1280 / 그 이상`). 컴포넌트 이름은 `componentName`이 우선이고, 없으면 Section 이름의 PascalCase에 `Page`를 붙입니다(`about` → `AboutPage`).

한 폭에만 있는 노드는 다른 폭에서 `display: none`으로 숨긴 복사본과 병합되며, 이때 복사본은 **Section 레이어 순서상 첫 폭**의 값을 가집니다 — 그래서 배열의 첫 slot에 desktop 값이 놓일 수 있습니다. 폭마다 줄바꿈 위치만 다른 텍스트는 `<Box as="br" display={[...]} />`로 쓰고, 컴포넌트 인스턴스의 variant prop이 폭마다 다르면 배열로 쓸 수 없으므로 가장 넓은 폭의 값을 쓰고 `responsiveUnrepresented`에 보고합니다. 함께 반환되는 `responsiveSlots`, `responsiveImports`, `responsiveComponents`가 slot과 import 목록입니다.

`rawPayload`는 `rawSnapshot`이 node 트리만 쓰는 것과 달리 수집 전체(variables, styles, stats, assets 포함, `referencePng` 제외)를 씁니다. 캡처를 fixture로 보관해 오프라인에서 서버와 같은 토큰 이름(`$gray200`, `typography="h4"`)으로 변환하려면 이것이 필요합니다.

### 시간 트리거 Smart Animate — CSS keyframes

frame에 `After delay` 트리거로 다른 frame에 **Smart animate**하는 reaction이 있고, 그 frame이 다시 다음 frame으로 이어지면 하나의 체인입니다(처음 frame으로 돌아오면 루프). 체인의 frame들은 요청한 node의 subtree 밖에 있는 형제 frame이므로, 요청 루트가 하나일 때 snapshot 스크립트가 체인을 따라가며 추가 루트로 함께 수집합니다(다중 루트 요청은 루트 목록을 그대로 둡니다).

변환기는 플러그인의 `getReactionProps` 규칙대로 frame 사이에서 바뀌는 것 — 위치, 크기, opacity, 첫 fill, 회전(누적 delta) — 을 이름이 같은 자식에서 먼저 찾아 자식마다 `animationName={keyframes({...})}` / `animationDuration` / `animationTimingFunction` / `animationFillMode` / `animationIterationCount`(루프면 `infinite`)로 쓰고, 바뀌는 자식이 없을 때만 frame 자체에 씁니다. `0%`는 시작 frame, 각 단계는 도착 시점의 퍼센트에 직전 keyframe과 다른 속성만, 루프는 `100%`에서 시작 값으로 닫히며 duration은 되돌아가는 구간까지 셉니다. 10ms 미만 timeout은 delay로 쓰지 않습니다. `keyframes`가 쓰이면 `@devup-ui/react`에서 import됩니다.

snapshot에 없는 목적지(legacy 경로, 다중 루트 요청)는 조용히 버리지 않고 `DEVUP_CODEGEN_ANIMATION_UNREACHABLE` diagnostic으로 보고합니다.

### Figma 이름 검색

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>",
  "query": "A : STORY-F-PROOFREAD",
  "nodeTypes": ["PAGE", "SECTION", "FRAME", "COMPONENT_SET"],
  "match": "normalized",
  "limit": 20
}
```

검색 범위는 URL이 정합니다. `node-id`가 있으면 그 노드와 그 subtree만, 없으면 파일 전체를 봅니다. 응답의 `scope`가 실제로 어느 쪽을 검색했는지 보고하므로, 결과가 기대와 다를 때 범위 때문인지 이름 때문인지 구분할 수 있습니다. `limit`은 **반환할 일치 개수**입니다.

파일 전체 검색은 먼저 read-only Plugin API로 실제 `figma.root.children` page catalog를 얻고, page마다 한 번씩 전환하는 작은 query projection을 병렬 실행합니다. 전체 page snapshot을 응답하지 않으므로 큰 파일에서도 공식 MCP text 상한을 피합니다. 결과는 원문 exact, Unicode NFC·공백·대소문자를 정규화한 exact, prefix, contains 순으로 정렬하고 `match: "fuzzy"`일 때만 오타 허용 검색을 추가하며, node ID, type, page, 전체 breadcrumb와 후속 `devup_figma_export`에 그대로 전달할 canonical URL을 포함합니다.

### 링크 주변 화면 탐색

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "limit": 50,
  "includeTextPreview": true,
  "refresh": false
}
```

요구사항 제목이나 설명 node 링크가 실제 구현 화면이 아닐 때 `devup_figma_explore`를 먼저 호출합니다. `url`에는 `node-id`가 있어야 하고(없으면 `DEVUP_FIGMA_NODE_NOT_FOUND`), `limit`은 1~100 범위 밖이면 거절됩니다. anchor와 같은 공간 묶음의 frame/component 후보를 시각 순서와 canonical URL로 반환하며, 다음 요구사항 제목에서 탐색 범위를 끝냅니다.

`limit`은 **반환할 후보 개수일 뿐이고 그 이상 아무것도 아닙니다.** 디자인을 얼마나 읽을지는 바꾸지 않으므로(투영 예산은 항상 고정입니다), `limit`을 올려서 할 수 있는 일은 답을 길게 만드는 것뿐입니다. 예전에는 `limit`이 투영 예산으로 4배 확대돼 30→33으로 올렸더니 후보가 23→18로 **줄어드는** 일이 있었습니다.

잘린 이유도 이제 둘로 나눠 보고합니다.

- `truncation.candidates` — `limit`이 목록을 잘랐다는 뜻입니다. **`limit`을 올리면 나머지가 나옵니다.**
- `truncation.projection` — 수집한 스냅샷 자체가 불완전했다는 뜻입니다. **`limit`을 아무리 올려도 그 화면들은 돌아오지 않습니다.**

최상위 `truncated`는 둘의 OR입니다. 둘을 구분하지 못하면 고칠 수 없는 상황에서 `limit`만 올리며 호출을 반복하게 됩니다.

`includeTextPreview`는 **별도 예산**을 씁니다. 미리보기는 구조 투영이 쓰고 남긴 공간만 소비하므로, 켜고 끄는 것이 후보 개수·ID·`truncation.projection` 중 무엇도 바꾸지 않습니다. 같은 파일·옵션에서 이미 수집한 더 큰 탐색 결과는 exact·related-node·superset 범위로 재사용되고, 동시에 들어온 호환 요청도 공식 Figma 호출 하나를 공유합니다. `refresh: true`는 모든 재사용을 건너뜁니다. 원하는 후보의 canonical URL을 `devup_figma_export`에 넘겨 정확한 화면만 변환합니다.

탐색과 검색은 변수 catalog를 수집하지 않습니다. 정확한 UI 변환 단계에서 선택 subtree의 모든 보존 필드에 있는 `VARIABLE_ALIAS`와 paint/text/effect/grid style ID를 재귀적으로 스캔하고, 실제 사용된 ID만 공식 Figma API로 조회합니다. `outputs: ["devupJson"]`에 `scope: "file"`을 함께 준 경우에만 file 전체 로컬 catalog를 수집합니다.

Figma 연결은 direct 하나뿐입니다. `sourcePolicy` 파라미터는 `auto`와 `direct` 둘 다 같은 동작이었으므로 제거했습니다 — 분기하지 않는 선택지는 호출자에게 틀릴 기회만 주었습니다. direct 경로는 연결과 read-only capability catalog 조회를 각각 30초, 개별 tool 호출을 5분으로 제한합니다. deadline을 넘기면 해당 remote session을 폐기하고 디자인 원문 없이 `retryable` timeout 단계만 반환합니다.

정확한 node 링크의 UI 변환은 하나 이상의 공식 `use_figma` 호출 안에서 subtree와 실제 사용 리소스를 수집합니다. 수집 스크립트는 checked-in manifest(devup-ui 변환기가 실제로 읽는 필드만)만 확인하고 — 프로토타입 체인 전체를 훑거나 미분류 필드를 `extra`에 담지 않습니다 — `null`/빈 배열/미바인딩 style ID 같은 기본값은 봉투에서 생략합니다. 결과는 항상 텍스트(`devupFastSnapshotEnvelope`)이며 PNG 같은 바이너리 transport는 없습니다. 한 subtree가 15KB 텍스트 한도를 넘으면 같은 스크립트를 `offset`을 옮겨 다시 호출하는 방식으로 텍스트 페이지네이션합니다 — 각 라운드는 그 라운드가 보낸 node에서만 리소스를 스캔해 자기 완결적이며, Rust가 여러 라운드의 node와 리소스를 병합합니다. Rust는 schema·대상 ID·node graph·리소스 참조·(페이지 중이 아닐 때의) 자식 완전성을 모두 검증한 뒤에만 결과를 채택합니다. 한 항목이라도 불일치하면 fast 결과 전체를 버리고 기존 cursor 수집을 0부터 재시작합니다. Section multi-root에서는 성공한 root와 resource는 그대로 보존하고 실패하거나 상한을 넘은 root만 legacy로 다시 수집한 뒤 원래 시각 순서로 합칩니다. direct upstream은 연결과 read-only tool catalog를 한 session에서 재사용하고 30초 TTL, 연결 종료 또는 transport 오류 때만 재연결·재검증합니다. 결과의 `stats`에는 `figmaToolCalls`, `transport`(`text` | `text-paginated` | `legacy-cursor`), `fallbackUsed`, node/variable/style 수와 byte 수만 포함되며 원본 디자인이나 인증 정보는 포함되지 않습니다.

완전성 등급은 다음과 같습니다.

- `full-local-plus-used-remote`: 로컬 전체와 사용된 외부 token을 모두 확인
- `used-tokens`: 확보한 token만 변환했으며 외부 전체를 보장하지 않음
- `resolved-values-only`: 의미 있는 token binding 없이 계산값만 확보

## 프로젝트 쪽 도구

### `devup_project_context` — 무엇을 읽고 무엇을 건너뛰는가

`scope`는 `theme`(프로젝트 `devup.json`), `api`(`openapi.json`), `db`(Vespertide `models/*.json`), `all`입니다. 호출 시점에 디스크에 실제로 있는 파일만 읽고, 세션 간 캐시하지 않으며, 없는 것을 추측하지 않습니다. 스캔 깊이는 4단계입니다.

다음 디렉터리는 스캔하지 않습니다.

- 빌드·의존성 산출물: `node_modules`, `target`, `dist`, `build`, `out`, `.next`, `.turbo`, `.nuxt`, `.venv`, `venv`, `__pycache__`, `.cache`, `coverage`, `.git`
- 중첩 체크아웃: `.worktrees`, `.worktree`, `.git-worktrees`

**제외는 조용히 일어나지 않습니다.** 무언가를 건너뛰었으면 `excludedPaths[]`에 `path`와 `reason`(`nested-checkout-directory` 또는 `nested-git-checkout`)으로 보고합니다. 파일을 하나도 못 찾았는데 제외한 것이 있으면 그 사실도 함께 보고하므로, "없음"과 "제외해서 안 보임"을 구분할 수 있습니다. 예전에는 스테일 브랜치의 `devup.json`이 권위 있는 것과 섞여 4개가 반환됐고, 어느 것이 맞는지 표시가 없었습니다.

같은 종류의 파일이 여러 개일 때는 각 항목이 `authority`(`project-root` | `package-local`)와 `appliesTo`(적용 디렉터리)를 갖고, `authorityNote`가 **어느 하나도 프로젝트 전체를 지배하지 않는다**는 사실을 알려줍니다. 코드를 쓰는 파일이 어느 `appliesTo` 안에 있는지로 골라야 합니다.

### `devup_ui_validate` — 심각도로 판정한다

응답은 다음을 담습니다.

| 필드 | 내용 |
|---|---|
| `ok` | 심각도로 판정. error가 있으면 실패, `strict: true`면 warning도 실패 |
| `okReason` | `clean` · `info-only` · `warnings-only` · `warnings-and-info` · `strict-warnings` · `error-violations` |
| `violations[]` | `rule`, `severity`(`info`\|`warning`\|`error`), `byteRange`, `message`, `suggestion` |
| `violationCounts` | `error` / `warning` / `info` 개수 |
| `themeNotes[]` | 비어 있는 토큰 카테고리를 종류마다 한 번씩만 요약 |
| `tokens` | `referencedByTsx`, `definedByTheme`, `unknownTokenCheckRan` |
| `checkedTokens` / `availableTokenCount` | 위 두 값과 같은 수를 예전 이름으로 유지 |
| `themeAvailable` / `themeGuardrail` | 테마를 찾았는지, 못 찾았으면 왜 |

규칙은 `invalid-syntax`, `unknown-token`, `hardcoded-color`, `hardcoded-length`, `unknown-prop`, `runtime-value`입니다.

하드코딩 값의 판정 기준은 **일치하는 토큰이 실제로 있는지**입니다. 있으면 `warning`으로 올리고 토큰 이름을 알려줍니다. 없으면 `info`로 낮추고 **권고 없이 사실만** 적으며 strict 모드에서도 실패시키지 않습니다. 그래서 `length` 토큰이 하나도 없는 테마가 "토큰을 쓰라"는 권고를 받는 일은 없고, 대신 `themeNotes`가 "이 테마에는 length 토큰이 없다"고 한 번 알려줍니다.

`checkedTokens`와 `availableTokenCount`는 **서로 다른 것을 셉니다** — 앞은 이 TSX가 참조한 `$token` 수, 뒤는 `devup.json`이 정의한 토큰 수입니다. 비율이 아닙니다. `tokens`가 그 사실을 이름으로 다시 말해 줍니다.

### `devup_stack_diff` — 휴리스틱이고, 그렇게 말한다

레이어는 `db-entity`, `entity-route`, `route-openapi`, `openapi-client` 넷이며 생략하면 전부 실행합니다. 컴파일러가 아니라 텍스트/JSON 휴리스틱이므로 모든 발견은 `confidence`를 달고 나오고, 그 값은 `low` 또는 `medium`입니다 — **`high`는 없습니다.**

`_`와 `-`의 차이는 **대조할 때만** 접습니다. Vespera가 `openapi.json`에 kebab-case를 쓰고 코드는 원문을 쓰기 때문입니다. 보고되는 드리프트는 **각 레이어가 실제로 쓴 철자 그대로** 싣고, 정규화로 일치시킨 건수는 `spellingNormalizedMatches`로 따로 공개합니다. 같은 라우트가 언더스코어판과 하이픈판으로 양쪽에 중복 신고되던 문제는 이 방식으로 사라집니다.

### `devup_visual_compare` — 렌더하지 않는다

`actual.png` 하나와 reference PNG 하나를 비교합니다. reference는 경로로 주거나 `artifactId`로 캐시된 것을 가리킵니다. 기본 threshold는 `0.005`(0.5%)이고, 원하면 diff PNG를 `auto | inline | resource`로 받습니다.

**이 도구는 스스로 그리지 않습니다.** 브라우저를 띄우지도, 명령을 실행하지도 않습니다. `actual.png`는 소비자 repository가 자기 font·asset·DevupUI 환경에서 만들어 건네는 것이고, 여기서는 순수 Rust로 픽셀만 비교합니다. 경로는 allowlist 안이어야 합니다.

그래서 **통과 여부보다 판정 근거가 중요합니다.** renderer 환경 manifest가 없거나 불완전하거나 유효하지 않으면, 픽셀이 일치해 `visual.passed`가 `true`여도 최종 판정은 `inconclusive`입니다 — 무엇으로 그린 그림인지 모르는 채 "같다"고 말하는 것은 보증이 아니기 때문입니다. renderer pinning과 환경 manifest 계약은 [`docs/visual-renderer-contract.md`](docs/visual-renderer-contract.md)에 있습니다.

### `devup_feature_trace` — anchor가 없으면 거절한다

화면부터 컬럼까지 한 기능의 슬라이스를 읽어 acceptance matrix로 돌려줍니다. 출발점은 **명시적 anchor**여야 합니다 — `routePath`, `figmaNodeId` 또는 `artifactId`, `operationId` 또는 `apiPath`+`method`, `componentPath`, `tableName`.

**요구사항 산문은 anchor가 아닙니다.** "로그인 화면 만들어줘" 같은 문장만 주면 추측해서 답하지 않고 거절합니다. 근거 없이 이어 붙인 슬라이스는 틀렸을 때 어디서 틀렸는지 알 수 없기 때문입니다.

각 hop은 근거가 있거나 `UNVERIFIED`로 표시되며, 생성 산출물의 소유권(그 파일을 손으로 고치면 안 되는지), 순위가 매겨진 UI 재사용 후보, 디자인의 문자열과 request/response 필드의 대응, 필수 상태 커버리지, 그리고 잘림이 일어났다면 그 상한을 함께 보고합니다. `requirement`와 `acceptanceCriteria`를 넘기면 **해석하지 않고 그대로** 실어 돌려줍니다.

정적 파싱은 런타임 동작을 증명하지 않습니다. 이 도구가 "연결돼 있다"고 말하는 것은 코드가 그렇게 적혀 있다는 뜻이지, 실행했을 때 그렇게 동작한다는 뜻이 아닙니다.

## 읽기 전용·개인정보 보호

- upstream 호출은 `get_metadata`, `get_variable_defs`, `get_design_context`, `get_code_connect_map`, `get_screenshot`과 내장된 read-only `use_figma` script로 닫혀 있습니다.
- 사용자 입력 JavaScript를 받지 않으며 Figma document mutation API를 호출하지 않습니다. `figma.io.write`는 asset export(`devup_figma_export`의 `assetRequests`)에만 read-only로 사용하며 Figma 파일을 변경하지 않습니다. fast snapshot/theme envelope는 항상 텍스트로만 반환되며 바이너리 transport를 쓰지 않습니다.
- stdout에는 MCP frame만 출력하고 trace는 stderr로 보냅니다.
- access token, refresh token, OAuth code, PKCE verifier는 Debug, trace와 MCP error에 포함하지 않습니다.
- Figma snapshot과 screenshot을 기본적으로 디스크에 저장하지 않습니다.
- screenshot, asset, TSX resource는 bounded memory artifact와 같은 TTL을 가지며 resource manifest에는 이름·MIME·크기·hash만 노출됩니다.
- 호환성 fixture는 고정한 JavaScript 플러그인의 268개 synthetic 입력입니다. 별도의 WQUW-151 회귀 fixture는 공식 MCP에서 read-only로 수집한 디자인 node/텍스트/token 이름만 포함하며 OAuth token, header, callback parameter, 사용자 계정·email은 포함하지 않습니다.

### 플러그인 호환성 corpus

`fixtures/devup-figma-plugin`은 `dev-five-git/devup-figma-plugin`의 고정 commit `243db650f1d635ab5385546a2a297eae4ea93515`에서 수집한 54개 test file과 978개 passing-test inventory를 추적합니다. upstream test 252개가 만든 JSON/golden 268쌍은 Rust serde/codegen 경로에서 byte parity를 전부 실행하고, 666개는 같은 동작 영역의 실제 Rust assertion에 연결했습니다. 나머지는 plugin module/codegen handler/iframe/notify/browser download 수명주기 38개와 read-only MCP가 의도적으로 수행하지 않는 Figma document/style/import write 22개입니다. `not_ported`는 0개이며, 비-parity 항목도 구체적인 MCP 경계 test를 가리킵니다. 즉 정확한 보장은 “268/268 snapshot byte parity, 666개 실행 가능한 대표 Rust assertion 연결, 60개 명시적 runtime/write 경계, 978-entry inventory”이고 JavaScript assertion 978개를 각각 별도 fixture로 복제했다는 뜻은 아닙니다. manifest는 LF로 정규화한 fixture와 snapshot 536개 파일의 SHA-256을 검증하고, coverage registry는 ledger가 실제 Rust test symbol 또는 근거가 있는 비-parity 분류만 참조하도록 강제합니다. 상세 분류와 실행 방법은 [`fixtures/devup-figma-plugin/README.md`](fixtures/devup-figma-plugin/README.md)를 참고하세요.

### 실제 Figma JSON contract gate

`crates/devup-mcp/tests/live_figma_contract.rs`는 기본적으로 ignore됩니다. `DEVUP_MCP_LIVE_FIGMA=1`을 설정하고 공식 MCP의 fast `use_figma` 결과를 stdin에 한 줄로 전달하면 실제 payload를 디스크에 쓰거나 출력하지 않고 envelope 무결성, serde round-trip, 요청 context, node/리소스 수와 DevupUI codegen을 검증하고 안전한 count/hash 요약만 출력합니다. 별도의 비-ignore corruption test는 깨진 fast 응답이 legacy metadata 수집으로 원자적으로 폴백하는지 확인합니다.

`crates/devup-mcp-figma/tests/explore_script_behavior.mjs`는 compile-in `explore.js` 자체를 mock Figma scene graph에서 실행합니다. 두 단계 이상 중첩된 화면의 parent chain, 화면이 없는 1,000-node Section의 `projectionLimit * 8` 방문 상한, 필수 node만 남기는 14,000자 이하 fallback을 검증하며 CI의 Node 내장 test runner로 실행됩니다. 제품 binary와 기본 Cargo test에는 JavaScript runtime 의존성이 추가되지 않습니다.

legacy 경로에서 실제 확인된 공식 metadata는 XML text content envelope이며, local 변수/style은 catalog 후 resource 단위로 수집합니다. style의 `consumers`처럼 단일 field가 공식 MCP의 text 상한(실측 20,480 UTF-8 바이트, 넘는 만큼 잘리고 `// truncated to 20kb`가 붙음)을 넘을 수 있으므로, base field와 320개 단위의 compact consumer relation을 분리해 읽고 Rust에서 원래 exhaustive JSON shape로 재조립합니다. legacy node snapshot도 byte budget과 cursor를 사용해 같은 상한 아래에서 자동 재개합니다. range의 누락·중복이나 수집 중 목록 변경은 성공으로 숨기지 않고 오류로 처리합니다.

### Server module ownership

`server/mod.rs`는 MCP tool router, service construction, handoff 연결과 `ServerHandler`만 소유합니다. `projection.rs`는 TSX/theme/source-map/asset/reference output 생성과 delivery transaction을, `validation.rs`는 output/schema/artifact capability 입력 검증을 담당합니다. `delivery.rs`, `artifacts.rs`, `output.rs`, `resources.rs`, `quality.rs`는 각각 크기 결정, memory artifact, allowlisted filesystem transaction, MCP resource protocol, typed 품질 집계를 담당하며 source-level boundary test가 generator와 filesystem 구현이 router로 되돌아오는 것을 막습니다.

2026-09-01 실제 파일 검증에서는 13개 page 전체 검색으로 `[FR-026] 본연체` Section (`4217:7743`)을 찾고, 그 안의 360×740 화면 10개를 시각 순서대로 인덱싱해 `A : STORY-F-PROOFREAD` (`3879:35518`)를 정확한 대상으로 선택했습니다. Section 전체 fast envelope는 8 MiB 안전 상한을 넘어서므로 성공으로 오인하지 않고, 각 화면을 공식 read-only MCP로 개별 수집했습니다. 열 화면은 각각 15~210개 node를 가지며 모든 child, styled text segment, 변수 3~25개와 text style 2~13개의 참조 완전성을 실제 JSON fixture와 DevupUI TSX snapshot으로 검증합니다. 대표 proofread 화면은 공식 read-only MCP 1회, 3개 PNG envelope 청크에서 144개 node, 변수 20개와 text style 11개를 수집했고 폴백은 없었습니다. instance children, concrete boolean property, mixed typography, nested `[1. 이름]`, token binding과 개별 footer stroke도 Rust snapshot/live contract로 검증했습니다. 같은 파일의 전체 theme export는 legacy 공식 read-only 호출 89개를 통해 collection 1개, variable 49개, style 37개, mode 2개를 수집해 42,794자 `devup.json`을 생성했으며 diagnostics는 0개였습니다. 현재 full-theme fast collector는 같은 collection/variable/style 전체를 단일 read-only `use_figma` 호출로 수집하고, envelope 검증 실패 시에만 이 legacy 경로를 0부터 다시 시작하도록 contract test로 고정했습니다.

## Snapshot 의미와 현재 한계

Figma Remote MCP에서는 `JSON_REST_V1` export가 허용되지 않으므로 host object를 그대로 REST JSON으로 만들 수 없습니다. 대신 checked-in property manifest와 runtime prototype/enumerable 탐색을 함께 사용해 모든 발견한 data field의 key와 읽기 결과를 보존합니다. 함수, 순환 node object는 제외하거나 id로 바꾸고, binary asset은 bytes 대신 metadata로 나타내며, 타 plugin private data와 오류를 내는 getter는 읽을 수 없습니다. 단일 값이 byte budget을 넘으면 key를 없애지 않고 `{ "$truncated": ..., "byteLength": ... }`와 `DEVUP_FIELD_VALUE_TRUNCATED`를 남기며, `characters`, styled segment, resource binding처럼 UI 변환에 필요한 값은 우선 보존합니다.

현재 private MVP의 남은 한계는 다음과 같습니다.

- 공식 `get_metadata`의 file-level page 목록은 실제 page 전체보다 적게 반환될 수 있습니다. 이름 검색은 Plugin API page catalog와 per-page projection으로 우회하며 실제 13개 page 파일에서 검증했습니다.
- 매우 큰 computed field(예: vector `fillGeometry`)는 현재 값 전체 대신 명시적인 byte-length marker로 보존됩니다. 모든 대용량 field 값을 lossless하게 export하는 기능은 후속 wire-format 개선 대상입니다.
- exact-node fast envelope가 8 MiB 안전 상한을 넘거나 공식 MCP가 image transport를 바꾸면 자동 legacy fallback이 여러 cursor call을 사용하므로 subtree 크기에 따라 시간이 늘어날 수 있습니다.
- direct OAuth registration은 Figma MCP Catalog 승인이 없는 `client_name`으로는 거절됩니다. 승인된 이름(기본값 `Codex`)으로만 등록이 성립하며, 그 등록은 Figma에게 해당 제품으로 기록됩니다.
- 사용되지 않은 외부 Figma library 변수 전체는 Remote MCP가 제공하지 않을 수 있습니다.
- node/page theme scope는 로컬 변수 API의 file-wide 결과를 기반으로 하며 세밀한 사용 범위 필터는 후속 보강 대상입니다.
- vector, mask, image, absolute layout과 일부 effect는 diagnostics를 포함한 제한적 fallback입니다.
- Figma Remote MCP의 `use_figma` tool contract가 바뀌면 live smoke test와 adapter 갱신이 필요합니다.

상세 설계는 [`docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md`](docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md)를 참고하세요.

`docs/superpowers/` 아래의 plan·spec은 **작성 시점의 기록**이지 현재 API 문서가 아닙니다. 예를 들어 위에서 제거했다고 적은 `devup_figma_to_ui`/`devup_figma_to_json`을 그 문서들은 아직 현재 도구처럼 기술합니다. 현재 도구 목록과 동작의 기준은 이 README와 서버가 게시하는 스키마·tool description입니다.

### 생성 코드의 배치 계약

absolute 자식이 있는 프레임은 생성 코드가 containing block을 만듭니다. `standalone`에서는 PAGE/SECTION 아래의 루트도 FIXED 축의 원본 크기를 보존합니다. 예를 들어 WQUW-120의 360×740 수직 프레임은 `VStack pos="relative" w="360px" h="740px"`가 되어 모달의 `left="50%"`, `w="100%"`가 이 루트를 기준으로 계산됩니다. absolute 자식이 없는 화면의 기존 유동 크기 정책은 유지됩니다.

`placementContracts`는 `includeDiagnostics` 없이도 반환하며 SECTION 응답에는 프레임별 목록도 있습니다. 각 계약의 `details.output`, `generatedRoot`, `sourceSize`, `containingBlock`, `parentCollected`, `parentIncludedInOutput`, `hostRequirements`를 함께 확인하세요. 캡처 밖 부모가 없으면 그 사실을 명시하며, 일반 루트는 앱의 normal flow에 삽입됩니다. Figma canvas의 x/y가 앱 안의 위치를 뜻하지 않습니다.

`embedded`에서도 absolute 자식의 기준 요소는 유지하지만 루트 크기는 호스트/내용에 의존합니다. 이 의존이나 absolute 루트의 외부 부모 의존은 `hostRequirements`와 `projectionIssues`에 근사로 기록합니다. 이를 충족하거나 containing parent를 `standalone`으로 export한 후 배치를 수용하세요. 이 계약은 CSS 생성 근거이며 브라우저 픽셀 비교나 반응형 동일성 보증이 아닙니다.

### R1 응답 일관성

`projectionIssues`는 `includeDiagnostics` 없이도 근사·손실의 노드, 필드, 사유와 원본/적용 근거를 반환합니다. 분류 근거 없는 `uncoveredLayout`은 `lossy`이며 `exact`나 최종 산출물로 표시하지 않습니다. 숨김 자산 참조를 제거하고, 제외 자산이 여전히 필요한 출력은 실패 사유와 함께 보류합니다.

`quality.assets`는 자산 바이트 수집 품질이며 manifest 생성 성공을 뜻하지 않습니다. `assetSummary.description`과 `excludedCount`가 미수집·부분 수집·숨김 제외를 설명합니다. 바이트 수집을 요청하지 않았다면 `not-collected` 요약과 `quality.assets: not-requested`가 함께 올 수 있습니다.

배치 응답의 `recommendedBatchSize`는 최대 3, `maxFrameCount`는 6, `maxFrameOutputUnits`는 12입니다. 자세한 정의는 [R1 응답 계약](docs/r1-result-contract.md)을 참고하세요.


### R8 provenance, scrolling and acquisition

`sourceMap.version=2`는 공개 오프셋을 제거한 계약입니다. `exact`는 원본 필드에 대응하는 생성 속성/태그/텍스트 매핑을 검증했다는 뜻입니다. 줄 위치, 브라우저 픽셀 일치, 반응형 동등성 또는 화면 전체 완성도를 보증하지 않습니다. `generatedProperty`는 `h="740px"`, `flex="1"`, `aspectRatio="320 / 48"`처럼 출력에 있는 속성을 담습니다. `characters`는 `children`, 암묵적 cross-axis stretch는 `implicit:align-self:stretch`로 표시합니다. grow는 실제 `flex` 속성과 `accounted-for-implicit-flex-grow`를 함께 읽습니다. 기존 sizing 검증과 진단은 유지됩니다. 내부 renderer/validator의 위치 정보는 공개 sourceMap에 직렬화하지 않습니다.

`overflowDirection`은 캡처 manifest에 포함됩니다. `NONE`은 명시적 비스크롤, JSON `null`은 수집 시 원본 속성이 없음, 키 부재는 이전 수집기의 미수집, `fieldErrors.overflowDirection`은 읽기 실패입니다. 세로/가로/양방향 스크롤은 각각 `overflowY="auto"`/`overflowX="auto"`/`overflow="auto"`로 생성합니다. `clipsContent=true`는 스크롤을 취소하지 않으며, 한 축 스크롤에서는 다른 축을 clip합니다. ABSOLUTE FIXED 높이는 파생 padding이 있어도 명시적 높이로 보존합니다. FILL/HUG를 px로 고정하지 않습니다.

일반 화면 export도 수집이 길어지면 약 1초의 초기 대기 후 `exportJob`을 반환합니다(`assetJob`은 호환 별칭). `jobId`로 상태를 조회하고 paused 상태는 `jobAction:"resume"`으로 재개합니다. `calls`의 frame/root ID·pagination·elapsedMs와 projection 시간을 확인할 수 있습니다. 예산은 frame-output unit당 5–20초의 계획용 추정이며 보장이 아닙니다. 요청 한도 6 frames/12 units와 별개로 upstream 지연은 job으로 처리합니다. 지연 프레임을 분리하려면 한 프레임씩 호출하십시오. 작업은 서버 프로세스 안에서 30분, 완료 응답은 5분 보존되며 재시작을 넘겨 보존되지 않습니다.

artifact의 `artifactState`는 `expired`(확인된 TTL 만료), `evicted`(확인된 캐시 제거), `unknown`(현재 서버에서 원인 판단 불가)을 구분합니다. 최근 제거 64개 이내의 복구 metadata가 있으면 canonical URL과 선택을 `nextAction.arguments`로 제공합니다. URL 원문의 파일 제목은 저장하지 않아도 같은 대상을 가리키는 canonical URL을 구성합니다. metadata가 없으면 `recoveryState:"unrecoverable"`로 명시하며, 서버 재시작이라고 단정하지 않습니다.


### R9 sourceMap 필드와 resolution

TSX 항목의 기본 필드는 `nodeId`, `property`, `generatedProperty`, `resolution`입니다. 선택적 `variableId`는 원본 변수 토큰 참조, `styleId`는 typography/style 토큰 참조, `assetId`는 이미지·벡터 자산 참조를 식별할 때 붙습니다. 한 항목에 여러 식별자가 함께 있을 수 있으며 해당 참조가 없으면 생략합니다. devup.json 매핑은 `jsonPointer`로 테마 위치를 가리키며 TSX 기본 필드가 모두 있는 것은 아닙니다.

| resolution | 의미와 검증 범위 |
| --- | --- |
| `exact` | 원본 필드와 생성 태그·텍스트의 의미 매핑. 화면 전체의 픽셀/반응형 동등성 보증은 아님. |
| `raw-fallback` | 토큰 또는 전용 매핑 라벨이 없는 일반 원본 값·생성 정책 매핑 경로. raw 값의 무변환 복사나 낮은 신뢰도를 뜻하지 않으며, 별도 ABSOLUTE component가 `verified`여도 이 라벨은 유지됩니다. |
| `verified-explicit-dimension` | 읽기 오류 없는 원본 width/height 수치와 생성 w/h/boxSize의 px 값이 정확히 같음을 확인. 반응형 배치까지 검증한 것은 아님. |
| `accounted-for-content-sizing` | textAutoResize에 따른 고정 크기 생략의 의미 매핑. 폰트 메트릭과 브라우저 픽셀은 미측정. |
| `verified-layout-sizing` | 원본 FIXED/FILL/layoutGrow 의도와 생성 h/boxSize/flex 관계를 sizing 검증기로 확인. |
| `accounted-for-implicit-flex-stretch` | 실제 생성 부모의 cross-axis stretch와 크기 기준으로 생략된 크기를 설명. |
| `accounted-for-implicit-flex-grow` | 실제 생성 부모의 main-axis flex-grow와 남은 공간으로 크기를 설명. |
| `restored-hug-after-mask-child-folding` | 접힌 mask 자식의 원본 크기와 독립적인 크기 기준으로 HUG/aspectRatio 복원. `-from-absoluteBoundingBox` 접미사는 bounding box에서 크기를 얻은 경우. |
| `variable-token` / `style-token` | 원본 변수/스타일 ID와 생성 토큰 참조의 매핑. |
| `asset` | 원본 자산 ID와 생성 자산 속성의 매핑. 바이너리 수집 성공 여부는 assetSummary에서 별도 확인. |
| `variant-selector` | 원본 variant 선택을 생성 selector 속성과 연결. |
| `unverified-property-mapping` | 원본에 대응하는 생성 속성을 확인하지 못함. 추가 검증 필요. |
| `variable` / `alias` / `style` | devup.json의 직접 변수 값 / 해석된 변수 alias / 스타일에서 나온 테마 값과 JSON pointer. |

`node`는 내부 노드 범위용이며 공개 v2 property entries에는 포함되지 않습니다.

R16의 `sourceMap.resolutionSemantics`는 이 라벨 사전과 `axis="mapping-method"`를 응답에 포함합니다. Section frame은 `dictionary="/resolutionSemantics"`로 상위 응답의 공통 사전을 참조하며, 이 사전은 resource delivery에도 남습니다. `diagnostics.details.components.*.state`는 별도의 제한된 검증 축입니다. 같은 screen/output 안에서 nodeId와 원본 property(width/height/x/y → width/height/horizontal/vertical)로 연결합니다. 예를 들어 `w="100%"`의 매핑은 `raw-fallback`이지만, 확정된 동일 FIXED 부모 폭에서 해석한 값이 원본 폭과 같으면 ABSOLUTE width는 `verified`일 수 있습니다. 이 두 라벨 모두 화면 전체의 픽셀·반응형 등가성 측정은 아닙니다.

R16은 실패한 치수 증명의 `blockedBy`를 추가합니다. 부모 폭 불명(`parent-width-unknown`), 부모 폭 불일치(`parent-width-unequal`), 읽기 오류(`read-error`), 비FIXED sizing(`non-fixed-sizing`), dimension 충돌(`conflicting-dimension-props`)을 구분합니다. 성공 시 null이며 widthPreservation에도 같은 값이 들어갑니다. 첫 차단 이유만 보고하므로 나머지 조건의 통과나 다른 실패 분기의 실행을 뜻하지 않습니다. 자산은 부모 percentage 증명 대신 실제 선택된 render-boundary 증명의 실패 이유를 보고합니다.

최종 응답과 각 frame의 `verdictScope`는 status/projection을 그대로 표시하고 `statusCauses`와 `projectionCauses`로 원인을 요약합니다. projection 원인은 nodeId/screenId/output/code/property와 미해결 component를 가리키므로, width/height가 verified여도 horizontal/vertical이 approximated인 화면의 전체 판정을 바로 설명합니다. `diagnosticIndex`는 같은 객체의 projectionIssues 인덱스입니다. includeDiagnostics=false 및 resource delivery에도 요약이 유지됩니다. 자세한 범위는 [R16 응답 계약](docs/r16/response-contract.md)을 참고합니다.

ABSOLUTE 진단의 `components`는 height/width/horizontal/vertical별 state·fidelityImpact·원본 값·생성 값·검증 이유를 제공합니다. `resolutionConditions`에는 남은 근사 항목의 해소 조건만 들어갑니다. CENTER는 원본 중심 오프셋, left/top 50%, translate 및 실제 생성 부모 좌표 기준까지 확인해야 검증됩니다. MIN은 원본 오프셋과 생성 px를 비교합니다. MAX는 크기 검증을 전제로 parentSize-offset-size와 right/bottom 여백을 비교합니다. 제약 필드 전체가 없는 이전 캡처는 `constraintDeclared=false`로 표시하며 생성된 로컬 오프셋만 비교하고 반응형 제약을 추정하지 않습니다. 증명되지 않은 MAX/STRETCH/SCALE, 누락 geometry 또는 필드 오류는 근사로 남습니다. 명시적 FIXED 높이 보존은 percentage 너비의 동등성을 증명하지 않습니다.

R14는 ABSOLUTE 자식의 FIXED 폭과 생성된 직계 containing block의 확정 px 폭이 같은 경우 `w="100%"`의 대응을 검증합니다. 특정 폭에 한정하지 않으며, 생성 코드는 바꾸지 않습니다. 현재 증명 범위는 읽기 오류가 없는 동일 크기의 FIXED 부모, 명시적 positioning, 부모 padding/border 및 크기 override가 없는 경우입니다. 부모 폭이 유동적이거나 값이 다르면 근사로 남습니다. `appliedValue.widthPreservation`에 `components.width`와 같은 근거, 부모 생성 폭, `resolvedPixels`, `sourceFields`, 계산식이 들어갑니다. `componentSummary.verified/unresolved`는 네 항목 중 검증된 것과 남은 것을 나눕니다.

ABSOLUTE 자산의 `components`는 `boundary`(선택한 export 경계·원본 bounding box와의 차이·sourceFields·계산식), `rounding`(입력·생성값·오차), `constraintInterpretation`(직접 제약·첫 자식 상속·기본값)을 분리합니다. 반올림은 `round(value*100)/100`와 생성값의 일치 및 최대 `0.005px` 오차를 확인하며 부동소수점 비교 여유는 `1e-12px`입니다. 경계 차이를 반올림 오차로 흡수하지 않습니다. 자산 치수의 verified는 수집된 render bounds 투영의 검증이며 원본 layout box와의 동일성이나 SVG/CSS 합성 측정은 아닙니다. GROUP의 null/누락 제약을 첫 자식에서 가져오거나 MIN으로 기본 처리한 경우 그 정책을 `assumed`로 표시합니다. 위치 계산과 반올림이 맞아도 GROUP의 배치 의도와 같다는 증명이 없으면 해당 위치 축은 `approximated`로 유지합니다.

비렌더 진단은 오류 없이 읽은 `visible=false` 또는 명시적 `absoluteRenderBounds=null`을 `accounted-for-non-rendering` / `fidelityImpact=none`으로 기록합니다. `verification`의 field·fieldPresent·value·readError가 근거입니다. 필드 부재나 읽기 실패는 `unverified-non-rendering` / `approximated`로 유지하며 재수집 조건을 안내합니다.

HUG 검증은 원본과 일치하는 생성 auto-layout에서 크기 override 없이 intrinsic sizing 의도를 표현했는지 확인합니다. FIXED percentage 크기는 일치하는 auto-layout과 명시적으로 같은 크기인 FIXED 부모, 테두리·padding·min/max override 부재가 확인된 제한된 경우만 검증합니다. CENTER 검증은 원본의 중심 오프셋 0과 생성 CSS의 중심 관계를 뜻합니다. 부모가 커질 때 원래 x를 고정한다는 뜻이 아니며, 부모의 명시적 px 값이 바뀌면 부모 자신의 dimension coverage 재검증이 실패합니다. 검증된 명시적 크기 매핑은 canvas/derived-padding의 일반 생략 규칙으로 재검증 의무를 면제하지 않습니다.

`projectionEvidence`는 확인된 비렌더와 검증 완료 ABSOLUTE 항목의 근거를 담으며, `includeDiagnostics=false`여도 최상위 및 프레임 응답에 제공합니다. 미해결 근사는 `projectionIssues`에 남으므로 두 배열을 구분해 읽습니다.

### R13 생성 속성 provenance와 출력 전달

최종 TSX의 모든 JSX 속성(표현식·boolean·spread 포함)을 파서로 검사합니다. 기존 `sourceMap`에 없는 속성은 생성 단계의 원본 필드와 계산으로 대조하며, 확인된 항목은 `DEVUP_CODEGEN_PROPERTY_EVIDENCE`로 `projectionEvidence`에 남깁니다. `sourceFields`, `originalValue`, `calculation`, `stage`, `generatedProperty`, `elementIndex`, `assetId` 및 `details.output`을 함께 읽습니다. 자산 경계 보정은 render bounds와 bounding box의 차이, 효과는 원본 `effects`를 근거로 남깁니다. 이 근거는 생성 계산의 대응이며 SVG/CSS 효과의 브라우저 합성 실측이 아닙니다.

설명할 수 없는 속성은 `DEVUP_CODEGEN_PROPERTY_UNMAPPED`, `mappingComplete=false`로 항상 보고합니다. 값은 유지되므로 이 진단의 `fidelityImpact`는 `none`입니다. 다른 손실·근사가 없는 경우 `quality.projection=mapping-incomplete`가 되며 `exact`와 최종 완료 판정을 허용하지 않습니다. 기존 lossy·approximated 판정은 유지합니다. 반응형 병합은 개별 breakpoint source mapping만으로 병합 속성을 증명할 수 없으므로 같은 진단을 제공합니다. `strict`는 매핑 누락도 거절합니다. `tsx`와 `componentTsx`를 함께 요청한 경우 `sourceMap.byOutput`이 출력별 map을 구분하며 기존 단일 map 필드는 호환성을 위해 유지합니다.

`outputPaths`의 프레임 출력 키는 `frame:<nodeId>:<output>`입니다. 예: `{"frame:3997:46715:tsx":"C:/allowed/Screen.tsx","frame:3997:46715:sourceMap":"C:/allowed/Screen.map.json"}`. `outputPathResults.supportedKeys`는 이번 투영에서 실제 쓸 수 있는 키를 열거합니다. 프레임 응답에 일반 `tsx`/`sourceMap` 키를 쓰거나 미지원·미생성 출력 키를 쓰면 `outputPathResults.diagnostics`에 명시하며 조용히 무시하지 않습니다. 최상위 단일 출력은 기존 일반 키를 사용합니다. `outputPaths` 응답은 성공적으로 커밋한 파일만 나타냅니다. 자산 binary는 계속 `assetRequests[].outputPath`를 사용합니다.

명시적 `resource`와 자동 resource 전환 모두 `nextAction.tool/arguments`로 동일 artifact의 작은 본문 대조 예시를 제공합니다. 일반 화면은 단일 frame·단일 출력이며 파일을 다시 쓰지 않습니다. `sizeEstimate.outputBytes`는 기존 선택 출력의 UTF-8 바이트 수(PNG는 binary 바이트 수)이고 `minimumWireBytes`는 메타데이터를 제외한 JSON 중복 전송 하한입니다. 재투영 응답의 전체 크기는 실측하지 않았으므로 한도 미만의 경우에도 `inlineFit=unknown`입니다. 확실히 한도를 넘는 경우 resource를 안내하며, inline 크기 초과 시 기존 R12 교정 인자를 유지합니다. 자산 요청·출력 경로가 코드 URL을 결정하는 경우에는 원래 결합된 resource 인자를 유지하여 잘못된 본문 대조를 방지합니다.
