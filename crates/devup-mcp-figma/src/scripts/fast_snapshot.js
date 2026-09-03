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

// A field whose value equals its default carries no information the converter
// can't recover from the key being absent, so it is dropped from the envelope.
// Which fields qualify is NOT a judgement call: it is proven for every rule
// below by `devup-mcp-devup-ui/tests/default_omission_golden.rs`, which
// replays this exact omission over ten real screens (1,500+ nodes) and
// requires the generated TSX to stay byte-identical. Keep the two tables in
// sync with that test.

// Figma reports an unbound style as `""`, and both readers of these fields
// already treat `""` and "absent" the same: `resources.rs::is_resource_id`
// rejects empty IDs, and `codegen/text.rs` looks the ID up in a token map
// where an empty key can never match.
const STYLE_ID_FIELDS = new Set([
  "backgroundStyleId",
  "effectStyleId",
  "fillStyleId",
  "gridStyleId",
  "strokeStyleId",
  "textStyleId",
]);

// `codegen/layout.rs` compares `view.value("maxWidth") != Some(&Value::Null)`,
// so for these two a present-null and an absent key take opposite branches.
// Their null must survive.
const NULL_SENSITIVE_FIELDS = new Set(["maxWidth", "maxHeight"]);

// Deliberately absent from this table, each because the converter branches on
// the field's *presence* rather than its value: `opacity` (hover-variant
// detection), `visible` (component registration snapshot), `layoutPositioning`
// (compared against "AUTO"), and the per-corner radii / per-side stroke
// weights (read as a group by the shorthand builders).
const SCALAR_DEFAULTS = new Map([
  ["rotation", 0],
  ["cornerRadius", 0],
  ["isAsset", false],
  ["isMask", false],
  ["clipsContent", false],
  ["blendMode", "PASS_THROUGH"],
  ["strokeAlign", "INSIDE"],
  ["textCase", "ORIGINAL"],
  ["textDecoration", "NONE"],
  ["textAlignHorizontal", "LEFT"],
  ["textAlignVertical", "TOP"],
  ["counterAxisAlignItems", "MIN"],
  ["primaryAxisAlignItems", "MIN"],
  ["gridColumnCount", 0],
  ["gridRowCount", 0],
  ["gridColumnGap", 0],
  ["gridRowGap", 0],
  ["gridColumnAnchorIndex", -1],
  ["gridRowAnchorIndex", -1],
]);

// Keys a styled text segment carries that the TEXT node itself does not, so
// they must survive even when the node has a single segment.
const SEGMENT_ONLY_KEYS = new Set([
  "start",
  "end",
  "characters",
  "fontWeight",
  "textStyleId",
  "fillStyleId",
  "listOptions",
  "indentation",
  "hyperlink",
]);

function isOmittableDefault(value, name) {
  if (value === null) return !NULL_SENSITIVE_FIELDS.has(name);
  if (Array.isArray(value)) return value.length === 0;
  if (typeof value === "object") return Object.keys(value).length === 0;
  if (value === "" && STYLE_ID_FIELDS.has(name)) return true;
  return SCALAR_DEFAULTS.has(name) && SCALAR_DEFAULTS.get(name) === value;
}

// One serializer for both node fields and variable/style resources. Resources
// need the prototype chain walked (their data lives on accessors, not own
// keys) and a few structural keys skipped; node fields never do, because the
// manifest already names every property worth reading.
const RESOURCE_SKIPPED_KEYS = new Set(["parent", "children", "consumers"]);
function serialize(value, resource = false, seen = new WeakSet(), depth = 0) {
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
  if (Array.isArray(value)) return value.map((item) => serialize(item, resource, seen, depth + 1));
  if (ArrayBuffer.isView(value)) {
    return { $binary: value.constructor.name, byteLength: value.byteLength };
  }
  if (value instanceof ArrayBuffer) return { $binary: "ArrayBuffer", byteLength: value.byteLength };
  if (seen.has(value)) return { $circular: true };
  seen.add(value);

  let keys;
  if (resource) {
    const names = new Set(Object.keys(value));
    let current = value;
    while (current && current !== Object.prototype) {
      for (const name of Object.getOwnPropertyNames(current)) names.add(name);
      current = Object.getPrototypeOf(current);
    }
    keys = [...names].sort().filter((name) => !name.startsWith("_") && !RESOURCE_SKIPPED_KEYS.has(name));
  } else {
    keys = Object.keys(value).sort();
  }

  const result = {};
  for (const key of keys) {
    try {
      const serialized = serialize(value[key], resource, seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[key] = serialized;
    } catch (error) {
      result[key] = resource
        ? { $error: "unavailable" }
        : { $error: String(error && error.message ? error.message : error) };
    }
  }
  seen.delete(value);
  return result;
}

function snapshotNode(node) {
  const fields = {};
  const fieldErrors = {};
  if (node.parent) fields.parentId = node.parent.id;
  const childrenIds = "children" in node ? node.children.map((child) => child.id) : [];
  if (childrenIds.length > 0) fields.childrenIds = childrenIds;

  // Only ever look at the checked-in manifest. No prototype-chain walk, no
  // "extra" bucket: an unlisted Figma Plugin API property is never collected.
  for (const name of manifest) {
    let value;
    try {
      if (!(name in node)) continue;
      value = node[name];
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
      // A single segment restates typography the node already carries at the
      // top level, and `codegen/text.rs` reads the node field first and only
      // falls back to the segment. Keep just the keys that exist nowhere else.
      // Proven over 269 real single-segment text nodes by
      // `devup-mcp-devup-ui/tests/default_omission_golden.rs`.
      if (segments.length === 1) {
        const only = segments[0];
        for (const key of Object.keys(only)) {
          if (!SEGMENT_ONLY_KEYS.has(key)) delete only[key];
        }
      }
      if (segments.length > 0) fields.styledTextSegments = segments;
    } catch (error) {
      fieldErrors.styledTextSegments = String(error && error.message ? error.message : error);
    }
  }
  // `extra` and `fieldErrors` are `#[serde(default)]` on the Rust `RawNode`,
  // so an empty one is the same as an absent one on the wire.
  const snapshotted = { id: node.id, type: node.type, fields };
  if (Object.keys(fieldErrors).length > 0) snapshotted.fieldErrors = fieldErrors;
  return snapshotted;
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

function styleTypeForField(field) {
  if (field === "textStyleId") return "TEXT";
  if (["fillStyleId", "strokeStyleId", "backgroundStyleId"].includes(field)) return "PAINT";
  if (field === "effectStyleId") return "EFFECT";
  if (field === "gridStyleId") return "GRID";
  return null;
}

function scanResources(value, variableIds, styleTypes) {
  if (Array.isArray(value)) {
    for (const child of value) scanResources(child, variableIds, styleTypes);
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
    scanResources(child, variableIds, styleTypes);
  }
}

// Resolves every variable/style the given page of nodes references. Only the
// nodes shipped in THIS page are scanned, so a page's resource block stays
// consistent with its own integrity counters; devup-mcp merges across pages.
async function collectResources(nodes) {
  const variableIds = new Set();
  const styleTypes = new Map();
  scanResources(nodes, variableIds, styleTypes);

  const sortedVariableIds = [...variableIds].sort();
  const sortedStyles = [...styleTypes.entries()]
    .map(([id, styleType]) => ({ id, styleType }))
    .sort((left, right) => left.id.localeCompare(right.id));

  const results = await Promise.all([
    ...sortedVariableIds.map(async (id) => {
      try {
        const variable = await figma.variables.getVariableByIdAsync(id);
        return variable
          ? {
              kind: "variable",
              value: serialize(variable, true),
              collectionId: variable.variableCollectionId,
            }
          : { kind: "unresolved", value: { id, kind: "variable", reason: "notFoundOrUnavailable" } };
      } catch (_) {
        return { kind: "unresolved", value: { id, kind: "variable", reason: "notFoundOrUnavailable" } };
      }
    }),
    ...sortedStyles.map(async ({ id, styleType }) => {
      try {
        const style = await figma.getStyleByIdAsync(id);
        if (!style) {
          return { kind: "unresolved", value: { id, kind: "style", reason: "notFoundOrUnavailable" } };
        }
        return {
          kind: "style",
          value: {
            ...serialize(style, true),
            styleType,
            value: serialize(
              styleType === "PAINT"
                ? style.paints
                : styleType === "EFFECT"
                  ? style.effects
                  : styleType === "GRID"
                    ? style.layoutGrids
                    : style,
              true,
            ),
          },
        };
      } catch (_) {
        return { kind: "unresolved", value: { id, kind: "style", reason: "notFoundOrUnavailable" } };
      }
    }),
  ]);

  const collectionIds = [...new Set(results
    .filter((result) => result.kind === "variable" && result.collectionId)
    .map((result) => result.collectionId))].sort();
  const collections = (await Promise.all(collectionIds.map(async (id) => {
    try {
      const collection = await figma.variables.getVariableCollectionByIdAsync(id);
      return collection ? serialize(collection, true) : null;
    } catch (_) {
      return null;
    }
  }))).filter((collection) => collection !== null);

  const variables = results.filter((result) => result.kind === "variable").map((result) => result.value);
  const styles = results.filter((result) => result.kind === "style").map((result) => result.value);
  const unresolved = results.filter((result) => result.kind === "unresolved").map((result) => result.value);

  return {
    collections,
    variables,
    styles,
    usedRemoteVariables: variables.filter((variable) => variable.remote === true),
    usedVariableIds: sortedVariableIds,
    usedStyleIds: sortedStyles.map((style) => style.id),
    localComplete: false,
    usedRemoteComplete: unresolved.length === 0,
    unresolved,
    $variableRefCount: sortedVariableIds.length,
    $styleRefCount: sortedStyles.length,
  };
}

// Packs as many nodes as fit under `budget`, starting at `offset`. Same
// dynamic, byte-budget-driven pagination the legacy cursor snapshot uses.
function packPage(budget) {
  const pageNodes = [];
  let payloadBytes = 2;
  for (let index = offset; index < allNodes.length; index += 1) {
    const snapshotted = snapshotNode(allNodes[index]);
    const nodeBytes = jsonByteLength(snapshotted) + (pageNodes.length ? 1 : 0);
    if (pageNodes.length && payloadBytes + nodeBytes > budget) break;
    pageNodes.push(snapshotted);
    payloadBytes += nodeBytes;
  }
  return pageNodes;
}

function buildEnvelope(pageNodes, resources) {
  const nextOffset = Math.min(allNodes.length, offset + pageNodes.length);
  const { $variableRefCount, $styleRefCount, ...resourceBlock } = resources;
  const nodes = [
    ...pageNodes,
    {
      id: "__DEVUP_SNAPSHOT_CURSOR__",
      type: "DEVUP_INTERNAL",
      // `offset` is what lets the Rust decoder tell a first page from a
      // continuation page, which decides whether the root must be present
      // here. All four fields are read by the shared `read_snapshot_cursor`.
      fields: {
        offset,
        nextOffset,
        complete: nextOffset >= allNodes.length,
        totalNodes: allNodes.length,
      },
      extra: {},
      fieldErrors: {},
    },
  ];
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
    resources: resourceBlock,
    // No `pagination` mirror: the __DEVUP_SNAPSHOT_CURSOR__ marker node is the
    // single source of truth for page state, and duplicating it is exactly how
    // the two copies drifted apart before.
    integrity: {
      nodeCount: nodes.length,
      variableRefCount: $variableRefCount,
      styleRefCount: $styleRefCount,
      utf8Bytes: 0,
    },
  };
  // Writing the byte count into the envelope changes the envelope's own
  // length, so iterate to the fixed point. `utf8ByteLength` measures without
  // building a throwaway byte array.
  let bytes = 0;
  for (let attempt = 0; attempt < 8; attempt += 1) {
    bytes = utf8ByteLength(JSON.stringify(envelope));
    if (envelope.integrity.utf8Bytes === bytes) break;
    envelope.integrity.utf8Bytes = bytes;
  }
  if (envelope.integrity.utf8Bytes !== utf8ByteLength(JSON.stringify(envelope))) {
    throw new Error("DEVUP_ENVELOPE_LENGTH_UNSTABLE");
  }
  return { envelope, bytes };
}

// The node budget alone can't bound the envelope: a page also carries every
// variable/style its nodes reference, and that block is only sized once the
// nodes are chosen. So pack, build, and if the whole envelope overshoots the
// text limit, halve the node budget and try again. Fewer nodes can only
// reference fewer resources, so this converges.
let nodeBudget = maxPayloadBytes - 1024;
let built = null;
for (let attempt = 0; attempt < 5; attempt += 1) {
  const pageNodes = packPage(nodeBudget);
  if (pageNodes.length === 0) throw new Error("DEVUP_SNAPSHOT_RANGE_INVALID");
  const candidate = buildEnvelope(pageNodes, await collectResources(pageNodes));
  if (candidate.bytes <= MAX_TEXT_ENVELOPE_BYTES) {
    built = candidate;
    break;
  }
  if (pageNodes.length === 1) {
    // A single node whose own resources blow the limit; no smaller page
    // exists and there is no binary transport to fall back to.
    throw new Error("DEVUP_ENVELOPE_TOO_LARGE");
  }
  nodeBudget = Math.floor(nodeBudget / 2);
}
if (!built) throw new Error("DEVUP_ENVELOPE_TOO_LARGE");
return built.envelope;
