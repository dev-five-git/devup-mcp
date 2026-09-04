const root = await figma.getNodeByIdAsync("__DEVUP_NODE_ID__");
if (!root) throw new Error("DEVUP_NODE_NOT_FOUND");

const manifest = "__DEVUP_PLUGIN_API_MANIFEST__";
const manifestSet = new Set(manifest);
const textSegmentManifest = "__DEVUP_TEXT_SEGMENT_MANIFEST__";
const snapshotOptions = "__DEVUP_SNAPSHOT__";
const offset = Math.max(0, Math.floor(Number(snapshotOptions.offset) || 0));
const maxPayloadBytes = Math.min(
  16000,
  Math.max(4096, Math.floor(Number(snapshotOptions.maxPayloadBytes) || 12000)),
);
const maxFieldBytes = Math.min(
  maxPayloadBytes - 1024,
  Math.max(512, Math.floor(Number(snapshotOptions.maxFieldBytes) || 4096)),
);
const skipped = new Set(["id", "type", "parent", "children"]);
const hardProtectedFields = new Set([
  "parentId",
  "childrenIds",
  "name",
  "characters",
  "styledTextSegments",
  "boundVariables",
]);

"__DEVUP_LARGE_VALUE_HELPERS__";

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

function jsonByteLength(value) {
  return utf8ByteLength(JSON.stringify(value));
}

function propertyNames(value) {
  const names = new Set();
  let current = value;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  for (const name of manifest) {
    try {
      if (name in value) names.add(name);
    } catch (_) {}
  }
  return [...names].sort();
}

function serialize(value, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return { $undefined: true };
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (["function", "symbol"].includes(typeof value)) return { $unsupported: typeof value };
  if (depth > 12) return { $truncated: "max-depth" };
  if (
    typeof value === "object" &&
    "parent" in value &&
    typeof value.id === "string" &&
    typeof value.type === "string"
  ) {
    return { $nodeId: value.id, $nodeType: value.type };
  }
  if (Array.isArray(value)) return value.map((item) => serialize(item, seen, depth + 1));
  if (ArrayBuffer.isView(value)) {
    return { $binary: value.constructor.name, byteLength: value.byteLength };
  }
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

function truncatedValue(reason, byteLength) {
  return { $truncated: reason, byteLength };
}

function serializeField(nodeId, name, value, fieldErrors) {
  const serialized = serialize(value);
  const byteLength = jsonByteLength(serialized);
  if (byteLength <= maxFieldBytes) return serialized;
  const descriptor = devupLargeValueDescriptor(nodeId, name, serialized);
  if (descriptor.$truncated) {
    fieldErrors[name] = `DEVUP_FIELD_VALUE_UNSUPPORTED:${byteLength}>${DEVUP_MAX_LARGE_VALUE_BYTES}`;
  }
  return descriptor;
}

function isHardProtectedField(name) {
  return hardProtectedFields.has(name) || name.endsWith("StyleId");
}

function shrinkNodeToBudget(node, budget) {
  let currentBytes = jsonByteLength(node);
  if (currentBytes <= budget) return node;

  const candidates = [];
  for (const [sectionName, section] of [
    ["extra", node.extra],
    ["fields", node.fields],
  ]) {
    for (const [name, value] of Object.entries(section)) {
      if (value && typeof value === "object" && ("$truncated" in value || "$largeValue" in value)) {
        continue;
      }
      candidates.push({
        sectionName,
        name,
        byteLength: jsonByteLength(value),
        protected: sectionName === "fields" && isHardProtectedField(name),
      });
    }
  }
  candidates.sort(
    (left, right) =>
      Number(left.protected) - Number(right.protected) || right.byteLength - left.byteLength,
  );

  for (const candidate of candidates) {
    if (currentBytes <= budget) break;
    const descriptor = devupLargeValueDescriptor(
      node.id,
      candidate.name,
      node[candidate.sectionName][candidate.name],
    );
    node[candidate.sectionName][candidate.name] = descriptor;
    if (descriptor.$truncated) {
      node.fieldErrors[candidate.name] =
        `DEVUP_FIELD_VALUE_UNSUPPORTED:${candidate.byteLength}>${DEVUP_MAX_LARGE_VALUE_BYTES}`;
    }
    currentBytes = jsonByteLength(node);
  }
  return node;
}

function snapshotNode(node) {
  const fields = {};
  const extra = {};
  const fieldErrors = {};
  fields.parentId = node.parent ? node.parent.id : null;
  // Only the root needs this. Its parent lies outside the collected subtree,
  // so the id alone says nothing, and the parent's type is what decides
  // whether the root's width is a real constraint or merely the canvas the
  // design was drawn on. Every other node's parent is collected and can be
  // read directly.
  // Keyed on the parent's type rather than on being the requested root, so a
  // node carries the same fields however it is reached. See fast_snapshot.js.
  if (
    node.parent &&
    (node.parent.type === "PAGE" ||
      node.parent.type === "SECTION" ||
      node.parent.type === "COMPONENT_SET")
  ) {
    fields.parentType = node.parent.type;
  }
  fields.childrenIds = "children" in node ? node.children.map((child) => child.id) : [];

  for (const name of propertyNames(node)) {
    if (skipped.has(name) || name.startsWith("_")) continue;
    try {
      const value = node[name];
      if (typeof value === "function") continue;
      const serialized = serializeField(node.id, name, value, fieldErrors);
      (manifestSet.has(name) ? fields : extra)[name] = serialized;
    } catch (error) {
      fieldErrors[name] = String(error && error.message ? error.message : error);
    }
  }
  if (node.type === "TEXT" && typeof node.getStyledTextSegments === "function") {
    try {
      fields.styledTextSegments = serializeField(
        node.id,
        "styledTextSegments",
        node.getStyledTextSegments(textSegmentManifest),
        fieldErrors,
      );
    } catch (error) {
      fieldErrors.styledTextSegments = String(error && error.message ? error.message : error);
    }
  }
  return { id: node.id, type: node.type, fields, extra, fieldErrors };
}

const allNodes = [];
const queue = [root];
while (queue.length) {
  const node = queue.shift();
  allNodes.push(node);
  if ("children" in node) queue.push(...node.children);
}

const nodes = [];
const nodeBudget = maxPayloadBytes - 1024;
let payloadBytes = 2;
for (let index = offset; index < allNodes.length; index += 1) {
  const node = shrinkNodeToBudget(snapshotNode(allNodes[index]), nodeBudget);
  const nodeBytes = jsonByteLength(node) + (nodes.length ? 1 : 0);
  if (nodes.length && payloadBytes + nodeBytes > nodeBudget) break;
  nodes.push(node);
  payloadBytes += nodeBytes;
}

const nextOffset = Math.min(allNodes.length, offset + nodes.length);
nodes.push({
  id: "__DEVUP_SNAPSHOT_CURSOR__",
  type: "DEVUP_INTERNAL",
  // Same marker shape as the fast snapshot so both paths go through the one
  // `read_snapshot_cursor` reader in Rust.
  fields: {
    offset,
    nextOffset,
    complete: nextOffset >= allNodes.length,
    totalNodes: allNodes.length,
  },
  extra: {},
  fieldErrors: {},
});

return {
  fileKey: figma.fileKey || "",
  version: null,
  rootIds: [root.id],
  nodes,
  diagnostics: [],
};
