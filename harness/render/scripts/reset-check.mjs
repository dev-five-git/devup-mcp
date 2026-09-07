// Whether the reset the generated code is written against is actually in
// force in the page, and what it settles.
//
//   node scripts/reset-check.mjs [screen]

import { spawn, spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const HARNESS = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 4205;

async function main() {
  const screen = process.argv[2] ?? "about-422-3376";
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
    const page = await browser.newPage({ viewport: { width: 360, height: 800 } });
    await page.goto(`http://localhost:${PORT}/?screen=${encodeURIComponent(screen)}`, { waitUntil: "networkidle" });
    await page.waitForSelector("body[data-ready]", { timeout: 30000 });
    const found = await page.evaluate(() => {
      const sheets = [...document.styleSheets].map((sheet) => sheet.href ?? "inline");
      let resetRules = 0;
      for (const sheet of document.styleSheets) {
        let rules;
        try {
          rules = sheet.cssRules;
        } catch {
          continue;
        }
        for (const rule of rules) {
          if (rule.cssText.includes("tab-size") || rule.cssText.includes(":where(:root)")) resetRules += 1;
        }
      }
      const image = document.querySelector("img");
      return {
        sheets,
        resetRules,
        rootLineHeight: getComputedStyle(document.documentElement).lineHeight,
        rootFontSize: getComputedStyle(document.documentElement).fontSize,
        bodyMargin: getComputedStyle(document.body).margin,
        imageDisplay: image ? getComputedStyle(image).display : "no image",
        imageVerticalAlign: image ? getComputedStyle(image).verticalAlign : "-",
        imageObjectFit: image ? getComputedStyle(image).objectFit : "-",
      };
    });
    console.log(`screen ${screen}`);
    for (const [key, value] of Object.entries(found)) {
      console.log(`  ${key}: ${Array.isArray(value) ? value.length + " sheets" : value}`);
    }
    for (const sheet of found.sheets) console.log(`    sheet: ${sheet}`);
  } finally {
    await browser.close();
    if (preview.pid) spawnSync("taskkill", ["/PID", String(preview.pid), "/T", "/F"], { stdio: "ignore" });
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
