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
    exportDiagnostics.renderInfo.renderedAs = "not-exported";
    exportDiagnostics.renderInfo.effectsBakedIntoAsset = false;
    exportDiagnostics.renderInfo.fidelity = "unverified";
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
let effectiveMode = renderMode === "auto" ? "node" : renderMode;
function renderInfo(node, paint) {
  const finite = n => typeof n === "number" && Number.isFinite(n) && n > 0;
  const effects = Array.isArray(node.effects) ? node.effects : null;
  const ancestors = [];
  const boundedName = name => [...name].slice(0, 128).join("");
  const layerHierarchy = [typeof node.name === "string" ? boundedName(node.name) : "layer"];
  const seen = new Set();
  let branch = node;
  for (let parent = node.parent; parent && ancestors.length < 32 && !seen.has(parent); parent = parent.parent) {
    seen.add(parent);
    if (layerHierarchy.length < 4 && typeof parent.name === "string" && parent.name.length && !["PAGE", "DOCUMENT"].includes(parent.type)) {
      layerHierarchy.push(boundedName(parent.name));
    }
    ancestors.push({ type: parent.type || "unknown", clipsContent: parent.clipsContent ?? null,
      isMask: parent.isMask ?? null, effects: Array.isArray(parent.effects) ? parent.effects : null,
      opacity: parent.opacity ?? null, blendMode: parent.blendMode ?? null, visible: parent.visible ?? null,
      siblingMaskPresent: Array.isArray(parent.children) ? parent.children.some(n => n !== branch && n.isMask === true) : null });
    branch = parent;
  }
  return {
    sourceKind: paint ? "image-fill" : options.imageHash ? "image-node" : "vector-node",
    renderedAs: effectiveMode === "fill" ? "raw-fill" : "node-render",
    representation: effectiveMode === "fill" ? "original-fill" : "native-node",
    layerHierarchy: layerHierarchy.reverse(),
    scope: effectiveMode === "fill" ? "selected-paint-original-bytes" : "exact-node-subtree",
    exportSettings: effectiveMode === "node" ? { contentsOnly: true, useAbsoluteBounds: false } : null,
    requestedScale: options.scale,
    scaleSemantics: effectiveMode === "fill" ? "intrinsic-original-bytes-no-resize"
      : ["png", "jpg"].includes(options.format) ? "native-node-SCALE-constraint" : "not-applied-to-vector-document",
    nodeSize: finite(node.width) && finite(node.height) ? {width:node.width, height:node.height, aspectRatio:node.width/node.height} : null,
    outputSize: null, dimensionEvidence: "unknown-until-byte-inspection", aspectRatioComparison: "unknown",
    scaleMode: paint ? paint.scaleMode ?? null : null,
    imageTransform: paint ? paint.imageTransform ?? null : null,
    // A transform matrix is not a visible-area percentage. Rotation, fit,
    // clipping and source geometry are needed to determine visible coverage.
    visibleSourceRatio: null, visibleSourceRatioEvidence: "not-computed",
    effects,
    extraFills: Array.isArray(node.fills) ? Math.max(0, node.fills.filter(p => p.visible !== false).length - (paint ? 1 : 0)) : null,
    effectsBakedIntoAsset: effectiveMode === "node" && ["png", "jpg"].includes(options.format) && effects !== null,
    bakedEffects: effectiveMode === "node" && ["png", "jpg"].includes(options.format) && effects ? effects.filter(e => e.visible !== false) : [],
    instanceOverride: "unknown", instanceOverrideEvidence: "no-component-paint-comparison",
    sourceProperties: { scaleMode: paint ? paint.scaleMode ?? null : null,
      imageTransform: paint ? paint.imageTransform ?? null : null,
      rotation: paint ? paint.rotation ?? null : null, filters: paint ? paint.filters ?? null : null,
      opacity: paint ? paint.opacity ?? null : null, effects,
      blurEffects: effects ? effects.filter(e => e.visible !== false && String(e.type).includes("BLUR")) : null,
      isMask: node.isMask ?? null, clipsContent: node.clipsContent ?? null, ancestors },
    unappliedProperties: effectiveMode === "fill" ? ["scaleMode", "imageTransform", "rotation", "filters", "paintOpacity", "nodeGeometry", "nodeTransforms", "nodeEffectsIncludingBlur", "nodeAndAncestorMasks", "ancestorClipsAndEffects"] : [],
    warnings: effectiveMode === "fill" ? ["original-fill-properties-unapplied", "aspect-ratio-unknown"]
      : ["ancestor-canvas-appearance-unverified", "aspect-ratio-unknown"],
    fidelity: "unverified",
  };
}

// Unknown properties fail closed. Identity alone cannot establish canvas
// equality, and auto never infers source dimensions from the transform.
function rawSafety(node, paint, info) {
  const reasons = [];
  const require = (condition, reason) => { if (!condition) reasons.push(reason); };
  const identity = t => Array.isArray(t) && t.length === 2 && t.every((r,i) => Array.isArray(r) && r.length === 3 && r.every((v,j) => v === [[1,0,0],[0,1,0]][i][j]));
  const unrotated = t => Array.isArray(t) && t.length === 2 && t.every(r => Array.isArray(r) && r.length === 3 && r.every(Number.isFinite)) && t[0][0] === 1 && t[1][1] === 1 && t[0][1] === 0 && t[1][0] === 0 && Number.isInteger(t[0][2]) && Number.isInteger(t[1][2]);
  require(options.transport === "bridge", "raw-requires-bridge");
  require(options.scale === 1 && ["png","jpg"].includes(options.format), "raw-requires-unscaled-raster");
  require(!!paint && paint.visible === true && paint.opacity === 1 && paint.blendMode === "NORMAL", "paint-compositing-or-unknown");
  require(paint && paint.scaleMode === "FILL" && identity(paint.imageTransform) && paint.rotation === 0, "paint-transform-or-unknown");
  require(paint && paint.filters && Object.values(paint.filters).every(v => v === 0), "image-filters-or-unknown");
  require(Array.isArray(node.fills) && node.fills.length === 1, "multiple-fills-or-unknown");
  require(node.type === "RECTANGLE" && node.cornerRadius === 0 && Array.isArray(node.strokes) && node.strokes.length === 0 && (!node.children || node.children.length === 0), "node-geometry-or-unknown");
  // RectangleNode has no clipsContent in the Plugin API. Its absence is
  // non-applicable for this geometry, rather than missing Frame evidence.
  require(Array.isArray(node.effects) && node.effects.length === 0 && node.isMask === false && (node.clipsContent === undefined || node.clipsContent === false) && node.opacity === 1 && node.visible === true && node.blendMode === "NORMAL", "node-effects-mask-compositing-or-unknown");
  require(unrotated(node.absoluteTransform) && unrotated(node.relativeTransform), "node-transform-or-unknown");
  require(info.sourceProperties.ancestors.length > 0 && info.sourceProperties.ancestors.some(p => ["PAGE","DOCUMENT"].includes(p.type)), "ancestor-chain-incomplete");
  require(info.sourceProperties.ancestors.every(p => p.siblingMaskPresent === false), "ancestor-sibling-mask-or-unknown");
  for (const p of info.sourceProperties.ancestors.filter(p => !["PAGE","DOCUMENT"].includes(p.type))) {
    require(p.clipsContent === false && p.isMask === false && p.siblingMaskPresent === false && Array.isArray(p.effects) && p.effects.length === 0 && p.opacity === 1 && p.visible === true && ["NORMAL","PASS_THROUGH"].includes(p.blendMode), "ancestor-dependency-or-unknown");
  }
  return { eligible: reasons.length === 0, reasons: [...new Set(reasons)], evidence: "explicit-plugin-properties; intrinsic-dimensions-and-codec-required" };
}

function overrideEvidence(node, info) {
  let count = 0;
  for (let parent = node; parent && count++ < 32; parent = parent.parent) {
    if (parent.type === "INSTANCE" && Array.isArray(parent.overrides)) {
      const evidence = parent.overrides.find(o => o.id === node.id && Array.isArray(o.overriddenFields) && o.overriddenFields.includes("fills"));
      if (evidence) { info.instanceOverride = true; info.instanceOverrideEvidence = { kind: "figma-instance-overrides", instanceId: parent.id, nodeId: node.id, overriddenFields: ["fills"] }; return; }
    }
    if (["PAGE","DOCUMENT"].includes(parent.type)) break;
  }
}
try {
  if (!["fill", "node", "auto"].includes(renderMode)) return failed("DEVUP_ASSET_RENDER_MODE_UNSUPPORTED");
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
  overrideEvidence(node, exportDiagnostics.renderInfo);
  let autoBytes = null;
  if (renderMode === "auto") {
    const safety = rawSafety(node, selectedPaint, exportDiagnostics.renderInfo);
    if (safety.eligible) {
      try {
        const image = figma.getImageByHash(options.imageHash);
        const sourceSize = image && await timed(() => image.getSizeAsync());
        if (!sourceSize || sourceSize.width !== node.width || sourceSize.height !== node.height) safety.reasons.push("intrinsic-dimensions-mismatch-or-unknown");
        else {
          autoBytes = await timed(() => image.getBytesAsync());
          const codec = autoBytes[0] === 137 && autoBytes[1] === 80 && autoBytes[2] === 78 && autoBytes[3] === 71 ? "png" : autoBytes[0] === 255 && autoBytes[1] === 216 && autoBytes[2] === 255 ? "jpg" : "unknown";
          if (codec !== options.format) safety.reasons.push("source-codec-mismatch-or-unknown");
        }
      } catch (_error) { safety.reasons.push("source-inspection-unavailable"); }
    }
    safety.eligible = safety.reasons.length === 0;
    effectiveMode = safety.eligible ? "fill" : "node";
    exportDiagnostics.renderInfo = renderInfo(node, selectedPaint);
    overrideEvidence(node, exportDiagnostics.renderInfo);
    exportDiagnostics.renderInfo.autoDecision = { ...safety, selected: effectiveMode };
  }

  if (original || effectiveMode === "fill") {
    exportDiagnostics.method = "original-fill";
    stage = "reading the original image";
    const image = figma.getImageByHash(options.imageHash);
    if (!image) return failed("DEVUP_ORIGINAL_IMAGE_NOT_FOUND");
    const bytes = autoBytes || await timed(() => image.getBytesAsync());
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
    exportDiagnostics.renderInfo.renderedAs = "geometry-fallback";
    exportDiagnostics.renderInfo.effectsBakedIntoAsset = false;
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
