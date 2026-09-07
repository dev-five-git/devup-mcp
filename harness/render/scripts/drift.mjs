// How far a capture has drifted from the reference, band by band.
//
//   node scripts/drift.mjs <target> [bandHeight] [maxOffset]
//
// For each band of rows, finds the vertical offset that best lines the
// capture up with the reference. A band that matches at offset 0 is in the
// right place; a band that only matches at -24 is sitting 24px too high. The
// first band that drifts is where the height went wrong - everything below
// it inherits the same shift, which a plain pixel ratio reports as a whole
// page of differences.

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";

const HARNESS = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const STEP = 3;

function scoreAt(reference, actual, top, bottom, offset, width) {
  let changed = 0;
  let counted = 0;
  for (let y = top; y < bottom; y += STEP) {
    const from = y + offset;
    if (from < 0 || from >= actual.height) continue;
    for (let x = 0; x < width; x += STEP) {
      const referenceIndex = (y * reference.width + x) * 4;
      const actualIndex = (from * actual.width + x) * 4;
      counted += 1;
      for (const channel of [0, 1, 2]) {
        if (Math.abs(reference.data[referenceIndex + channel] - actual.data[actualIndex + channel]) > 24) {
          changed += 1;
          break;
        }
      }
    }
  }
  return counted === 0 ? 1 : changed / counted;
}

function main() {
  const [name, bandArgument, maxArgument] = process.argv.slice(2);
  const band = Number(bandArgument ?? 100);
  const maxOffset = Number(maxArgument ?? 60);
  const reference = PNG.sync.read(readFileSync(join(HARNESS, "out", `${name}.reference.png`)));
  const actual = PNG.sync.read(readFileSync(join(HARNESS, "out", `${name}.actual.png`)));
  const width = Math.min(reference.width, actual.width);
  const height = Math.min(reference.height, actual.height);

  console.log(`${name}: reference ${reference.width}x${reference.height}, capture ${actual.width}x${actual.height}`);
  console.log("  band            best  at        at 0   verdict");
  let previous = 0;
  for (let top = 0; top < height; top += band) {
    const bottom = Math.min(top + band, height);
    let best = { offset: 0, ratio: Infinity };
    for (let offset = -maxOffset; offset <= maxOffset; offset += 1) {
      const ratio = scoreAt(reference, actual, top, bottom, offset, width);
      if (ratio < best.ratio) best = { offset, ratio };
    }
    const atZero = scoreAt(reference, actual, top, bottom, 0, width);
    const moved = best.offset !== previous ? `  <- drift ${previous} to ${best.offset}` : "";
    const verdict = best.ratio < 0.02 ? "aligned" : best.ratio < 0.1 ? "close" : "differs";
    console.log(
      `  ${String(top).padStart(5)}-${String(bottom).padStart(5)}  ${(best.ratio * 100).toFixed(1).padStart(5)}%  ${String(best.offset).padStart(4)}  ${(atZero * 100).toFixed(1).padStart(5)}%  ${verdict}${moved}`,
    );
    previous = best.offset;
  }
}

main();
