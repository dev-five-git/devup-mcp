"""Every asset a module points at, against the manifest that says where they are.

The generated code refers to an asset by path; the manifest is what tells a
caller to export it. Anything the code points at that the manifest does not
list can never be written, and the screen renders with a hole where the
picture should be.
"""

import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "scripts"))
from acquire import HARNESS, Server, url_of  # noqa: E402


def referenced(module_path):
    source = open(os.path.join(HARNESS, module_path), encoding="utf-8").read()
    return sorted(set(re.findall(r"/(?:icons|images)/[^\"')]+", source)))


def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    manifest = json.load(open(os.path.join(HARNESS, "targets.json"), encoding="utf-8"))
    wanted = sys.argv[1:]
    server = Server()
    try:
        for target in manifest["targets"]:
            if wanted and not any(target["name"].startswith(name) for name in wanted):
                continue
            body = server.export({"url": url_of(target["frame"]), "outputs": ["assetManifest"],
                                  "scope": "node", "delivery": "inline"}, allow_error=True)
            if body.get("error"):
                print(f"{target['name']}: manifest refused: {body['error']}")
                continue
            assets = (body.get("assetManifest") or {}).get("assets", [])
            listed = {asset.get("path") for asset in assets if asset.get("path")}
            by_status = {}
            for asset in assets:
                by_status[asset.get("status")] = by_status.get(asset.get("status"), 0) + 1
            points_at = referenced(target["module"])
            unlisted = [path for path in points_at if path not in listed]
            absent = [path for path in points_at if not os.path.exists(os.path.join(HARNESS, "public" + path))]
            print(f"{target['name']}: code points at {len(points_at)}, manifest lists {len(listed)} {by_status}")
            for path in unlisted:
                print(f"   not in the manifest: {path}")
            for path in absent:
                if path not in unlisted:
                    print(f"   listed but not on disk: {path}")
    finally:
        server.close()


if __name__ == "__main__":
    main()
