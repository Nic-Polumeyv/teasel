// Differential test: Svelte's own parser versus teasel reading the same component by the Svelte
// grammar, over every component of a Svelte checkout.
//
//   SVELTE_DIR=~/Projects/svelte bun host.js [--verbose] [--limit N] [filter]

import { readFileSync } from 'node:fs';
import { relative } from 'node:path';
import { args, capped, corpus, diff, files, is_typescript, normalize_ts, root, teasel } from './lib.js';

const { parse } = await import(`${root}/packages/svelte/src/compiler/index.js`);
const { verbose, limit, filter } = args();
const grammar = new URL('../hosts/svelte.grammar', import.meta.url).pathname;

// positions as line and column, Svelte's own metadata, and the CSS tree, which teasel does not build yet
const DROP = new Set(['loc', 'name_loc', 'metadata', 'character', 'leadingComments', 'trailingComments']);

// Svelte keeps acorn's paren wrappers in snippet parameters alone; `this="h1"` on a dynamic
// element is a literal Svelte quotes by hand
function normal(value, source) {
	if (Array.isArray(value)) return value.map((v) => normal(v, source));
	if (!value || typeof value !== 'object') return value;
	while (value.type === 'ParenthesizedExpression') value = value.expression;
	// Svelte builds an each-block context's annotation by hand and leaves the identifier's end before it
	if (value.type === 'Identifier' && value.typeAnnotation && value.end < value.typeAnnotation.end) value = { ...value, end: value.typeAnnotation.end };
	const out = {};
	for (const [k, v] of Object.entries(value)) if (!DROP.has(k)) out[k] = normal(v, source);
	if (out.type === 'SvelteElement' && out.tag?.type === 'Literal') delete out.tag.raw;
	// `{ @const x = 1 }`: Svelte starts the declaration two characters in, whatever is there
	if (out.type === 'ConstTag' && source) out.declaration.start = source.indexOf('const', out.start);
	return out;
}

// the deepest node whose span holds `pos`
function innermost(node, pos, found = null) {
	if (Array.isArray(node)) {
		for (const item of node) found = innermost(item, pos, found);
		return found;
	}
	if (!node || typeof node !== 'object') return found;
	if (typeof node.start === 'number' && typeof node.end === 'number') {
		if (pos < node.start || pos >= node.end) return found;
		found = node;
	}
	for (const [k, v] of Object.entries(node)) if (k !== 'comments') found = innermost(v, pos, found);
	return found;
}

// block comments read by acorn lose the indentation of their first line; one between attributes
// Svelte reads itself and keeps whole
function dedent(comment, source, tree) {
	if (comment.type !== 'Block' || !/\n/.test(comment.value)) return comment;
	const around = innermost(tree, comment.start);
	if (around && 'attributes' in around && 'fragment' in around) return comment;
	let a = comment.start;
	while (a > 0 && source[a - 1] !== '\n') a -= 1;
	let b = a;
	while (/[ \t]/.test(source[b])) b += 1;
	const indentation = source.slice(a, b);
	return { ...comment, value: comment.value.replace(new RegExp(`^${indentation}`, 'gm'), '') };
}

// what Svelte gets wrong on purpose: `this="h{n}"` takes the text alone, an old bug it keeps
function known(source, error) {
	return /<svelte:element[^>]*\sthis="[^"{]*\{/.test(source) && /an expression as this/.test(error.message);
}

function replacer(key, value) {
	if (typeof value === 'bigint' || value instanceof RegExp) return null;
	return value;
}

// `<svelte:options>` becomes `options` on Svelte's side; teasel leaves the element in the fragment
// for now. A TypeScript component's scripts come from acorn-typescript, brought in line with acorn.
function tree(ast, ours, ts, source) {
	const out = normal(JSON.parse(JSON.stringify(ast, ts && !ours ? normalize_ts : replacer)), ours ? null : source);
	out.css = out.css ? 'present' : null;
	if (ours) {
		out.comments = out.comments.map((c) => dedent(c, source, out));
		const i = out.fragment.nodes.findIndex((n) => n.type === 'SvelteOptions');
		if (i !== -1) {
			out.fragment.nodes.splice(i, 1);
			out.options = 'present';
		}
	} else out.options = out.options ? 'present' : null;
	return out;
}

function expected(source) {
	try {
		return tree(parse(source, { modern: true }), false, is_typescript(source), source);
	} catch (e) {
		return { error: { message: e.message, code: e.code, pos: e.position?.[0] } };
	}
}

const jobs = [];
for (const path of files(corpus, /\.svelte$/)) {
	const name = relative(corpus, path);
	if (filter && !name.includes(filter)) continue;
	jobs.push({ name, source: readFileSync(path, 'utf8'), mode: 'doc' });
	if (capped(jobs, limit)) break;
}

const lines = await teasel(jobs, ['--host', grammar]);
const stats = { identical: 0, mismatch: 0, both_error: 0, only_svelte_error: 0, only_teasel_error: 0, oracle_bug: 0 };
const details = [];
for (const [i, job] of jobs.entries()) {
	const e = expected(job.source);
	let a;
	try {
		const value = JSON.parse(lines[i]);
		a = value.error ? value : tree(value.node, true, false, job.source);
	} catch {
		a = { error: { message: `bad output: ${String(lines[i]).slice(0, 80)}` } };
	}
	if (e.error && a.error) stats.both_error++;
	else if (a.error && known(job.source, a.error)) stats.oracle_bug++;
	else if (e.error) {
		stats.only_svelte_error++;
		details.push(`${job.name}: svelte error "${e.error.message}" @${e.error.pos}, teasel parsed`);
	} else if (a.error) {
		stats.only_teasel_error++;
		details.push(`${job.name}: teasel error "${a.error.message}" @${a.error.pos}, svelte parsed`);
	} else {
		const d = diff(e, a);
		if (d) {
			stats.mismatch++;
			details.push(`${job.name}: ${d}`);
		} else stats.identical++;
	}
}
console.log(`${jobs.length} components from ${corpus}`);
for (const [k, v] of Object.entries(stats)) console.log(`  ${k.padEnd(18)} ${v}`);
const agree = stats.identical + stats.both_error + stats.oracle_bug;
console.log(`  agreement          ${((100 * agree) / jobs.length).toFixed(2)}%`);
const shown = verbose ? details : details.slice(0, 40);
for (const line of shown) console.log(line);
if (!verbose && details.length > shown.length) console.log(`... ${details.length - shown.length} more (--verbose)`);
process.exit(jobs.length > 0 && agree === jobs.length ? 0 : 1);
