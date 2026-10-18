
const doc = {id:'0:0', type:'DOCUMENT'};
const page = {id:'1:1', type:'PAGE', name:'Page', parent:doc, children:[]};
function node(id, type, name, parent) {
 const n = {id, type, name, parent, children:[], x:0, y:0, width:390, height:844, visible:true};
 n.findAll = predicate => n.children.flatMap(c => [c, ...c.findAll(predicate)]).filter(predicate);
 parent.children.push(n); return n;
}
const section = node('2:1', 'SECTION', 'Loading section', page);
const frame = node('3:1', 'FRAME', 'Loading', section);
node('3:2', 'FRAME', 'Loading', page);
page.findAll = () => { throw new Error('must not traverse whole page'); };
const figma = {fileKey:'file', getNodeByIdAsync: async () => section,
 setCurrentPageAsync: async p => { if (p !== page) throw new Error('wrong page'); }};
