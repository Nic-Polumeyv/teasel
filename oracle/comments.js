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

// Svelte's walk pushes a comment inside an empty block, array or object out to the next node;
// teasel keeps it as the container's inner comment. The expected side is relocated to match.
function empty_containers(node, out = []) {
	if (Array.isArray(node)) node.forEach((n) => empty_containers(n, out));
	else if (node && typeof node === 'object' && typeof node.type === 'string') {
		const list = node.type === 'BlockStatement' || node.type === 'Program' ? node.body : node.type === 'ArrayExpression' ? node.elements : node.type === 'ObjectExpression' ? node.properties : null;
		if (list && list.length === 0) out.push(node);
		for (const [k, v] of Object.entries(node)) if (k !== 'loc') empty_containers(v, out);
	}
	return out;
}

function relocate(tree) {
	const containers = empty_containers(tree);
	if (!containers.length) return tree;
	const inside = (c) => containers.find((n) => c.start >= n.start && c.end <= n.end);
	const moved = new Map();
	const collect = (node) => {
		if (Array.isArray(node)) node.forEach(collect);
		else if (node && typeof node === 'object') {
			for (const [k, v] of Object.entries(node)) {
				if (k === 'leadingComments' || k === 'trailingComments') {
					for (const c of v) {
						const container = inside(c);
						if (container && container !== node) (moved.get(container) ?? moved.set(container, []).get(container)).push(c);
					}
				} else if (k !== 'loc') collect(v);
			}
		}
	};
	collect(tree);
	const strip = (node) => {
		if (Array.isArray(node)) return node.map(strip);
		if (!node || typeof node !== 'object' || node instanceof RegExp) return node;
		const out = {};
		for (const [k, v] of Object.entries(node)) {
			if (k === 'leadingComments' || k === 'trailingComments') {
				const kept = v.filter((c) => !inside(c) || inside(c) === node);
				if (kept.length) out[k] = kept;
				continue;
			}
			out[k] = strip(v);
		}
		if (moved.has(node)) out.innerComments = moved.get(node).sort((a, b) => a.start - b.start);
		return out;
	};
	return strip(tree);
}

// Svelte keeps one comment list per file and filters it by offset, so a script inherits every
// comment that came before it in the file; those are dropped. The `Program` node keeps acorn's
// positions on the teasel side and the `<script>` tag's on Svelte's, so its own span is dropped
// too, on both sides.
function normalize(node, source, from, is_root, raw_values, ts) {
	if (Array.isArray(node)) return node.map((n) => normalize(n, source, from, false, raw_values, ts));
	if (node instanceof RegExp) return null;
	if (typeof node === 'bigint') return null;
	if (!node || typeof node !== 'object') return node;
	// Svelte removes the parentheses acorn preserved in template expressions, comments and all.
	if (raw_values && node.type === 'ParenthesizedExpression') return normalize(node.expression, source, from, is_root, raw_values, ts);
	const out = {};
	if (!raw_values && ts) node = normalize_ts(null, node);
	// An empty program's comments are its inner comments here; Svelte calls them trailing.
	if (!raw_values && node.type === 'Program' && node.body.length === 0 && node.trailingComments) {
		const { trailingComments, ...rest } = node;
		node = { ...rest, innerComments: trailingComments };
	}
	for (const [k, v] of Object.entries(node)) {
		if (k === 'metadata' || k === 'character' || (is_root && node.type === 'Program' && (k === 'start' || k === 'end' || k === 'loc'))) continue;
		if (k === 'leadingComments' || k === 'trailingComments' || k === 'innerComments') {
			// Svelte's walker visits the comments it just attached as if they were nodes, so the
			// comments inside an empty block end up as the trailing comments of its leading one;
			// teasel reports them as the block's `innerComments`.
			const inner = v.flatMap((c) => c.trailingComments ?? []);
			if (inner.length) out.innerComments = inner.filter((c) => c.start >= from);
			// both sides lose a block comment's indentation, whichever side kept the raw text
			const kept = v.filter((c) => c.start >= from).map(({ leadingComments, trailingComments, loc, ...c }) => dedent(source, c));
			if (kept.length) out[k] = kept;
			continue;
		}
		out[k] = normalize(v, source, from, false, raw_values, ts);
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
	const typescript = is_typescript(source);
	const ts = typescript ? 'ts-' : '';
	const byte = (utf16) => Buffer.byteLength(source.slice(0, utf16), 'utf8');
	for (const script of [ast.instance, ast.module]) {
		if (!script) continue;
		const program = script.content;
		const blank = source.slice(0, program.start).replace(/[^\n]/g, ' ') + source.slice(program.start, program.end);
		jobs.push({ name: `${name}@${program.start} script`, source: blank, mode: `${ts}module+comments+undeclared-exports`, expected: normalize(relocate(program), source, program.start, true, false, typescript), from: program.start, ts: typescript });
	}
	for (const node of walk(ast.fragment)) {
		if (node.type !== 'ExpressionTag' || !node.expression) continue;
		const expression = node.expression;
		jobs.push({ name: `${name}@${expression.start}`, source, mode: `${ts}expr+comments:${byte(expression.start)}`, expected: normalize(relocate(expression), source, expression.start, false, false, typescript), from: expression.start, ts: typescript });
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
	const parsed = JSON.parse(line);
	if (parsed.error) return line;
	const node = parsed.node ?? parsed;
	delete node.comments;
	return JSON.stringify(normalize(node, jobs[i].source, jobs[i].from, true, true, jobs[i].ts));
});
process.exit(compare(jobs, (job) => job.expected, lines, { verbose, label: 'comment attachment', skipped: skipped_files }) ? 0 : 1);
