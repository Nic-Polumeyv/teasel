import { createRequire } from 'node:module';

export const platforms = {
	'linux-x64-gnu': { target: 'x86_64-unknown-linux-gnu', os: 'linux', cpu: 'x64', libc: 'glibc' },
	'linux-arm64-gnu': { target: 'aarch64-unknown-linux-gnu', os: 'linux', cpu: 'arm64', libc: 'glibc' },
	'darwin-x64': { target: 'x86_64-apple-darwin', os: 'darwin', cpu: 'x64' },
	'darwin-arm64': { target: 'aarch64-apple-darwin', os: 'darwin', cpu: 'arm64' },
	'win32-x64-msvc': { target: 'x86_64-pc-windows-msvc', os: 'win32', cpu: 'x64' },
};

export const here = `${process.platform}-${process.arch}${{ linux: '-gnu', win32: '-msvc' }[process.platform] ?? ''}`;

export function load() {
	const require = createRequire(import.meta.url);
	const file = `teasel.${here}.node`;
	try {
		return require(`./${file}`);
	} catch (e) {
		if (e.code !== 'MODULE_NOT_FOUND') throw e;
		return require(`@teasel/parser-${here}/${file}`);
	}
}
