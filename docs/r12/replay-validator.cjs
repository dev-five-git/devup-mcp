// Replays the complete WQUW-118 validator input against this worktree's binary.
// node docs/r12/replay-validator.cjs <calls.json> <output.json> <expected errors>
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { createInterface } = require('node:readline');
const [callsPath, outputPath, errors] = process.argv.slice(2);
const call = JSON.parse(fs.readFileSync(callsPath, 'utf8')).at(-1);
const binary = path.resolve('target/debug/devup-mcp.exe');
const child = spawn(binary, [], { stdio: ['pipe', 'pipe', 'inherit'] });
const timer = setTimeout(() => { child.kill(); process.exitCode = 1; }, 30000);
const send = message => child.stdin.write(JSON.stringify({ jsonrpc: '2.0', ...message }) + '\n');
createInterface({ input: child.stdout }).on('line', line => {
  const message = JSON.parse(line);
  if (message.id === 1) {
    send({ method: 'notifications/initialized' });
    send({ id: 2, method: 'tools/call', params: { name: call.tool, arguments: call.arguments } });
  } else if (message.id === 2) {
    clearTimeout(timer);
    const result = message.result;
    const structured = result?.structuredContent;
    const record = { binary, inputBytes: Buffer.byteLength(call.arguments.tsx),
      inputSha256: require('node:crypto').createHash('sha256').update(call.arguments.tsx).digest('hex'),
      isError: result?.isError, ok: structured?.ok, okReason: structured?.okReason,
      violationCounts: structured?.violationCounts, server: structured?.server,
      errors: structured?.violations?.filter(v => v.severity === 'error'),
      recoveryState: structured?.recoveryState, recoveryReason: structured?.recoveryReason,
      contentMatchesStructured: JSON.stringify(JSON.parse(result.content[0].text)) === JSON.stringify(structured) };
    fs.writeFileSync(outputPath, JSON.stringify(record, null, 2) + '\n');
    console.log(JSON.stringify({ binary, inputBytes: record.inputBytes, ok: structured?.ok,
      violationCounts: structured?.violationCounts, server: structured?.server }));
    if (structured?.violationCounts?.error !== Number(errors) ||
        JSON.stringify(JSON.parse(result.content[0].text)) !== JSON.stringify(structured)) process.exitCode = 1;
    child.stdin.end();
  }
});
child.on('error', error => { clearTimeout(timer); console.error(error); process.exitCode = 1; });
send({ id: 1, method: 'initialize', params: { protocolVersion: '2024-11-05', capabilities: {},
  clientInfo: { name: 'r12-local-replay', version: '1' } } });
