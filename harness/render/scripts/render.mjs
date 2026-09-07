// Render every acquired screen at its frame's size and compare it with the
// PNG Figma drew of the same frame.
//
//   node scripts/render.mjs [name ...]
//
// Follows docs/visual-renderer-contract.md: the viewport is the reference
// PNG's size, the page is captured only after fonts are loaded, images are
// decoded and the app has said it is ready, animations are off, and the
// comparison is left to `devup-mcp-visual`. A screen whose page errors is
// reported as `environment-invalid`, never as a pixel result.

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { PNG } from "pngjs";

const HARNESS = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const REPO = resolve(HARNESS, "..", "..");
const PORT = 4173;
// Where cargo puts the binary: the workspace may point its target directory
// elsewhere (this one shares one under ~/.cargo).
function visualBinary() {
  const metadata = spawnSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], { cwd: REPO, encoding: "utf8", shell: process.platform === "win32" });
  const targetDirectory = metadata.status === 0 ? JSON.parse(metadata.stdout).target_directory : join(REPO, "target");
  return join(targetDirectory, "release", process.platform === "win32" ? "devup-mcp-visual.exe" : "devup-mcp-visual");
}
const VISUAL = visualBinary();

function pngSize(path) {
  const bytes = readFileSync(path);
  if (bytes.readUInt32BE(12) !== 0x49484452) throw new Error(`${path}: not a PNG`);
  return { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: HARNESS, stdio: "pipe", encoding: "utf8", shell: process.platform === "win32", ...options });
  return result;
}

// Figma's `get_screenshot` flattens a frame's own transparency onto a canvas
// colour it does not report - a 70% black backdrop came back an opaque
// (21, 21, 21), which is neither black nor the page's #1E1E1E. The capture
// here keeps the transparency, so before comparing it is flattened onto the
// same colour, recovered from the reference at the capture's most
// transparent pixel: reference = alpha * ours + (1 - alpha) * canvas. A
// capture that is opaque everywhere is left as it is.
function flattenLikeFigma(referencePath, actualPath) {
  const reference = PNG.sync.read(readFileSync(referencePath));
  const actual = PNG.sync.read(readFileSync(actualPath));
  if (reference.width !== actual.width || reference.height !== actual.height) return null;
  let probe = -1;
  let lowest = 255;
  for (let index = 3; index < actual.data.length; index += 4) {
    if (actual.data[index] < lowest) {
      lowest = actual.data[index];
      probe = index - 3;
    }
  }
  if (probe < 0 || lowest === 255) return null;
  const alpha = lowest / 255;
  const canvas = [0, 1, 2].map((channel) => {
    const value = (reference.data[probe + channel] - alpha * actual.data[probe + channel]) / (1 - alpha);
    return Math.max(0, Math.min(255, Math.round(value)));
  });
  for (let index = 0; index < actual.data.length; index += 4) {
    const a = actual.data[index + 3] / 255;
    for (const channel of [0, 1, 2]) {
      actual.data[index + channel] = Math.round(a * actual.data[index + channel] + (1 - a) * canvas[channel]);
    }
    actual.data[index + 3] = 255;
  }
  writeFileSync(actualPath, PNG.sync.write(actual));
  return { canvas, alpha: lowest };
}

// `spawn` with a shell starts a shell that starts vite, so killing the child
// kills the shell and leaves vite holding the port. The next run then fails
// on `--strictPort`, or worse waits on a server serving the previous build.
// The whole tree has to go.
function stopPreview(preview) {
  if (!preview.pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(preview.pid), "/T", "/F"], { stdio: "ignore" });
  } else {
    try {
      process.kill(-preview.pid, "SIGKILL");
    } catch {
      preview.kill("SIGKILL");
    }
  }
}

async function waitForServer(url, attempts = 50) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`preview server did not answer at ${url}`);
}

// The theme a screen is rendered with: its own, written by the acquisition
// at node scope. An answer screen borrows the theme of the screen it answers
// for, so the two are compared under the same colours.
function themeFor(target) {
  const screen = target.screen ?? target.name;
  const candidates = [target.theme, `themes/${screen}.json`, `themes/${screen.replace(/-answer$/, "")}.json`];
  return candidates.find((candidate) => candidate && existsSync(join(HARNESS, candidate))) ?? null;
}

async function main() {
  const wanted = process.argv.slice(2);
  const manifest = JSON.parse(readFileSync(join(HARNESS, "targets.json"), "utf8"));
  const targets = manifest.targets.filter((target) => wanted.length === 0 || wanted.some((name) => target.name === name || target.name.startsWith(`${name}-`)));
  if (targets.length === 0) throw new Error("no targets; run scripts/acquire.py first");

  if (!existsSync(VISUAL)) {
    const cargo = run("cargo", ["build", "-p", "devup-mcp-visual", "--release"], { cwd: REPO });
    if (cargo.status !== 0) throw new Error(`cargo build failed:\n${cargo.stderr}`);
  }

  // This file holds several brands' collections, and more than one defines
  // `primary`; a theme covering all of them resolves one brand's value for
  // every screen, which drew the notice screen violet where Figma draws it
  // blue. Devup UI bakes the theme into the CSS at build time, so screens are
  // grouped by the theme they need and each group is built once.
  const groups = new Map();
  for (const target of targets) {
    const theme = themeFor(target);
    const key = theme ? createHash("sha256").update(readFileSync(join(HARNESS, theme))).digest("hex").slice(0, 12) : "as-configured";
    if (!groups.has(key)) groups.set(key, { theme, members: [] });
    groups.get(key).members.push(target);
  }

  const browser = await chromium.launch();
  const report = [];
  try {
    for (const [index, [key, group]] of [...groups].entries()) {
      if (group.theme) writeFileSync(join(HARNESS, "devup.json"), readFileSync(join(HARNESS, group.theme)));
      console.log(`theme ${key}${group.theme ? ` from ${group.theme}` : " (as configured)"}: ${group.members.map((member) => member.name).join(", ")}`);
      const build = run("npx", ["vite", "build"]);
      if (build.status !== 0) {
        console.error(build.stdout, build.stderr);
        throw new Error("vite build failed");
      }
      // A port of its own per group: a preview killed on Windows does not
      // always let go of its port before the next one asks for it.
      const port = PORT + index;
      const preview = spawn("npx", ["vite", "preview", "--port", String(port), "--strictPort"], { cwd: HARNESS, shell: process.platform === "win32", stdio: "ignore" });
      try {
        await renderGroup(browser, group.members, report, port);
      } finally {
        stopPreview(preview);
      }
    }
  } finally {
    await browser.close();
  }
  mkdirSync(join(HARNESS, "out"), { recursive: true });
  writeFileSync(join(HARNESS, "out", "report.json"), JSON.stringify(report, null, 2));
}

async function renderGroup(browser, targets, report, port) {
  {
    await waitForServer(`http://localhost:${port}/`);
    for (const target of targets) {
      const reference = join(HARNESS, target.reference);
      const size = pngSize(reference);
      const actual = join(HARNESS, "out", `${target.name}.actual.png`);
      const diff = join(HARNESS, "out", `${target.name}.diff.png`);
      const context = await browser.newContext({ viewport: size, deviceScaleFactor: 1, locale: "ko-KR", timezoneId: "Asia/Seoul", reducedMotion: "reduce" });
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (error) => errors.push(String(error)));
      page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
      const screen = target.screen ?? target.name;
      await page.goto(`http://localhost:${port}/?screen=${encodeURIComponent(screen)}`, { waitUntil: "networkidle" });
      await page.waitForSelector("body[data-ready]", { timeout: 30000 });
      const ready = await page.evaluate(() => document.body.dataset.ready);
      const entry = { name: target.name, frame: target.frame, viewport: size, reference: target.reference, actual: `out/${target.name}.actual.png` };
      if (ready !== "1" || errors.length > 0) {
        entry.environmentStatus = "environment-invalid";
        entry.errors = errors.concat(ready !== "1" ? [await page.evaluate(() => document.body.dataset.error ?? "not ready")] : []);
        report.push(entry);
        console.log(`${target.name}: environment-invalid ${JSON.stringify(entry.errors)}`);
        await context.close();
        continue;
      }
      // The page's own size, so a screen that comes out taller or shorter
      // than Figma drew it is reported as such and not only clipped.
      entry.rendered = await page.evaluate(() => {
        const root = document.getElementById("root");
        const rect = root.getBoundingClientRect();
        return { width: Math.round(rect.width), height: Math.round(document.documentElement.scrollHeight) };
      });
      // Figma exports a frame with its own transparency - a 70% black
      // backdrop is (0, 0, 0, 178) in the PNG - so the page is captured
      // without a background too, and the same pixel comes out the same.
      await page.screenshot({ path: actual, clip: { x: 0, y: 0, width: size.width, height: size.height }, fullPage: true, omitBackground: true });
      await context.close();
      entry.flattened = flattenLikeFigma(reference, actual);
      const compare = spawnSync(VISUAL, ["compare", "--reference", reference, "--actual", actual, "--diff", diff, "--channel-tolerance", "24"], { cwd: HARNESS, encoding: "utf8" });
      entry.environmentStatus = "valid";
      entry.compare = { exit: compare.status, stdout: compare.stdout.trim(), stderr: compare.stderr.trim() };
      try { entry.metrics = JSON.parse(compare.stdout); } catch {}
      report.push(entry);
      const summary = entry.metrics ? `${entry.metrics.status} changed ${(entry.metrics.changedRatio * 100).toFixed(2)}% (max channel delta ${entry.metrics.maxChannelDelta})` : (compare.stdout.trim() || compare.stderr.trim());
      const canvas = entry.flattened ? ` on canvas rgb(${entry.flattened.canvas.join(",")})` : "";
      console.log(`${target.name}: ${size.width}x${size.height} rendered ${entry.rendered.width}x${entry.rendered.height}${canvas} -> ${summary}`);
    }
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
