// Which element is on top at a point, and the stacking-relevant styles of an
// <img> and its ancestors. A one-off probe for a picture that is placed but
// not seen.
//
//   node scripts/probe_stack.mjs <target> <imgSrcFragment> <x> <y>

import { spawn, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const HARNESS = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 4213;

async function main() {
  const [name, fragment, xArg, yArg] = process.argv.slice(2);
  const manifest = JSON.parse(readFileSync(join(HARNESS, "targets.json"), "utf8"));
  const target = manifest.targets.find((entry) => entry.name === name);
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
      try { if ((await fetch(`http://localhost:${PORT}/`)).ok) break; } catch {}
      await new Promise((r) => setTimeout(r, 200));
    }
    const bytes = readFileSync(join(HARNESS, target.reference));
    const size = { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
    const context = await browser.newContext({ viewport: size, deviceScaleFactor: 1, locale: "ko-KR" });
    const page = await context.newPage();
    await page.goto(`http://localhost:${PORT}/?screen=${encodeURIComponent(target.screen ?? target.name)}`, { waitUntil: "networkidle" });
    await page.waitForSelector("body[data-ready]", { timeout: 30000 });
    const report = await page.evaluate(([fragment, x, y]) => {
      const img = [...document.querySelectorAll("img")].find((i) => i.getAttribute("src")?.includes(fragment));
      if (!img) return { error: "img not found" };
      const chain = [];
      let node = img;
      while (node && node !== document.body) {
        const cs = getComputedStyle(node);
        chain.push({
          tag: node.tagName.toLowerCase(),
          position: cs.position, zIndex: cs.zIndex, opacity: cs.opacity,
          overflow: cs.overflow, isolation: cs.isolation, transform: cs.transform,
          bg: cs.backgroundColor, display: cs.display,
        });
        node = node.parentElement;
      }
      window.scrollTo(0, Math.max(0, y - 300));
      const rect = img.getBoundingClientRect();
      const probeX = rect.left + rect.width / 2;
      const probeY = rect.top + rect.height / 2;
      const top = document.elementFromPoint(probeX, probeY);
      return {
        imgRect: { x: rect.left + window.scrollX, y: rect.top + window.scrollY, w: rect.width, h: rect.height },
        naturalSize: { w: img.naturalWidth, h: img.naturalHeight, complete: img.complete },
        topAtCentre: top ? { tag: top.tagName.toLowerCase(), cls: top.className, isImg: top === img, isAncestor: top.contains(img) } : null,
        chain,
      };
    }, [fragment, Number(xArg ?? 0), Number(yArg ?? 0)]);
    console.log(JSON.stringify(report, null, 2));
  } finally {
    await browser.close();
    if (process.platform === "win32") spawnSync("taskkill", ["/PID", String(preview.pid), "/T", "/F"], { stdio: "ignore" });
    else preview.kill();
  }
}

main().catch((error) => { console.error(error); process.exit(1); });
