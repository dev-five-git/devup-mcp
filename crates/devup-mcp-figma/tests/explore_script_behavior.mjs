import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(new URL("../src/scripts/explore.js", import.meta.url));
const exploreSource = (await readFile(scriptPath, "utf8")).replace(/\r\n/g, "\n");
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;

const sectionSource = await readFile(new URL("../src/scripts/section_index.js", import.meta.url), "utf8");

async function executeSection(section) {
  return new AsyncFunction("figma", sectionSource.replace('"__DEVUP_NODE_ID__"', JSON.stringify(section.id)))({
    fileKey: "fixture-file",
    getNodeByIdAsync: async () => section,
  });
}

test("R10 compact index connects descendant IDs to selectable screens", async () => {
  const child = sceneNode({id:"3997:46703", type:"VECTOR"});
  const frame = sceneNode({id:"3997:46690",type:"FRAME",width:360,height:740,children:[child]});
  const section = sceneNode({id:"4279:7806",type:"SECTION",children:[frame]});
  pageWith(section);
  const result = await executeSection(section);
  assert.equal(result.nodes[0].fields.nodeScreenIds?.["3997:46703"], "3997:46690");
  assert.equal(result.nodes.length, 2);
});

test("Section list previews visible text without exporting descendants", async () => {
  const hidden = sceneNode({ id: "hidden", type: "TEXT" });
  hidden.characters = "hidden draft";
  const hiddenGroup = sceneNode({ id: "hidden-group", children: [hidden] });
  hiddenGroup.visible = false;
  const text = sceneNode({ id: "copy", type: "TEXT" });
  text.characters = "  Upload\n guidance   TIP ";
  const frame = sceneNode({ id: "frame", type: "FRAME", children: [hiddenGroup, text] });
  const empty = sceneNode({ id: "empty", type: "FRAME" });
  const section = sceneNode({ id: "section", type: "SECTION", children: [frame, empty] });
  pageWith(section);
  const result = await executeSection(section);
  assert.equal(result.nodes.find(n => n.id === "frame").fields.textPreview, "Upload guidance TIP");
  assert.equal(result.nodes.find(n => n.id === "empty").fields.textPreview, "");
  assert.deepEqual(new Set(result.nodes.map(n => n.id)), new Set(["section", "frame", "empty"]));
});

test("Section preview text has per-candidate and aggregate limits", async () => {
  const frames = Array.from({ length: 30 }, (_, i) => {
    const text = sceneNode({ id: `copy-${i}`, type: "TEXT" });
    text.characters = "안내😀".repeat(300);
    return sceneNode({ id: `frame-${i}`, type: "FRAME", children: [text] });
  });
  const section = sceneNode({ id: "section", type: "SECTION", children: frames });
  pageWith(section);
  const result = await executeSection(section);
  const previews = result.nodes.slice(1).map(n => n.fields.textPreview);
  assert.equal(Array.from(previews[0]).length, 120);
  assert.ok(previews.every(value => Array.from(value).length <= 120));
  assert.ok(previews.reduce((sum, value) => sum + Buffer.byteLength(value), 0) <= 2048);
  assert.ok(previews.every(value => value.isWellFormed()));
});

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
    name: "[FR-026] Essence",
    width: 320,
    height: 48,
  });
  const wrapper = sceneNode({ id: "screen-wrapper", children: screens });
  const section = sceneNode({
    id: "4217:7743",
    type: "SECTION",
    name: "[FR-026] Essence",
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
  const longName = "A".repeat(2_000);
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
