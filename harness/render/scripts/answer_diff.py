import re
import sys
from collections import Counter

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

KNOWN = [
    # asset names: per-node suffixes and variant names, settled with the author
    (r'(src|maskImage)="[^"]*"', "asset-name"),
    (r"maskImage=\"url\('[^']*'\)\"", "asset-name"),
    # zero shorthand: the pinned corpus writes 0px, the newer plugin writes 0
    (r'borderRadius="[^"]*"', "radius-zero"),
    # settled in this repo's favour by the render
    (r'flexShrink="0"', "flexShrink"),
    (r'alignSelf="flex-start"', "alignSelf"),
    (r'\{" "\}(Config|FOUC)(<br />|\{" "\})', "headline-break"),
    (r'Building the Future of CSS-in-JS', "subhead-break"),
]


def classify(line):
    for pattern, tag in KNOWN:
        if re.search(pattern, line):
            return tag
    return None


def lines(path):
    out = []
    for raw in open(path, encoding="utf-8"):
        s = raw.strip()
        if not s or s.startswith("import ") or s.startswith("export function"):
            continue
        if s in ("return (", ")", "}", ");"):
            continue
        out.append(s)
    return Counter(out)


def compare(ours_path, plug_path, label):
    ours, plug = lines(ours_path), lines(plug_path)
    only_ours, only_plug = ours - plug, plug - ours
    print(f"== {label}: ours={sum(ours.values())} plugin={sum(plug.values())} "
          f"onlyOurs={sum(only_ours.values())} onlyPlugin={sum(only_plug.values())}")
    known = Counter()
    for side, bag in (("ours", only_ours), ("plugin", only_plug)):
        for line, n in bag.items():
            tag = classify(line)
            if tag:
                known[tag] += n
    print("   known/settled:", dict(known))
    print("   -- ours only (unexplained):")
    for line, n in sorted(only_ours.items()):
        if not classify(line):
            print(f"      {n}x {line[:140]}")
    print("   -- plugin only (unexplained):")
    for line, n in sorted(only_plug.items()):
        if not classify(line):
            print(f"      {n}x {line[:140]}")
    print()


TARGETS = {
    "pc": ("landing-832-2975", "pure-pc"),
    "tablet": ("landing-833-3322", "pure-tablet"),
    "mobile": ("landing-833-3640", "pure-mobile"),
}
for name in (sys.argv[1:] or TARGETS):
    screen, answer = TARGETS[name]
    compare(f"harness/render/src/screens/{screen}.tsx",
            f"fixtures/plugin-answers/devup-ui-landing/{answer}.tsx", name)
