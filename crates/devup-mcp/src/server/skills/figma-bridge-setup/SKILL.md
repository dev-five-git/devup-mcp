---
name: figma-bridge-setup
description: Use when Devup Bridge is disconnected, Figma is out of view, the relay host has a different build, or the desktop app or plugin needs setup.
---

# Prepare Devup Bridge

The agent performs local setup, not the user. Load the runtime's computer-use
skill before driving Figma. Use shell commands for installation checks and
process inspection; use computer use for desktop menus, tabs and file dialogs.
If computer use is unavailable, report the exact blocked step. Don't claim a
connection from a running process alone.

## Concurrent projects and split-screen desktop

Another running project is not a desktop-wide lock. Use OS split-screen with
separate visible Figma/browser windows; keep other projects, servers and plugins
running. Give each project its own workspace, port and browser context. Verify
the target window identity/bounds and file/page before each input or capture;
coordinate only conflicting mouse/keyboard input in short batches.

For several attached Bridge files, supply the intended file URL/identity on
reads and verify its page/selection and a small node read. Do not rely on the
single-plugin selection shortcut. A real tool lock, permission error or ambiguous
file identity blocks that operation only: report the exact error, respect the
restriction and continue independent work. Never infer a global project lock
or stop another project merely because its window is already in use.

Keep browser visual captures at their agreed CSS viewport, DPR and zoom. Use
an independent automated context when the reference viewport cannot fit in a
desktop pane; do not alter layout or zoom to force a comparison.

1. Call `devup_figma_auth` with `action: "status"`. Read `paths.bridge`, its
   host PID and commit/buildId, and `server.commit`/`server.buildId`. A matching
   version alone doesn't prove a matching build. Missing host identity is
   unknown, not current. A different commit is a mismatch, not proof of age.
2. Check for Figma: Windows `Test-Path "$env:LOCALAPPDATA\Figma"`; macOS
   `test -d /Applications/Figma.app` (also check `~/Applications/Figma.app`).
   If absent, ask with the runtime question tool: "Install desktop Bridge
   (Recommended)" or "Use metered direct OAuth". Install only after approval:
   Windows `winget install --id Figma.Figma --exact`, macOS
   `brew install --cask figma`, or https://www.figma.com/downloads/.
   Direct means `devup_figma_auth` login and the real Figma URL. If no question
   tool exists, leave the choice unresolved and report the limitation.
3. Launch Figma and open the supplied file URL in the desktop app. If another
   project uses Figma, open a separate window and tile them with OS split-screen.
   Focus only the intended window for its input batch. Click its file tab,
   navigate to the target page and select the intended frame/Section. Don't guess a file key.
   Let the user handle any sign-in, OTP or permission prompt you can't complete.
4. If Devup Bridge isn't registered, locate the actual downloaded repository's
   `plugin/manifest.json` and `plugin/dist/` bundle. In Figma choose
   **Plugins > Development > Import plugin from manifest...**, then that
   manifest. Don't invent a local path or rebuild an unchanged shipped bundle.
   Run **Plugins > Development > Devup Bridge** in the target file and leave
   its window open.
5. If `attachedFiles[].inView` is false, restore the intended Figma window in
   its split-screen pane, then click that file's tab. Keep it visible during
   reads without hiding another project's window.
   Call status again and verify the file, current page, selection and `inView`.
6. For a mismatched/legacy relay host, inspect the reported PID, executable
   path and owning client before changing anything. Restart/update that client's
   MCP connection to release the old host; don't kill every devup-mcp process,
   the current session, or an unrelated port owner. Ask through the question
   tool before interrupting another session. Once approved, stop only the
   verified old host, allow a current process to take the port, and rerun the
   plugin if needed. Recheck status within 30 seconds, then report a timeout
   with host identity and the remaining step rather than waiting forever.
7. Verify status reports `activePath: "bridge"` and the intended attached
   file/page/selection with the expected host build. Run a small read of a known
   node with `devup_figma_measure` to confirm it can serve reads. With one plugin
   attached, no URL is needed. Record the observed identities and result.

Status fields may differ on older hosts. This procedure doesn't assume a
`hostStale` field or an `ensure` tool exists. Missing diagnostics require manual
inspection, not an invented tool call. Never switch to metered OAuth silently.

## Stale Plugin Build

Check `paths.bridge.pluginStale` and each attached file's `pluginBuild`:
`expected` and `actual` are script SHA-256 identities; `expectedProtocol` and
`actualProtocol` identify the plugin wire contract. `state: stale-unknown`
means an older plugin did not supply identity, not that it is current. Matching
pluginVersion alone is insufficient. `DEVUP_BRIDGE_PLUGIN_STALE` on an export
reports the identity of the plugin that served that acquisition, even on reuse.

A plugin that sends `liveScripts: true` in its hello and speaks the same
protocol is sent this server's scripts over the bridge socket
(`scripts-update`) and reports the new hash on its own. Call status again
after a second; if it now reads `current`, nothing else is needed. The steps
below are for a plugin built before live scripts, a protocol mismatch, or a
plugin whose update failed.

1. Locate the real repository supplying this server and plugin. In `plugin/`,
   run `npm ci`, then `npm run build`. Record the printed scriptsHash.
2. Load computer-use and foreground Figma desktop and the intended file tab.
   If necessary choose **Plugins > Development > Import plugin from manifest...**
   and select that repository's `plugin/manifest.json`.
3. Close the running Devup Bridge window and re-run
   **Plugins > Development > Devup Bridge**. Reconnecting the old running window
   does not load rebuilt scripts.
4. Call `devup_figma_auth { action: "status" }`; require the attached plugin's
   actual hash/protocol to match expected, `pluginStale: false`, and the intended
   file/page/selection. Verify a small known-node read. If computer use is
   unavailable, report the exact GUI step blocked rather than claiming reload.

## Broken Design Component Sets

`DEVUP_DESIGN_COMPONENT_SET_ERROR` means Figma itself refused component/state
getters because the component set has existing errors or is invalid. It is not
evidence that reactions were omitted or that the plugin is old. Report nodeId,
component set name/id when known, and the original getter reason. Ask the user
to open the set in Figma and fix the warning Figma shows on it. Never auto-edit
the user's design. Readable reactions still contribute interaction candidates;
this design error does not invalidate other nodes' interaction coverage.
