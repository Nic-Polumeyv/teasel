// scripts/addon.ts imports this file to build the binary that node.ts loads on import, so the two stay apart
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

// a glibc build does not load on musl: ldd is a script that names its libc; without one glibc is assumed, and a musl box only gets the warning
function libc(): string {
	try {
		return readFileSync('/usr/bin/ldd', 'utf8').includes('musl') ? 'musl' : 'glibc';
	} catch {
		return 'glibc';
	}
}

// a build linked against a newer glibc than the system's fails to dlopen: WebAssembly parses there, as it does without a build
export function load(): Addon | undefined {
	const require = createRequire(import.meta.url);
	let why: { code?: string; message: string } | undefined;
	for (const [tag, p] of Object.entries(platforms)) {
		if (p.os !== process.platform || p.cpu !== process.arch || (p.libc !== undefined && p.libc !== libc())) continue;
		// we add .node so require dlopens it instead of reading it as js
		for (const path of [`../teasel.${tag}.node`, `@teasel/parser-${tag}/teasel.${tag}.node`]) {
			try {
				return require(path);
			} catch (e) {
				const error = e as Error & { code?: string };
				// a build that fails to load says more than one that is not there
				if (why === undefined || why.code === 'MODULE_NOT_FOUND') why = error;
			}
		}
	}
	// where no build exists there is nothing to fix
	if (why !== undefined) process.emitWarning(`no native build loaded for ${process.platform}-${process.arch} (${why.message.split('\n')[0]}); @teasel/parser parses in WebAssembly instead, which is slower`, { code: 'wasm_fallback' });
}
