// Differential test for comment attachment: the scripts and template expressions of every
// component in a Svelte checkout, with `leadingComments` and `trailingComments` the way Svelte's
// own parser attaches them after acorn.
//
//   SVELTE_DIR=~/Projects/svelte bun comments.js [--verbose] [--limit N] [filter]

import { readFileSync } from 'node:fs';
import { relative } from 'node:path';
import { args, compare, corpus, files, is_typescript, normalize_ts, root, teasel } from './lib.js';

const { parse } = await import(`${root}/packages/svelte/src/compiler/index.js`);
const { verbose, limit, filter } = args();

// Svelte strips a block comment's indentation from its value; teasel keeps the text as written.
function dedent(source, comment) {
	if (comment.type !== 'Block' || !/\n/.test(comment.value)) return comment;
	let a = comment.start;
	while (a > 0 && source[a - 1] !== '\n') a -= 1;
	let b = a;
	while (/[ \t]/.test(source[b])) b += 1;
	const indentation = source.slice(a, b);
	return { ...comment, value: comment.value.replace(new RegExp(`^${indentation}`, 'gm'), '') };
}

// Svelte keeps one comment list per file and filters it by offset, so a script inherits every
// comment that came before it in the file; those are dropped. The `Program` node keeps acorn's
// positions on the teasel side and the `<script>` tag's on Svelte's, so its own span is dropped
// too, on both sides.
function normalize(node, source, from, is_root, raw_values) {
	if (Array.isArray(node)) return node.map((n) => normalize(n, source, from, false, raw_values));
	if (node instanceof RegExp) return null;
	if (typeof node === 'bigint') return null;
	if (!node || typeof node !== 'object') return node;
	// Svelte removes the parentheses acorn preserved in template expressions, comments and all.
	if (raw_values && node.type === 'ParenthesizedExpression') return normalize(node.expression, source, from, is_root, raw_values);
	const out = {};
	if (!raw_values) node = normalize_ts(null, node);
	for (const [k, v] of Object.entries(node)) {
		if (k === 'metadata' || k === 'character' || (is_root && node.type === 'Program' && (k === 'start' || k === 'end' || k === 'loc'))) continue;
		if (k === 'leadingComments' || k === 'trailingComments') {
			const kept = v.filter((c) => c.start >= from).map((c) => (raw_values ? dedent(source, c) : c));
			if (kept.length) out[k] = kept;
			continue;
		}
		out[k] = normalize(v, source, from, false, raw_values);
	}
	return out;
}

const jobs = [];
let skipped_files = 0;
for (const path of files(corpus, /\.svelte$/)) {
	const name = relative(corpus, path);
	if (filter && !name.includes(filter)) continue;
	const source = readFileSync(path, 'utf8');
	let ast;
	try {
		ast = parse(source, { modern: true });
	} catch {
		skipped_files++;
		continue;
	}
	const ts = is_typescript(source) ? 'ts-' : '';
	const byte = (utf16) => Buffer.byteLength(source.slice(0, utf16), 'utf8');
	for (const script of [ast.instance, ast.module]) {
		if (!script) continue;
		const program = script.content;
		const blank = source.slice(0, program.start).replace(/[^\n]/g, ' ') + source.slice(program.start, program.end);
		jobs.push({ name: `${name}@${program.start} script`, source: blank, mode: `${ts}svelte+comments`, expected: normalize(program, source, program.start, true, false), from: program.start });
	}
	for (const node of walk(ast.fragment)) {
		if (node.type !== 'ExpressionTag' || !node.expression) continue;
		const expression = node.expression;
		jobs.push({ name: `${name}@${expression.start}`, source, mode: `${ts}expr+comments:${byte(expression.start)}`, expected: normalize(expression, source, expression.start, false, false), from: expression.start });
	}
	if (jobs.length >= limit) {
		jobs.length = limit;
		break;
	}
}

function* walk(node) {
	if (Array.isArray(node)) {
		for (const item of node) yield* walk(item);
		return;
	}
	if (!node || typeof node !== 'object') return;
	yield node;
	for (const [k, v] of Object.entries(node)) {
		if (k === 'loc' || k === 'metadata' || k === 'expression') continue;
		yield* walk(v);
	}
}

const lines = (await teasel(jobs)).map((line, i) => {
	if (!jobs[i]) return line;
	const node = JSON.parse(line);
	if (node.error) return line;
	return JSON.stringify(normalize(node, jobs[i].source, jobs[i].from, true, true));
});
process.exit(compare(jobs, (job) => job.expected, lines, { verbose, label: 'comment attachment', skipped: skipped_files }) ? 0 : 1);
