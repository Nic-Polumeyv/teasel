// bun release.js: publishes the platform packages from the addons under artifacts/, then this package
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { platforms } from './native.js';

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
