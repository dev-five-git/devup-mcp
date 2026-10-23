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
3. Launch Figma, foreground its window and open the supplied file URL in the
   desktop app. With computer use, click the target file tab, navigate to the
   target page and select the intended frame/Section. Don't guess a file key.
   Let the user handle any sign-in, OTP or permission prompt you can't complete.
4. If Devup Bridge isn't registered, locate the actual downloaded repository's
   `plugin/manifest.json` and `plugin/dist/` bundle. In Figma choose
   **Plugins > Development > Import plugin from manifest...**, then that
   manifest. Don't invent a local path or rebuild an unchanged shipped bundle.
   Run **Plugins > Development > Devup Bridge** in the target file and leave
   its window open.
5. If `attachedFiles[].inView` is false, use computer use to restore and
   foreground Figma, then click that file's tab. Keep it visible during reads.
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
