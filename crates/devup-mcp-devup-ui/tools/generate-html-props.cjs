// Generate the HTML prop-name catalog from React's real resolved types.
// node crates/devup-mcp-devup-ui/tools/generate-html-props.cjs <node_modules>
// Then run cargo fmt --all. No files under node_modules are modified.
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(process.argv[2]);
const ts = require(path.join(root, 'typescript'));
const reactFile = path.join(root, '@types/react/index.d.ts');
const version = JSON.parse(fs.readFileSync(path.join(root, '@types/react/package.json'))).version;
const program = ts.createProgram([reactFile], { skipLibCheck: true, noEmit: true });
const checker = program.getTypeChecker();
let intrinsic;
const visit = node => {
  if (ts.isInterfaceDeclaration(node) && node.name.text === 'IntrinsicElements') intrinsic = node;
  ts.forEachChild(node, visit);
};
visit(program.getSourceFile(reactFile));
if (!intrinsic) throw new Error('React JSX.IntrinsicElements not found');
const entries = intrinsic.members.filter(member => member.type.getText().includes('DetailedHTMLProps'))
  .map(member => [member.name.text, checker.getTypeAtLocation(member).getProperties()
    .map(prop => prop.name).filter(name => !name.startsWith('on') && !name.startsWith('aria-')).sort()]);
if (entries.length < 100 || !entries.find(([tag, props]) => tag === 'input' && props.includes('maxLength')))
  throw new Error('React HTML props did not resolve');
const common = entries.find(([tag]) => tag === 'span')[1];
if (common.includes('maxLength')) throw new Error('Element-specific prop leaked into common props');
const groups = new Map();
for (const [tag, props] of entries) {
  const extra = props.filter(prop => !common.includes(prop)).join(' ');
  if (!groups.has(extra)) groups.set(extra, []);
  groups.get(extra).push(tag);
}
const lines = [
  `//! Generated from @types/react ${version} JSX.IntrinsicElements via TypeScript's type checker.`,
  '//! Regenerate with tools/generate-html-props.cjs; do not add guessed attributes.',
  '//! @devup-ui/react 1.0.41 merges ComponentProps<T> for literal intrinsic as=T.',
  '',
  'pub(crate) fn is_html_prop(tag: &str, prop: &str) -> bool {',
  '    let specific = match tag {',
  ...[...groups].map(([props, tags]) => `        ${tags.map(JSON.stringify).join(' | ')} => ${JSON.stringify(props)},`),
  '        _ => return false,',
  '    };',
  `    ${JSON.stringify(common.join(' '))}.split_whitespace().chain(specific.split_whitespace()).any(|known| known == prop)`,
  '}',
  '',
];
fs.writeFileSync(path.join(__dirname, '../src/html_props.rs'), lines.join('\n'));
console.log(`Generated ${entries.length} HTML elements in ${groups.size} attribute groups from @types/react ${version}`);
