// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
const anchor = await figma.getNodeByIdAsync("__DEVUP_NODE_ID__");
if (!anchor) throw new Error("DEVUP_NODE_NOT_FOUND");

const options = "__DEVUP_EXPLORE__";
const projectionLimit = Math.max(20, Math.min(
  400,
  Number.isInteger(options.projectionLimit) ? options.projectionLimit : 200,
));
const textPreviewLimit = Math.max(0, Math.min(
  500,
  Number.isInteger(options.textPreviewLimit) ? options.textPreviewLimit : 160,
));
const MAX_PROJECTION_JSON_CHARS = 14_000;
const MAX_NAME_CHARS = 240;
const MAX_BREADCRUMB_SEGMENTS = 12;

function compactName(value) {
  return typeof value === "string" ? value.slice(0, MAX_NAME_CHARS) : "";
}

const requestedAnchor = anchor;
const requestedPage = requestedAnchor.type === "PAGE";
let pagePeer = requestedAnchor;
let nearestSection = requestedAnchor.type === "SECTION" ? requestedAnchor : null;
while (!requestedPage && pagePeer.parent && pagePeer.parent.type !== "PAGE") {
  pagePeer = pagePeer.parent;
  if (!nearestSection && pagePeer.type === "SECTION") nearestSection = pagePeer;
}
const page = requestedAnchor.type === "PAGE" ? requestedAnchor : pagePeer.parent;
if (!page || page.type !== "PAGE") throw new Error("DEVUP_PAGE_NOT_FOUND");
await figma.setCurrentPageAsync(page);

function bounds(node) {
  const absolute = node.absoluteBoundingBox;
  const value = absolute || {
    x: typeof node.x === "number" ? node.x : 0,
    y: typeof node.y === "number" ? node.y : 0,
    width: typeof node.width === "number" ? node.width : 0,
    height: typeof node.height === "number" ? node.height : 0,
  };
  if (![value.x, value.y, value.width, value.height].every(Number.isFinite)) return null;
  return { x: value.x, y: value.y, width: value.width, height: value.height };
}

// Carry a small measured navigation summary into screen planning, before a
// full page export. Missing measurements remain unknown rather than guessed.
function navigationContentWidth(root) {
  const queue = [root];
  let visited = 0;
  let width = null;
  while (queue.length && visited++ < 400) {
    const node = queue.shift();
    if (("visible" in node) && node.visible === false) continue;
    const children = "children" in node ? node.children.filter((child) => !("visible" in child) || child.visible !== false) : [];
    if (/header|nav|gnb/i.test(node.name || "") && children.length >= 2
      && ("layoutMode" in node) && node.layoutMode === "HORIZONTAL") {
      const sizes = children.map(bounds);
      if (sizes.every(Boolean)) {
        const measured = sizes.reduce((sum, box) => sum + box.width, 0) + (Number(node.itemSpacing) || 0) * (sizes.length - 1);
        width = Math.max(width || 0, measured);
      }
    }
    queue.push(...children.slice(0, 80));
  }
  return width;
}

function navigationSummary(root) {
  const width = navigationContentWidth(root);
  return width === null ? {} : { navigationContentWidth: width };
}


function horizontalOverlap(left, right) {
  return Math.min(left.x + left.width, right.x + right.width) - Math.max(left.x, right.x);
}

function isScreenLike(node) {
  const box = bounds(node);
  return box
    && ["FRAME", "COMPONENT", "INSTANCE", "COMPONENT_SET"].includes(node.type)
    && box.width >= 240
    && box.width <= 1800
    && box.height >= 300
    && box.height <= 2000
    && box.width / Math.max(1, box.height) >= 0.25
    && box.width / Math.max(1, box.height) <= 2.5;
}

function textPreview(node) {
  if (textPreviewLimit === 0) return "";
  const values = [];
  const queue = [node];
  let visited = 0;
  while (queue.length && visited < 80 && values.join(" ").length < textPreviewLimit) {
    const candidate = queue.shift();
    visited += 1;
    if (candidate.type === "TEXT" && typeof candidate.characters === "string") {
      values.push(candidate.characters.replace(/\s+/gu, " ").trim());
    }
    if ("children" in candidate) queue.push(...candidate.children.slice(0, 40));
  }
  return values.filter(Boolean).join(" ").slice(0, textPreviewLimit);
}

function breadcrumb(node) {
  const names = [];
  let current = node;
  while (current && current.type !== "DOCUMENT" && names.length < MAX_BREADCRUMB_SEGMENTS) {
    const name = compactName(current.name);
    if (name) names.push(name);
    current = current.parent;
  }
  return names.reverse();
}

const scopeAnchor = requestedPage ? page : !isScreenLike(requestedAnchor) && nearestSection
  ? nearestSection
  : pagePeer;
const anchorBounds = bounds(scopeAnchor) || bounds(requestedAnchor);
if (!anchorBounds) throw new Error("DEVUP_NODE_BOUNDS_UNAVAILABLE");
const pageChildren = "children" in page ? page.children : [];
const eligible = pageChildren
  .map((node, pageChildIndex) => ({ node, pageChildIndex, bounds: bounds(node) }))
  .filter((entry) => entry.bounds)
  .filter((entry) => {
    if (requestedPage) return true;
    if (entry.node.id === scopeAnchor.id) return true;
    const verticallyNear = entry.bounds.y >= anchorBounds.y - 240
      && entry.bounds.y <= anchorBounds.y + 12000;
    return verticallyNear && horizontalOverlap(entry.bounds, anchorBounds) > 0;
  })
  .sort((left, right) =>
    left.bounds.y - right.bounds.y
      || left.bounds.x - right.bounds.x
      || left.node.id.localeCompare(right.node.id),
  );
const selected = scopeAnchor.type === "SECTION"
  ? eligible.filter((entry) => entry.node.id === scopeAnchor.id)
  : eligible.slice(0, projectionLimit);
const included = new Map(selected.map((entry) => [entry.node.id, entry]));
if (!included.has(pagePeer.id)) {
  included.set(pagePeer.id, { node: pagePeer, pageChildIndex: -1, bounds: bounds(pagePeer) });
}
if (!included.has(scopeAnchor.id)) {
  included.set(scopeAnchor.id, { node: scopeAnchor, pageChildIndex: -1, bounds: bounds(scopeAnchor) });
}
if (!included.has(requestedAnchor.id)) {
  included.set(requestedAnchor.id, {
    node: requestedAnchor,
    pageChildIndex: -1,
    bounds: bounds(requestedAnchor),
  });
}
let requiredAncestor = requestedPage ? null : requestedAnchor.parent;
while (requiredAncestor && requiredAncestor.type !== "PAGE") {
  if (!included.has(requiredAncestor.id)) {
    included.set(requiredAncestor.id, {
      node: requiredAncestor,
      pageChildIndex: -1,
      bounds: bounds(requiredAncestor),
    });
  }
  if (requiredAncestor.id === scopeAnchor.id) break;
  requiredAncestor = requiredAncestor.parent;
}
if ("children" in scopeAnchor) {
  for (const child of scopeAnchor.children.slice(0, projectionLimit)) {
    if (!included.has(child.id)) {
      included.set(child.id, { node: child, pageChildIndex: -1, bounds: bounds(child) });
    }
  }
}
const descendantScope = scopeAnchor.type === "SECTION" || requestedPage;
let sectionTraversalTruncated = descendantScope
  && "children" in scopeAnchor
  && scopeAnchor.children.length > projectionLimit;
if (descendantScope && "children" in scopeAnchor) {
  const traversalLimit = projectionLimit * 8;
  const nestedQueue = scopeAnchor.children
    .filter((node) => !isScreenLike(node))
    .map((node) => ({ node, ancestors: [] }));
  let visited = 0;
  while (nestedQueue.length && visited < traversalLimit) {
    const { node, ancestors } = nestedQueue.shift();
    visited += 1;
    if (isScreenLike(node)) {
      const missingChain = [...ancestors, node]
        .filter((candidate) => !included.has(candidate.id));
      if (included.size + missingChain.length > projectionLimit + 2) {
        sectionTraversalTruncated = true;
        continue;
      }
      for (const candidate of missingChain) {
        included.set(candidate.id, {
          node: candidate,
          pageChildIndex: -1,
          bounds: bounds(candidate),
        });
      }
      continue;
    }
    if ("children" in node) {
      for (const child of node.children) {
        if (requestedPage && node.type !== "SECTION") continue;
        if (requestedPage && !["SECTION", "FRAME", "COMPONENT", "INSTANCE", "COMPONENT_SET"].includes(child.type)) continue;
        nestedQueue.push({ node: child, ancestors: [...ancestors, node] });
      }
    }
  }
  sectionTraversalTruncated ||= nestedQueue.length > 0;
}

const compact = [...included.values()]
  .filter((entry) => entry.bounds)
  .slice(0, projectionLimit + 2)
  .map(({ node, pageChildIndex, bounds: box }) => ({
    id: node.id,
    type: node.type,
    fields: {
      name: compactName(node.name),
      parentId: node.parent && node.parent.type !== "DOCUMENT" ? node.parent.id : null,
      childrenIds: [],
      x: box.x,
      y: box.y,
      width: box.width,
      height: box.height,
      childCount: "children" in node ? node.children.length : 0,
      // Select the structural projection before spending space on previews.
      textPreview: "",
      ...(requestedPage ? {} : navigationSummary(node)),
      pageChildIndex: pageChildIndex >= 0 ? pageChildIndex : null,
      // A page or the document itself has no `visible`, and Figma throws on
      // reading a property a node does not have rather than returning
      // undefined — so exploring from a page id failed outright.
      visible: !("visible" in node) || node.visible !== false,
      breadcrumb: breadcrumb(node),
    },
    extra: {},
    fieldErrors: {},
  }));

const projectedBounds = compact.reduce((result, node) => {
  const box = node.fields;
  if (!result) return { x: box.x, y: box.y, right: box.x + box.width, bottom: box.y + box.height };
  return {
    x: Math.min(result.x, box.x),
    y: Math.min(result.y, box.y),
    right: Math.max(result.right, box.x + box.width),
    bottom: Math.max(result.bottom, box.y + box.height),
  };
}, null) || { x: 0, y: 0, right: 0, bottom: 0 };
const projectionTruncated = (scopeAnchor.type !== "SECTION" && eligible.length > selected.length)
  || included.size > projectionLimit + 2
  || sectionTraversalTruncated;
const pageNode = {
  id: page.id,
  type: page.type,
  fields: {
    name: compactName(page.name),
    parentId: null,
    childrenIds: [],
    x: projectedBounds.x,
    y: projectedBounds.y,
    width: projectedBounds.right - projectedBounds.x,
    height: projectedBounds.bottom - projectedBounds.y,
    childCount: pageChildren.length,
    textPreview: "",
    projectionTruncated,
    visible: true,
    breadcrumb: breadcrumb(page),
    pageChildIndex: null,
  },
  extra: {},
  fieldErrors: {},
};

const output = {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: [page.id],
  nodes: [pageNode, ...compact.filter((node) => node.id !== page.id)],
  diagnostics: [],
};
const requiredIds = new Set([
  page.id,
  pagePeer.id,
  scopeAnchor.id,
  requestedAnchor.id,
]);
while (JSON.stringify(output).length > MAX_PROJECTION_JSON_CHARS) {
  let removableIndex = output.nodes.length - 1;
  while (removableIndex >= 0 && requiredIds.has(output.nodes[removableIndex].id)) {
    removableIndex -= 1;
  }
  if (removableIndex < 0) break;
  output.nodes.splice(removableIndex, 1);
  pageNode.fields.projectionTruncated = true;
}

if (JSON.stringify(output).length > MAX_PROJECTION_JSON_CHARS) {
  output.nodes = output.nodes
    .filter((node) => requiredIds.has(node.id))
    .map((node) => ({
      ...node,
      fields: {
        ...node.fields,
        name: compactName(node.fields.name).slice(0, 80),
        textPreview: "",
        breadcrumb: node.fields.breadcrumb.slice(-4).map((name) => name.slice(0, 80)),
        projectionTruncated: node.id === page.id ? true : node.fields.projectionTruncated,
      },
    }));
}
if (JSON.stringify(output).length > MAX_PROJECTION_JSON_CHARS) {
  throw new Error("DEVUP_EXPLORE_PROJECTION_TOO_LARGE");
}

// Optional previews may only consume space left by the structural projection.
// Page fallback can collect names across the page; bound UTF-8 bytes as well
// as the existing character budget on the metered text transport.
function utf8Bytes(value) {
  let count = 0;
  for (const character of value) {
    const point = character.codePointAt(0);
    count += point < 0x80 ? 1 : point < 0x800 ? 2 : point < 0x10000 ? 3 : 4;
  }
  return count;
}
const PAGE_TEXT_BYTES = 19 * 1024;
if (requestedPage) {
  while (utf8Bytes(JSON.stringify(output)) > PAGE_TEXT_BYTES && output.nodes.length > 1) {
    output.nodes.pop();
    pageNode.fields.projectionTruncated = true;
  }
  if (utf8Bytes(JSON.stringify(output)) > PAGE_TEXT_BYTES) throw new Error("DEVUP_EXPLORE_PROJECTION_TOO_LARGE");
}
// Count JSON escaping as well, so quotes/control characters cannot push a
// preview over the envelope cap or evict a candidate.
let previewBudget = MAX_PROJECTION_JSON_CHARS - JSON.stringify(output).length;
if (requestedPage) previewBudget = Math.min(previewBudget, PAGE_TEXT_BYTES - utf8Bytes(JSON.stringify(output)));
if (textPreviewLimit > 0 && previewBudget > 0) {
  // Each candidate takes an equal share of what is left rather than as much
  // as it wants, so the last one is funded before the first is indulged.
  // Spending the budget first-come left the tail with nothing at all, and a
  // candidate with no text is one the caller cannot tell from its
  // neighbours — which is the only thing a preview is for. What a candidate
  // does not spend stays in the budget and widens the shares after it.
  const fundable = output.nodes.filter(
    (projected) => included.get(projected.id) && projected.id !== page.id,
  );
  let unfunded = fundable.length;
  for (const projected of fundable) {
    const share = Math.floor(previewBudget / unfunded);
    unfunded -= 1;
    if (share <= 0) continue;
    let preview = "";
    let spent = 0;
    for (const character of textPreview(included.get(projected.id).node)) {
      const cost = (requestedPage ? utf8Bytes(JSON.stringify(character)) : JSON.stringify(character).length) - 2;
      if (spent + cost > share) break;
      preview += character;
      spent += cost;
    }
    projected.fields.textPreview = preview;
    previewBudget -= spent;
  }
}

return output;
