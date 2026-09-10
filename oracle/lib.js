import * as acorn from 'acorn';
import { tsPlugin } from '@sveltejs/acorn-typescript';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const TSParser = acorn.Parser.extend(tsPlugin());

export const root = process.env.SVELTE_DIR ?? join(process.env.HOME, 'Projects/svelte');
export const corpus = join(root, 'packages/svelte/tests');
const binary = new URL('../target/release/teasel', import.meta.url).pathname;

/// The files under `dir` whose names match, outside `node_modules` unless `into_modules`.
export function* files(dir, pattern, into_modules = false) {
	for (const name of readdirSync(dir)) {
		if ((name === 'node_modules' && !into_modules) || name === '.svelte-kit' || name.startsWith('.')) continue;
		const path = join(dir, name);
		let stat;
		try {
			stat = statSync(path);
		} catch {
			continue;
		}
		if (stat.isDirectory()) yield* files(path, pattern, into_modules);
		else if (pattern.test(name)) yield path;
	}
}

const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;

/// The `<script>` blocks of a component that are TypeScript, or those that are not.
export function* scripts(text, ts) {
	for (const match of text.matchAll(script_re)) {
		if (/lang=["']?ts/.test(match[1] ?? '') === ts) yield { index: match.index, source: match[2] };
	}
}

/// Every component of the corpus Svelte's own parser accepts, one at a time, with its tree and a
/// UTF-16 to byte offset map; `stats.skipped` counts the rest so far.
export async function components(filter) {
	const { parse } = await import(`${root}/packages/svelte/src/compiler/index.js`);
	const stats = { skipped: 0 };
	function* each() {
		for (const path of files(corpus, /\.svelte$/)) {
			const name = relative(corpus, path);
			if (filter && !name.includes(filter)) continue;
			const source = readFileSync(path, 'utf8');
			let ast;
			try {
				ast = parse(source, { modern: true });
			} catch {
				stats.skipped++;
				continue;
			}
			const byte = (utf16) => Buffer.byteLength(source.slice(0, utf16), 'utf8');
			yield { name, source, ast, ts: is_typescript(source), byte };
		}
	}
	return { each: each(), stats };
}

/// The TypeScript of the Svelte and SvelteKit checkouts: every `.ts` file and `lang="ts"` script.
export function ts_jobs(mode, filter) {
	const kit = process.env.KIT_DIR ?? join(process.env.HOME, 'Projects/kit');
	const jobs = [];
	for (const dir of [join(root, 'packages'), join(kit, 'packages')]) {
		for (const path of files(dir, /\.(ts|svelte)$/)) {
			const name = relative(dir, path);
			if (filter && !name.includes(filter)) continue;
			const text = readFileSync(path, 'utf8');
			if (path.endsWith('.ts')) jobs.push({ name, source: text, mode });
			else for (const { index, source } of scripts(text, true)) jobs.push({ name: `${name}#${index}`, source, mode });
		}
	}
	return { jobs, label: `${root} + ${kit}` };
}

/// Cuts a job list at `--limit`; whether it is full.
export function capped(jobs, limit) {
	if (jobs.length < limit) return false;
	jobs.length = limit;
	return true;
}

/// Every node of a Svelte tree, in document order.
export function* walk(node, skip = ['loc', 'metadata']) {
	if (Array.isArray(node)) {
		for (const item of node) yield* walk(item, skip);
		return;
	}
	if (!node || typeof node !== 'object') return;
	yield node;
	for (const [k, v] of Object.entries(node)) {
		if (!skip.includes(k)) yield* walk(v, skip);
	}
}

// the plugin rejects every ambient initializer and misses declarations that satisfy an export
export function plugin_rejects_valid(message) {
	return message.startsWith("A 'const' initializer in an ambient context") || /^Export '.*' is not defined$/.test(message);
}

function normalize(key, value) {
	if (typeof value === 'bigint') return null;
	if (value instanceof RegExp) return null;
	return value;
}

// acorn's `preserveParens` wraps a node; teasel marks it with `parenthesized: true` instead
function unwrap(value) {
	if (!value || value.type !== 'ParenthesizedExpression') return value;
	while (value.type === 'ParenthesizedExpression') value = value.expression;
	return { ...value, parenthesized: true };
}
const parenthesized = (key, value) => unwrap(normalize(key, value));
const parenthesized_ts = (key, value) => unwrap(normalize_ts(key, value));

// acorn-typescript bundles older acorn code paths and leaves a few keys out that acorn sets;
// teasel follows acorn there, so the expected side is brought in line before comparing.
export function normalize_ts(key, value) {
	if (typeof value === 'bigint') return null;
	if (value instanceof RegExp) return null;
	if (!value || typeof value !== 'object' || Array.isArray(value)) return value;
	let v = value;
	if (v.type === 'ImportExpression') {
		const { arguments: args, ...rest } = v;
		v = { ...rest, options: args?.[0] ?? null };
	}
	if (/^(Import|ExportNamed|ExportAll)Declaration$/.test(v.type) && !('attributes' in v)) v = { ...v, attributes: [] };
	if (v.type === 'CallExpression' && !('optional' in v)) v = { ...v, optional: false };
	if (v.type === 'ClassExpression' && !('id' in v)) v = { ...v, id: null };
	if (v.type === 'MemberExpression' && !('optional' in v)) v = { ...v, optional: false };
	// `a?.<T>()` marks the callee itself optional in the plugin; on a member callee the tree alone
	// cannot say whether the plugin or the source put it there, so `typescript.js` compares those
	// without it.
	if (v.type === 'CallExpression' && v.typeArguments && v.optional && v.callee?.type === 'Identifier' && 'optional' in v.callee) {
		const { optional, ...callee } = v.callee;
		v = { ...v, callee };
	}
	// `export declare` is a value export; the plugin marks every `export declare` type-only.
	if (v.type === 'ExportNamedDeclaration' && v.exportKind === 'type' && v.declaration && !/^TS(Interface|TypeAlias)Declaration$/.test(v.declaration.type)) {
		v = { ...v, exportKind: 'value' };
	}
	if ('extra' in v) {
		const { extra, ...rest } = v;
		v = rest;
	}
	return v;
}

/// acorn reports an unexpected token at the end of the input, where teasel names the end; the rewrite is keyed on the offset alone.
export function acorn_error(e, source, eof = true) {
	if (!(e instanceof SyntaxError) || e.pos === undefined) return { error: { message: `acorn threw ${e.name}: ${e.message}`, pos: -1 } };
	let message = e.message.replace(/ \(\d+:\d+\)$/, '');
	if (eof && e.pos === source.length && message === 'Unexpected token') message = 'Unexpected end of input';
	return { error: { message, pos: e.pos, loc: { line: e.loc.line, column: e.loc.column } } };
}

export function acorn_parse(source, mode) {
	try {
		const ast = acorn.parse(source, { ecmaVersion: 16, sourceType: mode, locations: true });
		return JSON.parse(JSON.stringify(ast, normalize));
	} catch (e) {
		return acorn_error(e, source);
	}
}

export function acorn_expression(source, offset, ts = false) {
	try {
		const parser = ts ? TSParser : acorn.Parser;
		const ast = parser.parseExpressionAt(source, offset, { ecmaVersion: 16, sourceType: 'module', locations: true, preserveParens: true });
		return JSON.parse(JSON.stringify(ast, ts ? parenthesized_ts : parenthesized));
	} catch (e) {
		return acorn_error(e, source);
	}
}

/// The way Svelte drove acorn for a statement: a parser started at `offset`, one statement.
export function acorn_statement(source, offset, ts = false) {
	try {
		const parser = new (ts ? TSParser : acorn.Parser)({ ecmaVersion: 16, sourceType: 'module', locations: true }, source, offset);
		parser.nextToken();
		const node = parser.parseStatement(null, true, Object.create(null));
		return JSON.parse(JSON.stringify(node, ts ? normalize_ts : normalize));
	} catch (e) {
		return acorn_error(e, source);
	}
}

export function is_typescript(source) {
	return /<script[^>]*lang=["']?ts/.test(source);
}

export function diff(a, b, path = '') {
	if (a === b) return null;
	if (typeof a !== typeof b || a === null || b === null || typeof a !== 'object') return `${path || '.'}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`;
	if (Array.isArray(a) !== Array.isArray(b)) return `${path}: array mismatch`;
	const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
	for (const key of keys) {
		const d = diff(a[key], b[key], `${path}.${key}`);
		if (d) return d;
	}
	return null;
}

/// Runs every job through one teasel process and returns its output lines, one JSON document
/// per job, unparsed. A job is { source, mode } where mode is a batch header of `src/bin/teasel.rs`.
export async function teasel(jobs, extra = []) {
	const proc = Bun.spawn([binary, '--batch', ...extra], { stdin: 'pipe', stdout: 'pipe', stderr: 'inherit' });
	let input = '';
	for (const job of jobs) input += `${job.mode} ${Buffer.byteLength(job.source, 'utf8')}\n${job.source}`;
	proc.stdin.write(input);
	proc.stdin.end();
	const output = await new Response(proc.stdout).text();
	return output.split('\n').slice(0, jobs.length);
}

function parse_line(line) {
	try {
		const value = JSON.parse(line);
		// acorn's errors have no code or end
		if (value.error) return { error: { message: value.error.message, pos: value.error.pos, loc: value.error.loc } };
		// an answer wraps the node with the offset the parse stopped at; a harness may hand over what it made of one instead
		return 'node' in value && !('type' in value) ? value.node : value;
	} catch {
		return { error: { message: `bad output: ${String(line).slice(0, 80)}` } };
	}
}

/// `expected` is a function of the job so acorn's ASTs are built one at a time; `lines` are
/// teasel's raw output lines.
/// `known(expected, actual)` names an oracle bug: such a pair counts as agreement and is
/// reported on its own line.
export function compare(jobs, expected, lines, { verbose, label = corpus, skipped = 0, known = () => false }) {
	const stats = { identical: 0, mismatch: 0, both_error: 0, error_differs: 0, only_acorn_error: 0, only_teasel_error: 0, oracle_bug: 0 };
	const details = [];
	for (const [i, job] of jobs.entries()) {
		const e = expected(job);
		const a = parse_line(lines[i]);
		if (known(e, a)) stats.oracle_bug++;
		else if (e.error && a.error) {
			if (!diff(e.error, a.error)) stats.both_error++;
			else {
				stats.error_differs++;
				details.push(`${job.name}: acorn "${e.error.message}" @${e.error.pos}, teasel "${a.error.message}" @${a.error.pos}`);
			}
		} else if (e.error) {
			stats.only_acorn_error++;
			details.push(`${job.name}: acorn error "${e.error.message}" @${e.error.pos}, teasel parsed`);
		} else if (a.error) {
			stats.only_teasel_error++;
			details.push(`${job.name}: teasel error "${a.error.message}" @${a.error.pos}, acorn parsed`);
		} else {
			const d = diff(e, a);
			if (d) {
				stats.mismatch++;
				details.push(`${job.name}: ${d}`);
			} else stats.identical++;
		}
	}
	const total = jobs.length;
	console.log(`${total} jobs from ${label} (acorn ${acorn.version})${skipped ? `, ${skipped} files skipped` : ''}`);
	for (const [k, v] of Object.entries(stats)) console.log(`  ${k.padEnd(18)} ${v}`);
	const agree = stats.identical + stats.both_error + stats.oracle_bug;
	console.log(`  agreement          ${((100 * agree) / total).toFixed(2)}%`);
	const shown = verbose ? details : details.slice(0, 25);
	for (const line of shown) console.log(line);
	if (!verbose && details.length > shown.length) console.log(`... ${details.length - shown.length} more (--verbose)`);
	return total > 0 && agree === total;
}

export function args() {
	const argv = process.argv.slice(2);
	const verbose = argv.includes('--verbose');
	const limit = argv.includes('--limit') ? Number(argv[argv.indexOf('--limit') + 1]) : Infinity;
	const filter = argv.find((a) => !a.startsWith('--') && a !== String(limit));
	return { verbose, limit, filter };
}
