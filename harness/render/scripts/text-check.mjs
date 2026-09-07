// Does the screen print the words the design says?
//
//   node scripts/text-check.mjs [target ...]
//
// The generator writes a text node's characters into JSX, and JSX has rules
// of its own about whitespace: a run of text broken across two source lines
// is joined with a space. Where the design has no space there, the screen
// prints a word that is not in the design, and the paragraph wraps somewhere
// Figma never wraps it.
//
// Reading the JSX to work out what it renders means implementing those rules,
// and guessing at them is how a check invents faults that are not there. The
// browser already implements them exactly, so this asks the rendered page
// instead: every `characters` string of every text node under the frame has
// to appear in what the page prints.

import { spawn, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const HARNESS = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 4208;

/// Whitespace the same way on both sides, so only a real difference shows.
/// Figma separates lines with U+2028; the generator writes those as breaks,
/// which the page prints as newlines.
function normalise(text) {
  return text
    .replace(/[\u2028\u2029\r\n]+/g, "\n")
    .replace(/[ \t\u00a0]+/g, " ")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .join("\n")
    .trim();
}

/// Every text a node under `root` holds, in the frame the screen renders.
function charactersUnder(snapshot, root) {
  const nodes = snapshot.nodes ?? {};
  const found = [];
  const pending = [root];
  const seen = new Set();
  while (pending.length > 0) {
    const id = pending.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    const node = nodes[id];
    if (!node) continue;
    const fields = node.fields ?? {};
    if (typeof fields.characters === "string" && fields.characters.trim().length > 0) {
      found.push({ id, text: fields.characters });
    }
    for (const child of fields.childrenIds ?? []) pending.push(child);
  }
  return found;
}

async function main() {
  const wanted = process.argv.slice(2);
  const manifest = JSON.parse(readFileSync(join(HARNESS, "targets.json"), "utf8"));
  const targets = manifest.targets.filter(
    (target) => wanted.length === 0 || wanted.some((name) => target.name === name || target.name.startsWith(`${name}-`)),
  );
  if (targets.length === 0) throw new Error("no targets; run scripts/acquire.py first");

  const browser = await chromium.launch();
  let checked = 0;
  let missing = 0;
  try {
    for (const [index, target] of targets.entries()) {
      const snapshotPath = join(
        HARNESS,
        "out",
        `${target.screen ?? target.name}-${target.frame.replace(":", "-")}.snapshot.json`,
      );
      if (!existsSync(snapshotPath)) {
        console.log(`${target.name}: no snapshot on disk; skipping`);
        continue;
      }
      const snapshot = JSON.parse(readFileSync(snapshotPath, "utf8"));
      const texts = charactersUnder(snapshot, target.frame);
      if (texts.length === 0) continue;

      if (target.theme && existsSync(join(HARNESS, target.theme))) {
        writeFileSync(join(HARNESS, "devup.json"), readFileSync(join(HARNESS, target.theme)));
      }
      const build = spawnSync("npx", ["vite", "build"], { cwd: HARNESS, encoding: "utf8", shell: process.platform === "win32" });
      if (build.status !== 0) throw new Error(`vite build failed:\n${build.stderr}`);
      const port = PORT + (index % 8);
      const preview = spawn("npx", ["vite", "preview", "--port", String(port), "--strictPort"], { cwd: HARNESS, shell: process.platform === "win32", stdio: "ignore" });
      try {
        let serving = false;
        for (let attempt = 0; attempt < 150 && !serving; attempt += 1) {
          try {
            serving = (await fetch(`http://localhost:${port}/`)).ok;
          } catch {}
          if (!serving) await new Promise((done) => setTimeout(done, 200));
        }
        if (!serving) throw new Error(`preview did not answer on ${port}`);
        const bytes = readFileSync(join(HARNESS, target.reference));
        const page = await browser.newPage({
          viewport: { width: bytes.readUInt32BE(16), height: Math.min(bytes.readUInt32BE(20), 2000) },
        });
        await page.goto(`http://localhost:${port}/?screen=${encodeURIComponent(target.screen ?? target.name)}`, { waitUntil: "networkidle" });
        await page.waitForSelector("body[data-ready]", { timeout: 30000 });
        const printed = normalise(await page.evaluate(() => document.getElementById("root")?.innerText ?? ""));
        await page.close();

        const absent = texts.filter(({ text }) => !printed.includes(normalise(text)));
        checked += texts.length;
        missing += absent.length;
        console.log(`${target.name}: ${texts.length} texts, ${absent.length} not printed as the design says`);
        for (const { id, text } of absent.slice(0, 4)) {
          const wanted = normalise(text);
          // The first place the two part company, to keep the report short.
          let prefix = wanted;
          while (prefix.length > 12 && !printed.includes(prefix)) prefix = prefix.slice(0, -1);
          console.log(`   ${id}: design has ...${wanted.slice(Math.max(0, prefix.length - 14), prefix.length + 14)}...`);
        }
      } finally {
        if (preview.pid) spawnSync("taskkill", ["/PID", String(preview.pid), "/T", "/F"], { stdio: "ignore" });
      }
    }
  } finally {
    await browser.close();
  }
  console.log(`\n${missing} of ${checked} texts are not printed as the design says`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
