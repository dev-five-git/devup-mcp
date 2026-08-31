const page = await figma.getNodeByIdAsync("__DEVUP_NODE_ID__");
if (!page || page.type !== "PAGE") throw new Error("DEVUP_PAGE_NOT_FOUND");
await figma.setCurrentPageAsync(page);

const options = "__DEVUP_SEARCH__";
const defaultTypes = ["PAGE", "SECTION", "FRAME", "COMPONENT_SET", "COMPONENT"];
const allowedTypes = new Set(
  (options.nodeTypes.length ? options.nodeTypes : defaultTypes).map((value) => value.toUpperCase()),
);

function normalize(value) {
  return value.normalize("NFC").toLocaleLowerCase().replace(/\s/gu, "");
}

function levenshtein(left, right) {
  let previous = Array.from({ length: right.length + 1 }, (_, index) => index);
  for (let leftIndex = 0; leftIndex < left.length; leftIndex += 1) {
    const current = [leftIndex + 1];
    for (let rightIndex = 0; rightIndex < right.length; rightIndex += 1) {
      current.push(
        Math.min(
          current[rightIndex] + 1,
          previous[rightIndex + 1] + 1,
          previous[rightIndex] + (left[leftIndex] === right[rightIndex] ? 0 : 1),
        ),
      );
    }
    previous = current;
  }
  return previous[right.length];
}

function score(name) {
  if (name === options.query) return 400;
  if (options.matchKind === "exact") return null;
  const candidate = normalize(name);
  const query = normalize(options.query);
  if (candidate === query) return 300;
  if (candidate.startsWith(query)) return 200;
  if (candidate.includes(query)) return 100;
  if (options.matchKind !== "fuzzy") return null;
  const distance = levenshtein(Array.from(candidate), Array.from(query));
  const threshold = Math.min(4, Math.max(1, Math.ceil(Array.from(query).length / 4)));
  return distance <= threshold ? Math.max(0, 50 - distance) : null;
}

const candidates = [page, ...page.findAll(() => true)]
  .filter((node) => allowedTypes.has(node.type) && typeof node.name === "string")
  .map((node) => ({ node, score: score(node.name) }))
  .filter((entry) => entry.score !== null)
  .sort((left, right) =>
    right.score - left.score ||
    left.node.name.localeCompare(right.node.name) ||
    left.node.id.localeCompare(right.node.id),
  )
  .slice(0, options.limit);

const included = new Map([[page.id, page]]);
for (const { node } of candidates) {
  let current = node;
  while (current && current.type !== "DOCUMENT") {
    included.set(current.id, current);
    current = current.parent;
  }
}

return {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: [page.id],
  nodes: [...included.values()].map((node) => ({
    id: node.id,
    type: node.type,
    fields: {
      name: node.name,
      parentId: node.parent && node.parent.type !== "DOCUMENT" ? node.parent.id : null,
      childrenIds: "children" in node ? node.children.map((child) => child.id) : [],
    },
    extra: {},
    fieldErrors: {},
  })),
  diagnostics: [],
};
