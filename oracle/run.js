// Differential test: acorn versus teasel over every script in a Svelte checkout.
//
//   SVELTE_DIR=~/Projects/svelte bun run.js [--verbose] [--limit N] [filter]

import * as acorn from 'acorn';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const args = process.argv.slice(2);
const verbose = args.includes('--verbose');
const limit = args.includes('--limit') ? Number(args[args.indexOf('--limit') + 1]) : Infinity;
const filter = args.find((a) => !a.startsWith('--') && a !== String(limit));

const root = process.env.SVELTE_DIR ?? join(process.env.HOME, 'Projects/svelte');
const corpus = join(root, 'packages/svelte/tests');
const binary = new URL('../target/release/teasel', import.meta.url).pathname;

function* files(dir) {
	for (const name of readdirSync(dir)) {
		const path = join(dir, name);
		if (statSync(path).isDirectory()) yield* files(path);
		else if (/\.(svelte|js)$/.test(name)) yield path;
	}
}

const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;

const jobs = [];
for (const path of files(corpus)) {
	const name = relative(corpus, path);
	if (filter && !name.includes(filter)) continue;
	const text = readFileSync(path, 'utf8');
	if (path.endsWith('.js')) {
		jobs.push({ name, source: text, mode: 'module' });
		if (!/^\s*(import|export)\b/m.test(text)) jobs.push({ name: `${name} (script)`, source: text, mode: 'script' });
	} else {
		for (const match of text.matchAll(script_re)) {
			if (/lang=["']?ts/.test(match[1] ?? '')) continue;
			jobs.push({ name: `${name}#${match.index}`, source: match[2], mode: 'module' });
		}
	}
	if (jobs.length >= limit) {
		jobs.length = limit;
		break;
	}
}

function normalize(key, value) {
	if (typeof value === 'bigint') return null;
	if (value instanceof RegExp) return null;
	return value;
}

function acorn_parse(source, mode) {
	try {
		const ast = acorn.parse(source, { ecmaVersion: 16, sourceType: mode, locations: true });
		return JSON.parse(JSON.stringify(ast, normalize));
	} catch (e) {
		if (!(e instanceof SyntaxError) || e.pos === undefined) return { error: { message: `acorn threw ${e.name}: ${e.message}`, pos: -1 } };
		return { error: { message: e.message.replace(/ \(\d+:\d+\)$/, ''), pos: e.pos, loc: { line: e.loc.line, column: e.loc.column } } };
	}
}

function diff(a, b, path = '') {
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

const proc = Bun.spawn([binary, '--batch'], { stdin: 'pipe', stdout: 'pipe', stderr: 'inherit' });
let input = '';
for (const job of jobs) {
	const bytes = Buffer.byteLength(job.source, 'utf8');
	input += `${job.mode} ${bytes}\n${job.source}`;
}
proc.stdin.write(input);
proc.stdin.end();
const output = await new Response(proc.stdout).text();
const lines = output.split('\n');

const stats = { identical: 0, mismatch: 0, both_error: 0, error_differs: 0, only_acorn_error: 0, only_teasel_error: 0 };
const details = [];
for (const [i, job] of jobs.entries()) {
	const expected = acorn_parse(job.source, job.mode);
	let actual;
	try {
		actual = JSON.parse(lines[i]);
	} catch {
		actual = { error: { message: `bad output: ${lines[i]?.slice(0, 80)}` } };
	}
	if (expected.error && actual.error) {
		if (expected.error.message === actual.error.message && expected.error.pos === actual.error.pos) stats.both_error++;
		else {
			stats.error_differs++;
			details.push(`${job.name}: acorn "${expected.error.message}" @${expected.error.pos}, teasel "${actual.error.message}" @${actual.error.pos}`);
		}
	} else if (expected.error) {
		stats.only_acorn_error++;
		details.push(`${job.name}: acorn error "${expected.error.message}" @${expected.error.pos}, teasel parsed`);
	} else if (actual.error) {
		stats.only_teasel_error++;
		details.push(`${job.name}: teasel error "${actual.error.message}" @${actual.error.pos}, acorn parsed`);
	} else {
		const d = diff(expected, actual);
		if (d) {
			stats.mismatch++;
			details.push(`${job.name}: ${d}`);
		} else stats.identical++;
	}
}

const total = jobs.length;
console.log(`${total} jobs from ${corpus} (acorn ${acorn.version})`);
for (const [k, v] of Object.entries(stats)) console.log(`  ${k.padEnd(18)} ${v}`);
const agree = stats.identical + stats.both_error;
console.log(`  agreement          ${((100 * agree) / total).toFixed(2)}%`);
const shown = verbose ? details : details.slice(0, 25);
for (const line of shown) console.log(line);
if (!verbose && details.length > shown.length) console.log(`... ${details.length - shown.length} more (--verbose)`);
process.exit(total > 0 && agree === total ? 0 : 1);
