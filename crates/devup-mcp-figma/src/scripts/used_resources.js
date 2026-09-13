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
    if (name.startsWith("_") || ["parent", "children", "consumers"].includes(name)) continue;
    try {
      const serialized = serialize(value[name], seen, depth + 1);
      if (!(serialized && serialized.$unsupported === "function")) result[name] = serialized;
    } catch (_error) {
      result[name] = { $error: "unavailable" };
    }
  }
  seen.delete(value);
  return result;
}

// 파일에 있는 변수는 한 번에 다 받아 두고 id 로 찾는다.
//
// 하나씩 `getVariableByIdAsync` 로 묻는 것이 이 파일에서는 건당 21.5초를 쓰고
// 끝내 null 을 냈다. 33개든 40개든 병렬이라 총 시간이 21.5초로 같았던 것이
// 단서였다 ? 작업이 아니라 건당 대기였다. 같은 순간 지역 목록을 받는 호출은
// 12ms 에 돌아온다. 그래서 그쪽을 먼저 쓴다.
const localVariableIndex = new Map();
let localVariableListing = "ok";
try {
  for (const variable of await figma.variables.getLocalVariablesAsync()) {
    localVariableIndex.set(variable.id, variable);
  }
} catch (_error) {
  localVariableListing = "unavailable";
}

const variableResults = await Promise.all(resources.variableIds.map(async (id) => {
  const local = localVariableIndex.get(id);
  if (local) {
    return { value: serialize(local), collectionId: local.variableCollectionId };
  }
  // 목록에 없으면 이 파일 것이 아니다(라이브러리 변수). 그때만 개별로 묻는다.
  try {
    const value = await figma.variables.getVariableByIdAsync(id);
    return value
      ? { value: serialize(value), collectionId: value.variableCollectionId }
      : { unresolved: { id, kind: "variable", reason: "notInFileAndLookupEmpty" } };
  } catch (_error) {
    return { unresolved: { id, kind: "variable", reason: "notInFileAndLookupThrew" } };
  }
}));

const collectionIds = [...new Set(variableResults
  .flatMap((result) => result.collectionId ? [result.collectionId] : []))].sort();
const collectionResults = await Promise.all(collectionIds.map(async (id) => {
  try {
    const collection = await figma.variables.getVariableCollectionByIdAsync(id);
    return collection ? [serialize(collection)] : [];
  } catch (_error) {
    return [];
  }
}));

const styleResults = await Promise.all(resources.styles.map(async (styleRef) => {
  try {
    const style = await figma.getStyleByIdAsync(styleRef.id);
    if (!style) {
      return { unresolved: { id: styleRef.id, kind: "style", reason: "notFoundOrUnavailable" } };
    }
    return {
      value: {
        ...serialize(style),
        styleType: styleRef.styleType,
        value: serialize(
          styleRef.styleType === "PAINT" ? style.paints
            : styleRef.styleType === "EFFECT" ? style.effects
              : styleRef.styleType === "GRID" ? style.layoutGrids
                : style
        )
      }
    };
  } catch (_error) {
    return { unresolved: { id: styleRef.id, kind: "style", reason: "notFoundOrUnavailable" } };
  }
}));

return {
  collections: collectionResults.flat(),
  variables: variableResults.flatMap((result) => result.value ? [result.value] : []),
  styles: styleResults.flatMap((result) => result.value ? [result.value] : []),
  usedVariableIds: resources.variableIds,
  usedStyleIds: resources.styles.map((style) => style.id),
  // 아무것도 해석되지 않았을 때 목록 자체를 못 받은 것인지 구분하기 위해 남긴다.
  localVariableListing,
  localVariableCount: localVariableIndex.size,
  unresolved: [...variableResults, ...styleResults]
    .flatMap((result) => result.unresolved ? [result.unresolved] : [])
};
