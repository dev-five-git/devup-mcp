const root = await figma.getNodeByIdAsync("__DEVUP_NODE_ID__");
if (!root) throw new Error("DEVUP_NODE_NOT_FOUND");

const manifest = "__DEVUP_PLUGIN_API_MANIFEST__";
const manifestSet = new Set(manifest);
const skipped = new Set(["id", "type", "parent", "children"]);

function propertyNames(value) {
  const names = new Set(manifest);
  let current = value;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  return [...names].sort();
}

function serialize(value, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return { $undefined: true };
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (["function", "symbol"].includes(typeof value)) return { $unsupported: typeof value };
  if (depth > 12) return { $truncated: "max-depth" };
  if (typeof value === "object" && typeof value.id === "string" && typeof value.type === "string") {
    return { $nodeId: value.id, $nodeType: value.type };
  }
  if (Array.isArray(value)) return value.map((item) => serialize(item, seen, depth + 1));
  if (ArrayBuffer.isView(value)) return { $binary: value.constructor.name, byteLength: value.byteLength };
  if (value instanceof ArrayBuffer) return { $binary: "ArrayBuffer", byteLength: value.byteLength };
  if (seen.has(value)) return { $circular: true };
  seen.add(value);
  const result = {};
  for (const key of Object.keys(value).sort()) {
    try {
      const serialized = serialize(value[key], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[key] = serialized;
    } catch (error) {
      result[key] = { $error: String(error && error.message ? error.message : error) };
    }
  }
  seen.delete(value);
  return result;
}

function snapshotNode(node) {
  const fields = {};
  const extra = {};
  const fieldErrors = {};
  fields.parentId = node.parent ? node.parent.id : null;
  fields.childrenIds = "children" in node ? node.children.map((child) => child.id) : [];

  for (const name of propertyNames(node)) {
    if (skipped.has(name) || name.startsWith("_")) continue;
    try {
      const value = node[name];
      if (typeof value === "function") continue;
      const serialized = serialize(value);
      (manifestSet.has(name) ? fields : extra)[name] = serialized;
    } catch (error) {
      fieldErrors[name] = String(error && error.message ? error.message : error);
    }
  }
  return { id: node.id, type: node.type, fields, extra, fieldErrors };
}

const nodes = [];
const queue = [root];
while (queue.length) {
  const node = queue.shift();
  nodes.push(snapshotNode(node));
  if ("children" in node) queue.push(...node.children);
}

return {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: [root.id],
  nodes,
  diagnostics: []
};
