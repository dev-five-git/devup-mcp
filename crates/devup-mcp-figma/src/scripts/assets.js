const options = "__DEVUP_ASSET__";
"__DEVUP_LARGE_VALUE_HELPERS__";

function failed(errorCode) {
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
    status: "failed",
    byteLength: null,
    sha256: null,
    errorCode,
  };
}

try {
  const node = await figma.getNodeByIdAsync(options.nodeId);
  if (!node || typeof node.exportAsync !== "function") {
    return failed("DEVUP_ASSET_UNSUPPORTED_BY_UPSTREAM");
  }
  if (typeof options.field === "string" && options.field.startsWith("fills/")) {
    const index = Number(options.field.slice("fills/".length));
    const fills = "fills" in node && Array.isArray(node.fills) ? node.fills : [];
    const paint = Number.isInteger(index) ? fills[index] : null;
    const imageHash = paint && paint.type === "IMAGE" ? paint.imageHash || paint.imageRef : null;
    if (!paint || imageHash !== options.imageHash) {
      return failed("DEVUP_ASSET_SOURCE_CHANGED");
    }
  } else if (options.field !== "node") {
    return failed("DEVUP_ASSET_FIELD_UNSUPPORTED");
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
  const settings = { format: inlineSvg ? "SVG_STRING" : format };
  if (format === "PNG" || format === "JPG") {
    settings.constraint = { type: "SCALE", value: scale };
  }
  const exported = await node.exportAsync(settings);
  const svgText = inlineSvg && typeof exported === "string" ? exported : null;
  const bytes =
    svgText === null
      ? exported instanceof Uint8Array
        ? exported
        : new Uint8Array(exported)
      : devupUtf8Encode(svgText);
  if (bytes.length === 0 || bytes.length > 8 * 1024 * 1024) {
    return failed("DEVUP_ASSET_RESPONSE_TOO_LARGE");
  }
  const sha256 = devupSha256(bytes);
  // A PNG past what one attachment carries is not written here either.
  // Figma's remote MCP returns a written PNG as an attachment only up to
  // about a megabyte once base64-encoded: a 665 KB photograph came back, a
  // 950 KB one was written, reported exported, and never arrived - the
  // devup-ui landing page's hero. Past 768 KiB, which is exactly one MiB
  // encoded, it is announced and read back in fragments like a large SVG.
  const pngTooLargeToAttach = format === "PNG" && bytes.length > 768 * 1024;
  if ((svgText !== null && bytes.length > 12 * 1024) || pngTooLargeToAttach) {
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
      status: "chunked",
      byteLength: bytes.length,
      sha256,
      cursor: { nextOffset: 0, maxChunkBytes: DEVUP_LARGE_VALUE_CHUNK_BYTES },
      errorCode: null,
    };
  }
  figma.io.write(`devup-asset-${options.assetId.replace(/[^A-Za-z0-9_-]/g, "_")}.${String(options.format).toLowerCase()}`, bytes);
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
    status: "exported",
    byteLength: bytes.length,
    sha256,
    // Present only for SVG. `mimeType` is what lets the Rust side recognise
    // this as the payload rather than as ordinary descriptor prose.
    mimeType: svgText === null ? null : "image/svg+xml",
    text: svgText,
    errorCode: null,
  };
} catch (_) {
  return failed("DEVUP_ASSET_EXPORT_FAILED");
}
