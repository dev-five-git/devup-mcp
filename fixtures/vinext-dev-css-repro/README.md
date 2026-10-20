# Cold dev CSS / SSR reload reproduction

This hand-written two-route app contains no Prime source and does not invoke
devup-mcp. A shared client header makes the base extracted stylesheet part of
the SSR graph. The second route adds newly extracted styles and a client form.
Tested on Windows with Node 26.8.2, Vite 8.2.2, vinext 1.0.1 and
@devup-ui/vite-plugin 1.0.67. Top-level dependencies are pinned; the experiment
used Prime's existing installed dependency graph through read-only junctions.

Copy this fixture to a fresh temporary directory, install its dependencies,
and start it there, capturing both stdout and stderr. For example in PowerShell:

```powershell
npm install
npm run dev *> server.log
```

In a second terminal, from the devup-mcp checkout:

```powershell
npm --prefix harness/runtime-audit ci --omit=dev
node harness/runtime-audit/run.mjs --plan fixtures/vinext-dev-css-repro/plan.json --out <temp-copy>/audit --server-log <temp-copy>/server.log
```

The fixture plan is an executor diagnostic, not a server-issued plan for ingest.
If Chromium is unavailable, install it through the harness's Playwright CLI.
The first `/seo-geo/` after `/about/` can return HTTP 500, while warm visits
work at every width. Look for generated CSS reloads and `(ssr) program reload`
immediately before the 500. With `--server-log`, the affected checks become
inconclusive environmental measurements instead of page failures.

As a diagnostic control, stop the server and start a new process with
`$env:IGNORE_EXTRACTED_CSS='1'` before `npm run dev`. The same cold sequence
then returns 200 in the observed run. This suppresses CSS hot updates and must
not be applied as a production workaround. A fixture without the shared client
header did not reproduce the failure. An additional `@devup-ui/react` optimizer
exclusion is unnecessary to reproduce it.
