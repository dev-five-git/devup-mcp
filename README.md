# devup-mcp

Rust-native MCP server that reads Figma designs and generates DevupUI artifacts. One binary acts as a local stdio MCP server and a read-only client of Figma Remote MCP.

저장소는 Cargo workspace이며 `devup-mcp` 실행 crate, OAuth·upstream·snapshot을 담당하는 `devup-mcp-figma`, TSX·theme projection을 담당하는 `devup-mcp-devup-ui`, PNG 비교 library/CLI인 `devup-mcp-visual`로 구성됩니다. 별도 IR/auth/server crate 없이 MCP 제품 설치 단위는 `devup-mcp` 하나입니다.

## 현재 제공 기능

- `devup_figma_auth`: Figma 연결 상태 확인, 브라우저 OAuth 로그인, 로그아웃, 그리고 연결 실패 원인을 실측해 보고하는 `doctor` 진단
- `devup_figma_to_ui`: Figma node 링크를 `@devup-ui/react` TSX로 변환
- `devup_figma_to_json`: Figma 변수와 로컬 스타일을 `devup.json`으로 변환
- `devup_figma_export`: Figma를 한 번 수집해 TSX, `devup.json`, raw snapshot, source map, asset manifest와 선택적 reference PNG를 함께 생성하거나 같은 artifact를 재사용
- `devup_figma_search`: 파일 전체의 page, section, frame, component를 이름으로 탐색
- `devup_figma_explore`: 링크된 요구사항/라벨 주변의 실제 화면 후보를 공간 순서로 탐색
- `devup_figma_continue`: host가 실행한 공식 Figma MCP read 결과로 중단된 변환을 재개
- Figma Plugin API의 readable data property를 raw JSON으로 보존하고, 알려지지 않은 runtime field는 `extra`, 실패한 getter는 `fieldErrors`로 유지

host handoff 경로에는 Figma PAT, 사용자가 만든 OAuth app, 내장 client secret이 필요하지 않습니다. direct 경로는 Figma Remote MCP의 OAuth discovery, Dynamic Client Registration, PKCE S256과 일시적인 `127.0.0.1` callback을 구현하지만, Figma는 현재 MCP Catalog에 승인된 client의 registration만 허용합니다. private build에서는 이미 인증된 공식 Figma MCP를 사용하는 `auto` 또는 `host`가 기본 경로입니다.

## 빌드와 설치

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

시스템 브라우저는 `devup_figma_auth`의 `login`을 명시적으로 호출할 때만 열립니다. 일반 변환의 기본 `auto` 정책은 direct credential이 없거나 Catalog/capability가 허용되지 않으면 브라우저를 열지 않고 공식 Figma MCP handoff를 반환합니다. 인증 정보는 운영체제 credential store에만 저장되며 `logout`은 해당 정보만 삭제합니다.

### 인증

```json
{ "action": "status" }
```

`action`은 `status`, `login`, `logout`, `doctor` 중 하나입니다. `status`/`login`/`logout`의 응답 형태는 항상 `{ "status": "connected" | "disconnected" }`입니다. Figma에 붙지 못하는 이유를 알고 싶으면 `doctor`를 호출하세요.

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
      "tokenState": "absent",
      "callbackPort": { "port": null, "free": null },
      "reason": "저장된 자격증명 없음. ..."
    },
    "localDevMode": { "endpoint": "http://127.0.0.1:3845/mcp", "reachable": false, "hint": "..." },
    "hostHandoff": { "expectedTool": "use_figma", "note": "..." }
  },
  "clientSetup": { "constraints": { ... }, "opencode": { ... }, "claudeCode": "...", "codex": "...", "localDevMode": { ... } }
}
```

`paths.localDevMode.reachable`은 `127.0.0.1:3845`에 대한 300ms 이내 로컬 TCP 연결 확인 결과이며 실패해도 오류를 던지지 않습니다. `needs_figma` 응답에도 같은 프로브 결과가 `hostRequirement.localDevMode`로 포함됩니다. `paths.direct.credentialSource`는 `cli-arg`, `env`, `credential-store`, `none` 중 하나이고, `tokenState`는 `valid`, `expired`, `absent` 중 하나이며, `callbackPort`는 `--figma-callback-port`를 지정했을 때만 실측한 `port`/`free`를 담습니다. 자세한 제약과 3가지 연결 경로는 아래 "Figma 연결 설정" 절을 참고하세요.

### direct 경로에 사전 등록된 client 자격증명 주입하기

Figma MCP Catalog에 승인된 client(예: 직접 waitlist로 등록해 발급받은 client)의 `client_id`/`client_secret`을 이미 가지고 있다면, devup-mcp에 다음 세 가지 방법 중 하나로 주입해 Dynamic Client Registration을 완전히 건너뛸 수 있습니다. 우선순위는 시작 인자 > 환경변수 > `configure`로 저장한 값입니다.

- **시작 인자**: `devup-mcp --figma-client-id <id> --figma-client-secret <secret>`
- **환경변수**: `DEVUP_FIGMA_CLIENT_ID`, `DEVUP_FIGMA_CLIENT_SECRET`
- **도구**: `devup_figma_auth { "action": "configure", "clientId": "...", "clientSecret": "..." }` — OS credential store(시작 인자/환경변수와는 별도 항목)에 저장되어 프로세스를 재시작해도 유지됩니다.

자격증명이 해석되면 `devup_figma_auth { "action": "login" }`은 registration 엔드포인트를 전혀 호출하지 않고 바로 authorization_code + PKCE 흐름으로 진입합니다. 자격증명이 없으면 기존과 동일하게 DCR을 시도하고, 403이면 host 핸드오프로 폴백합니다(하위호환 유지). devup-mcp는 자격증명이 있든 없든 DCR 요청의 `client_name`을 항상 정직하게 `"devup-mcp"`로 보냅니다 — 스스로를 `Codex`나 `Claude Code` 같은 다른 제품으로 신고하지 않습니다. `client_secret`은 로그, 에러, MCP 응답, `doctor` 출력 어디에도 노출되지 않으며 `doctor`는 `credentialSource`로 존재 여부만 보고합니다.

## Figma 연결 설정

devup-mcp가 Figma에 붙는 경로는 세 가지입니다.

1. **원격 OAuth (`direct`)** — `devup_figma_auth { action: "login" }`으로 브라우저 인증. Figma MCP Catalog에 승인된 client만 등록할 수 있습니다.
2. **로컬 Dev Mode MCP (`http://127.0.0.1:3845/mcp`)** — Figma 데스크톱 앱의 Dev Mode MCP 서버. OAuth가 필요 없고 어떤 MCP 클라이언트에서도 동일하게 동작하지만, Figma 데스크톱 앱에서 켜야 하고 Dev/Full 시트가 있는 유료 플랜이 필요합니다.
3. **호스트 핸드오프 (`host`)** — devup-mcp가 직접 Figma에 붙지 않고, 호스트에 이미 등록된 공식 Figma MCP가 `needs_figma` 응답의 `calls`를 대신 실행하도록 위임합니다. `auto` 정책의 기본 fallback 경로입니다.

세 경로 중 무엇이 지금 사용 가능한지는 `devup_figma_auth { action: "doctor" }`로 확인하세요.

### 원격 OAuth 등록 제약 (실측)

Figma Remote MCP 등록 엔드포인트는 `POST https://api.figma.com/v1/oauth/mcp/register`입니다. 요청 본문의 `client_name`은 정확히 일치하는 allowlist로만 승인됩니다.

| client_name | 결과 |
|---|---|
| `Codex` | 200 (client_id + client_secret 발급) |
| `Claude Code` | 200 |
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

Claude Code와 Codex는 allowlist에 있어 별도 설정 없이 등록할 수 있습니다.

```bash
claude mcp add --transport http figma https://mcp.figma.com/mcp
codex mcp add figma --url https://mcp.figma.com/mcp
```

### Figma → DevupUI

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "componentName": "OptionalComponentName",
  "includeDiagnostics": true,
  "rootLayout": "standalone",
  "sourcePolicy": "auto",
  "scope": "node",
  "outputPath": "optional/path/Component.tsx"
}
```

결과에는 `tsx`, import 목록, 사용된 token, source 식별자, 보존한 node 수와 fallback diagnostics가 포함됩니다. Auto Layout은 `Flex`, 일반 container는 `Box`, text는 `Text`로 변환하고 theme binding이 있으면 JSX prop에서 `$token`을 우선 사용합니다. 변수 token은 비어 있지 않은 Figma `codeSyntax.WEB`을 우선하고, 없으면 변수 경로의 마지막 이름을 정규화합니다. 따라서 TSX의 `$token`, `usedTokens`, `devup.json` key와 source map이 같은 이름을 사용합니다. `rootLayout` 기본값인 `standalone`은 선택한 root의 크기·위치 제약까지 포함하고 Figma instance의 실제 자식 상태를 펼쳐 정의되지 않은 component 참조를 만들지 않습니다. 이미 레이아웃을 소유한 React 부모 안에 삽입할 때는 `rootLayout: "embedded"`로 root의 외부 크기·위치 제약만 생략합니다.

### Figma → devup.json

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "scope": "file",
  "includeDiagnostics": true,
  "sourcePolicy": "auto",
  "outputPath": "optional/path/devup.json"
}
```

결과는 `theme.colors`, `theme.typography`, `theme.length`, `theme.shadow`를 포함하는 결정적 JSON 문자열과 counts, completeness를 반환합니다.

`outputPath`를 생략하면 결과를 메모리와 MCP 응답에만 유지합니다. 명시하면 생성된 TSX 또는 `devup.json`만 해당 경로에 기록하고 실제 절대 경로를 응답합니다. 기본 허용 write root는 `devup-mcp` process 시작 당시의 current directory 하나이며, 그 밖의 workspace는 반복 가능한 `--allow-write-root <directory>` 시작 인자로만 추가할 수 있습니다. Tool 입력으로 root 자체를 넓힐 수 없고 `..`, 다른 drive/UNC, alternate data stream, symlink/junction을 통한 root 탈출은 기록 전에 거절됩니다.

여러 text/asset output은 모두 검증한 뒤 같은 directory의 exclusive 임시 파일에 staging하고 한 transaction으로 교체합니다. 정상 runtime 오류에서는 이미 교체한 파일을 역순으로 되돌리고 기존 파일을 복구합니다. 개별 rename은 atomic하지만 여러 directory와 process crash를 가로지르는 완전한 원자성은 일반 filesystem 특성상 보장하지 않습니다.

### 통합 수집과 다중 출력

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "outputs": ["tsx", "devupJson", "sourceMap", "assetManifest", "referencePng"],
  "scope": "node",
  "strict": true,
  "refresh": false,
  "sourcePolicy": "auto",
  "delivery": "auto"
}
```

`devup_figma_export`는 동일한 node/resource acquisition에서 여러 projection을 생성합니다. 응답의 `cache.artifactId`를 다음 요청의 `artifactId`로 넘기면 Figma를 다시 호출하지 않고 다른 output을 만들 수 있습니다. URL 요청은 같은 process 안에서 10분 TTL, 최대 8개/항목당 32 MiB/전체 128 MiB인 memory-only LRU cache를 재사용하며, `refresh: true`는 완료 cache뿐 아니라 진행 중 요청 공유도 우회해 URL을 새로 수집합니다. 동일 acquisition의 선행 작업이 취소되더라도 닫힌 in-flight 표식을 다음 요청이 원자적으로 제거하고 다시 수집하므로 같은 key가 process 수명 동안 오염되지 않습니다. `cache`에는 `reuseKind`, `ageSeconds`, `remainingTtlSeconds`, `avoidedFigmaToolCalls`, 원 수집의 `originCollection`이 포함되고, 응답 최상위 `collection`은 현재 요청이 실제로 실행한 호출만 집계합니다. `cache.capabilities`는 artifact의 `kind`(`design`, `theme-only`, `search`, `explore`), `collectionScope`, `resourceScope`, `referencePng` 보유 여부와 redacted `assetCaptureCount`만 공개합니다. 내부 artifact는 asset ID·format·scale 전체를 보존하고 세 값이 정확히 같은 capture만 추가 Figma 호출 없이 재사용합니다. 재사용 요청이 이 범위를 넘으면 `DEVUP_FIGMA_HANDOFF_INVALID`로 투영과 파일 기록 전에 거절합니다. 예를 들어 node/used-resource artifact로 file 전체 `devupJson`을 만들거나 screenshot을 수집하지 않은 artifact로 `referencePng`를 만들 수 없습니다. credential, screenshot과 asset binary는 cache key나 통계에 포함하지 않고, process가 끝나면 cache도 사라집니다.

`delivery`는 `auto | inline | resource`입니다. `auto`는 JSON escape, base64와 structured/text 이중 표현을 포함한 실제 MCP wire 크기를 계산해 개별 256 KiB·합계 1 MiB 이하만 inline으로 반환하고, 그보다 큰 결과는 native MCP `ResourceLink`와 `devup://artifact/...` URI로 바꿉니다. 링크 URI는 JSON manifest를 가리키므로 link MIME은 `application/json`이고 payload MIME·길이·SHA-256은 `payload*` metadata로 분리합니다. `resource`는 크기와 무관하게 TSX/JSON/PNG를 bounded chunk resource로 제공하며, binary chunk는 base64 MCP blob입니다. asset manifest는 binary를 내장하지 않고 각 asset의 독립 resource URI·MIME·길이·SHA-256을 참조하므로 `resources/read`로 원본 bytes를 정확히 재구성할 수 있습니다. 같은 artifact와 정규화한 projection은 content hash가 같은 resource를 재사용합니다. 파일 출력과 새 resource publication을 함께 요청하면 resource 조회를 reservation 동안 차단한 하나의 transaction으로 다루며, 파일 commit이 전부 성공한 뒤에만 resource와 LRU 변경을 공개합니다. 실패하면 원래 파일을 fingerprint로 검증해 복원하고 복원 불능 backup 경로를 구조화해 보고합니다. 현재 transaction이 만든 temp는 정상 종료·rollback에서 직접 제거하지만, 소유권을 증명할 수 없는 pre-existing temp나 crash·rollback recovery backup은 자동 삭제하지 않습니다.

`referencePng`는 선택했을 때만 공식 read-only `get_screenshot`을 정확히 한 번 추가 호출합니다. 결과는 정확히 하나의 top-level `image/png` content block이어야 하며, JSON/text에 숨긴 image나 다중 image는 거절합니다. 16 MiB compressed, 8192px, 64 MiB decoded 상한 안에서 PNG 전체를 실제 decode한 뒤 byte length와 SHA-256을 확인해 artifact에 보존하며, 단일 링크 node에만 적용됩니다. Section의 여러 Frame은 먼저 반환된 canonical URL별로 수집해야 합니다. PNG bytes는 log·통계·cache key에 포함되지 않으며 `outputPaths.referencePng`를 명시하지 않으면 디스크에 기록하지 않습니다.

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

`acquisition`은 `complete | expected-projection | partial | failed`, `projection`은 `exact | approximated | lossy | failed | not-requested`, `theme`은 `complete | conflicted | unresolved | not-requested`, `assets`는 `complete | partial | failed | not-requested`입니다. 검색·탐색의 의도적인 얕은 graph는 `expected-projection`으로 정상 완료하지만, 포함된 field의 실패나 truncation은 `partial`입니다. mask/effect fallback은 `lossy`, absolute layout fallback은 `approximated`이며 `includeDiagnostics: false`여도 품질 판정에는 반영됩니다. 기존 `status`는 요청한 모든 축이 정확하거나 완전할 때만 `complete`이고, `strict: true`는 모든 요청 축이 exact/complete가 아니면 quality와 `completenessReport`를 담은 오류로 거절합니다.

모든 공개 TSX generator는 반환 전에 Rust의 고정된 TypeScript+JSX parser를 통과합니다. parser 오류는 디자인 원문을 노출하지 않고 byte range와 오류 category만 반환합니다. 응답의 `fidelity`는 생성된 mapping 수가 아니라 수집한 source snapshot에서 독립적으로 계산한 node/text segment/variable/style/asset/layout 기대 집합을 분모로 사용하고, 각 항목이 최종 TSX byte range에서 `emitted | flattened | ignored` 중 정확히 하나로 추적되었는지와 축별 coverage·typed impact count를 담습니다. component set, non-default variant selector와 inline instance도 최종 변환 후 source identity별 provenance를 다시 만들며, 반복된 동일 text segment는 하나의 mapping을 재사용하지 않고 occurrence별로 소비하고 multiline·중첩 text와 asset identity를 검증합니다. 알 수 없는 codegen warning/error도 각각 최소 `approximated`/`failed`로 보수적으로 판정하며, `strict`는 syntax, source-derived trace coverage, lossy/failed impact를 함께 검사합니다.

브라우저 시각 회귀는 MCP 서버가 임의 명령을 실행하지 않고 소비자 repository가 실제 font/asset/DevupUI 환경으로 `actual.png`를 만든 뒤 순수 Rust `devup-mcp-visual`로 비교합니다. renderer pinning, 기본 0.5% threshold, diff PNG와 개인정보 취급 계약은 [`docs/visual-renderer-contract.md`](docs/visual-renderer-contract.md)에 있습니다.

Section 링크에서 TSX를 요청하면 먼저 내부 screen frame 후보와 canonical URL을 `selection_required`로 반환합니다. `frameIds`로 검토한 frame만 고르거나 `allScreens: true`로 모든 화면을 시각 순서대로 batch export할 수 있으며 두 옵션은 동시에 사용할 수 없습니다. `sourceMap`은 생성 TSX/devup.json의 output 위치를 Figma node, variable, style, asset ID에 연결하는 sidecar입니다. `assetManifest`는 image hash/vector/export provenance를 항상 열거하고, `assetRequests`로 명시한 항목만 최대 16개·scale 1~4 범위에서 read-only SVG/PNG export합니다. `outputPath`를 지정하면 binary를 해당 파일로 디코딩하고 응답의 base64를 제거하며, 생략하면 후속 소비를 위해 base64가 memory-only artifact와 해당 MCP 응답에 남을 수 있습니다.

### Figma 이름 검색

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>",
  "query": "A : STORY-F-PROOFREAD",
  "nodeTypes": ["PAGE", "SECTION", "FRAME", "COMPONENT_SET"],
  "match": "normalized",
  "limit": 20,
  "sourcePolicy": "auto"
}
```

검색은 먼저 read-only Plugin API로 실제 `figma.root.children` page catalog를 얻고, page마다 한 번씩 전환하는 작은 query projection을 병렬 실행합니다. 전체 page snapshot을 응답하지 않으므로 큰 파일에서도 공식 MCP text 상한을 피합니다. 결과는 원문 exact, Unicode NFC·공백·대소문자를 정규화한 exact, prefix, contains 순으로 정렬하고 `match: "fuzzy"`일 때만 오타 허용 검색을 추가하며, node ID, type, page, 전체 breadcrumb와 후속 `devup_figma_to_ui`에 그대로 전달할 canonical URL을 포함합니다.

### 링크 주변 화면 탐색

```json
{
  "url": "https://www.figma.com/design/<file-key>/<name>?node-id=1-2",
  "limit": 50,
  "includeTextPreview": true,
  "refresh": false,
  "sourcePolicy": "auto"
}
```

요구사항 제목이나 설명 node 링크가 실제 구현 화면이 아닐 때 `devup_figma_explore`를 먼저 호출합니다. anchor와 같은 공간 묶음의 frame/component 후보를 시각 순서와 canonical URL로 반환하며, 다음 요구사항 제목에서 탐색 범위를 끝냅니다. 같은 파일·옵션에서 이미 수집한 더 큰 탐색 결과는 exact·related-node·superset 범위로 재사용되고, 동시에 들어온 호환 요청도 공식 Figma 호출 하나를 공유합니다. `refresh: true`는 모든 재사용을 건너뜁니다. 원하는 후보의 canonical URL을 `devup_figma_to_ui`에 넘겨 정확한 화면만 변환합니다.

탐색과 검색은 변수 catalog를 수집하지 않습니다. 정확한 UI 변환 단계에서 선택 subtree의 모든 보존 필드에 있는 `VARIABLE_ALIAS`와 paint/text/effect/grid style ID를 재귀적으로 스캔하고, 실제 사용된 ID만 공식 Figma API로 조회합니다. `devup_figma_to_json`만 file 전체 로컬 catalog를 수집합니다.

`sourcePolicy`는 `auto`, `direct`, `host` 중 하나입니다. `needs_figma` 응답의 read-only call을 host의 공식 Figma MCP에서 실행한 뒤 원본 result를 `devup_figma_continue`의 `sessionId`, `callId`, `result`로 전달하면 동일한 Rust collector가 이어서 처리합니다. session은 메모리에만 최대 10분 유지되며 완료·오류·만료 시 제거됩니다. direct 경로는 연결과 read-only capability catalog 조회를 각각 30초, 개별 tool 호출을 5분으로 제한합니다. deadline을 넘기면 해당 remote session을 폐기하고 디자인 원문 없이 `retryable` timeout 단계만 반환합니다.

정확한 node 링크의 UI 변환은 우선 하나의 공식 `use_figma` 호출 안에서 subtree 전체와 실제 사용 리소스를 수집합니다. JSON envelope를 512 KiB 단위로 나누고 각 조각을 CRC가 있는 1×1 PNG에 담아 MCP 응답 크기 제한을 피하며, Rust는 MIME·base64·PNG 구조·청크 순서·schema·대상 ID·node graph·리소스 참조를 모두 검증한 뒤에만 결과를 채택합니다. 한 항목이라도 불일치하면 fast 결과 전체를 버리고 기존 cursor 수집을 0부터 재시작합니다. Section multi-root에서는 성공한 root와 resource는 그대로 보존하고 실패하거나 상한을 넘은 root만 legacy로 다시 수집한 뒤 원래 시각 순서로 합칩니다. direct upstream은 연결과 read-only tool catalog를 한 session에서 재사용하고 30초 TTL, 연결 종료 또는 transport 오류 때만 재연결·재검증합니다. 결과의 `stats`에는 `figmaToolCalls`, `transport`, `fallbackUsed`, node/variable/style 수와 byte/청크 수만 포함되며 원본 디자인이나 인증 정보는 포함되지 않습니다.

완전성 등급은 다음과 같습니다.

- `full-local-plus-used-remote`: 로컬 전체와 사용된 외부 token을 모두 확인
- `used-tokens`: 확보한 token만 변환했으며 외부 전체를 보장하지 않음
- `resolved-values-only`: 의미 있는 token binding 없이 계산값만 확보

## 읽기 전용·개인정보 보호

- upstream 호출은 `get_metadata`, `get_variable_defs`, `get_design_context`, `get_code_connect_map`, `get_screenshot`과 내장된 read-only `use_figma` script로 닫혀 있습니다.
- 사용자 입력 JavaScript를 받지 않으며 Figma document mutation API를 호출하지 않습니다. `figma.io.write`는 공식 MCP 응답으로 검증 가능한 1×1 PNG를 반환하는 transport에만 사용하며 Figma 파일을 변경하지 않습니다.
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

legacy 경로에서 실제 확인된 공식 metadata는 XML text content envelope이며, local 변수/style은 catalog 후 resource 단위로 수집합니다. style의 `consumers`처럼 단일 field가 공식 MCP의 약 20,500자 text 상한을 넘을 수 있으므로, base field와 320개 단위의 compact consumer relation을 분리해 읽고 Rust에서 원래 exhaustive JSON shape로 재조립합니다. legacy node snapshot도 byte budget과 cursor를 사용해 같은 상한 아래에서 자동 재개합니다. range의 누락·중복이나 수집 중 목록 변경은 성공으로 숨기지 않고 오류로 처리합니다.

### Server module ownership

`server/mod.rs`는 MCP tool router, service construction, handoff 연결과 `ServerHandler`만 소유합니다. `projection.rs`는 TSX/theme/source-map/asset/reference output 생성과 delivery transaction을, `validation.rs`는 output/schema/artifact capability 입력 검증을 담당합니다. `delivery.rs`, `artifacts.rs`, `output.rs`, `resources.rs`, `quality.rs`는 각각 크기 결정, memory artifact, allowlisted filesystem transaction, MCP resource protocol, typed 품질 집계를 담당하며 source-level boundary test가 generator와 filesystem 구현이 router로 되돌아오는 것을 막습니다.

2026-09-01 실제 파일 검증에서는 13개 page 전체 검색으로 `[FR-026] 본연체` Section (`4217:7743`)을 찾고, 그 안의 360×740 화면 10개를 시각 순서대로 인덱싱해 `A : STORY-F-PROOFREAD` (`3879:35518`)를 정확한 대상으로 선택했습니다. Section 전체 fast envelope는 8 MiB 안전 상한을 넘어서므로 성공으로 오인하지 않고, 각 화면을 공식 read-only MCP로 개별 수집했습니다. 열 화면은 각각 15~210개 node를 가지며 모든 child, styled text segment, 변수 3~25개와 text style 2~13개의 참조 완전성을 실제 JSON fixture와 DevupUI TSX snapshot으로 검증합니다. 대표 proofread 화면은 공식 read-only MCP 1회, 3개 PNG envelope 청크에서 144개 node, 변수 20개와 text style 11개를 수집했고 폴백은 없었습니다. instance children, concrete boolean property, mixed typography, nested `[1. 이름]`, token binding과 개별 footer stroke도 Rust snapshot/live contract로 검증했습니다. 같은 파일의 전체 theme export는 legacy 공식 read-only 호출 89개를 통해 collection 1개, variable 49개, style 37개, mode 2개를 수집해 42,794자 `devup.json`을 생성했으며 diagnostics는 0개였습니다. 현재 full-theme fast collector는 같은 collection/variable/style 전체를 단일 read-only `use_figma` 호출로 수집하고, envelope 검증 실패 시에만 이 legacy 경로를 0부터 다시 시작하도록 contract test로 고정했습니다.

## Snapshot 의미와 현재 한계

Figma Remote MCP에서는 `JSON_REST_V1` export가 허용되지 않으므로 host object를 그대로 REST JSON으로 만들 수 없습니다. 대신 checked-in property manifest와 runtime prototype/enumerable 탐색을 함께 사용해 모든 발견한 data field의 key와 읽기 결과를 보존합니다. 함수, 순환 node object는 제외하거나 id로 바꾸고, binary asset은 bytes 대신 metadata로 나타내며, 타 plugin private data와 오류를 내는 getter는 읽을 수 없습니다. 단일 값이 byte budget을 넘으면 key를 없애지 않고 `{ "$truncated": ..., "byteLength": ... }`와 `DEVUP_FIELD_VALUE_TRUNCATED`를 남기며, `characters`, styled segment, resource binding처럼 UI 변환에 필요한 값은 우선 보존합니다.

현재 private MVP의 남은 한계는 다음과 같습니다.

- 공식 `get_metadata`의 file-level page 목록은 실제 page 전체보다 적게 반환될 수 있습니다. 이름 검색은 Plugin API page catalog와 per-page projection으로 우회하며 실제 13개 page 파일에서 검증했습니다.
- 매우 큰 computed field(예: vector `fillGeometry`)는 현재 값 전체 대신 명시적인 byte-length marker로 보존됩니다. 모든 대용량 field 값을 lossless하게 export하는 기능은 후속 wire-format 개선 대상입니다.
- exact-node fast envelope가 8 MiB 안전 상한을 넘거나 공식 MCP가 image transport를 바꾸면 자동 legacy fallback이 여러 cursor call을 사용하므로 subtree 크기에 따라 시간이 늘어날 수 있습니다.
- direct OAuth registration은 Figma MCP Catalog 승인이 없는 private client에서 거절됩니다. `auto`/`host` fallback은 host가 인증한 공식 Figma MCP로 실제 검증했습니다.
- 사용되지 않은 외부 Figma library 변수 전체는 Remote MCP가 제공하지 않을 수 있습니다.
- node/page theme scope는 로컬 변수 API의 file-wide 결과를 기반으로 하며 세밀한 사용 범위 필터는 후속 보강 대상입니다.
- vector, mask, image, absolute layout과 일부 effect는 diagnostics를 포함한 제한적 fallback입니다.
- Figma Remote MCP의 `use_figma` tool contract가 바뀌면 live smoke test와 adapter 갱신이 필요합니다.

상세 설계는 [`docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md`](docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md)를 참고하세요.
