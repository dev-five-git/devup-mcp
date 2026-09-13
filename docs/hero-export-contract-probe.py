"""Inspect acquired about assets without altering the design or render inputs.

Usage: python docs/hero-export-contract-probe.py harness/render OUTPUT.json
The result diagnoses whole-node exports; it does not certify isolated pixels.
"""

import hashlib
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def changed(first, second):
    a = np.asarray(first.convert("RGB"), dtype=np.int16)
    b = np.asarray(second.convert("RGB"), dtype=np.int16)
    return int(np.any(np.abs(a - b) > 24, axis=2).sum())


root = Path(sys.argv[1])
manifest = json.loads((root / "targets.json").read_text(encoding="utf-8"))
report = {"tolerance": 24, "hero": [], "crop": []}
for target in manifest["targets"]:
    if not target["name"].startswith("about-"):
        continue
    paths = target["acquisition"]["files"]
    snapshot_path = next(p for p in paths if p.endswith(".snapshot.json"))
    snapshot = json.loads((root / snapshot_path).read_text(encoding="utf-8"))
    nodes = snapshot["nodes"]
    frame = nodes[target["frame"]]["fields"]["absoluteBoundingBox"]
    reference_path = root / target["reference"]
    reference = Image.open(reference_path).convert("RGBA")
    report.setdefault("binarySha256", target["acquisition"]["binarySha256"])
    # Membership follows the requested frame's subtree, not all roots that a
    # section-scoped snapshot may also carry.
    pending = [target["frame"]]
    visited = set()
    while pending:
        node_id = pending.pop()
        if node_id in visited:
            continue
        visited.add(node_id)
        fields = nodes[node_id]["fields"]
        pending.extend(fields.get("childrenIds") or [])
        fills = fields.get("fills") or []
        if not isinstance(fills, list):
            continue
        images = [(i, p) for i, p in enumerate(fills)
                  if p.get("type") == "IMAGE" and p.get("visible") is not False]
        for index, paint in images:
            asset_id = f"{node_id}:fills:{index}"
            asset_paths = [p for p, value in manifest["assets"].items()
                           if value == asset_id and p in paths]
            if not asset_paths:
                continue
            bounds = fields["absoluteBoundingBox"]
            x, y = round(bounds["x"] - frame["x"]), round(bounds["y"] - frame["y"])
            width, height = round(bounds["width"]), round(bounds["height"])
            asset_path = root / asset_paths[0]
            asset = Image.open(asset_path).convert("RGBA")
            region = reference.crop((x, y, x + width, y + height))
            if asset.size != region.size:
                continue
            flattened = Image.new("RGBA", asset.size, "white")
            flattened.alpha_composite(asset)
            record = {
                "screen": target["name"], "nodeId": node_id,
                "assetId": asset_id, "assetPath": asset_paths[0],
                "bounds": [x, y, width, height], "fills": fills,
                "childIds": fields.get("childrenIds") or [],
                "assetSha256": sha(asset_path), "referenceSha256": sha(reference_path),
                "exportVsReferenceChangedPixels": changed(flattened, region),
            }
            actual_path = root / "out" / f"{target['name']}.actual.png"
            if actual_path.exists():
                actual = Image.open(actual_path).convert("RGBA")
                record["actualSha256"] = sha(actual_path)
                record["actualVsReferenceChangedPixels"] = changed(
                    actual.crop((x, y, x + width, y + height)), region)
            if record["childIds"] and any(p.get("type") == "SOLID" for p in fills):
                report["hero"].append(record)
            elif paint.get("scaleMode") == "CROP":
                report["crop"].append(record)

Path(sys.argv[2]).write_text(json.dumps(report, indent=2), encoding="utf-8")
print(json.dumps({"hero": [(r["nodeId"], r["exportVsReferenceChangedPixels"]) for r in report["hero"]],
                  "crop": [(r["nodeId"], r["exportVsReferenceChangedPixels"]) for r in report["crop"]]}, indent=2))
