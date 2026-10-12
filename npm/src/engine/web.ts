import type { Engine } from '../types.ts';
import { start, url } from './wasm.ts';

// read where there is a file system and fetched elsewhere
const { module, instance } =
	url.protocol === 'file:'
		? await WebAssembly.instantiate(await globalThis.process.getBuiltinModule('node:fs/promises').readFile(url), {})
		: await WebAssembly.instantiateStreaming(fetch(url), {});
export const engine: Engine = start(module, instance);
