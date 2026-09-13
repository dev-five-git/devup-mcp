import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { validateInputs } from "./inputs.mjs";

test("a replaced old theme is rejected before rendering", () => {
  const directory = mkdtempSync(join(tmpdir(), "harness-inputs-"));
  try {
    writeFileSync(join(directory, "theme.json"), "old theme");
    const target = { name: "about", theme: "theme.json", acquisition: {
      binarySha256: "binary", files: { "theme.json": createHash("sha256").update("new theme").digest("hex") },
    } };
    assert.throws(() => validateInputs(directory, target), /theme.json.*changed/);
    writeFileSync(join(directory, "theme.json"), "new theme");
    assert.doesNotThrow(() => validateInputs(directory, target));
    assert.throws(() => validateInputs(directory, target, "different binary"), /binary/);
  } finally {
    rmSync(directory, { recursive: true });
  }
});

test("an old manifest without acquisition evidence requires reacquisition", () => {
  assert.throws(() => validateInputs(".", { name: "popup", theme: "themes/popup.json" }), /reacquire/);
});
