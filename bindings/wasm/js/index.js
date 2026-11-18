// Node, Bun and Deno: the module is loaded from disk when this file is imported.
import { readFileSync } from "node:fs";
import * as raw from "../pkg/pinakes_wasm.js";
import { bind } from "./api.js";

raw.initSync({ module: readFileSync(new URL("../pkg/pinakes_wasm_bg.wasm", import.meta.url)) });

const api = bind(raw);
export const {
  read,
  write,
  normalise,
  assignKeys,
  keyFor,
  findDuplicates,
  normaliseId,
  detectId,
  formats,
} = api;
export default api;
