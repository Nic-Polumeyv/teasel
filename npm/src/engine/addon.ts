// scripts/addon.ts imports this file to build the binary that node.ts loads on import, so the two stay apart
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import type { Engine, Tree } from '../types.ts';

/** `crates/teasel-node/src/lib.rs`: the operations over a prepared source and a grammar, the source as V8's bytes. */
export interface Addon extends Omit<Engine, 'create' | 'once' | 'tree'> {
	readonly create: (source: Uint8Array, flags: number) => object;
	readonly once: (source: Uint8Array, flags: number, entry: number, stop: string, plan: object | undefined) => Uint32Array | string;
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
function libc() {
	try {
		return readFileSync('/usr/bin/ldd', 'utf8').includes('musl') ? 'musl' : 'glibc';
	} catch {
		return 'glibc';
	}
}

// a build linked against a newer glibc than the system's fails to dlopen: WebAssembly parses there, as it does without a build
export function load(): Addon | undefined {
	const require = createRequire(import.meta.url);
	let why: NodeJS.ErrnoException | undefined;
	for (const [tag, p] of Object.entries(platforms)) {
		if (p.os !== process.platform || p.cpu !== process.arch || (p.libc !== undefined && p.libc !== libc())) continue;
		// we add .node so require dlopens it instead of reading it as js
		for (const path of [`../teasel.${tag}.node`, `@teasel/parser-${tag}/teasel.${tag}.node`]) {
			try {
				return require(path);
			} catch (e) {
				const error = e as NodeJS.ErrnoException;
				// addons turned off by flag: WebAssembly was asked for
				if (error.code === 'ERR_DLOPEN_DISABLED') return;
				// a build that fails to load says more than one that is not there
				if (why === undefined || why.code === 'MODULE_NOT_FOUND') why = error;
			}
		}
	}
	// where no build exists there is nothing to fix
	if (why !== undefined) process.emitWarning(`no native build loaded for ${process.platform}-${process.arch} (${why.message.split('\n')[0]}); @teasel/parser parses in WebAssembly instead, which is slower`, { code: 'wasm_fallback' });
}
