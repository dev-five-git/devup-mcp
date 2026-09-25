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
pub const INSTRUCTIONS: &str = concat!(
    "Build identity: identify deployments by server.commit/buildId, not version alone; ",
    "server.displayVersion combines version and buildId. Reconnect the MCP server if the expected build differs. ",
    "server.updateAvailable reports whether a newer release exists; state \"unknown\" means the check is disabled, ",
    "not yet run, or could not reach the network, and it never blocks a call.\n",
    "1. devup-mcp is the primary source for turning a Figma design into code. Do not replace it with another source.\n",
    "1a. Connection order: bridge -> direct. Call devup_figma_auth status before asking anyone to log in: the Devup ",
    "Bridge plugin needs no login, and with one attached devup_figma_export takes no url.\n",
    "2. When the goal is implementation, call devup_figma_export first and take tsx. That is the deliverable; ",
    "a complete response marks it with deliverable.isFinal.\n",
    "2b. The tsx is devup-ui code. Call devup_skills before writing it: it reports the conventions this ",
    "workspace lacks and installs the ones it carries.\n",
    "3. Request only the outputs you will read, and read the rest of the rules before your second call: ",
    "resources/read \"devup://guide/usage\" carries output sizing, verification boundaries, SECTION batching, ",
    "asset placement and delivery. It is a resource so that a caller who never touches Figma never pays for it."
);

/// The rules that moved out of `instructions`, verbatim in meaning. This is a
/// relocation, not a rewrite: every numbered rule that was published before is
/// published here under the same number.
pub const GUIDE: &str = r#"# Using devup-mcp

These are the rules that used to be pushed to every client in `initialize`.
They are published here instead so a caller that never converts a Figma design
never carries them. Rule numbers match the original list, which skipped 9.

## 1a. Connection order: bridge -> direct

Two paths reach Figma and they are not equals. The Devup Bridge plugin, run in
the Figma desktop app on the file, needs no login and spends none of Figma's
allowance. The direct path is OAuth against Figma's MCP and is metered. Use the
bridge first; direct is the fallback for what the bridge cannot serve - a
file-scope metadata read and `referencePng`.

- Call `devup_figma_auth` with `action: "status"` before asking anyone to log
  in. It reports both paths: `connected`, `activePath` (`bridge`, `direct` or
  null), `paths.bridge.attachedFiles` with each plugin's file name, current
  page and selection, and a `nextAction`. `status` reads `disconnected` only
  when neither path can serve. Do not ask for a login while `activePath` is
  `bridge`.
- With exactly one plugin attached, `devup_figma_export`,
  `devup_figma_search` and `devup_figma_explore` take no `url`. The export
  reads the file that plugin has open and the node selected in Figma;
  `frameIds` without `url` name frames in that file - one id is that frame,
  several are screens of the selected Section. Explore anchors on the
  selection, and search reads the whole file.
- When a link is still needed, use `figma-bridge://current`, with
  `?node-id=1-2` to name a node. Never invent a Figma file key to fill `url`:
  a plugin that cannot report its key answers any key while it is alone, so an
  invented one is served and then mistaken for the file's.
- With no plugin attached, or several, a call without `url` is refused with a
  `nextAction` that lists the ways forward: run the plugin, log in and pass a
  link, or pick one of the attached files.
- An answer the plugin served says so: `source.kind` is `bridge`, with
  `bridgePort`, `fileName`, `pageName` and `bridgeReads`. `source.fileKey` is
  the key the plugin reported, or `null` with `fileKeyReason` when it could
  not report one.

## 2a. Ask for an output only when you will read it

Measured on the Korean WQUW-120 modal, semantic sourceMap is about 5.4x TSX
(48,832 versus 9,099 UTF-8 bytes; 12.6% smaller than its former offset map).
Sizes vary by screen; earlier rawPayload/rawSnapshot measurements were about
7x/2x TSX, so requesting them by default spends most of the response on bytes
nothing reads.

sourceMap records nodeId, original property, generatedProperty and resolution,
plus optional variableId (variable token), styleId (style token) and assetId
(asset reference), with no character/byte offsets.

Resolution labels carry different weight. `raw-fallback` means a raw-value
mapping, not necessarily an inaccurate value. `verified-explicit-dimension`
checks that emitted pixels equal the source dimension, while
`verified-layout-sizing` checks sizing intent against emitted CSS. `exact`
verifies that field-to-property mapping, not rendered pixel equivalence.

Use the `generatedSource` diagnostics for node code excerpts. `rawSnapshot` and
`rawPayload` are for banking a capture as an offline fixture, and both require
`debug: true`.

`componentTsx` is the same screen with instances left as `<Name />` references,
and `responsiveTsx` appears on its own whenever the capture carries more than
one width.

## 3. Screenshots and visual reasoning verify; they do not author

`get_design_context`, screenshots and visual reasoning are verification aids
only. Do not overwrite devup-mcp output with what a picture looked like.

## 4. Do not hand-interpret the design

Do not read a node tree yourself to write devup-ui code, and do not infer
layout from coordinates.

## 5. A failed call is a fact to report

If a devup-mcp call fails, record it explicitly. Do not silently route around
it.

## 6. Never guess a UI value

Do not guess color, spacing, radius or typography. If you could not obtain a
value, stop and report that.

## 7. A SECTION link is two steps, not one subtree

Do not implement a Section link as one whole subtree. Read the
`selection_required` candidates and continue with bounded per-screen `frameIds`
batches from `nextAction`. `allScreens` is valid only when the complete list
fits the advertised frame and output budget.

## 8. The generated component name is a starting point

The name comes from the Figma layer name and is not a contract. Rename it to
fit the codebase, and rename any name that is meaningless or not a valid
identifier.

## 10. An asset path in the output is a placeholder

A `maskImage` or `Image` src is built from the layer name. Rename the file to
fit the project. If the asset varies per usage, lift it into a prop instead of
hardcoding it.

## 11. A referenced asset must actually exist

A fixed asset such as an icon must be exported, never referenced by a path that
does not exist yet. Read `assetManifest` for the asset IDs, then call
`devup_figma_export` again with `assetRequests`, giving each entry an
`outputPath` under an allowed write root, and make the path in the code match
the path you wrote.

## 12. Prefer resource delivery for assets and large outputs

With `delivery: "resource"`, devup-mcp returns `devup://artifact/...` resource
links to read on demand instead of inlining bytes into every response.
"#;

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
    #[test]
    fn instructions_are_far_smaller_than_the_guidance_they_point_at() {
        assert!(
            INSTRUCTIONS.len() < 1_400,
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
