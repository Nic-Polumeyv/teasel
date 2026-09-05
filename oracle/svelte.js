// Differential test for the entry points the Svelte compiler needs beyond parseExpressionAt:
// each-block contexts (parse_pattern_at), snippet parameters (parse_params_at) and @const
// declarations (parse_statement_at). The expected side is what Svelte's own parser produced,
// since Svelte builds those nodes with acorn through string wrappers.
//
//   SVELTE_DIR=~/Projects/svelte bun svelte.js [--verbose] [--limit N] [filter]

import { readFileSync } from 'node:fs';
import { relative } from 'node:path';
import { acorn_statement, args, compare, corpus, files, root, teasel } from './lib.js';

const { parse } = await import(`${root}/packages/svelte/src/compiler/index.js`);
const { verbose, limit, filter } = args();

// Svelte adds `character` to `loc` positions and attaches comments; neither is acorn's output.
// Its pattern wrapper `(pattern = 1)` also shifts every column inside a destructuring pattern by
// one, a known Svelte bug, so `loc` is dropped for patterns and only `start`/`end` are compared.
function strip(node, drop_loc) {
	if (Array.isArray(node)) return node.map((n) => strip(n, drop_loc));
	if (!node || typeof node !== 'object') return node;
	const out = {};
	for (const [k, v] of Object.entries(node)) {
		if (k === 'leadingComments' || k === 'trailingComments' || k === 'metadata' || k === 'typeAnnotation' || k === 'character') continue;
		if (k === 'loc' && drop_loc) continue;
		out[k] = strip(v, drop_loc);
	}
	return out;
}

// Svelte rebuilds some nodes itself (the declarator of an @const tag) without a `loc`, so the
// actual side drops `loc` wherever the expected node has none.
function align(actual, expected) {
	if (Array.isArray(actual) && Array.isArray(expected)) return actual.map((a, i) => align(a, expected[i]));
	if (!actual || typeof actual !== 'object' || !expected || typeof expected !== 'object') return actual;
	const out = {};
	for (const [k, v] of Object.entries(actual)) {
		if (k === 'loc' && !('loc' in expected)) continue;
		out[k] = align(v, expected[k]);
	}
	return out;
}

function strip_actual(json, job) {
	const actual = JSON.parse(json);
	if (actual.error) return actual;
	return align(job.drop_loc ? strip(actual, true) : actual, job.expected);
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
	if (/<script[^>]*lang=["']?ts/.test(source)) {
		skipped_files++;
		continue;
	}
	let ast;
	try {
		ast = parse(source, { modern: true });
	} catch {
		skipped_files++;
		continue;
	}
	const byte = (utf16) => Buffer.byteLength(source.slice(0, utf16), 'utf8');
	for (const node of walk(ast.fragment)) {
		if (node.type === 'EachBlock' && node.context) {
			const drop_loc = node.context.type !== 'Identifier';
			jobs.push({ name: `${name}@${node.context.start} context`, source, mode: `pattern:${byte(node.context.start)}`, expected: strip(node.context, drop_loc), drop_loc });
		} else if (node.type === 'SnippetBlock' && node.parameters.length) {
			const open = source.indexOf('(', node.expression.end);
			jobs.push({ name: `${name}@${open} params`, source, mode: `params:${byte(open)}`, expected: strip(node.parameters) });
		} else if (node.type === 'ConstTag') {
			// Svelte post-processes the declaration node, so the expected side is acorn driven the way
			// Svelte drives it, from the `const` or `let` keyword after the `@`.
			const keyword = /(?:const|let)\b/g;
			keyword.lastIndex = node.start;
			const offset = keyword.exec(source)?.index ?? node.declaration.start;
			jobs.push({ name: `${name}@${offset} const`, source, mode: `stmt:${byte(offset)}`, expected: acorn_statement(source, offset) });
		}
	}
	if (jobs.length >= limit) {
		jobs.length = limit;
		break;
	}
}

const lines = (await teasel(jobs)).map((line, i) => (jobs[i] ? JSON.stringify(strip_actual(line, jobs[i])) : line));
process.exit(compare(jobs, (job) => job.expected, lines, { verbose, label: 'svelte entry points', skipped: skipped_files }) ? 0 : 1);
