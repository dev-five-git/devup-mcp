// `get_metadata` 가 주던 노드 계층을 플러그인 API 로 그대로 만든다.
//
// 공식 경로는 XML 을 주고 이 경로는 JSON 을 주지만, 받는 쪽은 한 곳이다.
// `metadata.rs` 의 `find_metadata` 가 첫 분기에서 `MetadataDocument` 를 그대로
// 역직렬화하고, XML 은 그 다음 분기에서 같은 구조체로 환원된다. 그래서 여기서
// 내보낼 것은 그 구조체의 camelCase 표현 하나뿐이다 — 필드를 더 얹으면 두 경로가
// 갈라진다.
//
//   { fileKey, version, rootId, nodes: [{ id, type, name, childrenIds, descendantCount }] }
//
// 좌표와 크기는 넣지 않는다. 디코더가 읽지 않고, 레이아웃은 snapshot 이 맡는다.

const root = await figma.getNodeByIdAsync("__DEVUP_NODE_ID__");
if (!root) throw new Error("DEVUP_NODE_NOT_FOUND");

// 다이나믹 페이지 문서에서는 자식이 아직 메모리에 없을 수 있다. search.js 와 같은
// 방법으로 대상이 속한 페이지를 먼저 올린다.
let page = root;
while (page && page.type !== "PAGE") page = page.parent;
if (page && page.type === "PAGE") await figma.setCurrentPageAsync(page);

const nodes = [];

// 자손 수를 돌려주면서 목록을 채운다. 깊이가 아주 깊은 문서에서 호출 스택이
// 터지지 않도록 명시적 스택으로 후위 순회한다.
function collect(start) {
  const stack = [{ node: start, visited: false }];
  const counts = new Map();
  while (stack.length > 0) {
    const frame = stack[stack.length - 1];
    const node = frame.node;
    const children = "children" in node ? node.children : [];
    if (!frame.visited) {
      frame.visited = true;
      for (let index = children.length - 1; index >= 0; index -= 1) {
        stack.push({ node: children[index], visited: false });
      }
      continue;
    }
    stack.pop();
    let descendantCount = 0;
    const childrenIds = [];
    for (const child of children) {
      childrenIds.push(child.id);
      descendantCount += 1 + (counts.get(child.id) || 0);
    }
    counts.set(node.id, descendantCount);
    nodes.push({
      id: node.id,
      type: node.type,
      name: node.name,
      childrenIds,
      descendantCount,
    });
  }
}

collect(root);

return {
  fileKey: figma.fileKey || "",
  version: null,
  rootId: root.id,
  nodes,
};
