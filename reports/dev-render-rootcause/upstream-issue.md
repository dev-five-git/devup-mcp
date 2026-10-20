# Cold DevupUI CSS extraction triggers vinext SSR program reload and first-route HTTP 500

Suggested upstream recipients: @devup-ui/vite-plugin and vinext/@vitejs/plugin-rsc
maintainers. This is a draft; no issue has been posted.

With Vite 8.2.2, vinext 1.0.1, @vitejs/plugin-rsc 0.5.34,
@devup-ui/vite-plugin 1.0.67, @devup-ui/react 1.0.40 and React 19.2.8 on
Windows/Node 26.8.2, visiting a previously uncompiled styled route can return
HTTP 500 after a shared client component has loaded the extracted base CSS.

The attached minimal app is `fixtures/vinext-dev-css-repro`: two server routes,
a shared client header and a second-route client form. It uses no devup-mcp,
devup-api, next-intl or React Compiler. Copy to a fresh directory, install its
pinned direct dependencies, start `npm run dev` with logs, and use the included
runtime-audit plan to navigate `/about` then `/seo-geo` at widths
1440/1100/858/600/390. The fixture README contains the exact commands.

Expected: the first and subsequent route visits render normally, with no SSR
program restart caused by the current request's generated CSS.

Observed on the first SEO visit:

```text
GET /about/ 200
[vite] (ssr) page reload df/devup-ui/devup-ui.css
[vite] (ssr) program reload
GET /seo-geo/ 500
```

The response is vinext's “This page couldn’t load” document. Later visits at
every width return 200. The same cold failure occurs in a larger app both with
and without `optimizeDeps.exclude: ['@devup-ui/react']`. It also occurs with 390
as the first viewport, so the width at which the failure appears is incidental
to cold route order.

Controls: removing the shared client header prevented the failure; ignoring
`**/df/devup-ui/**` watcher events prevented it in both the minimal and larger
app. That watcher setting disables CSS HMR and is only a diagnostic experiment.
Waiting for network idle plus 300ms between larger-app visits also avoided it.
No optimizer-reload announcement accompanied the minimal failure.

Installed DevupUI code writes base CSS during transform when
`updatedBaseStyle` is set and writes per-file CSS markers during extraction.
The evidence points to those writes invalidating the active SSR graph during
render. Instrumenting a private vinext copy showed `undefined` entering SSR
`onError` and the HTML stream recovery catch; there was no useful application
module stack. Please investigate whether generated CSS HMR can avoid an SSR
program restart, and whether the framework can recover/retry an interrupted
cold render instead of returning its error shell.

Evidence in `reports/dev-render-rootcause/evidence` includes failing/passing
server logs, browser audit reports and a screenshot. This issue makes no claim
about persistent missing mobile CSS on a successful HTTP 200 response; that
separate symptom was not reproduced.
