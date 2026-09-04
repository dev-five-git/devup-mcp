const section = await figma.getNodeByIdAsync("__DEVUP_NODE_ID__");
if (!section) throw new Error("DEVUP_NODE_NOT_FOUND");
if (section.type !== "SECTION") throw new Error("DEVUP_SECTION_REQUIRED");

const MAX_CANDIDATES = 100;
const MAX_TRAVERSED_NODES = 20000;

function bounds(node) {
  const value = node.absoluteBoundingBox || {
    x: typeof node.x === "number" ? node.x : 0,
    y: typeof node.y === "number" ? node.y : 0,
    width: typeof node.width === "number" ? node.width : 0,
    height: typeof node.height === "number" ? node.height : 0,
  };
  if (![value.x, value.y, value.width, value.height].every(Number.isFinite)) return null;
  return { x: value.x, y: value.y, width: value.width, height: value.height };
}

function isScreen(node, box) {
  if (node.type !== "FRAME" || node.visible === false || !box) return false;
  const aspect = box.width / Math.max(1, box.height);
  return box.width >= 240 && box.width <= 1800
    && box.height >= 300 && box.height <= 2000
    && aspect >= 0.25 && aspect <= 2.5;
}

function contains(ancestor, node) {
  let parent = node.parent;
  while (parent) {
    if (parent.id === ancestor.id) return true;
    parent = parent.parent;
  }
  return false;
}

function breadcrumb(node) {
  const names = [];
  let current = node;
  while (current && current.type !== "DOCUMENT") {
    if (typeof current.name === "string" && current.name) names.push(current.name);
    current = current.parent;
  }
  return names.reverse();
}

function utf8ByteLength(value) {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x80) bytes += 1;
    else if (code < 0x800) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
      bytes += 4;
      index += 1;
    } else bytes += 3;
  }
  return bytes;
}

function subtreeEstimate(root) {
  const queue = [root];
  let count = 0;
  let estimatedBytes = 0;
  for (let index = 0; index < queue.length && count < MAX_TRAVERSED_NODES; index += 1) {
    const node = queue[index];
    count += 1;
    estimatedBytes += 512 + utf8ByteLength(typeof node.name === "string" ? node.name : "");
    if (node.type === "TEXT" && typeof node.characters === "string") {
      estimatedBytes += utf8ByteLength(node.characters) * 2;
    }
    if ("children" in node) queue.push(...node.children);
  }
  return {
    subtreeNodeCount: count,
    estimatedSerializedBytes: estimatedBytes,
    truncated: queue.length > count,
  };
}

const queue = "children" in section ? [...section.children] : [];
const candidateNodes = [];
let traversalCount = 0;
for (let index = 0; index < queue.length && traversalCount < MAX_TRAVERSED_NODES; index += 1) {
  const node = queue[index];
  traversalCount += 1;
  const box = bounds(node);
  if (isScreen(node, box)) {
    let parent = node.parent;
    let nestedInScreen = false;
    while (parent && parent.id !== section.id) {
      if (isScreen(parent, bounds(parent))) {
        nestedInScreen = true;
        break;
      }
      parent = parent.parent;
    }
    if (!nestedInScreen) candidateNodes.push({ node, box });
  }
  if ("children" in node) queue.push(...node.children);
}
// Screen shape is a guess for finding screens on a page that has no grouping.
// A Section is grouping, already explicit, and the guess applied there answers
// with whatever happens to measure like a phone. A Section of small cases
// annotated with tall notes turns it upside down: the notes pass and the cases
// do not, so the index offered the notes and hid every case — an answer that
// looked complete, which is worse than the empty list a Section of cases used
// to give. What the Section holds is what it offers.
if ("children" in section) {
  const chosen = new Set(candidateNodes.map(({ node }) => node.id));
  for (const node of section.children) {
    if (chosen.has(node.id)) continue;
    // A child holding a screen would offer that screen twice over, once whole
    // and once inside itself.
    if (candidateNodes.some(({ node: screen }) => contains(node, screen))) continue;
    const box = bounds(node);
    if (box && node.visible !== false) candidateNodes.push({ node, box });
  }
}
candidateNodes.sort((left, right) =>
  left.box.y - right.box.y
    || left.box.x - right.box.x
    || left.node.id.localeCompare(right.node.id),
);
const projectionTruncated = queue.length > traversalCount || candidateNodes.length > MAX_CANDIDATES;
const selected = candidateNodes.slice(0, MAX_CANDIDATES);
const candidateIds = selected.map(({ node }) => node.id);
const sectionBox = bounds(section);
if (!sectionBox) throw new Error("DEVUP_NODE_BOUNDS_UNAVAILABLE");

const sectionNode = {
  id: section.id,
  type: section.type,
  fields: {
    name: section.name,
    parentId: section.parent && section.parent.type !== "DOCUMENT" ? section.parent.id : null,
    childrenIds: candidateIds,
    absoluteBoundingBox: sectionBox,
    visible: section.visible !== false,
    projectionTruncated,
  },
  extra: {},
  fieldErrors: {},
};
const candidates = selected.map(({ node, box }) => {
  const estimate = subtreeEstimate(node);
  return {
    id: node.id,
    type: node.type,
    fields: {
      name: typeof node.name === "string" ? node.name : "",
      parentId: section.id,
      childrenIds: [],
      absoluteBoundingBox: box,
      visible: node.visible !== false,
      breadcrumb: breadcrumb(node),
      directChildCount: "children" in node ? node.children.length : 0,
      subtreeNodeCount: estimate.subtreeNodeCount,
      estimatedSerializedBytes: estimate.estimatedSerializedBytes,
      selectionReasons: ["screen-like", "inside-section"],
      estimateTruncated: estimate.truncated,
    },
    extra: {},
    fieldErrors: {},
  };
});

return {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: [section.id],
  nodes: [sectionNode, ...candidates],
  diagnostics: [],
};
