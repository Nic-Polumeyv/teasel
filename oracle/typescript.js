// Differential test: acorn-typescript versus teasel over the TypeScript in a Svelte checkout, a
// SvelteKit checkout and the `lang="ts"` scripts of the Svelte test fixtures.
//
//   SVELTE_DIR=~/Projects/svelte KIT_DIR=~/Projects/kit bun typescript.js [--dts] [--verbose] [--limit N] [filter]
//
// `--dts` adds every `.d.ts` under the Svelte checkout's node_modules.

import * as acorn from 'acorn';
import { tsPlugin } from '@sveltejs/acorn-typescript';
import { readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { acorn_error, args, compare, diff, files, normalize_ts, plugin_rejects_valid, root, teasel, ts_jobs } from './lib.js';

const Parser = acorn.Parser.extend(tsPlugin());
const { verbose, limit, filter } = args();
const dts = process.argv.includes('--dts');

function reference(source) {
	try {
		const ast = Parser.parse(source, { ecmaVersion: 16, sourceType: 'module', locations: true });
		return JSON.parse(JSON.stringify(ast, normalize_ts));
	} catch (e) {
		return acorn_error(e, source, false);
	}
}

const { jobs, label } = ts_jobs('ts-module', filter);
if (dts) {
	const modules = join(root, 'node_modules/.pnpm');
	for (const path of files(modules, /\.d\.ts$/, true)) {
		const name = relative(modules, path);
		// a linked teasel package brings its own node_modules along
		if (name.includes('/@teasel/')) continue;
		if (filter && !name.includes(filter)) continue;
		jobs.push({ name, source: readFileSync(path, 'utf8'), mode: 'ts-module' });
	}
}
if (jobs.length > limit) jobs.length = limit;

// Where acorn-typescript is wrong and teasel follows TypeScript: it rejects every ambient
// initializer, misses declarations that satisfy an export, reports a modifier's column as its
// offset, and marks the callee of `a.b?.<T>()` optional.
function plugin_bug(expected, actual) {
	if (!expected.error && !actual.error) return !!diff(expected, actual) && !diff(without_callee_optional(expected), without_callee_optional(actual));
	if (!expected.error || (actual.error && actual.error.message !== expected.error.message)) return false;
	if (actual.error) return expected.error.pos === actual.error.loc?.column && actual.error.pos !== expected.error.pos;
	return plugin_rejects_valid(expected.error.message);
}

function without_callee_optional(node) {
	if (Array.isArray(node)) return node.map(without_callee_optional);
	if (!node || typeof node !== 'object') return node;
	const out = {};
	for (const [k, v] of Object.entries(node)) out[k] = without_callee_optional(v);
	if (node.type === 'CallExpression' && node.typeArguments && node.optional && node.callee?.type === 'MemberExpression') delete out.callee.optional;
	return out;
}

const lines = await teasel(jobs);
process.exit(compare(jobs, (job) => reference(job.source), lines, { verbose, label, known: plugin_bug }) ? 0 : 1);
