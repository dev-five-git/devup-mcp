# Hard-break whitespace: plugin golden review

The candidate preserves spaces adjacent to explicit line breaks only for
`textAutoResize=HEIGHT` (fixed inline width), without a positive `maxLines`,
lists or tabs. HUG, fixed-height, clamp and unrelated whitespace cases retain
their existing policy. Korean `wordBreak="keep-all"` is unchanged.

The complete 268-case parity run identifies exactly four changed snapshots.
Review against each collected fixture confirms six attribute additions and
no other output changes. Every addition is `whiteSpace="pre-wrap"` on a TEXT
node with the stated source condition; no node ID participates in production.

| Golden | Reviewed source nodes | Reason |
| --- | --- | --- |
| `upstream-codegen-165-501ee1df12` | `1:18` | Spaces before two explicit newlines in the service-extension paragraph; HEIGHT text. |
| `upstream-codegen-191-2021d398f8` | `107:32` | Space before the explicit newline in the two-line invitation; HEIGHT text. |
| `upstream-codegen-252-49e979239e` | `284:18751`, `284:18753`, `284:18754` | Korean heading, Korean notice and English notice each contain spaces before explicit newlines; all HEIGHT text. |
| `upstream-codegen-253-347a2e5fb2` | `213:7494` | Space before the explicit newline in the quoted heading; HEIGHT text. |

The candidate snapshot bodies and complete unified diffs are retained under
`harness/render/out/w22-corpus-review/` and
`harness/render/out/w22-corpus-review.json`. At review time the four tracked
snapshots and manifest were still untouched. Only these four checksums may
be updated under the owner's explicit approval; the corpus consistency test remains intact.

## Approval rationale

The owner approved exactly these four goldens, six `whiteSpace="pre-wrap"`
additions and their four manifest checksums. The plugin drops a source space
before an explicit line break because CSS collapses it. Preserving that space
makes painted text more faithful to the design's `characters`; the golden
changes correct the plugin's output rather than accepting a pixel gain alone.

This continues W8's existing rule that painted text must equal `characters`.
W8 removed JSX-inserted spaces absent from the design and prevented design
line breaks from becoming spaces, while reporting collapse "that no emission
can avoid". This bounded emission now avoids that collapse, so eligible cases
move from reported to fixed. The diagnostic remains for unsupported cases.

Korean `keep-all` is the opposite decision: matching Figma there would produce
worse Korean, so the owner retained the compensation and rejected the pixel
gain. Here preservation improves correctness. The superficially similar
golden changes therefore have different answers, and Korean `keep-all`
remains unchanged. HEIGHT sizing without positive `maxLines`, lists or tabs
is still required; HUG, fixed-height and clamp cases retain existing policy.

Independent browser probes measured the supported condition against the fresh
base at `da04103`: about mobile 5.514426% to 5.431783%, landing mobile
4.987310% to 4.783042%, about tablet 3.012156% to 2.933757%, about desktop
1.773154% to 1.768271%, and notice mobile 3.843164% to 3.736854%.
Wider landing/notice stay unchanged and popup has no eligible HEIGHT text.
These are localisation probes; the final report separately records freshly
acquired generator measurements and full gates.
