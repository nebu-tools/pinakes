// The public JavaScript interface, over the raw wasm-bindgen exports. Values cross into
// WebAssembly as JSON text; this file is the only place that knows it.

const text = (input) => (typeof input === "string" ? new TextEncoder().encode(input) : input);

/** @param {typeof import("../pkg/pinakes_wasm.js")} raw */
export function bind(raw) {
  const items = (refs) => JSON.stringify(Array.isArray(refs) ? refs : [refs]);
  return {
    read(input, options = {}) {
      return JSON.parse(raw.readJson(text(input), options.format ?? undefined));
    },
    write(references, format) {
      return JSON.parse(raw.writeJson(items(references), format));
    },
    normalise(references) {
      const lib = JSON.parse(raw.normaliseJson(items(references)));
      return { references: lib.references, diagnostics: lib.diagnostics };
    },
    assignKeys(references, options = {}) {
      return JSON.parse(raw.assignKeysJson(items(references), Boolean(options.replace)));
    },
    keyFor(reference) {
      return raw.keyForJson(items(reference));
    },
    findDuplicates(references) {
      return JSON.parse(raw.findDuplicatesJson(items(references)));
    },
    normaliseId(kind, value) {
      return raw.normaliseId(kind, value);
    },
    detectId(value) {
      const found = raw.detectIdJson(value);
      if (!found) return null;
      const [kind, normal] = JSON.parse(found);
      return { kind, value: normal };
    },
    formats() {
      return JSON.parse(raw.formatsJson());
    },
  };
}
