"""What the response actually carried for the asset export that was refused."""

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "scripts"))
from acquire import Server, TARGETS, url_for  # noqa: E402

ASSET = sys.argv[1] if len(sys.argv) > 1 else "793:8340:fills:0"
FRAME = sys.argv[4] if len(sys.argv) > 4 else TARGETS["landing"]["frames"][0]
FORMAT = sys.argv[2] if len(sys.argv) > 2 else "png"
SCALE = int(sys.argv[3]) if len(sys.argv) > 3 else 1


def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    target = TARGETS["landing"]
    server = Server()
    try:
        response = server.call_raw("tools/call", {
            "name": "devup_figma_export",
            "arguments": {
                "url": url_for(target, FRAME), "outputs": ["assetManifest"], "scope": "node",
                "assetRequests": [{"assetId": ASSET, "format": FORMAT, "scale": SCALE}], "refresh": True,
                "delivery": "inline", "includeDiagnostics": True,
            },
        })
    finally:
        server.close()
    if "error" in response:
        error = response["error"]
        print("  message:", error.get("message"))
        data = error.get("data") or {}
        print("  data:", json.dumps(data, ensure_ascii=False, indent=2)[:3000])
        return
    dump = os.path.join(os.environ.get("TEMP", "."), "opencode", "probe.json")
    with open(dump, "w", encoding="utf-8") as handle:
        json.dump(response, handle, ensure_ascii=False)
    print("  raw response saved to", dump)
    result = response["result"]
    print("  isError:", result.get("isError"), "content kinds:", [p.get("type") for p in result.get("content", [])])
    text = "".join(p.get("text", "") for p in result.get("content", []) if p.get("type") == "text")
    try:
        body = json.loads(text)
    except json.JSONDecodeError:
        print("  text is not JSON:", text[:1500])
        return
    print("  body keys:", list(body.keys()))
    manifest = body.get("assetManifest") or {}
    assets = manifest.get("assets", [])
    print("  assets:", len(assets), "diagnostics:", json.dumps(body.get("diagnostics"), ensure_ascii=False)[:1500])
    for asset in assets:
        print("  -", asset.get("assetId"), asset.get("status"), "bytes:", asset.get("byteLength"), "mime:", asset.get("mimeType"), "error:", asset.get("errorCode"), "path:", asset.get("path"), "hasData:", bool(asset.get("dataBase64")))
    stats = body.get("stats") or manifest.get("stats")
    print("  stats:", json.dumps(stats, ensure_ascii=False)[:800])


if __name__ == "__main__":
    main()
