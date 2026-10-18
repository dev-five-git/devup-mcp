//! Self-diagnosis for the "devup-mcp will not reach Figma" failure mode.
//!
//! Two paths reach Figma and they are not equals. The **bridge** reads through
//! a plugin in the Figma desktop app: no login, and none of the allowance
//! Figma meters. The **direct** path is remote OAuth (see `oauth.rs`) and is
//! metered, which a single screen's several reads exhaust quickly. So the
//! bridge is the path to reach for and direct is the fallback.
//!
//! This module reported only `direct`, which made it an instrument that could
//! give exactly one answer — run `devup_figma_auth login` — to every question
//! about the connection, including the ones whose real answer was "run the
//! plugin" or "the listener never bound". Naming one path made it the only
//! path, and the metered one at that. [`bridge_path`] is the other half.
//!
//! - [`connection_report`] backs `devup_figma_auth`'s `status` (and the
//!   answers to `login` and `logout`): whether each path is usable right now,
//!   which one a read would take, what the attached plugins have open, and
//!   the next call to make.
//! - [`doctor_report`] backs `{"action":"doctor"}`: the same report plus
//!   client-specific setup data for the constraints that were verified by
//!   hand (client_name allowlist, redirect_uri shape, the silent callback port
//!   collision, PAT rejection).
//!
//! All facts embedded here (allowlist behavior, redirect_uri constraints,
//! the callback-port trap) were measured against the real Figma Remote MCP
//! registration endpoint; see `README.md`'s "Figma 연결 설정" section for
//! the same data in prose form. `doctor_report` makes no network call at
//! all, so it stays cheap enough to call on every diagnosis.
//!
//! The Figma desktop app's local Dev Mode MCP was reported here as a third
//! path, probed for and described as usable without OAuth. It is not one:
//! it serves six read tools and `use_figma` is not among them, so every
//! collection devup-mcp performs — snapshot, explore, section index, theme —
//! has no tool to run. Its tools also take only a node id, addressing
//! whatever the desktop app currently has open rather than a file key.
//! Naming it as a path sent agents to a dead end, so it is named nowhere.

// Source sections share this module to preserve privacy and test names.
include!("diagnostics/response_diagnostics.rs");
include!("diagnostics/tests.rs");
