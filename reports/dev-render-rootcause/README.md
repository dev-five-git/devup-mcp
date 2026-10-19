# Dev render root cause — 2026-10-20

The reproducible HTTP 500 is a **dev CSS extraction / SSR reload integration
race** between @devup-ui/vite-plugin and vinext's Vite RSC/SSR graph. Newly
visited route styles write `df/devup-ui/devup-ui.css` and per-file CSS while a
shared client component has those styles in the SSR graph. Vite consequently
reloads the SSR program during the request; vinext returns its error document.
This reproduces in the checked-in hand-written two-route fixture without
devup-mcp, Prime code, next-intl, devup-api, or React Compiler.

The separately reported **200 response with missing mobile styles** and the
**grey band / narrow document on `/about`** were not reproduced. Do not treat
this investigation as proof that those two visual symptoms have the same
cause. Warm `/seo-geo` visits had the expected responsive title geometry.

## Scope and environment

Prime in margate was read only. Experiments used
`C:/Users/devfivedevleader/AppData/Local/Temp/devup-rootcause-B-20261020`;
the reduced fixture ran in `.../Temp/devup-minimal-B`. Each copy owned its
source, generated `df`, Vite caches and output logs. Installed dependency
directories were reused through junctions; the top-level node_modules/cache
directory belonged to each copy. Environment files, dist and Next caches were
not copied. No Prime file was edited or committed, and nothing was pushed.

Observed versions: Windows, Node 26.8.2, Chromium via Playwright 1.63.0,
Vite 8.2.2, vinext 1.0.1, @vitejs/plugin-rsc 0.5.34,
@devup-ui/react 1.0.40, @devup-ui/vite-plugin 1.0.67, React 19.2.8.
The minimal fixture's top-level dependencies are pinned. Clean dependency
installation on another OS was not tested; local experiments used the existing
installed dependency graph, without changing it.

## Reproduction and controls

Run the instructions in [the fixture](../../fixtures/vinext-dev-css-repro/README.md).
Start a fresh server process, visit `/about`, then `/seo-geo`, in that order
at 1440, 1100, 858, 600 and 390 CSS px, using a fresh context per width.
Use the runtime-audit executor's normal load/readiness behavior; do not add
settling delays before the first route change.

The minimal server log records:

```text
GET /about/ 200
[vite] (ssr) page reload df/devup-ui/devup-ui.css
[vite] (ssr) program reload
GET /seo-geo/ 500
GET /about/ 200
GET /seo-geo/ 200
```

| Experiment | First about | First SEO | Later requested widths |
|---|---|---|---|
| Prime copy, current config, normal harness | 200 | 500 | 200 |
| Prime copy, cold fast navigation, exclusion retained | 200 | 500 | 200 |
| Prime copy, cold fast navigation, exclusion removed, 390 first | 200 | 500 at 390 | 200 |
| Prime copy, network-idle + 300ms settling between visits, either exclusion setting | 200 | 200 | 200 |
| Prime copy, generated CSS watcher events ignored | 200 | 200 | 200 |
| Minimal fixture, shared client header omitted | 200 | 200 | 200 |
| Minimal fixture, shared client header present | 200 | 500 | 200 |
| Minimal fixture, shared header + CSS watcher ignored | 200 | 200 | 200 |

Suppressing CSS watcher events was an experimental control only. It disables
CSS hot updates and is not a proposed fix. Both the failing and passing minimal
experiments used a new process/cache. The shared-header and watcher controls
make the CSS/SSR graph relationship reproducible rather than merely correlated
with an optimizer warning.

At 390, successful Prime SEO measurements had a 350px title box at x=20,
font-size 36px, and height 46.796875px; at 1440 the title box was 980px wide
at x=230, font-size 48px, height 62.390625px. Intermediate 858 and 600 widths
also used 36px titles. The measured document width matched each viewport.
The first cold failing SEO response was the vinext error shell with only its
error stylesheet, not an unstyled successful route.

## Attribution

* **(a) Devup extraction race:** confirmed at the integration boundary. The
  plugin's installed `dist/index.mjs` transform calls `codeExtract`, writes
  the shared CSS when `updatedBaseStyle` is true, and writes a per-file CSS
  marker for extracted modules. Vite's SSR watcher reload is triggered by those
  files. This is not evidence that extraction emits incorrect mobile rules.
* **(b) Optimizer reload:** not observed as the trigger of this 500. Neither
  `new dependencies optimized, reloading` nor
  `optimized dependencies changed. reloading` accompanies the minimal failure.
  They are distinct environmental hazards now recognized by the harness.
* **(c) RSC/client duplication:** removing `@devup-ui/react` from the optimizer
  exclusion really does produce `rsc:use-client` warnings. Named modules are
  `@devup-ui/react/dist/hooks/use-theme.js`, `utils/get-theme.js`,
  `utils/init-theme.js` and `utils/set-theme.js`, with an optimizer `?v=` suffix.
  But the 500 reproduces with the exclusion retained, so those warnings do not
  establish its cause. No thrown duplication exception was captured.
* **(d) devup-mcp output:** excluded as a necessary cause by the hand-written
  fixture. There is no generated-code/completeness-v2 correction supported by
  this experiment. The two shipped skills now clarify that the reset-css setup
  rule is scoped, and distinguish optimizer warnings from CSS-triggered SSR
  reloads.

For error-path diagnosis only, a private copy of vinext was instrumented at
`app-ssr-entry.js` `onError` and `app-page-stream.js` recovery catch. Both
received **undefined**, so the original exception has no actionable module
stack; the fallback converts it into `Error: undefined`. The triggering modules
are the generated shared/per-file CSS paths named by the server. Do not invent
a failing application JS module from the theme warnings. This diagnostic copy
also exposed Windows short-path serving-allow-list 403s; those are separate
experimental artifacts and absent from the uninstrumented reproduction.

## Harness change and validation

`run.mjs --server-log <existing file>` reads only newly appended output.
Optimizer reload messages become `dev-reoptimization-reload`; SSR program
reloads and browser WebSocket full-reload messages become `dev-server-reload`.
A bare HMR frame does not prove optimizer involvement. Console reload messages
are also observed. Windows PowerShell UTF-16LE logs are supported.

Affected screens are skipped with inconclusive checks; captures are withheld
from comparison and the report is partial. Later independent routes still run.
Other page errors continue to fail normally. This distinction does not turn a
transient environment failure into a passing audit.

The natural minimal cold run with server-log observation yielded **18 pass,
0 fail, 2 inconclusive**, partial, with `dev-server-reload` evidence. The
watcher-suppressed control yielded **20 pass, 0 fail**, completed. All **47**
tests across checks, browser, appearance, layout and dev-environment suites
passed; `git diff --check` passed. The production pixel-comparison suite was
not needed for the dev-only observer change and was not run.

## Evidence and limits

[Evidence](evidence/) contains the minimal normal/classified/control reports,
server logs, error screenshot, and Prime observation summaries with server
stdout/stderr. The normal report has three checks per route; the final
classified/control reports use the checked-in two-check fixture plan. JSON
screenshot paths other than `minimal-error.png` refer to captures retained in
the experiment's temporary audit directories, not additional committed files.

The initial Prime harness blocked its external CDN font because it was not in
allowedOrigins; that causes separate console/environment errors and must not
be mistaken for the 500. The independent browser captures permitted the font
and reproduced the 500. Fast screenshot capture can also add a transient
`caret-color: transparent` style before form hydration, producing a React
attribute warning; the recorded diff identifies that instrumentation artifact.

Only the two affected routes were audited, not every Prime route. The missing
mobile styles on an otherwise successful response, original `/about` grey band,
and production-vs-dev pixel comparison remain unverified. The upstream issue
draft is [here](upstream-issue.md); it is deliberately limited to the proven
cold-navigation CSS/SSR reload bug and has not been posted.
