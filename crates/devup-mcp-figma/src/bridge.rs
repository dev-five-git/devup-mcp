//! 열려 있는 Figma 파일을 우리 플러그인으로 직접 읽는 전송 계층.
//!
//! 공식 MCP 경로(`RemoteFigmaClient`)는 읽기 스크립트를 `use_figma` 로 보내는데,
//! 그 도구가 요금 한도를 쓴다. 스크립트가 건드리는 것은 문서화된 Plugin API 뿐이라
//! 우리가 돌리는 플러그인이 같은 일을 할 수 있고, 그 경로에는 한도가 없다.
//!
//! 방향이 거꾸로인 점이 설계를 결정한다. Figma 플러그인은 들어오는 연결을 받지
//! 못하고 나가는 WebSocket 만 열 수 있어서, **서버는 이쪽**이고 플러그인이 붙는다.
//!
//! 한 기기에 devup-mcp 가 여럿 떠 있는 것은 정상이다 — MCP 클라이언트마다, 세션마다
//! 하나씩 뜬다. 플러그인이 붙을 수 있는 포트는 manifest 에 적힌 하나뿐이라, 그 포트를
//! 잡은 프로세스(호스트)만 플러그인을 받는다. 잡지 못한 프로세스는 호스트의 `/relay`
//! 에 붙어 그를 통해 읽고, 호스트가 떠나면 그중 하나가 포트를 이어받는다. 어떻게,
//! 왜 그렇게 하는지는 [`relay`] 에 있다.
//!
//! 응답은 원격이 주는 모양 그대로 감싼다. 디코더들은 `content[].text` 안의 JSON
//! 을 찾도록 쓰여 있으므로, 여기서 모양을 바꾸면 두 경로가 조용히 갈라진다. 중계를
//! 거친 답도 이 모양이다 — 감싸는 일은 요청한 프로세스가 한다.

// Source sections share this module to preserve privacy and test names.
include!("bridge/transport_and_state.rs");
include!("bridge/job_budgets.rs");
include!("bridge/stamp_file_key.rs");
include!("bridge/fallback_reasons.rs");
include!("bridge/texts.rs");
