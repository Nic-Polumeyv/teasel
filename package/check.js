// node check.js DIR...: every script under the directories, parsed three ways and the answers
// diffed: the addon's decoded stream against the JSON the binary prints, and the wasm module's
// against the addon's. `cargo build --release` first.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import * as node from './index.js';
import * as wasm from './wasm.js';
import { ENTRY, names } from './api.js';
import { decode } from './decode.js';

const native = createRequire(import.meta.url)('./binding.cjs');
const engine = { constants: native.constants, shapes: native.shapes };
const binary = new URL('../target/release/teasel', import.meta.url).pathname;
const files = [];
function walk(dir) {
	for (const name of readdirSync(dir)) {
		if (name === 'node_modules' || name.startsWith('.')) continue;
		const path = join(dir, name);
		if (statSync(path).isDirectory()) walk(path);
		else if (/\.(js|mjs|ts|svelte)$/.test(name)) files.push(path);
	}
}
for (const dir of process.argv.slice(2)) walk(dir);
let checked = 0;
let failed = 0;

const bits_of = names;
function result(answer, source) {
	if (typeof answer !== 'string') return decode(answer, source, engine);
	const { message, ...error } = JSON.parse(answer).error;
	throw Object.assign(new SyntaxError(message), error);
}

function outcome(fn) {
	try {
		return { value: fn() };
	} catch (e) {
		if (!(e instanceof SyntaxError)) throw e;
		return { error: { code: e.code, message: e.message, pos: e.pos, end: e.end, loc: e.loc } };
	}
}

// not assert.deepStrictEqual: it renders both graphs on failure, which never ends on linked scopes
function differ(a, b, seen = new Map(), path = '$') {
	if (a === b) return null;
	if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return `${path}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`;
	if (seen.has(a)) return seen.get(a) === b ? null : `${path}: identity differs`;
	seen.set(a, b);
	const ka = Object.keys(a), kb = Object.keys(b);
	if (ka.length !== kb.length || ka.some((k) => !kb.includes(k))) return `${path}: keys ${ka} vs ${kb}`;
	for (const k of ka) {
		const r = differ(a[k], b[k], seen, `${path}.${k}`);
		if (r) return r;
	}
	return null;
}

function report(name, difference) {
	checked++;
	if (!difference) return;
	failed++;
	if (failed <= 20) console.log(`${name}: ${difference}`);
}

// the batch header the binary reads for the same parse: byte offsets, every switch of the options
const MODE = { program: '', expression: 'expr', pattern: 'pattern', params: 'params', statement: 'stmt', typeParameters: 'typeparams' };
function mode(source, options, entry, at) {
	const switches = ['comments', 'scopes', 'parenthesized'].filter((flag) => options[flag]).map((flag) => `+${flag}`);
	if (options.typescript === 'erase') switches.push('+erase');
	const head = entry === 'program' ? (options.sourceType === 'module' ? 'module' : 'script') : MODE[entry];
	const offset = entry === 'program' ? '' : `:${Buffer.byteLength(source.slice(0, at))}`;
	return `${options.typescript ? 'ts-' : ''}${head}${switches.join('')}${offset}`;
}

/** The addon's answers as JSON, each with the batch job that asks the binary for the same. */
const jobs = [];
function json(name, source, options, entry, at) {
	const answer = new native.Source(Buffer.from(source), names(options)).parse(ENTRY[entry], at, undefined, '');
	const tree = typeof answer === 'string' ? answer : JSON.stringify(decode(answer, source, engine, false));
	jobs.push({ name, source, mode: mode(source, options, entry, at), tree });
}

const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;
const brace_re = /\{/g;
for (const file of files) {
	const text = readFileSync(file, 'utf8');
	const svelte = file.endsWith('.svelte');
	const sources = svelte ? [...text.matchAll(script_re)].map((m) => [m[2], /lang=["']?ts/.test(m[1] ?? '')]) : [[text, file.endsWith('.ts')]];
	for (const [source, typescript] of sources) {
		for (const options of [
			{ sourceType: 'module', typescript, locations: true, comments: true },
			{ sourceType: 'module', typescript: typescript ? 'erase' : false, locations: true },
			{ sourceType: 'module', typescript, locations: true, scopes: true },
			{ typescript, locations: true, parenthesized: true },
		]) {
			json(file, source, options, 'program', 0);
			report(`${file} wasm`, differ(outcome(() => new wasm.Source(source, options).parse()), outcome(() => new node.Source(source, options).parse())));
		}
	}
	// every brace in a component is somewhere an expression, a pattern or a statement might start
	if (svelte) {
		const options = { sourceType: 'module', typescript: /lang=["']?ts/.test(text), locations: true, comments: true, scopes: true };
		const held = new node.Source(text, options);
		const twin = new wasm.Source(text, options);
		for (const m of text.matchAll(script_re)) {
			const start = m.index + m[0].indexOf('>') + 1;
			const at = { end: start + m[2].length };
			report(`${file} script ${start} wasm`, differ(outcome(() => twin.parse('program', start, at)), outcome(() => held.parse('program', start, at))));
		}
		const raw = new native.Source(Buffer.from(text), bits_of(options));
		for (const match of text.matchAll(brace_re)) {
			const at = match.index + 1;
			for (const entry of Object.keys(ENTRY)) {
				if (entry === 'program') continue;
				json(`${file}@${at} ${entry}`, text, options, entry, at);
				report(`${file}@${at} ${entry} wasm`, differ(outcome(() => twin.parse(entry, at)), outcome(() => held.parse(entry, at))));
				// the package answers a bare identifier itself; it must say what the engine says
				report(`${file}@${at} ${entry} engine`, differ(outcome(() => held.parse(entry, at)), outcome(() => result(raw.parse(ENTRY[entry], at, undefined, ''), text))));
			}
		}
		twin.free();
	}
}
{
	let input = '';
	for (const job of jobs) input += `${job.mode} ${Buffer.byteLength(job.source, 'utf8')}\n${job.source}`;
	const run = spawnSync(binary, ['--batch'], { input, maxBuffer: 1 << 30 });
	if (run.status !== 0) throw new Error(`${binary}: ${run.stderr}`);
	const lines = run.stdout.toString().split('\n');
	for (const [i, job] of jobs.entries()) {
		const text = lines[i];
		report(job.name, job.tree === text ? null : `differs from the JSON at ${[...job.tree].findIndex((c, i) => c !== text[i])}`);
	}
}
console.log(`${checked} answers compared, ${failed} differ`);
process.exit(failed ? 1 : 0);
