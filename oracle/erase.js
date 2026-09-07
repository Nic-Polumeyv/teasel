// Differential test for TypeScript erasure: what Svelte's `remove_typescript_nodes` makes of
// acorn-typescript's tree versus teasel's erased output, over the TypeScript in a Svelte checkout,
// a SvelteKit checkout and the `lang="ts"` scripts of the Svelte test fixtures.
//
//   SVELTE_DIR=~/Projects/svelte KIT_DIR=~/Projects/kit bun erase.js [--verbose] [--limit N] [filter]

import * as acorn from 'acorn';
import { tsPlugin } from '@sveltejs/acorn-typescript';
import { existsSync, unlinkSync, writeFileSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { acorn_error, args, compare, normalize_ts, plugin_rejects_valid, root, teasel, ts_jobs } from './lib.js';

// the file lives on Svelte's main; a checkout on the teasel branch dropped it, so it is taken
// from git and placed beside its imports for the duration of the run
const stripper = `${root}/packages/svelte/src/compiler/phases/1-parse/remove_typescript_nodes.js`;
const temporary = existsSync(stripper) ? null : stripper.replace(/\.js$/, '.oracle.js');
if (temporary) writeFileSync(temporary, execSync('git show main:packages/svelte/src/compiler/phases/1-parse/remove_typescript_nodes.js', { cwd: root }));
const { remove_typescript_nodes } = await import(temporary ?? stripper);
if (temporary) unlinkSync(temporary);
// Svelte's errors locate themselves in the current source
const { set_source } = await import(`${root}/packages/svelte/src/compiler/state.js`);
const Parser = acorn.Parser.extend(tsPlugin());
const { verbose, limit, filter } = args();

// Svelte rejects what erasure cannot express; teasel lists it instead.
const REJECTED = new Set(['TSEnumDeclaration', 'TSModuleDeclaration', 'TSParameterProperty', 'Decorator']);
const rejected = { error: { message: 'typescript_invalid_feature', pos: -1 } };

function reference(source) {
	let ast;
	try {
		ast = Parser.parse(source, { ecmaVersion: 16, sourceType: 'module', locations: true });
	} catch (e) {
		return acorn_error(e, source, false);
	}
	try {
		set_source(source);
		// Svelte rejects `declare global` and `declare module` for the values in them, which are ambient
		ast.body = ast.body.filter((n) => !(n.type === 'TSModuleDeclaration' && n.declare));
		return normalize(remove_typescript_nodes(JSON.parse(JSON.stringify(ast, normalize_ts))));
	} catch (e) {
		if (e.code === 'typescript_invalid_feature') return rejected;
		throw e;
	}
}

// Svelte deletes the type keys it visits, but skips the children of a default export and leaves a
// class index signature, an abstract property, an overload signature and a method's `this`
// parameter in place.
const TYPE_KEYS = new Set(['typeAnnotation', 'typeParameters', 'typeArguments', 'returnType', 'accessibility', 'readonly', 'definite', 'override', 'abstract', 'implements', 'superTypeParameters', 'superTypeArguments', 'declare', 'importKind', 'exportKind', 'typescript']);
const dropped = (n) => n && (n.type === 'EmptyStatement' || n.type === 'TSIndexSignature' || (n.type === 'PropertyDefinition' && n.abstract) || (n.type === 'MethodDefinition' && n.value?.type === 'TSDeclareMethod') || (n.type === 'ExportNamedDeclaration' && !n.declaration && !n.source && n.specifiers?.length === 0) || (n.type === 'ExportDefaultDeclaration' && ['TSDeclareFunction', 'TSInterfaceDeclaration'].includes(n.declaration?.type)));

// Both sides: no EmptyStatement placeholders, no type keys, no `export {}`, which Svelte drops.
function normalize(node) {
	if (Array.isArray(node)) return node.filter((n) => !dropped(n)).map(normalize);
	if (!node || typeof node !== 'object') return node;
	const out = {};
	for (const [k, v] of Object.entries(node)) {
		if (TYPE_KEYS.has(k) || (k === 'optional' && node.type !== 'MemberExpression' && node.type !== 'CallExpression')) continue;
		out[k] = normalize(v);
	}
	if (Array.isArray(out.params) && out.params[0]?.type === 'Identifier' && out.params[0].name === 'this') out.params.shift();
	return out;
}

// acorn-typescript rejects every ambient initializer and misses declarations that satisfy an
// export; Svelte rejects `accessor` fields, which erasure keeps for it to judge.
function known(expected, actual) {
	if (!expected.error || actual.error) return false;
	const message = expected.error.message;
	if (plugin_rejects_valid(message)) return true;
	return message === 'typescript_invalid_feature' && JSON.stringify(actual).includes('"accessor":true');
}

const { jobs } = ts_jobs('ts-module+erase', filter);
if (jobs.length > limit) jobs.length = limit;

const lines = (await teasel(jobs)).map((line) => {
	const answer = JSON.parse(line);
	if (answer.error) return line;
	if (answer.typescript.some((k) => REJECTED.has(k.type))) return JSON.stringify(rejected);
	return JSON.stringify(normalize(answer));
});
process.exit(compare(jobs, (job) => reference(job.source), lines, { verbose, label: 'typescript erasure', known }) ? 0 : 1);
