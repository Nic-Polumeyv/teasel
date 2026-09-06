// Differential test for TypeScript erasure: what Svelte's `remove_typescript_nodes` makes of
// acorn-typescript's tree versus teasel's erased output, over the TypeScript in a Svelte checkout,
// a SvelteKit checkout and the `lang="ts"` scripts of the Svelte test fixtures.
//
//   SVELTE_DIR=~/Projects/svelte KIT_DIR=~/Projects/kit bun erase.js [--verbose] [--limit N] [filter]

import * as acorn from 'acorn';
import { tsPlugin } from '@sveltejs/acorn-typescript';
import { existsSync, readFileSync, readdirSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { join, relative } from 'node:path';
import { args, compare, normalize_ts, root, teasel } from './lib.js';

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
const kit = process.env.KIT_DIR ?? join(process.env.HOME, 'Projects/kit');
const { verbose, limit, filter } = args();
const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;

function* sources(dir) {
	for (const name of readdirSync(dir)) {
		if (name === 'node_modules' || name === '.svelte-kit' || name.startsWith('.')) continue;
		const path = join(dir, name);
		let stat;
		try {
			stat = statSync(path);
		} catch {
			continue;
		}
		if (stat.isDirectory()) yield* sources(path);
		else if (/\.(ts|svelte)$/.test(name)) yield path;
	}
}

// Svelte rejects what erasure cannot express; teasel lists it instead.
const REJECTED = new Set(['TSEnumDeclaration', 'TSModuleDeclaration', 'TSParameterProperty', 'Decorator']);
const rejected = { error: { message: 'typescript_invalid_feature', pos: -1 } };

function reference(source) {
	let ast;
	try {
		ast = Parser.parse(source, { ecmaVersion: 16, sourceType: 'module', locations: true });
	} catch (e) {
		return { error: { message: e.message.replace(/ \(\d+:\d+\)$/, ''), pos: e.pos, loc: { line: e.loc.line, column: e.loc.column } } };
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
	if (message.startsWith("A 'const' initializer in an ambient context") || /^Export '.*' is not defined$/.test(message)) return true;
	return message === 'typescript_invalid_feature' && JSON.stringify(actual).includes('"accessor":true');
}

const jobs = [];
for (const dir of [join(root, 'packages'), join(kit, 'packages')]) {
	for (const path of sources(dir)) {
		const name = relative(dir, path);
		if (filter && !name.includes(filter)) continue;
		const text = readFileSync(path, 'utf8');
		const push = (name, source) => jobs.push({ name, source, mode: 'ts-module+erase' });
		if (path.endsWith('.ts')) push(name, text);
		else for (const match of text.matchAll(script_re)) {
			if (!/lang=["']?ts/.test(match[1] ?? '')) continue;
			push(`${name}#${match.index}`, match[2]);
		}
	}
}
if (jobs.length > limit) jobs.length = limit;

const lines = (await teasel(jobs)).map((line, i) => {
	if (!jobs[i]) return line;
	const answer = JSON.parse(line);
	if (answer.error) return line;
	if (answer.typescript.some((k) => REJECTED.has(k.type))) return JSON.stringify(rejected);
	return JSON.stringify(normalize(answer));
});
process.exit(compare(jobs, (job) => reference(job.source), lines, { verbose, label: 'typescript erasure', known }) ? 0 : 1);
