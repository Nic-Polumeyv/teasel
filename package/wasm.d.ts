export * from './index.js';

/**
 * Instantiates the module: the bytes, a compiled `WebAssembly.Module`, or a `Response` (or a
 * promise of one) to stream it from; by default `teasel.wasm` next to this file is fetched.
 */
export function init(module?: BufferSource | WebAssembly.Module | Response | Promise<Response>): Promise<void>;
