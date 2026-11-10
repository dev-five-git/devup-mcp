---
name: devup-retro
description: |
  Session retrospective for work done with devup-mcp. Reviews the conversation
  from the first devup-mcp call to now, lists every follow-up prompt the user had
  to write to correct or complete the result, classifies each by the devup-mcp
  surface that should have prevented it, asks the user to confirm the list, and
  only then prints ready-to-paste improvement prompts for devup-mcp development.

  Invoke as /devup-retro [scope or notes] once a piece of devup-mcp work is
  finished. Also use when asked to "turn this session's corrections into
  devup-mcp feedback" or to explain why follow-up prompts were needed.
---

# Session retrospective: follow-up prompts to improvement prompts

The goal is that the user never has to write the same follow-up prompt again.
Every correction the user typed after devup-mcp produced something is evidence
of a gap. This command turns that evidence into prompts the devup-mcp
maintainers can act on. It reviews; it does not fix the application and it does
not change devup-mcp.

Three phases, in this order, never merged: **review, confirm, prompts**.
Nothing is written to disk and no improvement prompt is printed before the user
confirms the review list.

## Phase 1: review

1. Call `devup_skills` with `action: "retro"` and the same `projectRoot` the
   session used. It returns the installed build (`server.version`,
   `server.commit`, `server.buildId`), this server process's devup-mcp call log
   with timestamps (`callLog`), the `.feedback/` folder state (`feedback`), the
   surface list (`surfaces`) and where release notes live (`releases`).
   The call log only covers this server process: after a reconnect or in a
   resumed session it is partial, and `callLog.note` says so. It is an aid for
   ordering, never the evidence itself.
2. Find the starting point: the first devup-mcp call of the work under review
   (export, plan, validation, audit, compare). An argument given to the command
   narrows the scope ("only the landing page", "since the second export"). If
   the conversation was compacted and the early part is no longer visible, say
   which part you could not read. Do not reconstruct it from memory.
3. Walk every user message after that point. Keep a message when the user had
   to correct, redo or complete something devup-mcp's output, validation or
   guidance was responsible for, for example:
   - missing or wrong motion ("add the animation", "the reveal replays");
   - wrong layout (spacing, width, alignment, overflow, a width that was never audited);
   - wrong text wrapping, including Korean word breaks;
   - missing interaction states, links or events left unwired;
   - wrong, missing or placeholder assets;
   - tests, lint, typecheck or audits not run, or reported as run without evidence;
   - wrong CI or completion evidence (wrong commit, wrong run, counts that do not match);
   - a question asked late, or asked again, that intake should have settled.
   Drop messages that are new requirements, changes of mind, approvals, or
   ordinary conversation. A message is a follow-up only if the earlier result
   should already have satisfied it.
4. For each kept message, record one item:
   - `quote`: the user's words, verbatim and in their language, trimmed to the relevant sentence;
   - `what`: what was wrong, in one sentence, stated as an observation;
   - `evidence`: the file, Figma node id, tool call or tool output that shows it
     (a `callLog` entry, a validation result, an audit report, a diff). Cite what
     you actually read in this session;
   - `surface`: exactly one value from `surfaces` (below);
   - `cause`: observed fact versus your inference, labelled separately;
   - `status`: see "Already fixed" below.
   Merge messages about the same defect into one item and keep every quote.

### Rules that keep the list honest

- No invented problems. An item with no user-prompt evidence is excluded, even
  if you noticed the defect yourself. Mention such observations separately,
  after the list, as "not from a follow-up prompt"; they never become prompts.
- Not every problem is a devup-mcp bug. Classify agent mistakes, Devup UI
  compiler behaviour, application code and Figma data as what they are.
- Do not strengthen the user's words. "UI가 잘못됐어" stays that quote; the
  specific defect comes from the evidence, not from the quote.
- When the evidence is missing, the item says "evidence not found in this
  session" and is listed as unverified rather than dropped silently.

### Surfaces

| surface | The gap is in |
| --- | --- |
| `codegen` | the TSX/CSS the converter generates from Figma nodes |
| `export-output` | what `devup_figma_export` returns: outputs, assets, manifests, deliverable flags, sidecars |
| `validation-rule` | a `devup_ui_validate` rule that is missing, too weak or wrong |
| `runtime-audit` | `devup_runtime_audit`, audit plans, `devup_visual_compare` checks and their evidence |
| `skill-guidance` | the installed skills (`devup`, `devup-implementation-plan`, `devfive-frontend` and references) |
| `bootstrap-instructions` | the server instructions and `devup://guide/usage` |
| `project-plan-intake` | `devup_project_plan` / `devup_screen_plan` questions, rounds, ERD, split |
| `out-of-scope:devup-ui` | Devup UI runtime or compiler, not devup-mcp |
| `out-of-scope:app` | the application's own code, tests, CI or environment |
| `out-of-scope:figma-data` | the design file itself (missing variants, wrong constraints) |

Pick the surface that should have **prevented** the follow-up, which is not
always the one that produced the defect: a converter gap that validation would
have caught is `codegen`, and the acceptance check in its prompt names the
validation rule.

### Already fixed

Compare each item with the installed build from `devup_skills retro`, the
release notes at `releases.url` (use `gh release list -R dev-five-git/devup-mcp`
or a fetch tool when one is available), and any status line in the project's
`.feedback/` records. Set `status` to one of:

- `open`: no evidence of a fix;
- `fixed-in-installed`: the installed version already contains the fix; the
  follow-up was caused by something else (a stale skill copy, an agent mistake)
  and the item says which;
- `fixed-not-installed`: a newer release fixes it; `server.updateAvailable` or
  the release notes show it; the action is to update, not to file a prompt;
- `already-recorded`: an existing `.feedback/` record covers it; name the file;
- `unverified`: release notes could not be read. Say so; never guess "fixed".

## Phase 2: confirm

Show the review list as a table, numbered, in the user's language: quote,
what was wrong, surface, evidence, status. Then ask with the runtime question
tool (Claude Code `AskUserQuestion`, OpenCode `question`), in one call:

1. Review list: `OK` (Recommended) / `Edit items` / `Drop items` / `Cancel`.
2. If a `.feedback/` folder exists (`feedback.exists`): also save each item as a
   feedback record? `Yes` / `No`. Without the folder, do not offer to create one.

On `Edit items` or `Drop items`, apply what the user says, show the updated
table and ask again. Repeat until `OK` or `Cancel`. On `Cancel`, stop: print
nothing further and write nothing. Without a question tool, print the table,
ask the same question in one message and wait; never assume `OK`.

## Phase 3: prompts

Only after `OK`. Write in the user's language (Korean when the session is
Korean); keep identifiers, tool names, file paths and code in their original
form. Group items by surface and print one ready-to-paste prompt per surface,
each in its own fenced block so it can be copied whole.

Items with status `fixed-in-installed`, `fixed-not-installed` or
`already-recorded` get no prompt. List them under the prompts with the action
that applies (update devup-mcp, reinstall skills, see the existing record).
`out-of-scope:*` items get a short prompt addressed to that owner (Devup UI,
the application, the designer), clearly marked as not for devup-mcp.

Every prompt has these four parts, in this order:

1. **Goal**: one sentence, addressed to devup-mcp development, naming the
   surface and the outcome ("devup-mcp의 UI 구현·검증 흐름에 … 를 기본 품질
   항목으로 추가해 주세요.").
2. **Required behaviour**: a numbered list of concrete, checkable behaviours.
   Each one traces to an item of the confirmed list; quote the user prompt and
   cite the evidence (file, node id, tool output) once, under the list.
3. **Acceptance checks and regression tests**: what must pass, including at
   least one regression test that fails on today's behaviour, and which tool
   output proves it (`devup_ui_validate` finding id, audit check, snapshot).
4. **Scope boundary**: what this prompt does not ask for, and which neighbouring
   surface owns the rest.

State the installed build once above the prompts (`version`, `commit`,
`buildId`) so the maintainers know what the session ran against. Do not pad:
a prompt with one item has a one-item list. Do not propose a fix the evidence
does not support, and do not prescribe implementation details beyond what the
required behaviour needs.

### Saving feedback records (only when the user chose Yes)

Read the folder first: its index file (`feedback.index`), and the two most
recent records, and follow their format exactly, including language, field
labels, file extension and file-name pattern. Number new records from
`feedback.nextNumber`, one file per confirmed item, and add one line per record
to the index in its existing style. Never renumber, rewrite or delete existing
records, and never record an item the user dropped. Report the files written.
If the folder's format cannot be determined, do not write; say why.

## Final answer

End with: the number of user follow-ups reviewed, items confirmed, prompts
printed per surface, items needing no prompt and why, files written (or
"nothing written"), and anything you could not review.
