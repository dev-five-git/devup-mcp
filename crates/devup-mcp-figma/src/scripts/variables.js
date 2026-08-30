const resources = "__DEVUP_RESOURCE_BATCH__";

function serialize(value, seen = new WeakSet(), depth = 0) {
  if (value === null || ["string", "number", "boolean"].includes(typeof value)) return value;
  if (typeof value === "undefined") return { $undefined: true };
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (["function", "symbol"].includes(typeof value)) return { $unsupported: typeof value };
  if (depth > 12) return { $truncated: "max-depth" };
  if (typeof value === "object" && "parent" in value && typeof value.id === "string" && typeof value.type === "string") {
    return { $nodeId: value.id, $nodeType: value.type };
  }
  if (Array.isArray(value)) return value.map((item) => serialize(item, seen, depth + 1));
  if (ArrayBuffer.isView(value)) return { $binary: value.constructor.name, byteLength: value.byteLength };
  if (value instanceof ArrayBuffer) return { $binary: "ArrayBuffer", byteLength: value.byteLength };
  if (seen.has(value)) return { $circular: true };
  seen.add(value);
  const result = {};
  const names = new Set(Object.keys(value));
  let current = value;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  for (const name of [...names].sort()) {
    if (name.startsWith("_") || ["parent", "children"].includes(name)) continue;
    try {
      const serialized = serialize(value[name], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[name] = serialized;
    } catch (error) {
      result[name] = { $error: String(error && error.message ? error.message : error) };
    }
  }
  seen.delete(value);
  return result;
}

const [variableValues, styleValues] = await Promise.all([
  Promise.all(resources.variableIds.map((id) => figma.variables.getVariableByIdAsync(id))),
  Promise.all(resources.styles.map((style) => figma.getStyleByIdAsync(style.id)))
]);
const styleTypes = new Map(resources.styles.map((style) => [style.id, style.styleType]));

return {
  variables: variableValues.filter(Boolean).map((value) => serialize(value)),
  styles: styleValues.filter(Boolean).map((style) => {
    const styleType = styleTypes.get(style.id);
    return {
      ...serialize(style),
      styleType,
      value: serialize(
        styleType === "PAINT" ? style.paints
          : styleType === "EFFECT" ? style.effects
            : styleType === "GRID" ? style.layoutGrids
              : style
      )
    };
  })
};
