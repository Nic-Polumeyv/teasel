// node versions.js: carries the version changesets wrote into package.json over to the crates, so
// the Version Packages PR holds every number and the release gate reads the one the changelog announces
import { readFileSync, writeFileSync } from 'node:fs';

const { version } = JSON.parse(readFileSync('package.json', 'utf8'));

function patch(file, from, to) {
	const text = readFileSync(file, 'utf8');
	if (!from.test(text)) throw new Error(`${file}: nothing matches ${from}`);
	writeFileSync(file, text.replace(from, to));
}

for (const file of ['../Cargo.toml', '../bindings/node/Cargo.toml', '../bindings/wasm/Cargo.toml']) {
	patch(file, /^version = "[^"]*"$/m, `version = "${version}"`);
}
for (const name of ['teasel', 'teasel-node', 'teasel-wasm']) {
	patch('../Cargo.lock', new RegExp(`(\\[\\[package\\]\\]\\nname = "${name}"\\nversion = ")[^"]*`), `$1${version}`);
}
console.log(`crates at ${version}`);
