// Differential test: acorn-typescript versus teasel over the TypeScript in a Svelte checkout, a
// SvelteKit checkout and the `lang="ts"` scripts of the Svelte test fixtures.
//
//   SVELTE_DIR=~/Projects/svelte KIT_DIR=~/Projects/kit bun typescript.js [--dts] [--verbose] [--limit N] [filter]
//
// `--dts` adds every `.d.ts` under the Svelte checkout's node_modules.

import * as acorn from 'acorn';
import { tsPlugin } from '@sveltejs/acorn-typescript';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { args, compare, normalize_ts, root, teasel } from './lib.js';

const Parser = acorn.Parser.extend(tsPlugin());
const kit = process.env.KIT_DIR ?? join(process.env.HOME, 'Projects/kit');
const { verbose, limit, filter } = args();
const dts = process.argv.includes('--dts');
const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;

function* sources(dir, pattern = /\.(ts|svelte)$/, into_modules = false) {
	for (const name of readdirSync(dir)) {
		if ((name === 'node_modules' && !into_modules) || name === '.svelte-kit' || name.startsWith('.')) continue;
		const path = join(dir, name);
		let stat;
		try {
			stat = statSync(path);
		} catch {
			continue;
		}
		if (stat.isDirectory()) yield* sources(path, pattern, into_modules);
		else if (pattern.test(name)) yield path;
	}
}

function reference(source) {
	try {
		const ast = Parser.parse(source, { ecmaVersion: 16, sourceType: 'module', locations: true });
		return JSON.parse(JSON.stringify(ast, normalize_ts));
	} catch (e) {
		if (!(e instanceof SyntaxError) || e.pos === undefined) return { error: { message: `acorn threw ${e.name}: ${e.message}`, pos: -1 } };
		return { error: { message: e.message.replace(/ \(\d+:\d+\)$/, ''), pos: e.pos, loc: { line: e.loc.line, column: e.loc.column } } };
	}
}

const jobs = [];
const push = (name, source) => jobs.push({ name, source, mode: 'ts-module' });
for (const dir of [join(root, 'packages'), join(kit, 'packages')]) {
	for (const path of sources(dir)) {
		const name = relative(dir, path);
		if (filter && !name.includes(filter)) continue;
		const text = readFileSync(path, 'utf8');
		if (path.endsWith('.ts')) push(name, text);
		else for (const match of text.matchAll(script_re)) {
			if (!/lang=["']?ts/.test(match[1] ?? '')) continue;
			push(`${name}#${match.index}`, match[2]);
		}
	}
}
if (dts) {
	const modules = join(root, 'node_modules/.pnpm');
	for (const path of sources(modules, /\.d\.ts$/, true)) {
		const name = relative(modules, path);
		if (filter && !name.includes(filter)) continue;
		push(name, readFileSync(path, 'utf8'));
	}
}
if (jobs.length > limit) jobs.length = limit;

const lines = await teasel(jobs);
process.exit(compare(jobs, (job) => reference(job.source), lines, { verbose, label: `${root} + ${kit}` }) ? 0 : 1);
