// Differential test for the entry points the Svelte compiler needs beyond parseExpressionAt:
// each-block contexts and await-block patterns (parse_pattern_at), snippet parameters
// (parse_params_at) and declaration tags (parse_statement_at). The expected side is what
// Svelte's own parser produced, since Svelte builds those nodes with acorn through string
// wrappers, plus a table of invalid inputs to check the errors those wrappers surface.
//
//   SVELTE_DIR=~/Projects/svelte bun svelte.js [--verbose] [--limit N] [filter]

import { readFileSync } from 'node:fs';
import { relative } from 'node:path';
import { acorn_statement, args, compare, corpus, files, is_typescript, root, teasel } from './lib.js';

const { parse } = await import(`${root}/packages/svelte/src/compiler/index.js`);
const { verbose, limit, filter } = args();

// Svelte adds `character` to `loc` positions and attaches comments; neither is acorn's output.
function strip(node) {
	if (Array.isArray(node)) return node.map(strip);
	if (node instanceof RegExp) return null;
	if (!node || typeof node !== 'object') return node;
	// Svelte builds an each-block context's annotation by hand and leaves the identifier's end
	// before it; teasel extends the end over the annotation, as acorn-typescript does elsewhere.
	if (node.type === 'Identifier' && node.typeAnnotation && node.end < node.typeAnnotation.end) node = { ...node, end: node.typeAnnotation.end };
	// Svelte on acorn kept the parens of a snippet parameter's default; nothing else keeps them
	if (node.type === 'ParenthesizedExpression') return strip(node.expression);
	const out = {};
	for (const [k, v] of Object.entries(node)) {
		if (k === 'leadingComments' || k === 'trailingComments' || k === 'metadata' || k === 'character') continue;
		out[k] = strip(v);
	}
	return out;
}

// Svelte's pattern wrapper `(pattern = 1)` drops a character before the pattern, so the `loc`
// columns on its first line are off by one when the pattern is not on line 1; `start` and `end`
// are right. Every `loc` on the expected side is recomputed from them.
function locate(source) {
	const starts = [0];
	for (let i = 0; i < source.length; i++) {
		const c = source[i];
		if (c === '\n' || c === ' ' || c === ' ' || (c === '\r' && source[i + 1] !== '\n')) starts.push(i + 1);
	}
	return (offset) => {
		let line = starts.findIndex((s) => s > offset);
		if (line === -1) line = starts.length;
		return { line, column: offset - starts[line - 1] };
	};
}

function fix_loc(node, position) {
	if (Array.isArray(node)) return node.map((n) => fix_loc(n, position));
	if (!node || typeof node !== 'object') return node;
	const out = {};
	for (const [k, v] of Object.entries(node)) out[k] = k === 'loc' ? { start: position(node.start), end: position(node.end) } : fix_loc(v, position);
	// The type annotation of a context is a node Svelte builds by hand, without a `loc`.
	if (typeof node.type === 'string' && !('loc' in node)) out.loc = { start: position(node.start), end: position(node.end) };
	return out;
}

function* walk(node) {
	if (Array.isArray(node)) {
		for (const item of node) yield* walk(item);
		return;
	}
	if (!node || typeof node !== 'object') return;
	yield node;
	for (const [k, v] of Object.entries(node)) {
		if (k === 'loc' || k === 'metadata') continue;
		yield* walk(v);
	}
}

const jobs = [];
let skipped_files = 0;
for (const path of files(corpus, /\.svelte$/)) {
	const name = relative(corpus, path);
	if (filter && !name.includes(filter)) continue;
	const source = readFileSync(path, 'utf8');
	const ts = is_typescript(source);
	const prefix = ts ? 'ts-' : '';
	let ast;
	try {
		ast = parse(source, { modern: true });
	} catch {
		skipped_files++;
		continue;
	}
	const byte = (utf16) => Buffer.byteLength(source.slice(0, utf16), 'utf8');
	const position = locate(source);
	const pattern = (node) => jobs.push({ name: `${name}@${node.start} pattern`, source, mode: `${prefix}pattern:${byte(node.start)}`, expected: fix_loc(strip(node), position) });
	for (const node of walk(ast.fragment)) {
		if (node.type === 'EachBlock' && node.context) pattern(node.context);
		if (node.type === 'AwaitBlock') {
			if (node.value) pattern(node.value);
			if (node.error) pattern(node.error);
		}
		if (node.type === 'SnippetBlock' && node.parameters.length) {
			const open = source.indexOf('(', node.expression.end);
			jobs.push({ name: `${name}@${open} params`, source, mode: `${prefix}params:${byte(open)}`, expected: strip(node.parameters) });
		}
		// Svelte hands the declaration of `{const x = 1}` back unchanged, so this is its own node.
		if (node.type === 'DeclarationTag') {
			const offset = node.declaration.start;
			jobs.push({ name: `${name}@${offset} declaration`, source, mode: `${prefix}stmt:${byte(offset)}`, expected: strip(node.declaration) });
		}
		// Svelte rebuilds the declaration of `{@const x = 1}`, so the expected side is acorn driven
		// the way Svelte drives it, from the `const` or `let` keyword after the `@`.
		if (node.type === 'ConstTag') {
			const keyword = /(?:const|let)\b/g;
			keyword.lastIndex = node.start;
			const offset = keyword.exec(source)?.index ?? node.declaration.start;
			jobs.push({ name: `${name}@${offset} const`, source, mode: `${prefix}stmt:${byte(offset)}`, expected: acorn_statement(source, offset, ts) });
		}
	}
	if (jobs.length >= limit) {
		jobs.length = limit;
		break;
	}
}

// Invalid inputs, compared against the error Svelte reports (which is acorn's, through the
// wrapper). Only inputs Svelte's own reader hands to acorn belong here; `§` marks the offset.
const invalid = [
	['{#each x as §{eval}}{/each}', 'pattern'],
	['{#each x as §[...a, b]}{/each}', 'pattern'],
	['{#each x as §{a: (b = 1)}}{/each}', 'pattern'],
	['{#each x as §[a?.b]}{/each}', 'pattern'],
	['{#await x then §[a = yield]}{/await}', 'pattern'],
	['{#snippet s§(a.b)}{/snippet}', 'params'],
	['{#snippet s§(a[0])}{/snippet}', 'params'],
	['{#snippet s§(1)}{/snippet}', 'params'],
	['{#snippet s§(this)}{/snippet}', 'params'],
	['{#snippet s§((a))}{/snippet}', 'params'],
	['{#snippet s§({a: (b)})}{/snippet}', 'params'],
	['{#snippet s§(a?.b)}{/snippet}', 'params'],
	['{#snippet s§(await)}{/snippet}', 'params'],
	['{#snippet s§(a = await x)}{/snippet}', 'params'],
	['{#snippet s§({[await x]: y})}{/snippet}', 'params'],
	['{#snippet s§(a, a)}{/snippet}', 'params'],
	['{#snippet s§(...a, b)}{/snippet}', 'params'],
	['{§const x = ;}', 'stmt'],
	['{§const let = 1}', 'stmt'],
	['{§let x = yield}', 'stmt'],
	['{§const x = await}', 'stmt'],
	['{§let [a, a] = x}', 'stmt'],
];
for (const [marked, mode] of invalid) {
	const offset = marked.indexOf('§');
	const source = marked.replace('§', '');
	let expected;
	try {
		parse(source, { modern: true });
		expected = { error: { message: 'Svelte parsed it' } };
	} catch (e) {
		if (e.code !== 'js_parse_error') throw new Error(`${source}: Svelte failed before acorn: ${e.message}`);
		expected = { error: { message: e.message.split('\n')[0], pos: e.position[0] } };
	}
	jobs.push({ name: source, source, mode: `${mode}:${offset}`, expected, error_only: true });
}

function actual(line, job) {
	const node = JSON.parse(line);
	if (node.error && job.error_only) return { error: { message: node.error.message, pos: node.error.pos } };
	if (job.mode.startsWith('params:') || job.mode.startsWith('ts-params:')) return node.error ? node : node.params;
	return node.node ?? node;
}

const lines = (await teasel(jobs)).map((line, i) => (jobs[i] ? JSON.stringify(actual(line, jobs[i])) : line));
process.exit(compare(jobs, (job) => job.expected, lines, { verbose, label: 'svelte entry points', skipped: skipped_files }) ? 0 : 1);
