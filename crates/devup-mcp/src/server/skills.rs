//! The skills an agent needs for the code devup-mcp emits, and whether they
//! are installed.
//!
//! devup-mcp hands back devup-ui TSX. On a machine that has devup-mcp and
//! nothing else, the agent receiving that TSX has never seen devup-ui: it does
//! not know that `@devup-ui/react` components are compile-time placeholders,
//! that `$token` refers to `devup.json`, or that a style prop takes a
//! responsive array. It guesses, and the guesses are wrong in ways this server
//! can neither see nor correct.
//!
//! The fix is not to paste the rules into a response. Every agent runtime
//! already has a skill loader that reads `SKILL.md` files from a directory and
//! surfaces them by their own triggers, at the moment they apply. A blob
//! returned once is read once; an installed skill keeps working for the rest of
//! the session and every session after it. So this module's job is to report
//! **whether each skill is installed** and hand over the one action that
//! installs it - a concrete gap the agent can close, rather than advice.
//!
//! ## Three origins, and why they are handled differently
//!
//! `embedded` skills are DevFive's own but live in another repository -
//! devup-ui, vespera, vespertide. Their canonical `SKILL.md` is vendored into
//! the binary as an offline fallback; install prefers current upstream documents.
//! The fallback matters because the
//! situation this exists for, a bare machine, is the one in which a download is
//! least likely to work. A vendored copy can fall behind its repository, so
//! each one pins the commit it copied and `scripts/refresh-skills.mjs` is how
//! that copy is moved forward.
//!
//! `own` skills are written here, in this repository - devfive-frontend,
//! devup-implementation-plan and devup. There
//! is no upstream to vendor from and nothing for the refresh script to compare
//! against, so they pin no commit; this repository's own history is their
//! provenance. They are otherwise installed exactly like an embedded skill,
//! which is the point of giving them an origin rather than a special case.
//!
//! `external` skills belong to someone else. vercel-labs/agent-skills publishes
//! **no LICENSE file**, so its content is all-rights-reserved and devup-mcp
//! neither vendors nor redistributes it. They are also multi-file - one of them
//! is a `SKILL.md` plus an `AGENTS.md` and some seventy rule files - so copying
//! them was never the right shape anyway. devup-mcp reports where they come
//! from and the command their publisher provides, and the agent runs it.
//!
//! devup-mcp never executes that command. A design-to-code server that shells
//! out to a package installer is both out of character and a way to turn one
//! compromised registry entry into arbitrary execution on every machine that
//! ever exported a screen.

// Source sections share this module to preserve privacy and test names.
include!("skills/catalog.rs");
include!("skills/installation.rs");
include!("skills/tests.rs");
