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

export const here = `${process.platform}-${process.arch}${process.platform === 'linux' ? '-gnu' : process.platform === 'win32' ? '-msvc' : ''}`;

export function load(): Addon {
	const require = createRequire(import.meta.url);
	const file = `teasel.${here}.node`;
	try {
		return require(`../${file}`);
	} catch (e) {
		if (!(e instanceof Error && 'code' in e && e.code === 'MODULE_NOT_FOUND')) throw e;
		return require(`@teasel/parser-${here}/${file}`);
	}
}
