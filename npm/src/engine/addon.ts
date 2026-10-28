// scripts/addon.ts imports this file to build the binary that node.ts loads on import, so the two stay apart
const { existsSync, readFileSync } = process.getBuiltinModule('node:fs');
const { createRequire } = process.getBuiltinModule('node:module');
import type { Engine, Tree } from '../types.ts';

/** `crates/teasel-node/src/lib.rs`: the operations over a source and a grammar, the source as V8's bytes. */
export interface Addon extends Omit<Engine, 'parse' | 'tree'> {
	readonly parse: (source: Uint8Array, flags: number, entry: number, offset: number, end: number | undefined, stop: string, plan: object | undefined) => Uint32Array | string;
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
	// the permission model refuses a read of the platform package before it refuses the build
	if (process.permission?.has('addon') === false) return;
	const require = createRequire(import.meta.url);
	let why: NodeJS.ErrnoException | undefined;
	for (const [tag, p] of Object.entries(platforms)) {
		if (p.os !== process.platform || p.cpu !== process.arch || (p.libc !== undefined && p.libc !== libc())) continue;
		// we add .node so require dlopens it instead of reading it as js
		const own = `../teasel.${tag}.node`;
		try {
			return require(existsSync(new URL(own, import.meta.url)) ? own : `@teasel/parser-${tag}/teasel.${tag}.node`);
		} catch (e) {
			why = e as NodeJS.ErrnoException;
			// --no-addons asks for WebAssembly
			if (why.code === 'ERR_DLOPEN_DISABLED') return;
		}
	}
	// where no build exists there is nothing to fix
	if (why !== undefined) process.emitWarning(`no native build loaded for ${process.platform}-${process.arch} (${why.message.split('\n')[0]}); @teasel/parser parses in WebAssembly instead, which is slower`, { code: 'wasm_fallback' });
}
