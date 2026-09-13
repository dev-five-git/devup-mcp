import { createHash } from "node:crypto";
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

export function validateInputs(directory, target, binarySha256) {
  const evidence = target.acquisition;
  if (!evidence?.binarySha256 || !evidence.files || !evidence.files[target.theme]) {
    throw new Error(`${target.name}: no acquisition evidence; reacquire this screen`);
  }
  if (binarySha256 && evidence.binarySha256 !== binarySha256) {
    throw new Error(`${target.name}: binary differs from acquisition; reacquire this screen`);
  }
  for (const [path, expected] of Object.entries(evidence.files)) {
    const absolute = join(directory, path);
    if (!existsSync(absolute)) throw new Error(`${target.name}: ${path} missing; reacquire this screen`);
    const actual = createHash("sha256").update(readFileSync(absolute)).digest("hex");
    if (actual !== expected) throw new Error(`${target.name}: ${path} changed since acquisition; reacquire this screen`);
  }
}
