// scripts/addon.ts imports this file to build the binary that native.ts loads on import, so the two stay apart
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import type { Tree } from '../types.ts';

/** What V8 holds for the addon: a prepared source, or a grammar's plan. */
export type External = object;

/** `crates/teasel-node/src/lib.rs`: the operations over a prepared source and a grammar, the source as V8's bytes. */
export interface Addon {
	readonly create: (source: Uint8Array, flags: number) => External;
	readonly parse: (held: External, entry: number, offset: number, end: number | undefined, stop: string, plan: External | undefined) => Uint32Array | string;
	readonly once: (source: Uint8Array, flags: number, entry: number, stop: string, plan: External | undefined) => Uint32Array | string;
	/** The grammar read once; V8 lets go of it with the external. */
	readonly plan: (grammar: Uint8Array) => External;
	readonly children: (plan: External | undefined) => string;
	readonly free: (held: External) => void;
	readonly layout: () => string;
	/** Whether the tree is the TypeScript one, then each view as long as its buffer's room; the lengths ride in the answer's words. */
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

// a build linked against a newer glibc than the system's fails to dlopen: WebAssembly parses there, as it does without a build
export function load(): Addon | undefined {
	const require = createRequire(import.meta.url);
	// we add .node so require dlopens it instead of reading it as js
	const file = `teasel.${here}.node`;
	let failed: Error | undefined;
	for (const path of [`../${file}`, `@teasel/parser-${here}/${file}`]) {
		try {
			return require(path);
		} catch (e) {
			if (!(e instanceof Error && 'code' in e && (e.code === 'MODULE_NOT_FOUND' || e.code === 'ERR_DLOPEN_FAILED'))) throw e;
			if (e.code === 'ERR_DLOPEN_FAILED') failed ??= e;
		}
	}
	// where no build exists there is nothing to fix
	if (here in platforms) {
		const why = failed === undefined ? `@teasel/parser-${here} is not installed` : `the native build for ${here} did not load (${failed.message})`;
		process.emitWarning(`${why}; @teasel/parser parses in WebAssembly instead, slower`, { code: 'wasm_fallback' });
	}
}
