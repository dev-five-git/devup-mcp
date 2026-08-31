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

let anchorPeer = anchor;
while (anchorPeer.parent && anchorPeer.parent.type !== "PAGE") anchorPeer = anchorPeer.parent;
let current = anchorPeer.parent;
while (current && current.type !== "PAGE") current = current.parent;
const page = anchor.type === "PAGE" ? anchor : current;
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

function horizontalOverlap(left, right) {
  return Math.min(left.x + left.width, right.x + right.width) - Math.max(left.x, right.x);
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

const anchorBounds = bounds(anchorPeer) || bounds(anchor);
if (!anchorBounds) throw new Error("DEVUP_NODE_BOUNDS_UNAVAILABLE");
const pageChildren = "children" in page ? page.children : [];
const eligible = pageChildren
  .map((node, pageChildIndex) => ({ node, pageChildIndex, bounds: bounds(node) }))
  .filter((entry) => entry.bounds)
  .filter((entry) => {
    if (entry.node.id === anchorPeer.id) return true;
    const verticallyNear = entry.bounds.y >= anchorBounds.y - 240
      && entry.bounds.y <= anchorBounds.y + 12000;
    return verticallyNear && horizontalOverlap(entry.bounds, anchorBounds) > 0;
  })
  .sort((left, right) =>
    left.bounds.y - right.bounds.y
      || left.bounds.x - right.bounds.x
      || left.node.id.localeCompare(right.node.id),
  );
const selected = eligible.slice(0, projectionLimit);
const included = new Map(selected.map((entry) => [entry.node.id, entry]));
if (!included.has(anchorPeer.id)) {
  included.set(anchorPeer.id, { node: anchorPeer, pageChildIndex: -1, bounds: bounds(anchorPeer) });
}
if (!included.has(anchor.id)) {
  included.set(anchor.id, { node: anchor, pageChildIndex: -1, bounds: bounds(anchor) });
}
if ("children" in anchor) {
  for (const child of anchor.children.slice(0, projectionLimit)) {
    if (!included.has(child.id)) {
      included.set(child.id, { node: child, pageChildIndex: -1, bounds: bounds(child) });
    }
  }
}

const compact = [...included.values()]
  .filter((entry) => entry.bounds)
  .slice(0, projectionLimit + 2)
  .map(({ node, pageChildIndex, bounds: box }) => ({
    id: node.id,
    type: node.type,
    fields: {
      name: typeof node.name === "string" ? node.name : "",
      parentId: node.parent && node.parent.type !== "DOCUMENT" ? node.parent.id : null,
      childrenIds: [],
      x: box.x,
      y: box.y,
      width: box.width,
      height: box.height,
      childCount: "children" in node ? node.children.length : 0,
      textPreview: textPreview(node),
      pageChildIndex,
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
const projectionTruncated = eligible.length > selected.length
  || included.size > projectionLimit + 2;
const pageNode = {
  id: page.id,
  type: page.type,
  fields: {
    name: page.name,
    parentId: null,
    childrenIds: [],
    x: projectedBounds.x,
    y: projectedBounds.y,
    width: projectedBounds.right - projectedBounds.x,
    height: projectedBounds.bottom - projectedBounds.y,
    childCount: pageChildren.length,
    textPreview: "",
    projectionTruncated,
  },
  extra: {},
  fieldErrors: {},
};

return {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: [page.id],
  nodes: [pageNode, ...compact.filter((node) => node.id !== page.id)],
  diagnostics: [],
};
