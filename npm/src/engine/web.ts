import type { Engine } from '../types.ts';
import { start } from './wasm.ts';

// `teasel.wasm` beside the package entry, read where there is a file system and fetched elsewhere
const url = new URL('../teasel.wasm', import.meta.url);
const { module, instance } =
	url.protocol === 'file:'
		? await WebAssembly.instantiate(await globalThis.process.getBuiltinModule('node:fs/promises').readFile(url), {})
		: await WebAssembly.instantiateStreaming(fetch(url), {});
export const engine: Engine = start(module, instance);
