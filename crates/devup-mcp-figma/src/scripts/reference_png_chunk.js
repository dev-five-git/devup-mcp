// The next piece of a referencePng answer that referencePng held because it
// did not fit one bridge answer. Bridge only: the direct path never asks.
const options = "__DEVUP_LARGE_VALUE__";
const store = globalThis.__devupReferencePngAnswers;
const entry = store && store.get(options.id);
if (!entry || typeof entry.text !== "string") {
  throw new Error(`DEVUP_REFERENCE_PNG_ANSWER_GONE ${options.id}`);
}
const text = entry.text;
const offset = Math.max(0, Math.floor(Number(options.offset) || 0));
const end = Math.min(text.length, offset + 4 * 1024 * 1024);
if (end >= text.length) store.delete(options.id);
return { kind: "devupReferencePngChunk", id: options.id, offset, nextOffset: end, complete: end >= text.length, text: text.slice(offset, end) };
