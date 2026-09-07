const MAX_ENVELOPE_BYTES = 8 * 1024 * 1024;
// The Figma MCP cuts a text result at 20,480 UTF-8 bytes; the measurement is
// written up in fast_snapshot.js.
const MAX_TEXT_ENVELOPE_BYTES = 19 * 1024;
// A file's resources rarely fit one answer - forty variables with their
// modes are twice this - so they are paged: every page carries the whole
// scan (which ids are used, what could not be resolved) and a run of the
// resources, in one fixed order, from `offset`; the page says where the next
// one starts. The theme used to be one envelope or nothing, and a file whose
// theme did not fit fell back to a path that knows only local styles.
const pageOptions = "__DEVUP_THEME__";
const pageOffset = Math.max(0, Math.floor(Number(pageOptions.offset) || 0));

function propertyNames(value) {
  const names = new Set(Object.keys(value));
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
  const output = {};
  for (const name of propertyNames(value)) {
    if (name.startsWith("_") || ["parent", "children", "consumers"].includes(name)) continue;
    try {
      const serialized = serialize(value[name], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) output[name] = serialized;
    } catch (_) {
      output[name] = { $error: "unavailable" };
    }
  }
  seen.delete(value);
  return output;
}

function styleTypeForField(field) {
  if (field === "textStyleId") return "TEXT";
  if (["fillStyleId", "strokeStyleId", "backgroundStyleId"].includes(field)) return "PAINT";
  if (field === "effectStyleId") return "EFFECT";
  if (field === "gridStyleId") return "GRID";
  return null;
}

const usedVariableIds = new Set();
const usedStyleTypes = new Map();
function scanResources(value, fieldName = "", seen = new WeakSet(), depth = 0) {
  if (depth > 16 || value === null || typeof value !== "object") return;
  if (seen.has(value)) return;
  seen.add(value);
  if (Array.isArray(value)) {
    for (const child of value) scanResources(child, fieldName, seen, depth + 1);
    return;
  }
  if (
    value.type === "VARIABLE_ALIAS" &&
    typeof value.id === "string" &&
    value.id &&
    value.id !== "figma.mixed" &&
    value.id !== "MIXED"
  ) {
    usedVariableIds.add(value.id);
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
      if (!usedStyleTypes.has(child)) usedStyleTypes.set(child, styleType);
    }
    scanResources(child, field || fieldName, seen, depth + 1);
  }
}

const documentNodes = [figma.root, ...figma.root.findAll(() => true)];
for (const node of documentNodes) {
  for (const field of [
    "boundVariables",
    "fills",
    "strokes",
    "effects",
    "layoutGrids",
    "textStyleId",
    "fillStyleId",
    "strokeStyleId",
    "backgroundStyleId",
    "effectStyleId",
    "gridStyleId",
  ]) {
    try {
      scanResources({ [field]: node[field] });
    } catch (_) {}
  }
}

const [localCollections, localVariables, paints, texts, effects, grids] = await Promise.all([
  figma.variables.getLocalVariableCollectionsAsync(),
  figma.variables.getLocalVariablesAsync(),
  figma.getLocalPaintStylesAsync(),
  figma.getLocalTextStylesAsync(),
  figma.getLocalEffectStylesAsync(),
  figma.getLocalGridStylesAsync(),
]);
const localVariableIds = new Set(localVariables.map((variable) => variable.id));
const localStyleIds = new Set([...paints, ...texts, ...effects, ...grids].map((style) => style.id));

const unresolved = [];
const remoteVariableJobs = [...usedVariableIds]
  .filter((id) => !localVariableIds.has(id))
  .sort()
  .map(async (id) => {
    try {
      const variable = await figma.variables.getVariableByIdAsync(id);
      return variable || null;
    } catch (_) {
      unresolved.push({ id, kind: "variable", reason: "notFoundOrUnavailable" });
      return null;
    }
  });
const remoteStyleJobs = [...usedStyleTypes.entries()]
  .filter(([id]) => !localStyleIds.has(id))
  .sort(([left], [right]) => left.localeCompare(right))
  .map(async ([id, styleType]) => {
    try {
      const style = await figma.getStyleByIdAsync(id);
      return style ? { style, styleType } : null;
    } catch (_) {
      unresolved.push({ id, kind: "style", reason: "notFoundOrUnavailable" });
      return null;
    }
  });
const remoteVariables = (await Promise.all(remoteVariableJobs)).filter(Boolean);
const remoteStyles = (await Promise.all(remoteStyleJobs)).filter(Boolean);
for (const id of [...usedVariableIds].filter((id) => !localVariableIds.has(id))) {
  if (!remoteVariables.some((variable) => variable.id === id) && !unresolved.some((item) => item.id === id)) {
    unresolved.push({ id, kind: "variable", reason: "notFoundOrUnavailable" });
  }
}
for (const [id] of [...usedStyleTypes.entries()].filter(([id]) => !localStyleIds.has(id))) {
  if (!remoteStyles.some((item) => item.style.id === id) && !unresolved.some((item) => item.id === id)) {
    unresolved.push({ id, kind: "style", reason: "notFoundOrUnavailable" });
  }
}

const remoteCollectionIds = [...new Set(remoteVariables.map((variable) => variable.variableCollectionId))]
  .filter((id) => !localCollections.some((collection) => collection.id === id))
  .sort();
const remoteCollections = (
  await Promise.all(
    remoteCollectionIds.map(async (id) => {
      try {
        return await figma.variables.getVariableCollectionByIdAsync(id);
      } catch (_) {
        return null;
      }
    }),
  )
).filter(Boolean);

function serializeStyle(style, styleType) {
  return {
    ...serialize(style),
    styleType,
    value: serialize(
      styleType === "PAINT"
        ? style.paints
        : styleType === "EFFECT"
          ? style.effects
          : styleType === "GRID"
            ? style.layoutGrids
            : style,
    ),
  };
}

const styles = [
  ...paints.map((style) => serializeStyle(style, "PAINT")),
  ...texts.map((style) => serializeStyle(style, "TEXT")),
  ...effects.map((style) => serializeStyle(style, "EFFECT")),
  ...grids.map((style) => serializeStyle(style, "GRID")),
  ...remoteStyles.map(({ style, styleType }) => serializeStyle(style, styleType)),
].sort((left, right) => left.id.localeCompare(right.id));
const variables = [...localVariables, ...remoteVariables]
  .map((variable) => serialize(variable))
  .sort((left, right) => left.id.localeCompare(right.id));
const collections = [...localCollections, ...remoteCollections]
  .map((collection) => serialize(collection))
  .sort((left, right) => left.id.localeCompare(right.id));
unresolved.sort((left, right) => left.kind.localeCompare(right.kind) || left.id.localeCompare(right.id));

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
    if (codePoint < 0x80) bytes.push(codePoint);
    else if (codePoint < 0x800) bytes.push(0xc0 | (codePoint >> 6), 0x80 | (codePoint & 0x3f));
    else if (codePoint < 0x10000) {
      bytes.push(0xe0 | (codePoint >> 12), 0x80 | ((codePoint >> 6) & 0x3f), 0x80 | (codePoint & 0x3f));
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

// Every resource in one order: collections, variables, styles. A page is a
// run of this list.
const items = [
  ...collections.map((value) => ({ kind: "collection", value })),
  ...variables.map((value) => ({ kind: "variable", value })),
  ...styles.map((value) => ({ kind: "style", value })),
];
if (pageOffset > items.length) throw new Error("DEVUP_SNAPSHOT_RANGE_INVALID");

function buildEnvelope(pageItems, nextOffset) {
  const pageCollections = pageItems.filter((item) => item.kind === "collection").map((item) => item.value);
  const pageVariables = pageItems.filter((item) => item.kind === "variable").map((item) => item.value);
  const pageStyles = pageItems.filter((item) => item.kind === "style").map((item) => item.value);
  const envelope = {
    kind: "devupFastThemeEnvelope",
    schemaVersion: 1,
    source: { fileKey: figma.fileKey || "", version: null },
    resources: {
      collections: pageCollections,
      variables: pageVariables,
      styles: pageStyles,
      usedRemoteVariables: pageVariables.filter((variable) => variable.remote === true),
      usedVariableIds: [...usedVariableIds].sort(),
      usedStyleIds: [...usedStyleTypes.keys()].sort(),
      localComplete: true,
      usedRemoteComplete: unresolved.length === 0,
      unresolved,
    },
    // Read by the Rust decoder: a page is one of several when `complete` is
    // false, and the next one is asked for from `nextOffset`.
    page: {
      offset: pageOffset,
      nextOffset,
      complete: nextOffset >= items.length,
      totalItems: items.length,
    },
    integrity: {
      collectionCount: pageCollections.length,
      variableCount: pageVariables.length,
      styleCount: pageStyles.length,
      unresolvedCount: unresolved.length,
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
  return { envelope, bytes: envelopeBytes.length };
}

// Pack from the offset until the page would not fit, then build it; the
// scan the page also carries is only sized once built, so if the whole
// still overshoots, take the overshoot off and try again. One item alone
// that does not fit cannot be split, and is reported rather than cut.
let count = items.length - pageOffset;
let built = null;
for (let attempt = 0; attempt < 6 && count > 0; attempt += 1) {
  const candidate = buildEnvelope(items.slice(pageOffset, pageOffset + count), pageOffset + count);
  if (candidate.bytes <= MAX_TEXT_ENVELOPE_BYTES) {
    built = candidate;
    break;
  }
  if (count === 1) break;
  const itemBytes = items
    .slice(pageOffset, pageOffset + count)
    .map((item) => utf8Encode(JSON.stringify(item.value)).length + 1);
  const overshoot = candidate.bytes - MAX_TEXT_ENVELOPE_BYTES + 256;
  let dropped = 0;
  let next = count;
  while (next > 1 && dropped < overshoot) {
    next -= 1;
    dropped += itemBytes[next];
  }
  count = next;
}
if (built === null) {
  if (items.length === 0) {
    built = buildEnvelope([], 0);
  } else {
    throw new Error("DEVUP_ENVELOPE_TOO_LARGE");
  }
}
if (built.bytes > MAX_ENVELOPE_BYTES) throw new Error("DEVUP_ENVELOPE_TOO_LARGE");
return built.envelope;
