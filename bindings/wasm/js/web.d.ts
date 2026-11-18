import type { Pinakes } from "./index.js";
export type * from "./index.js";

/** Load the WebAssembly module, from `url` if given, and return the Pinakes functions. */
export default function load(url?: string | URL): Promise<Pinakes>;
