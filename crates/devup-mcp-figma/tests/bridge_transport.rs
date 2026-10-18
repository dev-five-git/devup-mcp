//! 브리지 전송 계층을 진짜 소켓으로 통과시킨다.
//!
//! Figma 앱 자체는 여기서 띄울 수 없으므로 플러그인 자리에 WebSocket 클라이언트를
//! 세운다. 그 너머(스크립트 실행)는 플러그인 빌드가 책임지고, 여기서는 그 앞의
//! 모든 것 — 등록, 이름·값 매핑, 상관, 봉투 모양, 라우팅 판정 — 을 확인한다.

// Source sections share this module to preserve privacy and test names.
include!("bridge_transport/use_declaration.rs");
include!("bridge_transport/impl.rs");
