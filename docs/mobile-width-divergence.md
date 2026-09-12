# Why the narrowest width diverges most

Measured on `main` at `2df7fb4`, with the harness runnable again after PRs #34
and #35. Recorded here because the finding cost three harness repairs to obtain
and refutes the hypothesis that motivated them.

## The measurement

`landing`, acquired fresh and compared against Figma's own PNG of each frame:

| screen | width | measured | committed threshold |
| --- | --- | --- | --- |
| `landing-833-3640` | 360 | **11.41%** | 11.41 |
| `landing-833-3322` | 992 | 3.11% | 3.1 |
| `landing-832-2975` | 1920 | 1.84% | 1.84 |

`render.mjs` exited 0: nothing exceeded its own figure.

Binary: `devup-mcp 0.4.5 (2efefa40a6fd)`, which contains the W8 JSX text fix
merged in `0c41e8a`. The call bank caches Figma responses only; the module is
generated locally on every run, so this measures the current generator.

## The hypothesis this refutes

Before the harness was runnable it was reasonable to suppose the mobile gap was
largely the JSX whitespace defect W8 repaired: a narrow width wraps more, so a
spurious space would distort wrapping more visibly, and the three texts
`text-check.mjs` reported wrong were one paragraph at three widths.

**It is not.** The numbers are unchanged to two decimal places against
thresholds written before that fix. W8 repaired a real defect - it just is not
this one. Whatever text `landing` contains does not hit the pattern where JSX
inserted a space.

## What it actually is: cumulative vertical drift

`drift.mjs` aligns each 100px band independently and reports the offset that
minimises the difference. `at 0` is the difference in place; `best … at N` is the
difference after shifting N pixels.

```
band            best  at        at 0   verdict
    0-  100    0.3%     0    0.3%  aligned
  300-  400   12.1%     2   21.0%  differs  <- drift 1 to 2
  900- 1000   14.2%     5   23.5%  differs  <- drift 0 to 5
 1500- 1600   14.7%     6   24.3%  differs  <- drift 7 to 6
 1700- 1800    6.3%     8   14.7%  close    <- drift 7 to 8
```

The alignment offset grows monotonically down the page: 0 → 2 → 5 → 6 → 8. The
band at y 900-1000 is 23.5% different in place and 14.2% different shifted five
pixels. That is not a set of individually wrong elements; it is a small height
deficit accumulating from the top.

The frame total is not wrong - capture and reference are both 360x2955 - so the
sum is right while the internal distribution is not. Something absorbs the
difference at the end.

`bands.mjs` puts the worst bands at y 1500, 2500, 900, 1800 and 300, roughly 700
pixels apart, which reads as the same error recurring at a repeating section
boundary rather than one bad node.

```
y  1500- 1600   26.22%
y  2500- 2600   24.10%
y   900- 1000   23.86%
y  1800- 1900   21.36%
y   300-  400   20.27%
```

## What to look at next, and what not to assume

This is a vertical sizing or spacing problem, not a text problem. The open
question is which property accumulates roughly eight pixels over the page:
padding, gap, line-height, or FILL/HUG distribution are all consistent with the
evidence so far and they require different fixes.

`boxes.mjs` compares DOM boxes against Figma coordinates directly and is the
cheapest way to name the property; run it on the worst bands (y≈900, 1500, 2500)
rather than guessing from the aggregate.

Do not read the desktop figures as "good enough by comparison". 1.84% at 1920 and
11.41% at 360 for the same design is a ratio of about six, and the narrowest
width is where real users are.

## Why this was hard to measure at all

Three separate defects kept `harness/render` from running, each found by running
it rather than reading it:

1. `acquire.py` hardcoded `~/.cargo/bin/devup-mcp.exe`, absent on a machine that
   builds but never installs, and the literal `.exe` made it Windows-only (#34).
2. It predated the R8 export-job contract: it read a `status: "in_progress"`
   reply as final and opened an output file the server had not written yet (#35).
3. It requested assets 16 at a time against a server cap of 6, so every batch was
   refused before it started. Nothing was lost - the one-at-a-time fallback
   acquired them individually, which is why this went unnoticed - but each batch
   paid a guaranteed-failing round trip (#35).

The harness inputs remain gitignored as generated, so a fresh checkout still has
to acquire once against live Figma. The call bank makes only the re-runs free.
