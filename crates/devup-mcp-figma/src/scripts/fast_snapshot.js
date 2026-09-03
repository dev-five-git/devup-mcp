const requestedRootIds = "__DEVUP_ROOT_IDS__";
if (!Array.isArray(requestedRootIds) || requestedRootIds.length === 0) {
  throw new Error("DEVUP_ROOTS_INVALID");
}
const roots = await Promise.all(requestedRootIds.map((id) => figma.getNodeByIdAsync(id)));
if (roots.some((root) => !root)) throw new Error("DEVUP_NODE_NOT_FOUND");
if (roots.length === 1 && roots[0].type === "SECTION") {
  throw new Error("DEVUP_TARGET_IS_SECTION");
}
const envelopeRootId = "__DEVUP_NODE_ID__";

const manifest = "__DEVUP_PLUGIN_API_MANIFEST__";
const textSegmentManifest = "__DEVUP_TEXT_SEGMENT_MANIFEST__";
const pageOptions = "__DEVUP_SNAPSHOT__";
const offset = Math.max(0, Math.floor(Number(pageOptions.offset) || 0));
// Upper bound for one round's serialized payload. Kept well under the ~20,500
// character Figma MCP text-response limit so a page always survives as text
// (no PNG fallback exists any more).
const maxPayloadBytes = Math.min(
  18000,
  Math.max(4096, Math.floor(Number(pageOptions.maxPayloadBytes) || 12000)),
);
const MAX_TEXT_ENVELOPE_BYTES = 15 * 1024;
const MAX_ENVELOPE_BYTES = 1024 * 1024;

// Values that carry no information beyond "this field is at its default" are
// dropped from the envelope. Consumers must treat an absent key exactly like
// its default (this already holds for every accessor in the Rust codegen,
// which reads through Option-returning TypedNode helpers).
//
// `""` is only dropped for *StyleId fields: Figma reports an unbound style
// as `""`, and both consumers of these fields already treat `""` and
// "field absent" identically —
//   - `resources.rs::is_resource_id` rejects empty IDs before treating a
//     *StyleId field as a real style reference (used by both the fast-path
//     JS resource scanner above and the legacy Rust scanner), and
//   - `codegen/text.rs` looks `textStyleId` up in a token map, where an
//     empty-string key can never match (same `None` result as a missing key).
const STYLE_ID_FIELDS = new Set([
  "backgroundStyleId",
  "effectStyleId",
  "fillStyleId",
  "gridStyleId",
  "strokeStyleId",
  "textStyleId",
]);
function isOmittableDefault(value, name) {
  if (value === null) return true;
  if (Array.isArray(value) && value.length === 0) return true;
  if (value === "" && STYLE_ID_FIELDS.has(name)) return true;
  return false;
}

function propertyNames(value) {
  // Only ever look at the checked-in manifest. No prototype-chain walk, no
  // "extra" bucket: an unlisted Figma Plugin API property is never collected.
  const names = [];
  for (const name of manifest) {
    try {
      if (name in value) names.push(name);
    } catch (_) {}
  }
  return names;
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

function snapshotNode(node) {
  const fields = {};
  const fieldErrors = {};
  fields.parentId = node.parent ? node.parent.id : null;
  fields.childrenIds = "children" in node ? node.children.map((child) => child.id) : [];

  for (const name of propertyNames(node)) {
    try {
      const value = node[name];
      if (typeof value === "function") continue;
      const serialized = serialize(value);
      if (!isOmittableDefault(serialized, name)) fields[name] = serialized;
    } catch (error) {
      fieldErrors[name] = String(error && error.message ? error.message : error);
    }
  }
  if (node.type === "TEXT" && typeof node.getStyledTextSegments === "function") {
    try {
      const segments = serialize(node.getStyledTextSegments(textSegmentManifest));
      if (!isOmittableDefault(segments)) fields.styledTextSegments = segments;
    } catch (error) {
      fieldErrors.styledTextSegments = String(error && error.message ? error.message : error);
    }
  }
  return { id: node.id, type: node.type, fields, extra: {}, fieldErrors };
}

const allNodes = [];
const queue = [...roots];
const visitedNodeIds = new Set();
for (let index = 0; index < queue.length; index += 1) {
  const node = queue[index];
  if (visitedNodeIds.has(node.id)) continue;
  visitedNodeIds.add(node.id);
  allNodes.push(node);
  if ("children" in node) queue.push(...node.children);
}
if (offset >= allNodes.length && allNodes.length > 0) {
  throw new Error("DEVUP_SNAPSHOT_RANGE_INVALID");
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

function jsonByteLength(value) {
  return utf8ByteLength(JSON.stringify(value));
}

// Pack as many nodes as fit under maxPayloadBytes starting at offset. This is
// the same dynamic, byte-budget-driven pagination the legacy cursor snapshot
// already uses, applied to the fast (single-call, resource-inclusive) path.
const pageNodeBudget = maxPayloadBytes - 1024;
const pageNodes = [];
let pagePayloadBytes = 2;
for (let index = offset; index < allNodes.length; index += 1) {
  const snapshotted = snapshotNode(allNodes[index]);
  const nodeBytes = jsonByteLength(snapshotted) + (pageNodes.length ? 1 : 0);
  if (pageNodes.length && pagePayloadBytes + nodeBytes > pageNodeBudget) break;
  pageNodes.push(snapshotted);
  pagePayloadBytes += nodeBytes;
}
const nextOffset = Math.min(allNodes.length, offset + pageNodes.length);
const complete = nextOffset >= allNodes.length;
const nodes = pageNodes;
nodes.push({
  id: "__DEVUP_SNAPSHOT_CURSOR__",
  type: "DEVUP_INTERNAL",
  // `offset` is what lets the Rust decoder tell a first page from a
  // continuation page, which in turn decides whether the root must be
  // present in this page. `nextOffset`/`complete`/`totalNodes` are the
  // fields the shared `take_snapshot_cursor` reads.
  fields: { offset, nextOffset, complete, totalNodes: allNodes.length },
  extra: {},
  fieldErrors: {},
});

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
// Only the nodes actually shipped in THIS page are scanned, so the resources
// this page returns stay self-consistent with this page's own integrity
// counters. The host (devup-mcp) merges resources across pages.
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
  kind: "devupFastSnapshotEnvelope",
  schemaVersion: 1,
  source: { fileKey: figma.fileKey || "", rootId: envelopeRootId },
  snapshot: {
    fileKey: figma.fileKey || "",
    version: null,
    rootIds: roots.map((root) => root.id),
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
  pagination: { offset, nextOffset, complete, totalNodes: allNodes.length },
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
if (envelopeBytes.length > MAX_TEXT_ENVELOPE_BYTES) {
  // The byte budget above is sized to stay under this safety margin; a
  // single misbehaving node (huge boundVariables/componentProperties tree)
  // is the only way to reach here. Surface it as a hard error instead of
  // silently falling back to an unsupported binary transport.
  throw new Error("DEVUP_ENVELOPE_TOO_LARGE");
}
return envelope;
