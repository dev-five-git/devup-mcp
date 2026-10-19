import { readFile } from 'node:fs/promises';

// Vite versions use both messages. Dependency discovery alone is harmless;
// only an announced reload makes the current measurement unreliable.
export function reoptimizationReload(text) {
  return /(?:new dependencies optimized\s*[,.:]\s*reloading|optimized dependencies changed\s*[,.：:]\s*reloading)/i.test(
    text.replace(/\x1b\[[0-9;]*m/g, ''),
  );
}

export function observeDevServer(page, errors) {
  page.on('console', (message) => {
    if (reoptimizationReload(message.text())) {
      errors.push({ kind: 'dev-reoptimization-reload', message: message.text() });
    }
  });
  page.on('websocket', (socket) => socket.on('framereceived', ({ payload }) => {
    try {
      const event = JSON.parse(String(payload));
      // Vite's optimizer sends {type:'full-reload', path:'*'}, without a
      // reason. Preserve that uncertainty unless the server log confirms it.
      if (event.type === 'full-reload') errors.push({
        kind: 'dev-server-reload', message: `dev server requested a full reload (${event.path ?? 'unspecified path'})`,
      });
    } catch { /* Other WebSocket protocols are unrelated to Vite HMR. */ }
  }));
}

export async function serverLogReader(path) {
  if (!path) return async () => [];
  // Ignore historical startup output. Only bytes appended during this audit
  // can invalidate a screen. A missing/unreadable log is a caller error.
  const initial = await readFile(path);
  // Windows PowerShell redirects native output as UTF-16LE with a BOM.
  const encoding = initial[0] === 0xff && initial[1] === 0xfe ? 'utf16le' : 'utf8';
  let offset = initial.length;
  let pending = '';
  return async () => {
    const bytes = await readFile(path);
    if (bytes.length < offset) { offset = 0; pending = ''; }
    const end = encoding === 'utf16le' ? bytes.length - bytes.length % 2 : bytes.length;
    pending += bytes.subarray(offset, end).toString(encoding);
    offset = end;
    const lines = pending.split(/\r?\n/);
    pending = lines.pop();
    return lines.flatMap((message) => {
      if (reoptimizationReload(message)) return [{ kind: 'dev-reoptimization-reload', message }];
      if (/\(ssr\)\s+program reload/i.test(message.replace(/\x1b\[[0-9;]*m/g, ''))) {
        return [{ kind: 'dev-server-reload', message }];
      }
      return [];
    });
  };
}

export function invalidateReloadedScreen(record, errors) {
  if (!errors.some((error) => ['dev-server-reload', 'dev-reoptimization-reload'].includes(error.kind))) return false;
  record.status = 'skipped';
  record.reason = 'dev server reloaded during measurement; stabilize dependencies and rerun this screen';
  for (const check of record.checks) {
    check.status = 'inconclusive';
    check.reason = record.reason;
  }
  // Keep diagnostic captures on disk, but do not offer them as valid visual
  // comparison inputs after their document was replaced during measurement.
  delete record.screenshot;
  for (const check of record.checks) delete check.screenshot;
  return true;
}
