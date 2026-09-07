// The DOM boxes down the top of a screen, to compare against Figma's own.
//
//   node scripts/boxes.mjs <target> [maxY]

import { spawn, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const HARNESS = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 4206;

async function main() {
  const [name, maxArgument] = process.argv.slice(2);
  const maxY = Number(maxArgument ?? 700);
  const manifest = JSON.parse(readFileSync(join(HARNESS, "targets.json"), "utf8"));
  const target = manifest.targets.find((entry) => entry.name === name || entry.name.startsWith(`${name}-`));
  if (!target) throw new Error(`no target ${name}`);
  if (target.theme && existsSync(join(HARNESS, target.theme))) {
    writeFileSync(join(HARNESS, "devup.json"), readFileSync(join(HARNESS, target.theme)));
  }
  const build = spawnSync("npx", ["vite", "build"], { cwd: HARNESS, encoding: "utf8", shell: process.platform === "win32" });
  if (build.status !== 0) throw new Error(`vite build failed:\n${build.stderr}`);
  const preview = spawn("npx", ["vite", "preview", "--port", String(PORT), "--strictPort"], { cwd: HARNESS, shell: process.platform === "win32", stdio: "ignore" });
  const browser = await chromium.launch();
  try {
    for (let attempt = 0; attempt < 50; attempt += 1) {
      try {
        if ((await fetch(`http://localhost:${PORT}/`)).ok) break;
      } catch {}
      await new Promise((done) => setTimeout(done, 200));
    }
    const bytes = readFileSync(join(HARNESS, target.reference));
    const page = await browser.newPage({
      viewport: { width: bytes.readUInt32BE(16), height: Math.min(bytes.readUInt32BE(20), 2000) },
    });
    const screen = target.screen ?? target.name;
    await page.goto(`http://localhost:${PORT}/?screen=${encodeURIComponent(screen)}`, { waitUntil: "networkidle" });
    await page.waitForSelector("body[data-ready]", { timeout: 30000 });
    const rows = await page.evaluate((limit) => {
      const out = [];
      const walk = (element, depth) => {
        const rect = element.getBoundingClientRect();
        const top = rect.y + window.scrollY;
        if (top > limit) return;
        const style = getComputedStyle(element);
        out.push({
          depth,
          tag: element.tagName.toLowerCase(),
          box: `${Math.round(rect.width)}x${Math.round(rect.height)}`,
          at: `${Math.round(rect.x)},${Math.round(top)}`,
          display: style.display,
          position: style.position,
          padding: style.padding,
          justify: style.justifyContent,
          align: style.alignItems,
          text: (element.childElementCount === 0 ? (element.textContent ?? "").trim().slice(0, 18) : ""),
        });
        for (const child of element.children) walk(child, depth + 1);
      };
      walk(document.getElementById("root"), 0);
      return out;
    }, maxY);
    console.log(`${target.name}: boxes down to y=${maxY}`);
    for (const row of rows) {
      console.log(
        `  ${"  ".repeat(row.depth)}${row.tag} ${row.box} @${row.at} ${row.display}/${row.position}` +
          `${row.padding !== "0px" ? ` pad=${row.padding}` : ""}` +
          `${row.justify !== "normal" ? ` just=${row.justify}` : ""}` +
          `${row.align !== "normal" ? ` align=${row.align}` : ""}` +
          `${row.text ? ` "${row.text}"` : ""}`,
      );
    }
  } finally {
    await browser.close();
    if (preview.pid) spawnSync("taskkill", ["/PID", String(preview.pid), "/T", "/F"], { stdio: "ignore" });
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
