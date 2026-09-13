// bun release.js build [--target TRIPLE]: builds the addon for this machine, or the target given, into teasel.<platform>.node
// bun release.js versions: carries the version changesets wrote into package.json over to the crates, so
//   the Version Packages PR holds every number and the release gate reads the one the changelog announces
// bun release.js publish: publishes the platform packages from the addons under artifacts/, then this package
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { here, platforms } from './native.js';

const command = process.argv[2];
if (command === 'build') {
	const at = process.argv.indexOf('--target');
	const target = at === -1 ? platforms[here].target : process.argv[at + 1];
	const [tag, { os }] = Object.entries(platforms).find(([, p]) => p.target === target) ?? [];
	if (tag === undefined) throw new Error(`no platform builds for ${target}`);

	const { status } = spawnSync('cargo', ['build', '--release', '-p', 'teasel-node', '--target', target], { stdio: 'inherit', cwd: '..' });
	if (status !== 0) process.exit(status ?? 1);
	const lib = os === 'win32' ? 'teasel_node.dll' : os === 'darwin' ? 'libteasel_node.dylib' : 'libteasel_node.so';
	copyFileSync(`../target/${target}/release/${lib}`, `teasel.${tag}.node`);
} else if (command === 'versions') {
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
} else if (command === 'publish') {
	const root = JSON.parse(readFileSync('package.json', 'utf8'));
	const { version, description, license, repository } = root;

	// a version the registry has is left alone, so a run that died halfway can be run again
	async function publish(cwd, name) {
		const { ok } = await fetch(`https://registry.npmjs.org/${name}/${version}`);
		if (ok) {
			console.log(`${name}@${version} is published`);
			return;
		}
		const { status } = spawnSync('npm', ['publish', '--access', 'public', '--provenance'], { stdio: 'inherit', cwd });
		if (status !== 0) process.exit(status ?? 1);
	}

	for (const [tag, { target, os, cpu, libc }] of Object.entries(platforms)) {
		const name = `@teasel/parser-${tag}`;
		const file = `teasel.${tag}.node`;
		const dir = `artifacts/${tag}`;
		mkdirSync(dir, { recursive: true });
		copyFileSync(`artifacts/binding-${target}/${file}`, `${dir}/${file}`);
		const manifest = { name, version, description, license, repository, os: [os], cpu: [cpu], ...(libc && { libc: [libc] }), main: file, files: [file] };
		writeFileSync(`${dir}/package.json`, JSON.stringify(manifest, null, '\t') + '\n');
		await publish(dir, name);
	}

	root.optionalDependencies = Object.fromEntries(Object.keys(platforms).map((tag) => [`@teasel/parser-${tag}`, version]));
	writeFileSync('package.json', JSON.stringify(root, null, '\t') + '\n');
	await publish('.', root.name);
} else {
	throw new Error(`bun release.js build | versions | publish, not ${command}`);
}
