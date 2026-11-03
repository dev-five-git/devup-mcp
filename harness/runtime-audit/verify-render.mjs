#!/usr/bin/env node
// A capture is evidence, not a visual verdict. Run Chromium, then compare every
// planned screen/viewport against an independently acquired, pinned Figma PNG.
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { runPlan } from './run.mjs';

const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');

function compare(binary, args) {
  return new Promise((resolveResult, reject) => {
    const child = spawn(binary, args, { windowsHide: true });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (data) => { stdout += data; });
    child.stderr.on('data', (data) => { stderr += data; });
    child.on('error', reject);
    child.on('close', (code) => {
      if (code !== 0 && code !== 1) return reject(new Error(`visual comparator exited ${code}: ${stderr}`));
      try { resolveResult({ exitCode: code, ...JSON.parse(stdout) }); } catch (error) { reject(error); }
    });
  });
}

export function referenceForViewport(entries, screen, viewport) {
  const matches = entries.filter((entry) => entry.screen === screen && entry.width === viewport.width
    && (entry.viewportHeight == null || entry.viewportHeight === viewport.height));
  if (matches.length !== 1) throw new Error(`Require exactly one Figma reference for ${screen} at CSS viewport ${viewport.width}x${viewport.height}; use viewportHeight to disambiguate repeated widths`);
  return matches[0];
}

export function captureCalibrationIssues(viewport, context) {
  const issues = [];
  if (!context || context.innerWidth !== viewport.width || context.innerHeight !== viewport.height
    || context.devicePixelRatio !== (viewport.deviceScaleFactor ?? 1)
    || context.visualViewportScale !== 1 || context.rootCssZoom !== 1 || context.bodyCssZoom !== 1) {
    issues.push('Actual viewport/DPR/visualViewport scale/CSS zoom evidence missing or uncalibrated');
  }
  if ((viewport.deviceScaleFactor ?? 1) !== 1) issues.push('Non-unit DPR does not establish PNG density; independent logical capture/reference scale calibration is unavailable');
  return issues;
}

export async function verifyRender({ plan, references, out, visualBin, maxMinutes = 10 }) {
  const document = JSON.parse(await readFile(plan, 'utf8'));
  const manifest = JSON.parse(await readFile(references, 'utf8'));
  const entries = manifest.references;
  if (!Array.isArray(entries) || !entries.length) throw new Error('Figma reference manifest is empty');
  const expected = document.plan.screens.flatMap((screen) => document.plan.viewports.map((viewport) => ({ screen: screen.id, viewport })));
  if (entries.length !== expected.length) throw new Error('Require one Figma reference per planned screen and viewport');
  // Validate and pin all originals before opening the browser. Never derive an
  // expected image from the generated page or silently accept missing widths.
  const originals = [];
  for (const screen of document.plan.screens) {
    for (const kind of ['page-health', 'image-loaded']) {
      if (!screen.checks.some((check) => check.kind === kind && !check.widths?.length)) {
        throw new Error(`Screen ${screen.id} requires ${kind} at every viewport`);
      }
    }
  }
  for (const { screen, viewport } of expected) {
    const entry = referenceForViewport(entries, screen, viewport);
    if (!Number.isFinite(entry.threshold) || entry.threshold < 0 || entry.threshold >= 1) throw new Error('Reference threshold must be in [0, 1)');
    const path = resolve(dirname(references), entry.path);
    if (!/^[a-f0-9]{64}$/.test(entry.sha256) || hash(await readFile(path)) !== entry.sha256) throw new Error(`Figma reference hash differs: ${path}`);
    originals.push({ ...entry, path, viewport });
  }
  const runtime = await runPlan({ plan, out, maxMinutes });
  const comparisons = [];
  for (const [index, original] of originals.entries()) {
    const run = runtime.runs.find((item) => item.viewport.width === original.viewport.width && item.viewport.height === original.viewport.height);
    const screen = run?.screens.find((item) => item.id === original.screen);
    if (!screen?.screenshot) {
      comparisons.push({ screen: original.screen, width: original.width, status: 'not-run' });
      continue;
    }
    const actual = resolve(out, screen.screenshot.path);
    if (hash(await readFile(actual)) !== screen.screenshot.sha256) throw new Error(`Screenshot hash differs: ${actual}`);
    const calibrationIssues = captureCalibrationIssues(original.viewport, screen.captureContext);
    const referenceBytes = await readFile(original.path), actualBytes = await readFile(actual);
    const referenceDimensions = [referenceBytes.readUInt32BE(16), referenceBytes.readUInt32BE(20)];
    const actualDimensions = [actualBytes.readUInt32BE(16), actualBytes.readUInt32BE(20)];
    if (referenceDimensions.some((value, axis) => value !== actualDimensions[axis])) calibrationIssues.push('Natural capture and reference dimensions differ; strict pixels inconclusive, inspect logical geometry separately without resizing/padding/cropping');
    if (calibrationIssues.length) {
      comparisons.push({ screen: original.screen, width: original.width, status: 'inconclusive', calibrationIssues, referenceDimensions, actualDimensions });
      continue;
    }
    const result = await compare(visualBin, ['compare', '--reference', original.path, '--actual', actual,
      '--diff', resolve(out, `visual-diff-${index}.png`), '--max-changed-ratio', String(original.threshold)]);
    comparisons.push({ screen: original.screen, width: original.width, referenceSha256: original.sha256, actual, ...result });
  }
  const runtimePassed = runtime.status === 'completed'
    && runtime.runs.length === document.plan.viewports.length
    && runtime.runs.every((run) => !run.environmentErrors.length && run.screens.length === document.plan.screens.length
      && run.screens.every((screen) => screen.status === 'completed' && screen.checks.every((check) => check.status === 'pass')));
  const pixelPassed = comparisons.every((item) => item.exitCode === 0 && ['exact', 'within-threshold'].includes(item.status));
  const verdict = runtime.runs.some((run) => run.environmentErrors.length) ? 'inconclusive' : !runtimePassed ? 'fail' : comparisons.some((item) => item.status === 'inconclusive') ? 'inconclusive' : pixelPassed ? 'pass' : 'fail';
  const result = { verdict, runtimePassed, strictPixelsPassed: pixelPassed, runtimeReport: join(out, 'report.json'), comparisons };
  await writeFile(join(out, 'real-render.json'), `${JSON.stringify(result, null, 2)}\n`);
  return result;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const options = {};
    const names = { '--plan': 'plan', '--references': 'references', '--out': 'out', '--visual-bin': 'visualBin' };
    for (let index = 2; index < process.argv.length; index += 2) {
      const name = names[process.argv[index]];
      if (!name || !process.argv[index + 1]) throw new Error('usage: node verify-render.mjs --plan <plan.json> --references <references.json> --out <dir> --visual-bin <devup-mcp-visual>');
      options[name] = resolve(process.argv[index + 1]);
    }
    if (Object.keys(options).length !== 4) throw new Error('plan, references, out and visual-bin are required');
    const result = await verifyRender(options);
    console.log(`${result.verdict}: ${result.comparisons.length} real-browser comparisons`);
    process.exitCode = result.verdict === 'pass' ? 0 : 1;
  } catch (error) {
    console.error(String(error?.message ?? error));
    process.exitCode = 2;
  }
}
