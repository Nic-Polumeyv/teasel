// node scripts/check.ts [--host GRAMMAR EXTENSION] DIR...: every script under the directories, parsed three
// ways and the answers diffed: the addon's decoded answer against the JSON the binary prints, and the wasm
// module's against the addon's; with a host, every file of its extension as a document too.
// `cargo build --release` first.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { load } from '../dist/engine/addon.js';
import { engine as native } from '../dist/engine/node.js';
import { compile } from '../dist/engine/wasm.js';
import { decode, LINKED, REFERENCE, SCOPE } from '../dist/decode.js';
import { CODES, type Code } from '../dist/types.js';
import type { Engine } from '../dist/types.js';
import { flags, type Options } from '../dist/options.js';
import { target } from './target.ts';

// without the addon node.js is the wasm engine already, and every diff would pass
if (load() === undefined) throw new Error('no addon to check: npm run build');
const wasm = compile();
const binary = `${target}/release/teasel`;
const files: string[] = [];
function walk(dir: string) {
	for (const name of readdirSync(dir)) {
		if (name === 'node_modules' || name.startsWith('.')) continue;
		const path = join(dir, name);
		if (statSync(path).isDirectory()) walk(path);
		else if (/\.(js|mjs|ts|svelte|css)$/.test(name) || (host !== undefined && name.endsWith(host.extension))) files.push(path);
	}
}
const args = process.argv.slice(2);
const host = args[0] === '--host' ? { path: args[1], grammar: readFileSync(args[1], 'utf8'), extension: args[2] } : undefined;
const grammar = host === undefined ? undefined : native.grammar(host.grammar);
const names: string[] | undefined = grammar === undefined ? undefined : JSON.parse(native.about(grammar)).names;
for (const dir of host ? args.slice(3) : args) walk(dir);
let checked = 0;
let failed = 0;

// what a parse answers or throws, read off the engine directly
function outcome(engine: Engine, source: string, on: number, entry: number, at: number, end?: number) {
	const cut = source.slice(0, end);
	return decode(engine.parse(cut, on, entry, at, '', undefined), cut, engine, on, entry === 3);
}

const show = (value: unknown) => (typeof value === 'bigint' ? `${value}n` : JSON.stringify(value));

// not assert.deepStrictEqual: it renders both graphs on failure, which never ends on linked scopes
function differ(a: unknown, b: unknown, seen = new Map<object, unknown>(), path = '$'): string | null {
	if (a === b) return null;
	if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return `${path}: ${show(a)} vs ${show(b)}`;
	if (seen.has(a)) return seen.get(a) === b ? null : `${path}: identity differs`;
	seen.set(a, b);
	const ka = Object.keys(a), kb = Object.keys(b);
	if (ka.length !== kb.length || ka.some((k) => !kb.includes(k))) return `${path}: keys ${ka} vs ${kb}`;
	for (const k of ka) {
		const r = differ((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k], seen, `${path}.${k}`);
		if (r) return r;
	}
	return null;
}

function report(name: string, difference: string | null) {
	checked++;
	if (!difference) return;
	failed++;
	if (failed <= 20) console.log(`${name}: ${difference}`);
}

// each entry of parser/mod.rs by index, naming the report's lines
const MODE = ['', 'expr', 'pattern', 'params', 'stmt', 'typeparams'];
const STYLESHEET = 6;
// the binary's own entry: a whole document of the grammar's language
const DOCUMENT = 7;

// the batch header the binary reads for the same parse: the entry, a byte offset and the option word
const header = (entry: number, offset: number, on: number) => `${entry} ${offset} ${on}`;

// JSON has no RegExp or BigInt, so the writer spells a regular expression's or a BigInt's value null
// the binary reports a code as its byte, the package as its name
const as_json = (key: string, value: unknown) => (value instanceof RegExp || typeof value === 'bigint' ? null : key === 'code' && typeof value === 'string' ? CODES.indexOf(value as Code) : value);

type Decoded = ReturnType<typeof decode>;

// the binary prints as numbers what the decoder links as objects, beside each node's position
function numbered(answer: Decoded) {
	const tables = Object.keys(LINKED) as (keyof typeof LINKED)[];
	const number = Object.fromEntries(tables.map((table) => [table, new Map<object, number>((answer[table] ?? []).map((row: Decoded, i: number) => [row, i]))])) as Record<keyof typeof LINKED, Map<object, number>>;
	const last = new Map<object, object>(), first = new Map<object, object>();
	const root = new Map<object, number>();
	const listed = (by: Map<object, number[]>, node: object, i: number) => by.set(node, [...(by.get(node) ?? []), i]);
	const defines = new Map<object, number[]>(), writes = new Map<object, number[]>();
	answer.roots?.forEach((row: Decoded, i: number) => row.node !== null && root.set(row.node, i));
	answer.bindings?.forEach((row: Decoded, i: number) => row.declaration !== null && listed(defines, row.declaration, i));
	answer.references?.forEach((row: Decoded, i: number) => row.writeExpr !== null && listed(writes, row.writeExpr, i));
	const facts = (out: Decoded, n: Decoded) => {
		if (n[SCOPE] !== undefined) {
			out.scope = number.scopes.get(n[SCOPE]);
			last.set(n[SCOPE], n);
		}
		if (n[REFERENCE] !== undefined) {
			const declares = number.bindings.has(n[REFERENCE]);
			out[declares ? 'declares' : 'reference'] = (declares ? number.bindings : number.references).get(n[REFERENCE]);
			if (!first.has(n[REFERENCE])) first.set(n[REFERENCE], n);
		}
		if (root.has(n)) out.root = root.get(n);
		if (defines.has(n)) out.defines = defines.get(n);
		if (writes.has(n)) out.writes = writes.get(n);
	};
	const value = (v: unknown): unknown => {
		if (Array.isArray(v)) return v.map(value);
		if (typeof v !== 'object' || v === null || v instanceof RegExp) return v;
		const n = v as Decoded, keys = Object.keys(n), out: Decoded = {};
		const head = keys[0] === 'type' && keys[1] === 'start' && keys[2] === 'end' ? (keys[3] === 'loc' ? 'loc' : 'end') : 'type';
		for (const key of keys) {
			out[key] = value(n[key]);
			if (key === head) facts(out, n);
		}
		return out;
	};
	const row = (table: keyof typeof LINKED, n: Decoded) => {
		const out: Decoded = {};
		for (const key of Object.keys(n)) {
			if (key in LINKED[table].held) continue;
			const v = n[key];
			if (!(key in LINKED[table].into)) out[key] = v;
			else {
				const into = number[LINKED[table].into[key]];
				if (!Array.isArray(v)) out[key] = v === null ? null : into.get(v);
				else out[key] = v.length === 0 ? [] : [into.get(v[0]), into.get(v[v.length - 1])! + 1];
			}
		}
		return out;
	};
	const out: Decoded = {};
	for (const key of Object.keys(answer)) out[key] = key in LINKED ? answer[key].map((n: Decoded) => row(key as keyof typeof LINKED, n)) : value(answer[key]);
	// the binary does not print a row's own links to its node: they read off the nodes that link to the row
	const held = (answer.scopes ?? []).some((s: Decoded) => s.node !== (last.get(s) ?? null)) || [...(answer.bindings ?? []), ...(answer.references ?? [])].some((r: Decoded) => r.node !== (first.get(r) ?? null)) || (answer.bindings ?? []).some((b: Decoded) => b.binding !== b);
	if (held) out.wrong = 'a row and its node disagree';
	return out;
}

// the addon's answers as JSON, each with the batch job that asks the binary for the same
const jobs: { name: string; source: string; header: string; tree: string }[] = [];
function json(name: string, source: string, options: Options, entry: number, at: number) {
	const on = flags(options);
	const tree = JSON.stringify(numbered(decode(native.parse(source, on, entry, at, '', undefined), source, native, on, entry === 3)), as_json);
	jobs.push({ name, source, header: header(entry, Buffer.byteLength(source.slice(0, at)), on), tree });
}

// a whole document of the host's
function document(name: string, source: string, options: Options) {
	const on = flags(options);
	const tree = JSON.stringify(numbered(decode(native.parse(source, on, 0, 0, '', grammar), source, native, on, false, names)), as_json);
	const switches = Object.entries(options).flatMap(([key, value]) => (value === true ? [key] : value ? [`${key}:${value}`] : []));
	jobs.push({ name: `${name} doc ${switches.join('+')}`, source, header: header(DOCUMENT, 0, on), tree });
}

const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;
const brace_re = /\{/g;
for (const file of files) {
	const text = readFileSync(file, 'utf8');
	if (file.endsWith('.css')) {
		for (const locations of [true, 'js'] as const) {
			const options: Options = { locations, comments: true };
			json(file, text, options, STYLESHEET, 0);
			report(`${file} wasm`, differ(outcome(wasm, text, flags(options), STYLESHEET, 0), outcome(native, text, flags(options), STYLESHEET, 0)));
		}
		continue;
	}
	if (host !== undefined && file.endsWith(host.extension)) {
		const typescript = /lang=["']?ts/.test(text);
		document(file, text, { sourceType: 'module', locations: true, comments: true, scopes: true });
		document(file, text, { sourceType: 'module', typescript: typescript ? 'erase' : false, locations: true, scopes: true });
		document(file, text, { sourceType: 'module', typescript, locations: true, errorRecovery: true, parenthesized: true });
		document(file, text, { sourceType: 'module', typescript: typescript ? 'erase' : false, locations: 'js', comments: true, scopes: true, parenthesized: true });
		if (!/\.(js|mjs|ts|svelte)$/.test(file)) continue;
	}
	const svelte = file.endsWith('.svelte');
	const sources: [string, boolean][] = svelte ? [...text.matchAll(script_re)].map((m) => [m[2], /lang=["']?ts/.test(m[1] ?? '')]) : [[text, file.endsWith('.ts')]];
	for (const [source, typescript] of sources) {
		const runs: Options[] = [
			{ sourceType: 'module', typescript, locations: true, comments: true },
			{ sourceType: 'module', typescript: typescript ? 'erase' : false, locations: true },
			{ sourceType: 'module', typescript, locations: true, scopes: true },
			{ typescript, locations: true, parenthesized: true },
			{ sourceType: 'module', typescript, locations: 'js', comments: true },
		];
		for (const options of runs) {
			json(file, source, options, 0, 0);
			report(`${file} wasm`, differ(outcome(wasm, source, flags(options), 0, 0), outcome(native, source, flags(options), 0, 0)));
		}
	}
	// every brace in a component is somewhere an expression, a pattern or a statement might start
	if (svelte) {
		const options: Options = { sourceType: 'module', typescript: /lang=["']?ts/.test(text), locations: true, comments: true, scopes: true };
		const on = flags(options);
		for (const m of text.matchAll(script_re)) {
			const start = m.index + m[0].indexOf('>') + 1;
			const end = start + m[2].length;
			report(`${file} script ${start} wasm`, differ(outcome(wasm, text, on, 0, start, end), outcome(native, text, on, 0, start, end)));
		}
		for (const match of text.matchAll(brace_re)) {
			const at = match.index + 1;
			for (let entry = 1; entry < MODE.length; entry++) {
				json(`${file}@${at} ${MODE[entry]}`, text, options, entry, at);
				report(`${file}@${at} ${MODE[entry]} wasm`, differ(outcome(wasm, text, on, entry, at), outcome(native, text, on, entry, at)));
			}
		}
	}
}
{
	let input = '';
	for (const job of jobs) input += `${job.header} ${Buffer.byteLength(job.source, 'utf8')}\n${job.source}`;
	const run = spawnSync(binary, host ? ['--batch', '--host', host.path] : ['--batch'], { input, maxBuffer: 1 << 30 });
	if (run.status !== 0) throw new Error(`${binary}: ${run.stderr}`);
	const lines = run.stdout.toString().split('\n');
	for (const [i, job] of jobs.entries()) {
		// a linked root's empty range has lost where it stood
		const roots = lines[i].lastIndexOf(',"roots":[');
		const text = roots === -1 ? lines[i] : lines[i].slice(0, roots) + lines[i].slice(roots).replace(/\[(\d+),\1\]/g, '[]');
		report(job.name, job.tree === text ? null : `differs from the JSON at ${[...job.tree].findIndex((c, i) => c !== text[i])}`);
	}
}
console.log(`${checked} answers compared, ${failed} differ`);
process.exit(failed ? 1 : 0);
