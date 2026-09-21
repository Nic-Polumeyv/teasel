// node scripts/check.ts [--host GRAMMAR EXTENSION] DIR...: every script under the directories, parsed three
// ways and the answers diffed: the addon's decoded answer against the JSON the binary prints, and the wasm
// module's against the addon's; with a host, every file of its extension as a document too.
// `cargo build --release` first.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { engine as native } from '../dist/native.js';
import { engine as wasm } from '../dist/wasm.js';
import { decode, type Engine, type Prepared } from '../dist/lib/decode.js';
import { target } from './target.ts';
import { ENTRY, type Entry, flags, type Options } from '../dist/lib/options.js';

const binary = `${target}/release/teasel`;
const files: string[] = [];
function walk(dir: string) {
	for (const name of readdirSync(dir)) {
		if (name === 'node_modules' || name.startsWith('.')) continue;
		const path = join(dir, name);
		if (statSync(path).isDirectory()) walk(path);
		else if (/\.(js|mjs|ts|svelte)$/.test(name) || (host !== undefined && name.endsWith(host.extension))) files.push(path);
	}
}
const args = process.argv.slice(2);
const host = args[0] === '--host' ? { path: args[1], grammar: readFileSync(args[1], 'utf8'), extension: args[2] } : undefined;
const plan = host === undefined ? undefined : native.plan(host.grammar);
for (const dir of host ? args.slice(3) : args) walk(dir);
let checked = 0;
let failed = 0;

// what a parse answers or throws, read off the engine directly
function outcome(engine: Engine, held: Prepared, source: string, entry: Entry, at: number, end?: number) {
	const answer = held.parse(ENTRY[entry], at, end, '', undefined);
	return typeof answer === 'string' ? { error: JSON.parse(answer).error } : { value: decode(answer, source, engine) };
}
function once(engine: Engine, source: string, options: Options, entry: Entry, at: number) {
	const held = engine.create(source, flags(options));
	try {
		return outcome(engine, held, source, entry, at);
	} finally {
		held.free();
	}
}

// not assert.deepStrictEqual: it renders both graphs on failure, which never ends on linked scopes
function differ(a: unknown, b: unknown, seen = new Map<object, unknown>(), path = '$'): string | null {
	if (a === b) return null;
	if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return `${path}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`;
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

// the batch header the binary reads for the same parse: byte offsets, every switch of the options
const MODE: Record<Entry, string> = { program: '', expression: 'expr', pattern: 'pattern', params: 'params', statement: 'stmt', typeParameters: 'typeparams' };
function mode(source: string, options: Options, entry: Entry, at: number) {
	const switches = (['comments', 'scopes', 'parenthesized'] as const).filter((flag) => options[flag]).map((flag) => `+${flag}`);
	if (options.typescript === 'erase') switches.push('+erase');
	const head = entry === 'program' ? (options.sourceType === 'module' ? 'module' : 'script') : MODE[entry];
	const offset = entry === 'program' ? '' : `:${Buffer.byteLength(source.slice(0, at))}`;
	return `${options.typescript ? 'ts-' : ''}${head}${switches.join('')}${offset}`;
}

// the addon's answers as JSON, each with the batch job that asks the binary for the same
const jobs: { name: string; source: string; mode: string; tree: string }[] = [];
function json(name: string, source: string, options: Options, entry: Entry, at: number) {
	const held = native.create(source, flags(options));
	const answer = held.parse(ENTRY[entry], at, undefined, '', undefined);
	held.free();
	const tree = typeof answer === 'string' ? answer : JSON.stringify(decode(answer, source, native, false));
	jobs.push({ name, source, mode: mode(source, options, entry, at), tree });
}

// a whole document of the host's, asked of the binary as `doc`
function document(name: string, source: string, typescript: boolean, options: Options, switches: string) {
	const held = native.create(source, flags(options));
	const answer = held.parse(ENTRY.program, 0, undefined, '', plan);
	held.free();
	const tree = typeof answer === 'string' ? answer : JSON.stringify(decode(answer, source, native, false));
	jobs.push({ name: `${name} doc${switches}`, source, mode: `${typescript ? 'ts-' : ''}doc${switches}`, tree });
}

const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;
const brace_re = /\{/g;
for (const file of files) {
	const text = readFileSync(file, 'utf8');
	if (host !== undefined && file.endsWith(host.extension)) {
		const typescript = /lang=["']?ts/.test(text);
		document(file, text, false, { sourceType: 'module', locations: true, comments: true, scopes: true }, '+comments+scopes');
		document(file, text, typescript, { sourceType: 'module', typescript: typescript ? 'erase' : false, locations: true, scopes: true }, `${typescript ? '+erase' : ''}+scopes`);
		document(file, text, typescript, { sourceType: 'module', typescript, locations: true, errorRecovery: true, parenthesized: true }, '+recover+parenthesized');
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
		];
		for (const options of runs) {
			json(file, source, options, 'program', 0);
			report(`${file} wasm`, differ(once(wasm, source, options, 'program', 0), once(native, source, options, 'program', 0)));
		}
	}
	// every brace in a component is somewhere an expression, a pattern or a statement might start
	if (svelte) {
		const options: Options = { sourceType: 'module', typescript: /lang=["']?ts/.test(text), locations: true, comments: true, scopes: true };
		const held = native.create(text, flags(options));
		const twin = wasm.create(text, flags(options));
		for (const m of text.matchAll(script_re)) {
			const start = m.index + m[0].indexOf('>') + 1;
			const end = start + m[2].length;
			report(`${file} script ${start} wasm`, differ(outcome(wasm, twin, text, 'program', start, end), outcome(native, held, text, 'program', start, end)));
		}
		for (const match of text.matchAll(brace_re)) {
			const at = match.index + 1;
			for (const entry of Object.keys(ENTRY) as Entry[]) {
				if (entry === 'program') continue;
				json(`${file}@${at} ${entry}`, text, options, entry, at);
				report(`${file}@${at} ${entry} wasm`, differ(outcome(wasm, twin, text, entry, at), outcome(native, held, text, entry, at)));
			}
		}
		held.free();
		twin.free();
	}
}
{
	let input = '';
	for (const job of jobs) input += `${job.mode} ${Buffer.byteLength(job.source, 'utf8')}\n${job.source}`;
	const run = spawnSync(binary, host ? ['--batch', '--host', host.path] : ['--batch'], { input, maxBuffer: 1 << 30 });
	if (run.status !== 0) throw new Error(`${binary}: ${run.stderr}`);
	const lines = run.stdout.toString().split('\n');
	for (const [i, job] of jobs.entries()) {
		const text = lines[i];
		report(job.name, job.tree === text ? null : `differs from the JSON at ${[...job.tree].findIndex((c, i) => c !== text[i])}`);
	}
}
console.log(`${checked} answers compared, ${failed} differ`);
process.exit(failed ? 1 : 0);
