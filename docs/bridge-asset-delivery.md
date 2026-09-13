# Native bridge asset delivery

The native bridge runs the compiled `assets.js` against Figma's standard
Plugin API, which has `base64Encode` but no `figma.io`. The export script used
the remote MCP's `figma.io.write` extension unconditionally on its attachment
path. A successful `node.exportAsync` was therefore followed by an exception
and returned as `DEVUP_ASSET_EXPORT_FAILED`. Small SVG exports also failed at
this write even though their text was already available for inline delivery.

This is a proven explanation for failures **after a successful export** in
the bridge PR's ["Still broken" lead](https://github.com/dev-five-git/devup-mcp/pull/50).
It is not a reproduced explanation of the reported approximately 11-second
latency: no live bridge timing is claimed. Unlike the variable defect, the
asset script performs one node lookup and one export, not many individual
variable lookups. PNGs/SVGs already large enough to take the pre-existing
fragment path returned before the missing writer and had a different path.

## Correction

`assets.js` checks the writer capability after producing and validating its
bounded bytes. With a remote file writer, attachment delivery and its existing
PNG/SVG fragment thresholds remain unchanged. Without one, the script returns
the requested MIME and base64 bytes directly in the bridge's JSON response;
SVG remains text. The native bridge can carry these bounded bytes in one
response, so it no longer needs to re-export a large node for every fragment.

The existing 8 MiB export limit, empty-result rejection, export failures,
identity checks, MIME/byte-length/SHA-256 validation and Rust delivery remain
in place. The largest base64 response is approximately 10.67 MiB before its
small JSON wrapper. This does not change what pixels are exported or how
codegen composes them, and introduces no new provenance/resolution claim.

The runtime change is confined to the export script and rebuilt
`plugin/dist/code.js`, with a new integration test and required changepack.
No shared codegen file, forbidden file, corpus golden,
manifest checksum, Korean wrapping rule or threshold changes.

## Test-first evidence

The new `asset_script_transport` integration test executes the actual compiled
script in Node with the native API surface and passes its bridge-shaped JSON
through the real Rust asset decoder. Before implementation:

```text
native_plugin_exports_without_a_remote_file_writer ... FAILED
remote_asset_delivery_retains_attachment_and_fragment_limits ... ok
bridge_does_not_hide_export_failure_or_bypass_byte_limit ... ok
test result: FAILED. 2 passed; 1 failed
```

The failing assertion recorded `Png/7`, `exports:1`, `writes:0`,
`status:"failed"`, `DEVUP_ASSET_EXPORT_FAILED` where `Exported` was required.
After implementation all three tests pass. They exercise small and 800,000-byte
PNG, JPG, PDF, small and 13,000-byte SVG, byte-for-byte decoding, MIME identity,
one export per result, remote writer/fragment behavior, renderer rejection,
zero bytes and the unchanged 8 MiB ceiling. Existing decoder tests retain hash
and length rejection coverage.

The generated plugin's `assets` function was also executed without a writer:
bytes `[1,2,3]` arrived as `AQID`, `image/png`, length 3 and SHA-256
`039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81`.
`npm install` and `npm run build` regenerated the committed bundle from the
same source used by Rust. This is an API-surface/decoder integration test,
not a live native-Figma export or a latency measurement.

## Pixel scope

[The isolated-fill investigation](isolated-fill-export-contract.md) separately
defines the missing representation, validates the existing crops and proves
the hero composite includes other paints and children. Authentication prevented
obtaining original/isolated pixels through the available Figma connector, so
there is no hero pixel fix in this transport correction.

Both own baseline render runs measure 5.4317833026 / 2.9337565805 /
1.7682713195 percent for about mobile/tablet/desktop. Reacquisition uses the
actual candidate binary identity, while the captured image bytes stay the same;
that is suitable for verifying unchanged generation but is not evidence of
live bridge delivery. Final repeated measurements and gate outcomes are recorded
in [isolated-fill-export-evidence.json](isolated-fill-export-evidence.json).

The final own candidate binary is SHA-256
`c3a5855d2fd50dc07ba712662378ff932c9d0657c9d20a4337d405579ef7fe60`.
After rebuilding, all three about screens were reacquired with that exact
binary, and two valid render runs repeated the baseline metrics exactly.
Every candidate actual PNG is **byte-identical** to the own baseline; all
acquisition input hashes and rendered dimensions also agree. The measured
delta is **0 pixels / 0pp** on each screen. No threshold is adjusted.

## Full gate

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1,115 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | 1,115 passed, 0 failed, 2 ignored; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | 2 passed, 0 failed |
| Plugin build, repeated | Pass; byte-identical `dist/code.js` |

All 268 plugin goldens and their manifest remain unchanged; corpus consistency
and `korean_characters_keep_words_whole` pass. The repeated plugin bundle hash
is `b1b59a59e6a1a3b2161b307d96a61f767da8c825210681e1b38b1598fdbd77b6`.
The native API surface was checked against the installed official
`@figma/plugin-typings` version 1.138.0.

All Cargo commands used this worktree's own target, two jobs,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and
`CARGO_INCREMENTAL=0`; `CARGO_TARGET_DIR` was never set. Ordinary MSVC builds
retain the pre-existing localized library-creation linker messages. Clippy's
required command reports zero warnings. Raw logs are under the local ignored
`harness/render/out/w24-*` paths, with their hashes in the evidence JSON.
