// A named slice of a screen, reference beside capture.
//
//   node scripts/crop.mjs <target> <top> [height]

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";

const HARNESS = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function main() {
  const [name, topArgument, heightArgument] = process.argv.slice(2);
  const top = Number(topArgument ?? 0);
  const tall = Number(heightArgument ?? 200);
  const reference = PNG.sync.read(readFileSync(join(HARNESS, "out", `${name}.reference.png`)));
  const actual = PNG.sync.read(readFileSync(join(HARNESS, "out", `${name}.actual.png`)));
  const width = Math.min(reference.width, actual.width);
  const height = Math.min(tall, reference.height - top, actual.height - top);
  if (height <= 0) throw new Error(`nothing to crop at y=${top}`);
  const sheet = new PNG({ width: width * 2 + 8, height });
  sheet.data.fill(255);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      for (const [source, offset] of [[reference, 0], [actual, width + 8]]) {
        const from = ((top + y) * source.width + x) * 4;
        const to = (y * sheet.width + x + offset) * 4;
        for (const channel of [0, 1, 2, 3]) sheet.data[to + channel] = source.data[from + channel];
      }
    }
  }
  const out = join(HARNESS, "out", `${name}.crop-${top}.png`);
  writeFileSync(out, PNG.sync.write(sheet));
  console.log(`wrote out/${name}.crop-${top}.png (${width}x${height}, reference | capture)`);
}

main();
