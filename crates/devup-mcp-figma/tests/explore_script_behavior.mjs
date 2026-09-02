import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(new URL("../src/scripts/explore.js", import.meta.url));
const exploreSource = (await readFile(scriptPath, "utf8")).replace(/\r\n/g, "\n");
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;

function sceneNode({
  id,
  type = "GROUP",
  name = id,
  width = 100,
  height = 100,
  children = [],
  onChildrenRead,
}) {
  const value = {
    id,
    type,
    name,
    x: 0,
    y: 0,
    width,
    height,
    visible: true,
    absoluteBoundingBox: { x: 0, y: 0, width, height },
    parent: null,
  };
  Object.defineProperty(value, "children", {
    configurable: true,
    enumerable: true,
    get() {
      onChildrenRead?.(id);
      return children;
    },
  });
  for (const child of children) child.parent = value;
  return value;
}

function pageWith(child, name = "Page") {
  const page = sceneNode({ id: "page", type: "PAGE", name, children: [child] });
  page.parent = { type: "DOCUMENT" };
  return page;
}

async function executeExplore(anchor, page, transform = (source) => source) {
  const source = transform(exploreSource)
    .replace('"__DEVUP_NODE_ID__"', JSON.stringify(anchor.id))
    .replace(
      '"__DEVUP_EXPLORE__"',
      JSON.stringify({ projectionLimit: 20, textPreviewLimit: 16 }),
    );
  const figma = {
    fileKey: "fixture-file",
    getNodeByIdAsync: async (id) => (id === anchor.id ? anchor : null),
    setCurrentPageAsync: async (selected) => assert.equal(selected, page),
  };
  return new AsyncFunction("figma", source)(figma);
}

function containsCompleteParentChain(result, nodeId, ancestorId) {
  const nodes = new Map(result.nodes.map((node) => [node.id, node]));
  let current = nodes.get(nodeId);
  while (current?.fields.parentId) {
    if (current.fields.parentId === ancestorId) return true;
    current = nodes.get(current.fields.parentId);
  }
  return false;
}

test("a screen below two wrappers retains its complete SECTION parent chain", async () => {
  const screen = sceneNode({ id: "screen", type: "FRAME", width: 360, height: 740 });
  const wrapper2 = sceneNode({ id: "wrapper-2", children: [screen] });
  const wrapper1 = sceneNode({ id: "wrapper-1", children: [wrapper2] });
  const section = sceneNode({ id: "section", type: "SECTION", children: [wrapper1] });
  const page = pageWith(section);

  const result = await executeExplore(section, page);
  assert.equal(containsCompleteParentChain(result, "screen", "section"), true);

  const mutated = await executeExplore(section, page, (source) => source.replace(
    "const missingChain = [...ancestors, node]",
    "const missingChain = [node]",
  ));
  assert.equal(containsCompleteParentChain(mutated, "screen", "section"), false);
});

test("a nested heading explores the same ten screens as its enclosing SECTION", async () => {
  const screenIds = [
    "3879:35518",
    "3879:35519",
    "3879:35520",
    "3879:35521",
    "3879:35522",
    "3879:35523",
    "3879:35524",
    "3879:35525",
    "3879:35526",
    "3879:35527",
  ];
  const screens = screenIds.map((id, index) => {
    const screen = sceneNode({ id, type: "FRAME", width: 360, height: 740 });
    screen.x = index * 400;
    screen.absoluteBoundingBox.x = index * 400;
    return screen;
  });
  const heading = sceneNode({
    id: "3879:35481",
    type: "TEXT",
    name: "[FR-026] 본연체",
    width: 320,
    height: 48,
  });
  const wrapper = sceneNode({ id: "screen-wrapper", children: screens });
  const section = sceneNode({
    id: "4217:7743",
    type: "SECTION",
    name: "[FR-026] 본연체",
    width: 4_400,
    height: 900,
    children: [heading, wrapper],
  });
  const page = pageWith(section);

  const fromHeading = await executeExplore(heading, page);
  const fromSection = await executeExplore(section, page);
  const projectedScreens = (result) => result.nodes
    .filter((node) => node.type === "FRAME")
    .map((node) => node.id);

  assert.deepEqual(projectedScreens(fromHeading), screenIds);
  assert.deepEqual(projectedScreens(fromHeading), projectedScreens(fromSection));
  assert.equal(fromHeading.nodes.some((node) => node.id === heading.id), true);
  assert.equal(containsCompleteParentChain(fromHeading, heading.id, section.id), true);
  assert.equal(JSON.stringify(fromHeading).length <= 14_000, true);
});

test("a large SECTION without screens visits at most projectionLimit times eight nodes", async () => {
  const visited = new Set();
  const children = Array.from({ length: 1_000 }, (_, index) => sceneNode({
    id: `group-${index}`,
    onChildrenRead: (id) => visited.add(id),
  }));
  const section = sceneNode({ id: "section", type: "SECTION", children });
  const page = pageWith(section);

  const result = await executeExplore(section, page);
  assert.equal(visited.size, 160);
  assert.equal(result.nodes[0].fields.projectionTruncated, true);

  visited.clear();
  await executeExplore(section, page, (source) => source.replace(
    "while (nestedQueue.length && visited < traversalLimit)",
    "while (nestedQueue.length)",
  ));
  assert.equal(visited.size, 1_000);
});

test("oversized required nodes collapse to a bounded required-only projection", async () => {
  const longName = "가".repeat(2_000);
  const anchor = sceneNode({ id: "anchor", type: "SECTION", name: longName });
  let nested = anchor;
  for (let index = 0; index < 10; index += 1) {
    nested = sceneNode({ id: `wrapper-${index}`, name: longName, children: [nested] });
  }
  const peer = sceneNode({ id: "peer", type: "FRAME", name: longName, children: [nested] });
  const page = pageWith(peer, longName);
  const forceRequiredFallback = (source) => source.replace(
    "const MAX_PROJECTION_JSON_CHARS = 14_000;",
    "const MAX_PROJECTION_JSON_CHARS = 3_500;",
  );

  const result = await executeExplore(anchor, page, forceRequiredFallback);
  assert.deepEqual(result.nodes.map((node) => node.id), ["page", "peer", "anchor"]);
  assert.equal(result.nodes[0].fields.projectionTruncated, true);
  assert.equal(JSON.stringify(result).length <= 3_500, true);
  assert.equal(result.nodes.every((node) => node.fields.name.length <= 80), true);
  assert.equal(result.nodes.every((node) => node.fields.breadcrumb.length <= 4), true);

  await assert.rejects(
    executeExplore(anchor, page, (source) => forceRequiredFallback(source).replace(
      "if (JSON.stringify(output).length > MAX_PROJECTION_JSON_CHARS) {\n  output.nodes = output.nodes",
      "if (false) {\n  output.nodes = output.nodes",
    )),
    /DEVUP_EXPLORE_PROJECTION_TOO_LARGE/,
  );
});
