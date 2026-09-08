// Where a screen's difference actually is.
//
//   node scripts/bands.mjs <target> [bandHeight]
//
// Compares the reference and the capture row band by row band and prints the
// bands that differ most, then writes the worst ones as a side-by-side crop
// (reference left, capture right) so the cause can be seen rather than
// guessed at. A tall screen shrunk to fit a screenshot shows nothing; a band
// of it at full size shows everything.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";

const HARNESS = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const TOLERANCE = 24;

function changedAt(reference, actual, index) {
  for (const channel of [0, 1, 2]) {
    if (Math.abs(reference.data[index + channel] - actual.data[index + channel]) > TOLERANCE) return true;
  }
  return false;
}

function main() {
  const [name, bandArgument, cropArgument] = process.argv.slice(2);
  if (!name) throw new Error("usage: node scripts/bands.mjs <target> [bandHeight] [crops]");
  const band = Number(bandArgument ?? 100);
  const crops = Number(cropArgument ?? 2);
  const reference = PNG.sync.read(readFileSync(join(HARNESS, "out", `${name}.reference.png`)));
  const actual = PNG.sync.read(readFileSync(join(HARNESS, "out", `${name}.actual.png`)));
  if (reference.width !== actual.width || reference.height !== actual.height) {
    console.log(`sizes differ: reference ${reference.width}x${reference.height}, actual ${actual.width}x${actual.height}`);
  }
  const height = Math.min(reference.height, actual.height);
  const width = Math.min(reference.width, actual.width);

  const bands = [];
  for (let top = 0; top < height; top += band) {
    const bottom = Math.min(top + band, height);
    let changed = 0;
    for (let y = top; y < bottom; y += 1) {
      for (let x = 0; x < width; x += 1) {
        if (changedAt(reference, actual, (y * reference.width + x) * 4)) changed += 1;
      }
    }
    bands.push({ top, bottom, ratio: changed / ((bottom - top) * width) });
  }

  const total = bands.reduce((sum, entry) => sum + entry.ratio * (entry.bottom - entry.top), 0) / height;
  console.log(`${name}: ${width}x${height}, changed ${(total * 100).toFixed(2)}%`);
  for (const entry of [...bands].sort((left, right) => right.ratio - left.ratio).slice(0, 12)) {
    const bar = "#".repeat(Math.round(entry.ratio * 40));
    console.log(`  y ${String(entry.top).padStart(5)}-${String(entry.bottom).padStart(5)}  ${(entry.ratio * 100).toFixed(2).padStart(6)}%  ${bar}`);
  }

  for (const entry of [...bands].sort((left, right) => right.ratio - left.ratio).slice(0, crops)) {
    const tall = entry.bottom - entry.top;
    const sheet = new PNG({ width: width * 2 + 8, height: tall });
    sheet.data.fill(255);
    for (let y = 0; y < tall; y += 1) {
      for (let x = 0; x < width; x += 1) {
        const from = ((entry.top + y) * reference.width + x) * 4;
        for (const [source, offset] of [[reference, 0], [actual, width + 8]]) {
          const to = (y * sheet.width + x + offset) * 4;
          for (const channel of [0, 1, 2, 3]) sheet.data[to + channel] = source.data[from + channel];
        }
      }
    }
    const out = join(HARNESS, "out", `${name}.band-${entry.top}.png`);
    writeFileSync(out, PNG.sync.write(sheet));
    console.log(`  wrote out/${name}.band-${entry.top}.png (reference | capture)`);
  }
}

main();
