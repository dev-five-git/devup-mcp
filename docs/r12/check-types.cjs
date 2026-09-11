// Read installed types without modifying the reference repository.
// node docs/r12/check-types.cjs <reference apps/front/node_modules>
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(process.argv[2]);
const ts = require(path.join(root, 'typescript'));
const file = path.join(root, '../r12-read-only-type-check.tsx');
const source = `import { Text, Box, Flex } from '@devup-ui/react';
<Text as="input" maxLength={50} placeholder="name" type="text" />;
<Text maxLength={500} as={'input'} />;
<Text as="textarea" maxLength={500} wrap="soft" />;
<Box as="form" action="/submit" encType="multipart/form-data" />;
<Flex as="video" playsInline controlsList="nodownload" />;
<Text maxLength={50} />;
<Text as="button" maxLength={50} />;
<Text as="input" maxLenght={50} />;
`;
const options = { strict: true, noEmit: true, skipLibCheck: true, jsx: ts.JsxEmit.ReactJSX,
  target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.Bundler };
const host = ts.createCompilerHost(options);
const originalRead = host.readFile.bind(host);
host.readFile = name => path.resolve(name) === file ? source : originalRead(name);
const originalExists = host.fileExists.bind(host);
host.fileExists = name => path.resolve(name) === file || originalExists(name);
const program = ts.createProgram([file], options, host);
const diagnostics = ts.getPreEmitDiagnostics(program);
const lines = diagnostics.map(d => ({ line: d.file.getLineAndCharacterOfPosition(d.start).line + 1,
  code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, ' ') }));
console.log(JSON.stringify({devup: JSON.parse(fs.readFileSync(path.join(root, '@devup-ui/react/package.json'))).version,
  reactTypes: JSON.parse(fs.readFileSync(path.join(root, '@types/react/package.json'))).version, diagnostics: lines}, null, 2));
if (JSON.stringify(lines.map(d => d.line)) !== JSON.stringify([7, 8, 9])) process.exitCode = 1;
