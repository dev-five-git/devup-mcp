// What size each picture actually comes out at, against the box Figma drew.
//
//   node scripts/elements.mjs <screen> [theme]
//
// Builds the harness with the screen's own theme, opens it at the reference
// width and reports every <img> and every element painted with a background
// image: where it sits, how big it came out, and - for an <img> - the
// intrinsic size the browser fell back on. An image given only a height takes
// its width from the file, which is the box of whichever width's node the
// file happened to be exported from, not the box on this page.

import { spawn, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const HARNESS = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 4199;

async function main() {
  const [name] = process.argv.slice(2);
  const manifest = JSON.parse(readFileSync(join(HARNESS, "targets.json"), "utf8"));
  const target = manifest.targets.find((entry) => entry.name === name || entry.name.startsWith(`${name}-`));
  if (!target) throw new Error(`no target ${name}`);
  const theme = target.theme && existsSync(join(HARNESS, target.theme)) ? target.theme : null;
  if (theme) writeFileSync(join(HARNESS, "devup.json"), readFileSync(join(HARNESS, theme)));
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
    const size = { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
    const context = await browser.newContext({ viewport: size, deviceScaleFactor: 1, locale: "ko-KR" });
    const page = await context.newPage();
    const screen = target.screen ?? target.name;
    await page.goto(`http://localhost:${PORT}/?screen=${encodeURIComponent(screen)}`, { waitUntil: "networkidle" });
    await page.waitForSelector("body[data-ready]", { timeout: 30000 });
    const found = await page.evaluate(() => {
      const rows = [];
      for (const element of document.querySelectorAll("img")) {
        const rect = element.getBoundingClientRect();
        rows.push({
          kind: "img",
          source: element.getAttribute("src"),
          box: `${Math.round(rect.width)}x${Math.round(rect.height)}`,
          at: `${Math.round(rect.x)},${Math.round(rect.y + window.scrollY)}`,
          intrinsic: `${element.naturalWidth}x${element.naturalHeight}`,
          objectFit: getComputedStyle(element).objectFit,
          parent: (() => {
            const parentRect = element.parentElement?.getBoundingClientRect();
            return parentRect ? `${Math.round(parentRect.width)}x${Math.round(parentRect.height)}` : "?";
          })(),
        });
      }
      for (const element of document.querySelectorAll("*")) {
        const style = getComputedStyle(element);
        if (!style.backgroundImage.includes("url(")) continue;
        const rect = element.getBoundingClientRect();
        rows.push({
          kind: "bg",
          source: (style.backgroundImage.match(/url\("?([^")]+)"?\)/) ?? [])[1],
          box: `${Math.round(rect.width)}x${Math.round(rect.height)}`,
          at: `${Math.round(rect.x)},${Math.round(rect.y + window.scrollY)}`,
          intrinsic: style.backgroundSize,
          objectFit: style.backgroundPosition,
          parent: "-",
        });
      }
      return rows;
    });
    console.log(`${target.name} at ${size.width}x${size.height}`);
    console.log(`  ${"kind".padEnd(4)} ${"box".padEnd(11)} ${"at".padEnd(11)} ${"intrinsic/size".padEnd(16)} ${"fit/pos".padEnd(18)} source`);
    for (const row of found) {
      console.log(`  ${row.kind.padEnd(4)} ${row.box.padEnd(11)} ${row.at.padEnd(11)} ${row.intrinsic.padEnd(16)} ${String(row.objectFit).padEnd(18)} ${decodeURIComponent(row.source ?? "")}`);
    }
    await context.close();
  } finally {
    await browser.close();
    if (preview.pid) spawnSync("taskkill", ["/PID", String(preview.pid), "/T", "/F"], { stdio: "ignore" });
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
