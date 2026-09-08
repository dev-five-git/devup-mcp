"""What `$primary` resolves to for one frame, at node scope and at file scope.

The harness renders with a file-scope theme so that every typography token a
screen uses is defined. This file holds several brands' collections, though,
and more than one defines `primary` - so file scope has to pick one. This
prints what each scope resolves, and what it reported while doing it.
"""

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "scripts"))
from acquire import HARNESS, Server, url_of  # noqa: E402

FRAME = sys.argv[1] if len(sys.argv) > 1 else "422:6865"
TOKENS = ("primary", "containerBackground", "background", "text", "border")


def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    server = Server()
    try:
        for scope in ("node", "file"):
            out = f"out/theme-{scope}-{FRAME.replace(':', '-')}.json"
            body = server.export({"url": url_of(FRAME), "outputs": ["devupJson"], "scope": scope,
                                  "outputPaths": {"devupJson": out}, "includeDiagnostics": True})
            path = os.path.join(HARNESS, out)
            if not os.path.exists(path):
                print(f"{scope}: no theme written ({body.get('status')})")
                continue
            colors = json.load(open(path, encoding="utf-8"))["theme"].get("colors", {})
            modes = list(colors)
            first = colors.get(modes[0], {}) if modes else {}
            print(f"{scope} scope: status={body.get('status')} modes={modes}")
            print(f"   counts={body.get('themeCounts')}")
            print("   " + "  ".join(f"{name}={first.get(name)}" for name in TOKENS))
            conflicts = [d for d in (body.get("diagnostics") or [])
                         if "CONFLICT" in (d.get("code") or "")]
            shown = [d for d in conflicts if "primary" in json.dumps(d, ensure_ascii=False)]
            print(f"   conflicts={len(conflicts)} mentioning primary={len(shown)}")
            for diagnostic in shown[:4]:
                print(f"     {diagnostic.get('message')}")
    finally:
        server.close()


if __name__ == "__main__":
    main()
