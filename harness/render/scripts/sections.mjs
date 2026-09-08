// Section heights, DOM against Figma, walking the screen's root and its
// direct children level by level. Where a section is taller than Figma drew
// it, the page below it drifts by that much; this says which one and by how
// much, before any crop is looked at.
//
//   node scripts/sections.mjs <target> [depth]

import { spawn, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const HARNESS = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 4214;

async function main() {
  const [name, depthArg] = process.argv.slice(2);
  const depth = Number(depthArg ?? 2);
  const manifest = JSON.parse(readFileSync(join(HARNESS, "targets.json"), "utf8"));
  const target = manifest.targets.find((entry) => entry.name === name);
  if (!target) throw new Error(`no target ${name}`);
  if (target.theme && existsSync(join(HARNESS, target.theme))) {
    writeFileSync(join(HARNESS, "devup.json"), readFileSync(join(HARNESS, target.theme)));
  }
  const frame = target.frame;
  const snapshot = JSON.parse(readFileSync(join(HARNESS, `out/${target.name}-${frame.replace(":", "-")}.snapshot.json`), "utf8"));
  const nodes = snapshot.nodes;
  const rootBox = nodes[frame].fields.absoluteBoundingBox;

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
    // The DOM tree of element boxes, depth-limited, in document order.
    const dom = await page.evaluate((depth) => {
      const root = document.getElementById("root").firstElementChild;
      const walk = (el, level) => {
        const r = el.getBoundingClientRect();
        const cs = getComputedStyle(el);
        const out = {
          level, tag: el.tagName.toLowerCase(),
          y: Math.round((r.top + window.scrollY) * 10) / 10, h: Math.round(r.height * 10) / 10,
          w: Math.round(r.width * 10) / 10,
          pos: cs.position, text: (el.innerText || "").trim().slice(0, 24).replace(/\s+/g, " "),
          children: [],
        };
        if (level < depth) for (const child of el.children) out.children.push(walk(child, level + 1));
        return out;
      };
      return walk(root, 0);
    }, depth);

    // Figma's tree, same shape, y relative to the root frame.
    const figma = (id, level) => {
      const node = nodes[id];
      const f = node.fields;
      const b = f.absoluteBoundingBox ?? { x: 0, y: 0, width: 0, height: 0 };
      const out = {
        level, id, type: node.type, name: f.name,
        y: Math.round((b.y - rootBox.y) * 10) / 10, h: Math.round(b.height * 10) / 10, w: Math.round(b.width * 10) / 10,
        pos: f.layoutPositioning, visible: f.visible !== false,
        children: [],
      };
      if (level < depth) for (const child of f.childrenIds ?? []) if (nodes[child]) out.children.push(figma(child, level + 1));
      return out;
    };
    const fig = figma(frame, 0);

    // Print side by side by index at each level: the code emits one element
    // per visible in-flow node, so index order is the only join available.
    const print = (d, f, indent) => {
      const pad = "  ".repeat(indent);
      const dy = d && f ? (d.y - f.y).toFixed(1) : "";
      const dh = d && f ? (d.h - f.h).toFixed(1) : "";
      const flag = d && f && Math.abs(d.h - f.h) > 1 ? "  <-- height differs" : "";
      const left = d ? `${d.tag.padEnd(4)} y=${String(d.y).padStart(7)} h=${String(d.h).padStart(7)}` : " ".repeat(27);
      const right = f ? `${(f.type ?? "").padEnd(9)} y=${String(f.y).padStart(7)} h=${String(f.h).padStart(7)} ${f.name}` : "(no figma node)";
      console.log(`${pad}${left} | ${right}  Δy=${dy} Δh=${dh}${flag}`);
      const dc = d?.children ?? [];
      const fc = (f?.children ?? []).filter((c) => c.visible);
      const n = Math.max(dc.length, fc.length);
      for (let i = 0; i < n; i += 1) print(dc[i], fc[i], indent + 1);
    };
    console.log(`${target.name}: DOM | Figma`);
    print(dom, fig, 0);
  } finally {
    await browser.close();
    if (process.platform === "win32") spawnSync("taskkill", ["/PID", String(preview.pid), "/T", "/F"], { stdio: "ignore" });
    else preview.kill();
  }
}

main().catch((error) => { console.error(error); process.exit(1); });
