import * as acorn from 'acorn';
import { tsPlugin } from '@sveltejs/acorn-typescript';
import { readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const TSParser = acorn.Parser.extend(tsPlugin());

export const root = process.env.SVELTE_DIR ?? join(process.env.HOME, 'Projects/svelte');
export const corpus = join(root, 'packages/svelte/tests');
const binary = new URL('../target/release/teasel', import.meta.url).pathname;

export function* files(dir, pattern) {
	for (const name of readdirSync(dir)) {
		const path = join(dir, name);
		if (statSync(path).isDirectory()) yield* files(path, pattern);
		else if (pattern.test(name)) yield path;
	}
}

function normalize(key, value) {
	if (typeof value === 'bigint') return null;
	if (value instanceof RegExp) return null;
	return value;
}

// acorn-typescript bundles older acorn code paths and leaves a few keys out that acorn sets;
// teasel follows acorn there, so the expected side is brought in line before comparing.
export function normalize_ts(key, value) {
	if (typeof value === 'bigint') return null;
	if (value instanceof RegExp) return null;
	if (value && typeof value === 'object' && !Array.isArray(value)) {
		if (value.type === 'ImportExpression') {
			const { arguments: args, ...rest } = value;
			return { ...rest, options: args?.[0] ?? null };
		}
		if (/^(Import|ExportNamed|ExportAll)Declaration$/.test(value.type) && !('attributes' in value)) return { ...value, attributes: [] };
		if (value.type === 'CallExpression' && !('optional' in value)) return { ...value, optional: false };
		if ('extra' in value) {
			const { extra, ...rest } = value;
			return rest;
		}
	}
	return value;
}

function acorn_error(e) {
	if (!(e instanceof SyntaxError) || e.pos === undefined) return { error: { message: `acorn threw ${e.name}: ${e.message}`, pos: -1 } };
	return { error: { message: e.message.replace(/ \(\d+:\d+\)$/, ''), pos: e.pos, loc: { line: e.loc.line, column: e.loc.column } } };
}

export function acorn_parse(source, mode) {
	try {
		const ast = acorn.parse(source, { ecmaVersion: 16, sourceType: mode, locations: true });
		return JSON.parse(JSON.stringify(ast, normalize));
	} catch (e) {
		return acorn_error(e);
	}
}

export function acorn_expression(source, offset, ts = false) {
	try {
		const parser = ts ? TSParser : acorn.Parser;
		const ast = parser.parseExpressionAt(source, offset, { ecmaVersion: 16, sourceType: 'module', locations: true, preserveParens: true });
		return JSON.parse(JSON.stringify(ast, ts ? normalize_ts : normalize));
	} catch (e) {
		return acorn_error(e);
	}
}

/// The way Svelte's parse_statement_at drives acorn: a parser started at `offset`, one statement.
export function acorn_statement(source, offset, ts = false) {
	try {
		const parser = new (ts ? TSParser : acorn.Parser)({ ecmaVersion: 16, sourceType: 'module', locations: true }, source, offset);
		parser.nextToken();
		const node = parser.parseStatement(null, true, Object.create(null));
		return JSON.parse(JSON.stringify(node, ts ? normalize_ts : normalize));
	} catch (e) {
		return acorn_error(e);
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
/// per job, unparsed. A job is { source, mode } where mode is 'module', 'script' or 'expr:BYTE_OFFSET'.
export async function teasel(jobs) {
	const proc = Bun.spawn([binary, '--batch'], { stdin: 'pipe', stdout: 'pipe', stderr: 'inherit' });
	let input = '';
	for (const job of jobs) input += `${job.mode} ${Buffer.byteLength(job.source, 'utf8')}\n${job.source}`;
	proc.stdin.write(input);
	proc.stdin.end();
	const output = await new Response(proc.stdout).text();
	return output.split('\n');
}

function parse_line(line) {
	try {
		return JSON.parse(line);
	} catch {
		return { error: { message: `bad output: ${line.slice(0, 80)}` } };
	}
}

/// `expected` is a function of the job so acorn's ASTs are built one at a time; `lines` are
/// teasel's raw output lines.
export function compare(jobs, expected, lines, { verbose, label = corpus, skipped = 0 }) {
	const stats = { identical: 0, mismatch: 0, both_error: 0, error_differs: 0, only_acorn_error: 0, only_teasel_error: 0 };
	const details = [];
	for (const [i, job] of jobs.entries()) {
		const e = expected(job);
		const a = parse_line(lines[i]);
		if (e.error && a.error) {
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
	const agree = stats.identical + stats.both_error;
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
