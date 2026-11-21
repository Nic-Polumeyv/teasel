// node scripts/check.ts DIR...: every script, stylesheet and Svelte component under the directories, parsed
// three ways and the answers diffed: the addon's decoded answer against the JSON the binary prints, and the
// wasm module's against the addon's; a component's scripts on their own too. `cargo build --release` first.
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
		else if (/\.(js|mjs|ts|svelte|css)$/.test(name)) files.push(path);
	}
}
for (const dir of process.argv.slice(2)) walk(dir);
let checked = 0;
let failed = 0;

// what a parse answers or throws, read off the engine directly
function outcome(engine: Engine, source: string, on: number, language: number) {
	return decode(engine.parse(source, on, language), source, engine);
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

// the engine's languages by number
const JS = 0, STYLESHEET = 1, SVELTE = 2;

// the batch header the binary reads for the same parse: the language and the option word
const header = (language: number, on: number) => `${language} ${on}`;

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
		const head = keys[0] === 'type' && keys[1] === 'start' && keys[2] === 'end' ? 'end' : 'type';
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
function json(name: string, source: string, options: Options, language: number) {
	const on = flags(options);
	const tree = JSON.stringify(numbered(decode(native.parse(source, on, language), source, native)), as_json);
	jobs.push({ name, source, header: header(language, on), tree });
	report(`${name} wasm`, differ(outcome(wasm, source, on, language), outcome(native, source, on, language)));
}

// a whole component
function component(name: string, source: string, options: Options) {
	const switches = Object.entries(options).flatMap(([key, value]) => (value === true ? [key] : value ? [`${key}:${value}`] : []));
	json(`${name} component ${switches.join('+')}`, source, options, SVELTE);
}

const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;
for (const file of files) {
	const text = readFileSync(file, 'utf8');
	if (file.endsWith('.css')) {
		json(file, text, { comments: true }, STYLESHEET);
		continue;
	}
	const svelte = file.endsWith('.svelte');
	if (svelte) {
		const typescript = /lang=["']?ts/.test(text);
		component(file, text, { sourceType: 'module', comments: true, scopes: true });
		component(file, text, { sourceType: 'module', typescript: typescript ? 'erase' : false, scopes: true });
		component(file, text, { sourceType: 'module', typescript, errorRecovery: true, parenthesized: true });
		component(file, text, { sourceType: 'module', typescript: typescript ? 'erase' : false, comments: true, scopes: true, parenthesized: true });
	}
	const sources: [string, boolean][] = svelte ? [...text.matchAll(script_re)].map((m) => [m[2], /lang=["']?ts/.test(m[1] ?? '')]) : [[text, file.endsWith('.ts')]];
	for (const [source, typescript] of sources) {
		const runs: Options[] = [
			{ sourceType: 'module', typescript, comments: true },
			{ sourceType: 'module', typescript: typescript ? 'erase' : false },
			{ sourceType: 'module', typescript, scopes: true },
			{ typescript, parenthesized: true },
		];
		for (const options of runs) json(file, source, options, JS);
	}
}
{
	let input = '';
	for (const job of jobs) input += `${job.header} ${Buffer.byteLength(job.source, 'utf8')}\n${job.source}`;
	const run = spawnSync(binary, ['--batch'], { input, maxBuffer: 1 << 30 });
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
