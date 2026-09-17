import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import type { Tree } from './arena.js';

/** The external V8 holds for the addon: a prepared source. */
export type External = object;

/** `crates/teasel-node/src/lib.rs`: the seven operations over a prepared source, the source as V8's bytes. */
export interface Addon {
	readonly create: (source: Uint8Array, flags: number, host: string) => External;
	readonly parse: (held: External, entry: number, offset: number, end: number | undefined, stop: string) => Uint32Array | string;
	readonly free: (held: External) => void;
	readonly constants: () => string[];
	readonly shapes: () => number[];
	readonly layout: () => string;
	/** Whether the tree is the TypeScript one, then each view followed by its length in elements. */
	readonly tree: () => Tree | undefined;
}

export const platforms: Record<string, { target: string; os: NodeJS.Platform; cpu: NodeJS.Architecture; libc?: string }> = {
	'linux-x64-gnu': { target: 'x86_64-unknown-linux-gnu', os: 'linux', cpu: 'x64', libc: 'glibc' },
	'linux-arm64-gnu': { target: 'aarch64-unknown-linux-gnu', os: 'linux', cpu: 'arm64', libc: 'glibc' },
	'darwin-x64': { target: 'x86_64-apple-darwin', os: 'darwin', cpu: 'x64' },
	'darwin-arm64': { target: 'aarch64-apple-darwin', os: 'darwin', cpu: 'arm64' },
	'win32-x64-msvc': { target: 'x86_64-pc-windows-msvc', os: 'win32', cpu: 'x64' },
};

// a glibc build does not load on musl: ldd is a script that names its libc, 0.07 ms; the report is 6 ms, for a Linux without one
function musl(): boolean {
	try {
		return readFileSync('/usr/bin/ldd', 'utf8').includes('musl');
	} catch {
		return !(process.report?.getReport() as { header: { glibcVersionRuntime?: string } }).header.glibcVersionRuntime;
	}
}

export const here = `${process.platform}-${process.arch}${process.platform === 'linux' ? (musl() ? '-musl' : '-gnu') : process.platform === 'win32' ? '-msvc' : ''}`;

const missing = (e: unknown) => e instanceof Error && 'code' in e && e.code === 'MODULE_NOT_FOUND';

export function load(): Addon {
	const require = createRequire(import.meta.url);
	// we add .node so require dlopens it instead of reading it as js
	const file = `teasel.${here}.node`;
	try {
		return require(`../${file}`);
	} catch (e) {
		if (!missing(e)) throw e;
	}
	try {
		return require(`@teasel/parser-${here}/${file}`);
	} catch (e) {
		if (!missing(e)) throw e;
		throw new Error(`no native build for ${here}; the WebAssembly build runs anywhere: node --no-addons`, { cause: e });
	}
}
