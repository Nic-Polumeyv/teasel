import { createRequire } from 'node:module';

/** The external V8 holds for the addon: a prepared source. */
export type External = object;

/** `crates/teasel-node/src/lib.rs`: the five operations over a prepared source, the source as V8's bytes. */
export interface Addon {
	readonly create: (source: Uint8Array, flags: number, host: string) => External;
	readonly parse: (held: External, entry: number, offset: number, end: number | undefined, stop: string) => Uint32Array | string;
	readonly free: (held: External) => void;
	readonly constants: () => string[];
	readonly shapes: () => number[];
}

export interface Platform {
	target: string;
	os: NodeJS.Platform;
	cpu: NodeJS.Architecture;
	libc?: string;
}
export const platforms: Record<string, Platform> = {
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
