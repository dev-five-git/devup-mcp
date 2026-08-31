const root = await figma.getNodeByIdAsync("__DEVUP_NODE_ID__");
if (!root) throw new Error("DEVUP_NODE_NOT_FOUND");

const manifest = "__DEVUP_PLUGIN_API_MANIFEST__";
const manifestSet = new Set(manifest);
const textSegmentManifest = "__DEVUP_TEXT_SEGMENT_MANIFEST__";
const skipped = new Set(["id", "type", "parent", "children"]);
const MAX_ENVELOPE_BYTES = 8 * 1024 * 1024;
const MAX_ENVELOPE_CHUNK_BYTES = 512 * 1024;
const MAX_INLINE_FIELD_BYTES = 64 * 1024;

"__DEVUP_LARGE_VALUE_HELPERS__";

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

function serializeField(nodeId, field, value, fieldErrors) {
  const serialized = serialize(value);
  const bytes = devupUtf8Encode(JSON.stringify(serialized));
  if (bytes.length <= MAX_INLINE_FIELD_BYTES) return serialized;
  const descriptor = devupLargeValueDescriptor(nodeId, field, serialized);
  if (descriptor.$truncated) {
    fieldErrors[field] =
      `DEVUP_FIELD_VALUE_UNSUPPORTED:${bytes.length}>${DEVUP_MAX_LARGE_VALUE_BYTES}`;
  }
  return descriptor;
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
for (let index = 0; index < queue.length; index += 1) {
  const node = queue[index];
  allNodes.push(node);
  if ("children" in node) queue.push(...node.children);
}
const nodes = allNodes.map(snapshotNode);

function styleTypeForField(field) {
  if (field === "textStyleId") return "TEXT";
  if (["fillStyleId", "strokeStyleId", "backgroundStyleId"].includes(field)) return "PAINT";
  if (field === "effectStyleId") return "EFFECT";
  if (field === "gridStyleId") return "GRID";
  return null;
}

const variableIds = new Set();
const styleTypes = new Map();
function scanResources(value, fieldName = "") {
  if (Array.isArray(value)) {
    for (const child of value) scanResources(child, fieldName);
    return;
  }
  if (!value || typeof value !== "object") return;
  if (
    value.type === "VARIABLE_ALIAS" &&
    typeof value.id === "string" &&
    value.id &&
    value.id !== "figma.mixed" &&
    value.id !== "MIXED"
  ) {
    variableIds.add(value.id);
  }
  for (const [field, child] of Object.entries(value)) {
    const styleType = styleTypeForField(field);
    if (
      styleType &&
      typeof child === "string" &&
      child &&
      child !== "figma.mixed" &&
      child !== "MIXED"
    ) {
      if (!styleTypes.has(child)) styleTypes.set(child, styleType);
    }
    scanResources(child, field || fieldName);
  }
}
scanResources(nodes);

function resourcePropertyNames(value) {
  const names = new Set(Object.keys(value));
  let current = value;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  return [...names].sort();
}

function serializeResource(value, seen = new WeakSet(), depth = 0) {
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
  if (Array.isArray(value)) {
    return value.map((item) => serializeResource(item, seen, depth + 1));
  }
  if (ArrayBuffer.isView(value)) {
    return { $binary: value.constructor.name, byteLength: value.byteLength };
  }
  if (value instanceof ArrayBuffer) return { $binary: "ArrayBuffer", byteLength: value.byteLength };
  if (seen.has(value)) return { $circular: true };
  seen.add(value);
  const result = {};
  for (const name of resourcePropertyNames(value)) {
    if (name.startsWith("_") || ["parent", "children", "consumers"].includes(name)) continue;
    try {
      const serialized = serializeResource(value[name], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[name] = serialized;
    } catch (_) {
      result[name] = { $error: "unavailable" };
    }
  }
  seen.delete(value);
  return result;
}

const sortedVariableIds = [...variableIds].sort();
const sortedStyles = [...styleTypes.entries()]
  .map(([id, styleType]) => ({ id, styleType }))
  .sort((left, right) => left.id.localeCompare(right.id));
const variableJobs = sortedVariableIds.map(async (id) => {
  try {
    const variable = await figma.variables.getVariableByIdAsync(id);
    return variable
      ? {
          kind: "variable",
          value: serializeResource(variable),
          collectionId: variable.variableCollectionId,
        }
      : { kind: "unresolved", value: { id, kind: "variable", reason: "notFoundOrUnavailable" } };
  } catch (_) {
    return { kind: "unresolved", value: { id, kind: "variable", reason: "notFoundOrUnavailable" } };
  }
});
const styleJobs = sortedStyles.map(async ({ id, styleType }) => {
  try {
    const style = await figma.getStyleByIdAsync(id);
    if (!style) {
      return { kind: "unresolved", value: { id, kind: "style", reason: "notFoundOrUnavailable" } };
    }
    return {
      kind: "style",
      value: {
        ...serializeResource(style),
        styleType,
        value: serializeResource(
          styleType === "PAINT"
            ? style.paints
            : styleType === "EFFECT"
              ? style.effects
              : styleType === "GRID"
                ? style.layoutGrids
                : style,
        ),
      },
    };
  } catch (_) {
    return { kind: "unresolved", value: { id, kind: "style", reason: "notFoundOrUnavailable" } };
  }
});
const resourceResults = await Promise.all([...variableJobs, ...styleJobs]);
const collectionIds = [...new Set(resourceResults
  .filter((result) => result.kind === "variable" && result.collectionId)
  .map((result) => result.collectionId))].sort();
const collectionJobs = collectionIds.map(async (id) => {
  try {
    const collection = await figma.variables.getVariableCollectionByIdAsync(id);
    return collection ? serializeResource(collection) : null;
  } catch (_) {
    return null;
  }
});
const collections = (await Promise.all(collectionJobs)).filter((collection) => collection !== null);
const variables = resourceResults
  .filter((result) => result.kind === "variable")
  .map((result) => result.value);
const styles = resourceResults
  .filter((result) => result.kind === "style")
  .map((result) => result.value);
const unresolved = resourceResults
  .filter((result) => result.kind === "unresolved")
  .map((result) => result.value);

function utf8Encode(value) {
  const bytes = [];
  for (let index = 0; index < value.length; index += 1) {
    let codePoint = value.charCodeAt(index);
    if (codePoint >= 0xd800 && codePoint <= 0xdbff) {
      const next = index + 1 < value.length ? value.charCodeAt(index + 1) : 0;
      if (next >= 0xdc00 && next <= 0xdfff) {
        codePoint = 0x10000 + ((codePoint - 0xd800) << 10) + (next - 0xdc00);
        index += 1;
      } else {
        codePoint = 0xfffd;
      }
    } else if (codePoint >= 0xdc00 && codePoint <= 0xdfff) {
      codePoint = 0xfffd;
    }

    if (codePoint < 0x80) {
      bytes.push(codePoint);
    } else if (codePoint < 0x800) {
      bytes.push(0xc0 | (codePoint >> 6), 0x80 | (codePoint & 0x3f));
    } else if (codePoint < 0x10000) {
      bytes.push(
        0xe0 | (codePoint >> 12),
        0x80 | ((codePoint >> 6) & 0x3f),
        0x80 | (codePoint & 0x3f),
      );
    } else {
      bytes.push(
        0xf0 | (codePoint >> 18),
        0x80 | ((codePoint >> 12) & 0x3f),
        0x80 | ((codePoint >> 6) & 0x3f),
        0x80 | (codePoint & 0x3f),
      );
    }
  }
  return new Uint8Array(bytes);
}

const envelope = {
  schemaVersion: 1,
  source: { fileKey: figma.fileKey || "", rootId: root.id },
  snapshot: {
    fileKey: figma.fileKey || "",
    version: null,
    rootIds: [root.id],
    nodes,
    diagnostics: [],
  },
  resources: {
    collections,
    variables,
    styles,
    usedRemoteVariables: variables.filter((variable) => variable.remote === true),
    usedVariableIds: sortedVariableIds,
    usedStyleIds: sortedStyles.map((style) => style.id),
    localComplete: false,
    usedRemoteComplete: unresolved.length === 0,
    unresolved,
  },
  integrity: {
    nodeCount: nodes.length,
    variableRefCount: sortedVariableIds.length,
    styleRefCount: sortedStyles.length,
    utf8Bytes: 0,
  },
};

let envelopeBytes = new Uint8Array();
for (let attempt = 0; attempt < 8; attempt += 1) {
  envelopeBytes = utf8Encode(JSON.stringify(envelope));
  if (envelope.integrity.utf8Bytes === envelopeBytes.length) break;
  envelope.integrity.utf8Bytes = envelopeBytes.length;
}
envelopeBytes = utf8Encode(JSON.stringify(envelope));
if (envelope.integrity.utf8Bytes !== envelopeBytes.length) {
  throw new Error("DEVUP_ENVELOPE_LENGTH_UNSTABLE");
}
if (envelopeBytes.length > MAX_ENVELOPE_BYTES) {
  throw new Error("DEVUP_ENVELOPE_TOO_LARGE");
}

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function u32(value) {
  return new Uint8Array([
    (value >>> 24) & 0xff,
    (value >>> 16) & 0xff,
    (value >>> 8) & 0xff,
    value & 0xff,
  ]);
}

function ascii(value) {
  return new Uint8Array([...value].map((character) => character.charCodeAt(0)));
}

function concat(parts) {
  const length = parts.reduce((sum, part) => sum + part.length, 0);
  const output = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }
  return output;
}

function pngChunk(type, data) {
  const typeBytes = ascii(type);
  return concat([u32(data.length), typeBytes, data, u32(crc32(concat([typeBytes, data])))]);
}

const chunkCount = Math.ceil(envelopeBytes.length / MAX_ENVELOPE_CHUNK_BYTES);
for (let sequence = 0; sequence < chunkCount; sequence += 1) {
  const start = sequence * MAX_ENVELOPE_CHUNK_BYTES;
  const end = Math.min(envelopeBytes.length, start + MAX_ENVELOPE_CHUNK_BYTES);
  const envelopeChunk = pngChunk(
    "duVp",
    concat([u32(sequence), u32(chunkCount), envelopeBytes.slice(start, end)]),
  );
  const png = concat([
    new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk("IHDR", new Uint8Array([0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0])),
    envelopeChunk,
    pngChunk(
      "IDAT",
      new Uint8Array([120, 1, 1, 5, 0, 250, 255, 0, 0, 0, 0, 0, 5, 0, 1]),
    ),
    pngChunk("IEND", new Uint8Array()),
  ]);
  figma.io.write(`devup-fast-snapshot-${sequence + 1}-of-${chunkCount}.png`, png);
}
return {
  kind: "devupFastSnapshotDescriptor",
  schemaVersion: 1,
  rootId: root.id,
  nodeCount: nodes.length,
  variableRefCount: sortedVariableIds.length,
  styleRefCount: sortedStyles.length,
  utf8Bytes: envelopeBytes.length,
  chunkCount,
};
