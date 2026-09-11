// bun release.js: publishes the platform packages from the addons under artifacts/, then this package
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { platforms } from './native.js';

const root = JSON.parse(readFileSync('package.json', 'utf8'));
const { version, description, license, repository } = root;

function publish(cwd) {
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
	publish(dir);
}

root.optionalDependencies = Object.fromEntries(Object.keys(platforms).map((tag) => [`@teasel/parser-${tag}`, version]));
writeFileSync('package.json', JSON.stringify(root, null, '\t') + '\n');
publish('.');
