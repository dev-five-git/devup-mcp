# Runtime audit contract

`devup_runtime_audit` checks generated screens at viewport widths their Figma frames were not drawn at. The server plans and verifies, and the consumer's repository runs the browser. Static reports such as `widthBehavior` and `layoutRisks` say what *should* follow a width change; this is how a project measures what actually renders.

Boundaries:

- The server never launches a browser, runs a command or fetches a URL. Playwright is a dependency of the executor package `harness/runtime-audit`, which the consumer installs. It is not a dependency of the MCP server.
- A plan is data. Every object in it refuses unknown fields, and no field is ever executed: strings are only routes, locator values, fill text and labels. A plan cannot carry a command or a script. Steps find elements by role and accessible name, test id or visible text. CSS selectors are accepted only in checks and `expect.visible`.
- URLs must be `http(s)` with a host and no credentials. `allowedOrigins` must include the `baseUrl` origin. Routes are paths on the `baseUrl` origin, starting with a single `/`.

## 1. Plan

```json
{
  "action": "plan",
  "outputPath": "audit/plan.json",
  "plan": {
    "baseUrl": "http://127.0.0.1:4173",
    "allowedOrigins": ["http://127.0.0.1:4173"],
    "viewports": [{ "width": 360, "height": 800 }, { "width": 1200, "height": 800 }],
    "readiness": { "selector": "body[data-ready=\"1\"]" },
    "fonts": ["Pretendard"],
    "screens": [
      {
        "id": "today",
        "routePath": "/today",
        "figma": { "nodeId": "1:138", "drawnWidths": [360] },
        "checks": [
          { "kind": "no-horizontal-overflow" },
          { "kind": "contained", "label": "badge text", "subject": { "testId": "badge-text" }, "container": { "testId": "badge" } },
          { "kind": "aligned", "edge": "right", "offsetPx": 16, "subject": { "testId": "qr" }, "container": { "testId": "card" } },
          { "kind": "spans", "insetPx": 15.5, "measure": "painted-mask", "subject": { "testId": "divider" }, "container": { "testId": "card" } }
        ]
      },
      {
        "id": "detail",
        "steps": [{ "action": "click", "target": { "role": "link", "name": "Details" } }],
        "expect": { "urlPath": "/today/detail" },
        "checks": [{ "kind": "visible", "subject": { "role": "heading", "name": "Details" } }]
      }
    ]
  }
}
```

| Field | Rule |
|---|---|
| `viewports` | 1 to 8. `width` and `height` 1 to 4096, `deviceScaleFactor` in (0, 4] (default 1), `mobile` and `touch` flags |
| `screens` | 1 to 60 with unique ids. The first needs a `routePath`; a screen without one is reached by its `steps` from the previous screen |
| viewports × screens | At most 400, the number of screenshots ingest verifies for one report. A larger plan could never pass, so it is refused; split it |
| `steps` | At most 20 per screen. `click` takes no value, `fill` takes a value of at most 300 characters. The target is `role` (with `name`), `testId` or `text` |
| `expect` | `urlPath` (pathname, compared exactly), `heading` (accessible name), `visible` (any locator) |
| `checks` | At most 40 per screen |
| Text fields | Non-empty, at most 300 characters |
| Timeouts | `stepTimeoutMs`, `readiness.timeoutMs` and `steps[].timeoutMs` from 1 to 120000 ms |
| px values | Finite, magnitude at most 10000 |

| Check | Subject | Container | Passes when |
|---|---|---|---|
| `no-horizontal-overflow` | no | no | Neither the document's `scrollWidth` nor any visible element's right edge reaches past the viewport. Content clipped by `overflow: hidden` counts |
| `visible` | yes | no | The subject renders with a non-empty box |
| `contained` | yes | yes | The subject's box lies inside the container's on every edge |
| `aligned` | yes | yes | The subject keeps `offsetPx` from the container's `left` or `right` edge, or shares its centre (`center`) |
| `spans` | yes | yes | The subject spans the container's width less `insetPx` on each side. `measure: "painted-mask"` measures the box a CSS mask paints instead of the element box, which is what tells a 1px rule drawn as a mask apart from the box it sits in |

Every check takes `tolerancePx` (default 0.5) and an optional `label`. A locator that matches no element, or more than one, makes its check `inconclusive`, never `fail`.

`planId` is the SHA-256 of the plan's normalized serialization: fields in schema order, with defaults filled in. The same plan always gets the same id, and any edit gives a new one.

The plan document is returned, and written to `outputPath` when one is given. It carries `schemaVersion: 1`, `kind: "devup-runtime-audit-plan"`, `planId`, `createdBy` (the server identity), the normalized `plan`, `expected` (`runs`, `screensPerRun`, `checkItems`), `figmaReferences`, and `executor` (package, install and run commands, isolation). The response adds `writtenTo` and `nextAction`. Nothing has run at this point.

## 2. Execute, in the consumer project

```bash
cd harness/runtime-audit
npm ci && npx playwright install chromium
node run.mjs --plan <plan.json> --out <report directory> [--max-minutes 10]
```

- The plan is parsed as JSON and never evaluated. A file that is not a version 1 plan document is refused, and so is any CLI argument the executor does not know. Exit code 2 means nothing was run; exit code 0 means a report was written, whatever its checks found.
- Each viewport gets one fresh browser context, with no profile, cookies or storage from anyone's browser. A request to an origin outside `allowedOrigins` (other than `data:`, `blob:` and `about:`) is aborted. The blocked requests are recorded as one `request-blocked` environment error listing up to 20 URLs.
- For each screen the executor navigates to `routePath` when the screen has one and runs its steps, then waits for `expect`, `document.fonts.ready`, decoded images and `readiness.selector`. Then it measures every check and writes one full-page PNG named `<width>x<height>@<scale>-<screen>.png`.
- Page errors, console errors, failed requests, an HTTP error status on navigation, a viewport whose `innerWidth` differs from the plan, and a planned font that is not loaded are environment errors, not layout failures.
- A screen whose navigation or steps fail is `failed` and its checks are `inconclusive`. A screen reached only through it is `skipped`. After `--max-minutes`, the remaining screens are `skipped` and the remaining viewports are not run.
- On SIGINT the run stops before the next viewport and writes the report with `status: "aborted"` and the runs finished so far. An interrupted run cannot be resumed: run the plan again.

The report carries `schemaVersion: 1`, `kind: "devup-runtime-audit-report"`, `planId` (copied from the plan) and `planSha256` (the SHA-256 of the plan file's bytes). `status` is `completed`, `partial` or `aborted`. `executor` holds the Node, Playwright and browser versions, and `environment` holds the OS, locale and timezone. Each `runs[]` entry has its `viewport`, `environmentErrors[]` and `screens[]`. Each screen has `id`, `status`, `checks[]` (`index`, `kind`, `label`, `status`, `reason`, `measured`) and `screenshot` (`path`, `sha256`, `bytes`). The report ends with the executor's own `summary`.

## 3. Ingest

```json
{ "action": "ingest", "planPath": "audit/plan.json", "reportPath": "audit/out/report.json" }
```

Both files are read through the server's allowed write roots, with the plan capped at 1 MiB and the report at 8 MiB. Neither is trusted as it stands:

1. The plan must be a version 1 plan document whose `plan` parses and validates again, and whose `planId` equals the id recomputed from it. Otherwise ingest rejects it with `plan-kind`, `plan-schema`, `plan-invalid` or `plan-edited`.
2. The report must be a version 1 report for that `planId`. Otherwise the reason is `report-kind` or `report-for-another-plan`.
3. When the report carries `planSha256`, it must equal the SHA-256 of the plan file. Otherwise the reason is `plan-file-differs`. If `planSha256` is absent, `integrity.planFileHashMatches` is `null`.
4. Every planned viewport × screen × check is looked up in the report by index. An item that is missing is `not-run`, and an unknown status counts as `inconclusive`. Every count is recomputed from these items, and `declaredSummaryMatches` compares the executor's own summary against them.
5. Ingest verifies up to 400 screenshots per report, each at most 16 MiB. A screenshot path must be relative to the report directory, with no `..` or root, and stay inside the allowed roots. Its SHA-256 and byte length must match the report; a mismatch rejects the whole report as `artifact-changed`. The PNG must decode within 8192 px per side, be exactly the viewport width × `deviceScaleFactor` wide and at least the viewport height × `deviceScaleFactor` tall. A screenshot that fails these checks is listed in its run's `screenshots.issues` and makes the verdict inconclusive.

| Verdict | When |
|---|---|
| `rejected` | Any rejection above. Nothing in the report is counted |
| `fail` | At least one check failed, every planned viewport was reported without environment errors, and every screenshot verified |
| `pass` | Every planned item passed, every environment is valid, and the report's status is `completed` |
| `inconclusive` | Anything else: a missing viewport, an environment error, an unverifiable screenshot, an item that is skipped, not run or inconclusive, or a partial or aborted run. A failure seen in an invalid environment is also inconclusive |

The response carries `verdict`, `planId`, `integrity`, `coverage` (`plannedCheckItems`, `notRun`, `reportStatus`), `summary` and `declaredSummaryMatches`. `failures` lists the first 50 failures, each with viewport, screen, check index, kind, label, reason and measured boxes. `runs` gives each viewport's environment errors, `environmentValid` and screenshot verification. The response also echoes `figmaReferences`, `executor` and `environment`. `evidence` separates what ingest verified itself from what the executor only declared:

- Ingest verifies the plan's content id and SHA-256, the report's binding to that plan, every count, and each screenshot's hash, length, PNG decoding and width.
- The executor declares the element geometry and each check's outcome; page, console and network errors; and the browser and environment versions.
- Nothing judges whether the screens look right to a person.

## Figma references

A screen has a visual original only at the widths its Figma frame was drawn at. `figmaReferences` lists those per screen as `visualOriginalAt`; compare screenshots at them with `devup_visual_compare` and its renderer manifest. At every other width (`layoutChecksOnlyAt`) only the layout checks apply.

## Not implemented

- Resuming an interrupted run. A rerun starts again from the first viewport.
- Steps beyond `click` and `fill`. There are no uploads, network mocks, authentication flows or declared side effects (`sideEffect`).
- Demo recording, narration (TTS) and subtitles, and a handoff package built from a run.

## Privacy

Screenshots show the consumer's screens and may contain personal data. They stay in the report directory. A response carries only their hashes and verification state, never their pixels. Do not commit them: `harness/runtime-audit/.gitignore` excludes `out/`.
