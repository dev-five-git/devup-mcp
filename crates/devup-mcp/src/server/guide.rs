//! The long-form usage guidance, published as an MCP resource rather than
//! pushed to every client in `initialize`.
//!
//! Everything here used to live in the `instructions` string, which every
//! client paid for on every session even when it only ever called
//! `devup_stack_diff` and never touched Figma. The rules did not get shorter,
//! so they moved to a channel the caller pulls: `resources` was already
//! implemented and advertised, and `resources/list` was returning an empty
//! array.
//!
//! Keeping the text compiled into the binary rather than in the README is the
//! point. A README describes whatever build the reader happens to be looking
//! at; this ships with the build that answers the call, so it cannot describe
//! behaviour the running server does not have.

/// The one guide resource. A stable URI so `instructions` can name it.
pub const GUIDE_URI: &str = "devup://guide/usage";
pub const GUIDE_NAME: &str = "devup-usage-guide";
pub const GUIDE_TITLE: &str = "Using devup-mcp";
pub const GUIDE_DESCRIPTION: &str = "Connection order (bridge -> direct), output selection, verification boundaries, SECTION batching and asset rules for devup-mcp";
pub const GUIDE_MIME_TYPE: &str = "text/markdown";

/// What `initialize` still says. Only the rules that change the very next
/// action stay: what this server is for, what to call when implementing, how
/// to read the identity on every response, and where the rest lives.
///
/// The original rule numbering is preserved in [`GUIDE`] - it skipped 9 - so a
/// reader comparing the two can see nothing was dropped in the move.
pub const INSTRUCTIONS: &str = include_str!("text/guide-INSTRUCTIONS-49bd7ec6.md");

/// The rules that moved out of `instructions`, verbatim in meaning. This is a
/// relocation, not a rewrite: every numbered rule that was published before is
/// published here under the same number.
pub const GUIDE: &str = include_str!("text/guide-GUIDE-c682b0c5.md");

#[cfg(test)]
mod tests {
    use super::*;

    /// The relocation must not lose a rule. Every numbered rule the server used
    /// to publish is still published, either in the short instructions or in
    /// the guide, and the guide is reachable because instructions names its URI.
    #[test]
    fn every_original_rule_number_survives_the_move() {
        let combined = format!("{INSTRUCTIONS}\n{GUIDE}");
        for number in [
            "1.", "1a.", "2.", "2a.", "2b.", "3.", "4.", "5.", "6.", "7.", "8.", "10.", "11.",
            "12.",
        ] {
            assert!(
                combined.contains(number),
                "rule {number} disappeared in the move to a resource"
            );
        }
        assert!(
            INSTRUCTIONS.contains(GUIDE_URI),
            "instructions must name the guide URI or the moved rules are unreachable"
        );
    }

    /// The whole point was to stop charging every session for Figma prose, so
    /// the measurement that justified the move is itself a regression test.
    ///
    /// The ceiling moved once, for rule 1a: the connection order has to be
    /// read before the first Figma call, because an agent that met a plugin
    /// attached and a `disconnected` direct path asked its user to log in.
    /// It moved again for rule 2c: the screen plan has to come before the
    /// first page is written, and an agent that has not read the guide yet
    /// is exactly the one that writes pages straight away.
    #[test]
    fn instructions_are_far_smaller_than_the_guidance_they_point_at() {
        super::super::source_limits::check();
        assert!(
            INSTRUCTIONS.len() < 1_500,
            "instructions grew back to {} bytes; the per-session cost is the thing being fixed",
            INSTRUCTIONS.len()
        );
        assert!(
            GUIDE.len() > INSTRUCTIONS.len() * 2,
            "the guide should hold the bulk of the text"
        );
    }

    /// Identity guidance is about reading every response, so it stays in the
    /// always-loaded string rather than moving behind a fetch.
    #[test]
    fn instructions_keep_identity_and_the_deliverable_rule() {
        assert!(INSTRUCTIONS.contains("server.commit/buildId"));
        assert!(INSTRUCTIONS.contains("server.updateAvailable"));
        assert!(INSTRUCTIONS.contains("devup_figma_export"));
        assert!(INSTRUCTIONS.contains("deliverable.isFinal"));
    }
}
