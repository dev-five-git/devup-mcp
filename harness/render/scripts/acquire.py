"""Acquire what the render harness needs from a running devup-mcp.

For each target in TARGETS: the generated module, the theme, every asset the
module refers to, and Figma's own PNG of each frame at its drawn size. All of
it lands under the harness, none of it is committed.

  python scripts/acquire.py [target-name ...]

The server is `~/.cargo/bin/devup-mcp.exe` over stdio, banking its Figma
calls in `fixtures/local-call-bank` so a re-run costs nothing already paid
for. Write roots: the server only writes under its current directory, so it
is started in the harness directory and every output path is relative to it.
"""

import json
import os
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
HARNESS = os.path.dirname(HERE)
REPO = os.path.dirname(os.path.dirname(HARNESS))
EXE = os.path.expanduser(r"~\.cargo\bin\devup-mcp.exe")
BANK = os.path.join(REPO, "fixtures", "local-call-bank")
FILE_KEY = "f1AJyo27afkkr6U9PhnWSu"

# name: the module to render, the frames whose PNGs it is compared against
# at their own widths, and which output the module is.
#   "responsiveTsx": one module for every width, taken from the first frame
#   "tsx": one module per frame, each compared at its own width
TARGETS = {
    "popup": {"output": "responsiveTsx", "frames": ["422:5682", "422:5705", "422:5728"]},
    "keyframes": {"output": "tsx", "frames": ["458:2021"]},
    "grid": {"output": "tsx", "frames": ["429:1966"]},
    "report": {"output": "tsx", "frames": ["446:1971"]},
    "notice": {"output": "tsx", "frames": ["422:6914", "422:7088", "422:6865"]},
    "about": {"output": "tsx", "frames": ["422:3376", "422:3180", "422:2987"]},
    # The devup-ui.com landing page, in its own file: one brand, one design
    # system, and a deployed implementation to compare against as well as the
    # plugin's answers. Mobile / tablet / PC, in ascending width as `about`
    # and `notice` are.
    "landing": {"output": "tsx", "frames": ["833:3640", "833:3322", "832:2975"],
                "file": "JVj6yCOUnF45JQAPvXLA4p", "name": "Devup-UI"},
}


class Server:
    def __init__(self):
        environment = dict(os.environ)
        environment["DEVUP_FIGMA_CALL_CACHE"] = BANK
        self.proc = subprocess.Popen(
            [EXE], cwd=HARNESS, env=environment,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            text=True, encoding="utf-8", bufsize=1,
        )
        self.lines = []
        self.next_id = 1
        threading.Thread(target=self._read, daemon=True).start()
        self.call_raw("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                     "clientInfo": {"name": "render-harness", "version": "0"}})
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})

    def _read(self):
        for line in self.proc.stdout:
            self.lines.append(line)

    def _send(self, message):
        self.proc.stdin.write(json.dumps(message) + "\n")
        self.proc.stdin.flush()

    def call_raw(self, method, params, limit=1800):
        request_id = self.next_id
        self.next_id += 1
        self._send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        deadline = time.time() + limit
        seen = 0
        while time.time() < deadline:
            while seen < len(self.lines):
                raw = self.lines[seen].strip()
                seen += 1
                if not raw:
                    continue
                try:
                    message = json.loads(raw)
                except json.JSONDecodeError:
                    continue
                if message.get("id") == request_id:
                    return message
            if self.proc.poll() is not None:
                raise SystemExit("devup-mcp exited")
            time.sleep(0.2)
        raise SystemExit(f"no response to {method} within {limit}s")

    def export(self, arguments, allow_error=False):
        # Asset naming is left to the server's own default, so what is
        # measured here is what a caller actually receives rather than
        # something this harness asked for.
        response = self.call_raw("tools/call", {"name": "devup_figma_export", "arguments": arguments})
        if "error" in response:
            message = response["error"].get("message")
            if allow_error:
                return {"error": message}
            raise SystemExit(f"export refused: {message}")
        text = "".join(part.get("text", "") for part in response["result"].get("content", [])
                       if part.get("type") == "text")
        return json.loads(text)

    def close(self):
        try:
            self.proc.kill()
            self.proc.wait(timeout=10)
        except Exception:
            pass


def family_of(screen):
    """Which target a screen was acquired for, or `None` for a screen this
    does not produce - a hand-made comparison screen such as `popup-answer`,
    which re-acquiring `popup` must leave alone."""
    if screen in TARGETS:
        return screen
    head = screen.rsplit("-", 2)[0] if screen.count("-") >= 2 else screen
    return head if head in TARGETS else None


def url_of(node_id, file_key=FILE_KEY, file_name="devup-Test"):
    return f"https://www.figma.com/design/{file_key}/{file_name}?node-id={node_id.replace(':', '-')}"


def url_for(target, node_id):
    """The URL of a frame in whichever file its target lives in."""
    return url_of(node_id, target.get("file", FILE_KEY), target.get("name", "devup-Test"))


def asset_path(name, asset):
    """Where the generated module expects the asset: `/icons/<layer>.svg` for
    a vector, `/images/<layer>.png` for an image fill (`-<index>` past the
    first fill), as `codegen::style` names them."""
    if asset["sourceKind"] == "vector-node":
        return f"public/icons/{name}.svg", "svg"
    index = asset["field"].rsplit("/", 1)[-1]
    suffix = "" if index in ("0", "fills") else f"-{index}"
    return f"public/images/{name}{suffix}.png", "png"


def acquire_theme(server, frame):
    """The theme for every screen: `devup.json` at file scope, so a token any
    screen uses is defined. A node-scope theme holds only the tokens of the
    node it was asked for, and a screen rendered with another node's theme
    loses every typography token it does not share - which is how the report
    section's heading came out with no size and its titles without weight."""
    body = server.export({"url": url_of(frame), "outputs": ["devupJson"], "scope": "file",
                          "outputPaths": {"devupJson": "devup.json"}, "includeDiagnostics": True})
    print(f"  theme (file scope): status={body.get('status')} quality={body.get('quality', {}).get('theme')} "
          f"counts={body.get('themeCounts')}", flush=True)


def acquire(server, name, target, manifest):
    output = target["output"]
    frames = target["frames"]
    for index, frame in enumerate(frames):
        module_name = name if output == "responsiveTsx" else f"{name}-{frame.replace(':', '-')}"
        module_path = f"src/screens/{module_name}.tsx"
        reference_path = f"out/{module_name}-{frame.replace(':', '-')}.reference.png" if output == "responsiveTsx" else f"out/{module_name}.reference.png"
        wants_module = output == "tsx" or index == 0
        scratch = f"out/{module_name}-{frame.replace(':', '-')}"
        # Every output goes to a file: a file has no size limit, where an
        # inline answer is capped at 1 MiB and a larger one is delivered as
        # resources this does not read.
        outputs = ["referencePng", "rawSnapshot"]
        paths = {"referencePng": reference_path, "rawSnapshot": f"{scratch}.snapshot.json"}
        theme_path = f"themes/{module_name}.json"
        if wants_module:
            outputs.append(output)
            paths[output] = module_path
            # The screen's own theme, at node scope. A file-scope theme holds
            # every collection in the file, and this file holds several
            # brands: more than one defines `primary`, so one of them wins and
            # the rest render in the wrong brand's colour - the notice screen
            # came out violet where Figma draws it blue. Scoped to the node,
            # the same token resolves to that screen's own value and nothing
            # conflicts.
            outputs.append("devupJson")
            paths["devupJson"] = theme_path
        # rawSnapshot describes the design rather than the screen, so it needs
        # debug: true. This harness is the case that flag is for - it compares
        # what the browser drew against what the design says.
        body = server.export({"url": url_for(target, frame), "outputs": outputs, "scope": "node",
                              "outputPaths": paths, "includeDiagnostics": True, "debug": True})
        print(f"  {frame}: status={body.get('status')} quality={body.get('quality')}", flush=True)
        # The module refers to assets by layer name; the manifest by node id.
        with open(os.path.join(HARNESS, paths["rawSnapshot"]), encoding="utf-8") as handle:
            snapshot = json.load(handle)
        # The manifest is the one output that cannot be written to a file;
        # alone it is small enough to arrive inline.
        asset_manifest = server.export({"url": url_for(target, frame), "outputs": ["assetManifest"], "scope": "node",
                                        "delivery": "inline"}).get("assetManifest") or {}
        nodes = snapshot.get("nodes") or {}
        requests = []
        for asset in asset_manifest.get("assets", []):
            if asset.get("status") != "available":
                continue
            # The manifest says where the code refers to the asset; the
            # fallback re-derives it from the layer name for a manifest that
            # does not.
            if asset.get("path"):
                path = "public" + asset["path"]
                fmt = "svg" if path.endswith(".svg") else "png"
            else:
                node = nodes.get(asset["nodeId"]) or {}
                layer = (node.get("fields") or {}).get("name") or "Asset"
                path, fmt = asset_path(layer, asset)
            # Done when the file is there; a record without the file is stale.
            if path in manifest["assets"] and os.path.exists(os.path.join(HARNESS, path)):
                continue
            manifest["assets"][path] = asset["assetId"]
            requests.append({"assetId": asset["assetId"], "format": fmt, "scale": 1, "outputPath": path})
        for start in range(0, len(requests), 16):
            batch = requests[start:start + 16]
            # An artifact holds only the assets its own collection exported,
            # so the bytes are asked for by URL; the node reads are replayed
            # from the bank and only the export itself is new.
            # The bytes go to their files whatever the delivery; the answer
            # itself may be too large to inline, and is not read.
            # `refresh`: the collection the process cached for this URL holds
            # no exports, and the server asks for one that does. The node
            # reads replay from the bank; only the export itself is new.
            body = server.export({"url": url_for(target, frame), "outputs": ["assetManifest"], "scope": "node",
                                  "assetRequests": batch, "refresh": True}, allow_error=True)
            if body.get("error"):
                # One node the server will not export refuses the whole call,
                # and the other fifteen are lost with it. Asked for one at a
                # time, the refusal is confined to the node that caused it and
                # says which one that is.
                print(f"    assets {start + 1}-{start + len(batch)}: {body['error']}", flush=True)
                refused = []
                for entry in batch:
                    one = server.export({"url": url_for(target, frame), "outputs": ["assetManifest"], "scope": "node",
                                         "assetRequests": [entry], "refresh": True}, allow_error=True)
                    if one.get("error"):
                        refused.append(entry["assetId"])
                        manifest["assets"].pop(entry["outputPath"], None)
                if refused:
                    print(f"      refused: {', '.join(refused)}", flush=True)
                body = {}
            missing = [entry["outputPath"] for entry in batch if not os.path.exists(os.path.join(HARNESS, entry["outputPath"]))]
            print(f"    assets {start + 1}-{start + len(batch)}: quality={body.get('quality', {}).get('assets')} missing={missing}", flush=True)
        manifest["targets"].append({
            # One entry per frame; a responsive module is rendered once per
            # frame, at that frame's size, so the entries share a screen.
            "name": f"{module_name}-{frame.replace(':', '-')}" if output == "responsiveTsx" else module_name,
            "screen": module_name,
            "module": module_path,
            "frame": frame,
            "reference": reference_path,
            "responsive": output == "responsiveTsx",
            "theme": theme_path,
        })


def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    wanted = sys.argv[1:] or list(TARGETS)
    os.makedirs(os.path.join(HARNESS, "src", "screens"), exist_ok=True)
    os.makedirs(os.path.join(HARNESS, "public", "icons"), exist_ok=True)
    os.makedirs(os.path.join(HARNESS, "public", "images"), exist_ok=True)
    os.makedirs(os.path.join(HARNESS, "out"), exist_ok=True)
    manifest_path = os.path.join(HARNESS, "targets.json")
    manifest = {"targets": [], "assets": {}}
    if os.path.exists(manifest_path):
        with open(manifest_path, encoding="utf-8") as handle:
            manifest = json.load(handle)
    manifest["targets"] = [t for t in manifest["targets"] if family_of(t.get("screen", t["name"])) not in wanted]
    server = Server()
    try:
        if not os.path.exists(os.path.join(HARNESS, "devup.json")) or "theme" in wanted:
            wanted = [name for name in wanted if name != "theme"]
            acquire_theme(server, TARGETS[wanted[0] if wanted else "popup"]["frames"][0])
        for name in wanted:
            print(f"== {name}", flush=True)
            acquire(server, name, TARGETS[name], manifest)
    finally:
        server.close()
    with open(manifest_path, "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, ensure_ascii=False, indent=2)
    print(f"targets: {len(manifest['targets'])}, assets: {len(manifest['assets'])}")


if __name__ == "__main__":
    main()
