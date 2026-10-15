// Figma's own MCP runs scripts with hidden layers inside instances left out
// of `children`, `findAll` and `getNodeByIdAsync`; a plugin sees them. Set
// here so the direct path and the Devup Bridge read the same document.
figma.skipInvisibleInstanceChildren = false;
const options = "__DEVUP_ASSET__";
"__DEVUP_LARGE_VALUE_HELPERS__";
"__DEVUP_ASSET_RENDER_HELPERS__";

const started = Date.now();
let stage = "lookup";
let nodeType = "unknown";
const timeoutMs = Math.min(600000, Math.max(1000, Number(options.timeoutMs) || 90000));
const exportDiagnostics = { method: "native", stages: [] };
function observe(stage, elapsedMs, nativeError) {
  exportDiagnostics.stages.push({ stage, elapsedMs });
  if (nativeError) exportDiagnostics.nativeError = nativeError;
  if (stage === "geometry-fallback") exportDiagnostics.method = "geometry";
}
async function timed(work) {
  let timer;
  try {
    return await Promise.race([work(), new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`DEVUP_ASSET_TIMEOUT: ${stage} exceeded ${timeoutMs} ms asset budget`)), Math.max(0, timeoutMs - (Date.now() - started)));
    })]);
  } finally { clearTimeout(timer); }
}

function describe(error) {
  const message = error && typeof error.message === "string" ? error.message : String(error);
  const editor = typeof figma.editorType === "string" ? ` (editor: ${figma.editorType})` : "";
  return `${stage} failed after ${Date.now() - started} ms${editor} (node: ${options.nodeId}, type: ${nodeType}): ${message}`.slice(0, 500);
}

function failed(errorCode, errorDetail) {
  if (errorCode && exportDiagnostics.renderInfo) {
    exportDiagnostics.renderInfo.attemptedRepresentation = exportDiagnostics.renderInfo.representation;
    exportDiagnostics.renderInfo.representation = "not-exported";
  }
  return {
    kind: "devupAssetExport",
    fileKey: figma.fileKey || "",
    version: options.version,
    assetId: options.assetId,
    nodeId: options.nodeId,
    field: options.field,
    imageHash: options.imageHash,
    format: options.format,
    scale: options.scale,
    renderMode,
    status: "failed",
    byteLength: null,
    sha256: null,
    errorCode,
    exportDiagnostics,
    ...(errorDetail ? { errorDetail } : {}),
  };
}

const original = typeof options.field === "string" && options.field.startsWith("$original-image/fills/");
const renderMode = options.renderMode === undefined ? (original ? "fill" : "node") : options.renderMode;
function renderInfo(node, paint) {
  const finite = n => typeof n === "number" && Number.isFinite(n) && n > 0;
  const effects = Array.isArray(node.effects) ? node.effects : null;
  const ancestors = [];
  const seen = new Set();
  let branch = node;
  for (let parent = node.parent; parent && ancestors.length < 32 && !seen.has(parent); parent = parent.parent) {
    seen.add(parent);
    ancestors.push({ type: parent.type || "unknown", clipsContent: parent.clipsContent ?? null,
      isMask: parent.isMask ?? null, effects: Array.isArray(parent.effects) ? parent.effects : null,
      siblingMaskPresent: Array.isArray(parent.children) ? parent.children.some(n => n !== branch && n.isMask === true) : null });
    branch = parent;
  }
  return {
    representation: renderMode === "fill" ? "original-fill" : "native-node",
    scope: renderMode === "fill" ? "selected-paint-original-bytes" : "exact-node-subtree",
    exportSettings: renderMode === "node" ? { contentsOnly: true, useAbsoluteBounds: false } : null,
    requestedScale: options.scale,
    scaleSemantics: renderMode === "fill" ? "intrinsic-original-bytes-no-resize"
      : ["png", "jpg"].includes(options.format) ? "native-node-SCALE-constraint" : "not-applied-to-vector-document",
    nodeSize: finite(node.width) && finite(node.height) ? {width:node.width, height:node.height, aspectRatio:node.width/node.height} : null,
    outputSize: null, dimensionEvidence: "unknown-until-byte-inspection", aspectRatioComparison: "unknown",
    sourceProperties: { scaleMode: paint ? paint.scaleMode ?? null : null,
      imageTransform: paint ? paint.imageTransform ?? null : null,
      rotation: paint ? paint.rotation ?? null : null, filters: paint ? paint.filters ?? null : null,
      opacity: paint ? paint.opacity ?? null : null, effects,
      blurEffects: effects ? effects.filter(e => e.visible !== false && String(e.type).includes("BLUR")) : null,
      isMask: node.isMask ?? null, clipsContent: node.clipsContent ?? null, ancestors },
    unappliedProperties: renderMode === "fill" ? ["scaleMode", "imageTransform", "rotation", "filters", "paintOpacity", "nodeGeometry", "nodeTransforms", "nodeEffectsIncludingBlur", "nodeAndAncestorMasks", "ancestorClipsAndEffects"] : [],
    warnings: renderMode === "fill" ? ["original-fill-properties-unapplied", "aspect-ratio-unknown"]
      : ["ancestor-canvas-appearance-unverified", "aspect-ratio-unknown"],
    fidelity: "unverified",
  };
}
try {
  if (!["fill", "node"].includes(renderMode)) return failed("DEVUP_ASSET_RENDER_MODE_UNSUPPORTED");
  if (!original && renderMode === "fill" &&
      (!String(options.field).startsWith("fills/") || !["png", "jpg"].includes(options.format) || options.scale !== 1)) {
    return failed("DEVUP_ASSET_FILL_SETTINGS_UNSUPPORTED", "Original fill requires fills/N, png or jpg matching its codec, and scale 1; no resizing or transcoding.");
  }
  // Original uploads are a different representation from node renditions.
  // Only the local bridge opts in: the remote MCP's bounded text envelope
  // cannot carry arbitrary original bytes, and its file writer is not a
  // general binary transport. Never silently return a node PNG instead.
  if ((original || renderMode === "fill") && options.transport !== "bridge") {
    return failed(original ? "DEVUP_ORIGINAL_IMAGE_REQUIRES_BRIDGE" : "DEVUP_ASSET_FILL_REQUIRES_BRIDGE");
  }
  const node = await timed(() => devupLookupAssetNode(options.nodeId));
  observe("lookup", Date.now() - started);
  nodeType = node && node.type || "unknown";
  if (!node || (renderMode === "node" && typeof node.exportAsync !== "function")) {
    return failed("DEVUP_ASSET_UNSUPPORTED_BY_UPSTREAM", describe(new Error(node ? "node has no exportAsync" : "node not found")));
  }
  let selectedPaint = null;
  if (original || (typeof options.field === "string" && options.field.startsWith("fills/"))) {
    const index = Number(options.field.slice(original ? "$original-image/fills/".length : "fills/".length));
    const fills = "fills" in node && Array.isArray(node.fills) ? node.fills : [];
    const paint = Number.isInteger(index) ? fills[index] : null;
    const imageHash = paint && paint.type === "IMAGE" ? paint.imageHash || paint.imageRef : null;
    if (!paint || imageHash !== options.imageHash) {
      return failed("DEVUP_ASSET_SOURCE_CHANGED");
    }
    selectedPaint = paint;
  } else if (options.field !== "node") {
    return failed("DEVUP_ASSET_FIELD_UNSUPPORTED");
  }
  exportDiagnostics.renderInfo = renderInfo(node, selectedPaint);

  if (original || renderMode === "fill") {
    exportDiagnostics.method = "original-fill";
    stage = "reading the original image";
    const image = figma.getImageByHash(options.imageHash);
    if (!image) return failed("DEVUP_ORIGINAL_IMAGE_NOT_FOUND");
    const bytes = await timed(() => image.getBytesAsync());
    if (bytes.length === 0 || bytes.length > 8 * 1024 * 1024) {
      return failed("DEVUP_ASSET_RESPONSE_TOO_LARGE", `${bytes.length} bytes; one answer carries 1 to 8388608`);
    }
    const starts = values => values.every((value, index) => bytes[index] === value);
    const mimeType = starts([137,80,78,71,13,10,26,10]) ? "image/png"
      : starts([255,216,255]) ? "image/jpeg"
      : starts([71,73,70,56]) ? "image/gif"
      : starts([82,73,70,70]) && bytes[8] === 87 && bytes[9] === 69 && bytes[10] === 66 && bytes[11] === 80 ? "image/webp"
      : null;
    if (!mimeType) return failed("DEVUP_ORIGINAL_IMAGE_CODEC_UNSUPPORTED");
    exportDiagnostics.renderInfo.originalMimeType = mimeType;
    if (!original && mimeType !== { png: "image/png", jpg: "image/jpeg" }[options.format]) {
      return failed("DEVUP_ASSET_FILL_CODEC_MISMATCH", `Original fill codec is ${mimeType}, but ${options.format} was requested; choose the matching format or renderMode node. No transcoding was performed.`);
    }
    if (!original) return { ...failed(null), status: "exported", mimeType,
      byteLength: bytes.length, sha256: devupSha256(bytes), data: figma.base64Encode(bytes) };
    const size = await timed(() => image.getSizeAsync());
    return {
      ...failed(null), status: "exported", kind: "devupOriginalImage",
      representation: "original-image-v1", format: null, scale: null,
      mimeType, width: size.width, height: size.height,
      byteLength: bytes.length, sha256: devupSha256(bytes), data: figma.base64Encode(bytes),
    };
  }

  const format = String(options.format || "").toUpperCase();
  if (!["PNG", "JPG", "SVG", "PDF"].includes(format)) {
    return failed("DEVUP_ASSET_FORMAT_UNSUPPORTED");
  }
  const scale = Math.min(4, Math.max(1, Math.floor(Number(options.scale) || 1)));
  // SVG is exported as a string and carried back inline. Figma's remote MCP
  // does not return a written `.svg` as an attachment the way it does a PNG,
  // so writing the file alone left the caller holding a descriptor and no
  // bytes at all, and every SVG request failed. SVG is text and small, so an
  // inline copy is bounded well under the text-response limit; anything
  // larger is reported rather than silently truncated.
  const inlineSvg = format === "SVG";
  const settings = { format: inlineSvg ? "SVG_STRING" : format, contentsOnly: true, useAbsoluteBounds: false };
  if (format === "PNG" || format === "JPG") {
    settings.constraint = { type: "SCALE", value: scale };
  }
  stage = "export";
  const remaining = Math.max(1, timeoutMs - (Date.now() - started));
  // Reserve up to five seconds for read-only geometry after a hung renderer.
  // A late native answer has no file writer and cannot replace the fallback.
  const nativeBudget = inlineSvg ? Math.max(1, remaining - Math.min(5000, Math.floor(remaining / 5))) : remaining;
  const nativeBegan = Date.now();
  // Image-filled nodes must use Figma's renderer for crop, filters, effects,
  // transforms and masks within this subtree. Never substitute raw fill bytes
  // or geometry on renderer failure. The canvas outside this node is unverified.
  const exported = await timed(() => selectedPaint ? node.exportAsync(settings)
    : devupRenderAsset(node, settings, nativeBudget, observe));
  if (selectedPaint) observe("native", Date.now() - nativeBegan);
  if (exportDiagnostics.method === "geometry") {
    exportDiagnostics.renderInfo.representation = "geometry-fallback";
    exportDiagnostics.renderInfo.warnings.push("geometry-fallback-not-native-render");
  }
  if (Date.now() - started >= timeoutMs) throw new Error(`DEVUP_ASSET_TIMEOUT: export exceeded ${timeoutMs} ms asset budget`);
  stage = "encode";
  const encodeBegan = Date.now();
  const svgText = inlineSvg && typeof exported === "string" ? exported : null;
  const bytes =
    svgText === null
      ? exported instanceof Uint8Array
        ? exported
        : new Uint8Array(exported)
      : devupUtf8Encode(svgText);
  if (bytes.length === 0 || bytes.length > 8 * 1024 * 1024) {
    return failed("DEVUP_ASSET_RESPONSE_TOO_LARGE", `${format} at scale ${scale} is ${bytes.length} bytes; one answer carries 1 to 8388608`);
  }
  const sha256 = devupSha256(bytes);
  observe("encode", Date.now() - encodeBegan);
  // figma.io is an extension of the remote MCP, not the native Plugin API.
  // The bridge returns JSON over its local socket and has no attachment
  // writer. Carry its bounded bytes inline, without exporting again for each
  // fragment; the Rust decoder still validates length, MIME type and hash.
  const hasFileWriter = figma.io && typeof figma.io.write === "function";
  // A PNG past what one attachment carries is not written here either.
  // Figma's remote MCP returns a written PNG as an attachment only up to
  // about a megabyte once base64-encoded: a 665 KB photograph came back, a
  // 950 KB one was written, reported exported, and never arrived - the
  // devup-ui landing page's hero. Past 768 KiB, which is exactly one MiB
  // encoded, it is announced and read back in fragments like a large SVG.
  const pngTooLargeToAttach = format === "PNG" && bytes.length > 768 * 1024;
  if (hasFileWriter && ((svgText !== null && bytes.length > 12 * 1024) || pngTooLargeToAttach)) {
    // An SVG past what one text response holds is not written here at all:
    // it is announced with its length and hash, and read back in fragments
    // through the large-value script, which re-exports it and slices — the
    // same transport a large node field takes. An illustration of fifty
    // vectors is 125 KB; before this it simply failed.
    return {
      kind: "devupAssetExport",
      fileKey: figma.fileKey || "",
      version: options.version,
      assetId: options.assetId,
      nodeId: options.nodeId,
      field: options.field,
      imageHash: options.imageHash,
      format: options.format,
      scale,
      renderMode,
      status: "chunked",
      byteLength: bytes.length,
      sha256,
      cursor: { nextOffset: 0, maxChunkBytes: DEVUP_LARGE_VALUE_CHUNK_BYTES },
      errorCode: null,
      exportDiagnostics,
    };
  }
  if (hasFileWriter) {
    figma.io.write(`devup-asset-${options.assetId.replace(/[^A-Za-z0-9_-]/g, "_")}.${String(options.format).toLowerCase()}`, bytes);
  }
  return {
    kind: "devupAssetExport",
    fileKey: figma.fileKey || "",
    version: options.version,
    assetId: options.assetId,
    nodeId: options.nodeId,
    field: options.field,
    imageHash: options.imageHash,
    format: options.format,
    scale,
    renderMode,
    status: "exported",
    byteLength: bytes.length,
    sha256,
    // MIME identifies inline bridge bytes or remote SVG text as a payload.
    mimeType: !hasFileWriter
      ? { PNG: "image/png", JPG: "image/jpeg", SVG: "image/svg+xml", PDF: "application/pdf" }[format]
      : svgText === null ? null : "image/svg+xml",
    ...(!hasFileWriter && svgText === null ? { data: figma.base64Encode(bytes) } : {}),
    text: svgText,
    errorCode: null,
    exportDiagnostics,
  };
} catch (error) {
  return failed(String(error.message || error).includes("DEVUP_ASSET_TIMEOUT:") ? "DEVUP_ASSET_TIMEOUT" : original || renderMode === "fill" ? "DEVUP_ORIGINAL_IMAGE_READ_FAILED" : "DEVUP_ASSET_EXPORT_FAILED", describe(error));
}
