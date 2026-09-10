const section = await figma.getNodeByIdAsync("__DEVUP_NODE_ID__");
if (!section) throw new Error("DEVUP_NODE_NOT_FOUND");
if (section.type !== "SECTION") {
  let ancestor = section.parent;
  const seen = new Set();
  while (ancestor && ancestor.type !== "SECTION" && !seen.has(ancestor.id)) {
    seen.add(ancestor.id);
    ancestor = ancestor.parent;
  }
  const sectionId = ancestor && ancestor.type === "SECTION" ? ancestor.id : null;
  const url = sectionId && figma.fileKey
    ? `https://www.figma.com/design/${figma.fileKey}?node-id=${sectionId.replace(/:/g, "-")}` : null;
  throw new Error("DEVUP_SECTION_REQUIRED " + JSON.stringify({
    pluginCode: "DEVUP_SECTION_REQUIRED", stage: "section-index",
    nodeId: section.id, nodeType: section.type, sectionId,
    nextAction: {
      tool: "devup_figma_export",
      how: sectionId ? `The url must point to a SECTION. This capture's SECTION is ${sectionId}; select frames with frameIds.`
        : "The url must point to a SECTION. This node has no ancestor SECTION; choose the intended SECTION in Figma and copy its link.",
      arguments: url ? (section.type === "FRAME" && section.parent === ancestor
        ? {url, frameIds: [section.id]} : {url}) : null,
      requiredArguments: url ? [] : ["url"]
    }
  }));
}

const MAX_CANDIDATES = 100;
const MAX_TRAVERSED_NODES = 20000;
const MAX_PREVIEW_CHARACTERS = 120;
const MAX_PREVIEW_NODES = 64;
let remainingPreviewBytes = 2048;

// The menu needs enough copy to distinguish similarly named frames, without
// returning their descendants or letting previews dominate the compact index.
function textPreview(root) {
  const queue = [root];
  let preview = "";
  let characters = 0;
  let bytes = 0;
  for (let index = 0; index < queue.length; index += 1) {
    const node = queue[index];
    if (node.visible === false) continue;
    if (node.type === "TEXT" && typeof node.characters === "string") {
      const text = node.characters.replace(/\s+/g, " ").trim();
      for (const character of (preview && text ? " " : "") + text) {
        const size = utf8ByteLength(character);
        if (characters >= MAX_PREVIEW_CHARACTERS || bytes + size > remainingPreviewBytes) {
          remainingPreviewBytes -= bytes;
          return preview.trimEnd();
        }
        preview += character;
        characters += 1;
        bytes += size;
      }
    }
    if ("children" in node && queue.length < MAX_PREVIEW_NODES) {
      queue.push(...node.children.slice(0, MAX_PREVIEW_NODES - queue.length));
    }
  }
  remainingPreviewBytes -= bytes;
  return preview;
}

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
    const box = bounds(node);
    if (!box || node.visible === false) continue;
    // Keep the whole child available for explicit selection alongside any
    // automatic screens inside it. Rust cannot recover discarded descendants.
    candidateNodes.push({ node, box });
  }
}
candidateNodes.sort((left, right) =>
  left.box.y - right.box.y
    || left.box.x - right.box.x
    || left.node.id.localeCompare(right.node.id),
);
const projectionTruncated = queue.length > traversalCount || candidateNodes.length > MAX_CANDIDATES;
const selected = candidateNodes.slice(0, MAX_CANDIDATES);
const selectedIds = new Set(selected.map(({ node }) => node.id));
// Keep ID ownership while the tree is available. Geometry/text for descendants
// still stays out of this compact response. Only visited nodes are claimed.
const nodeScreenIds = {};
for (const node of queue.slice(0, traversalCount)) {
  let owner = node;
  const seen = new Set();
  while (owner && owner.id !== section.id && !selectedIds.has(owner.id) && !seen.has(owner.id)) {
    seen.add(owner.id);
    owner = owner.parent;
  }
  nodeScreenIds[node.id] = owner && selectedIds.has(owner.id) ? owner.id : null;
}
// Link to the nearest retained ancestor: intermediate layout groups are not
// included in this compact projection, but container/screen ancestry survives.
const parentIds = new Map(selected.map(({ node }) => {
  let parent = node.parent;
  while (parent && parent.id !== section.id && !selectedIds.has(parent.id)) {
    parent = parent.parent;
  }
  return [node.id, parent && selectedIds.has(parent.id) ? parent.id : section.id];
}));
const childrenIds = (id) => selected
  .filter(({ node }) => parentIds.get(node.id) === id)
  .map(({ node }) => node.id);
const sectionBox = bounds(section);
if (!sectionBox) throw new Error("DEVUP_NODE_BOUNDS_UNAVAILABLE");

const sectionNode = {
  id: section.id,
  type: section.type,
  fields: {
    name: section.name,
    parentId: section.parent && section.parent.type !== "DOCUMENT" ? section.parent.id : null,
    childrenIds: childrenIds(section.id),
    absoluteBoundingBox: sectionBox,
    visible: section.visible !== false,
    projectionTruncated,
    nodeScreenIds,
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
      parentId: parentIds.get(node.id),
      childrenIds: childrenIds(node.id),
      absoluteBoundingBox: box,
      visible: node.visible !== false,
      breadcrumb: breadcrumb(node),
      directChildCount: "children" in node ? node.children.length : 0,
      textPreview: textPreview(node),
      subtreeNodeCount: estimate.subtreeNodeCount,
      estimatedSerializedBytes: estimate.estimatedSerializedBytes,
      selectionReasons: [isScreen(node, box) ? "screen-like" : "explicit-selection-only", "inside-section"],
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
