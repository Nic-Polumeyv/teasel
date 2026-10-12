import { readFileSync } from 'node:fs';
import { start } from './wasm.ts';

// in step: 7 ms against 13 async for 1.1 MB, and require() refuses a graph with top-level await
// TODO: `import source` once we require node >= 22.19 and bundlers accept it: no fs, ~10 ms less startup
export const engine = start(new WebAssembly.Module(readFileSync(new URL('../teasel.wasm', import.meta.url))));
