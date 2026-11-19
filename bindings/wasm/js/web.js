// Browsers and bundlers: fetch the module once, then use what `load` returns.
import init, * as raw from "../pkg/pinakes_wasm.js";
import { bind } from "./api.js";

let loaded;

/** Load the WebAssembly module, from `url` if given, and return the Pinakes functions. */
export default function load(url) {
  loaded ??= init(url ? { module_or_path: url } : undefined).then(() => bind(raw));
  return loaded;
}
