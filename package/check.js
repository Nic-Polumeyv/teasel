// Every script under the directories, parsed every way: the Node addon's tree decoded from the
// token stream is diffed against its JSON, the JSON the oracles are checked against, and the
// WebAssembly module's tree against the addon's.
//
//   node check.js DIR...
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import * as node from './index.js';
import * as wasm from './wasm.js';
import { bits } from './options.js';
import { decode } from './decode.js';

const native = createRequire(import.meta.url)('./binding.cjs');
await wasm.init(readFileSync(new URL('./teasel.wasm', import.meta.url)));
const ENTRY = { expression: 1, pattern: 2, statement: 4 };
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

/** A tree or the error it throws, as data. */
function outcome(fn) {
	try {
		return { value: fn() };
	} catch (e) {
		return { error: { code: e.code, message: e.message, pos: e.pos, end: e.end, loc: e.loc } };
	}
}

/**
 * The first difference between two trees, following links (scopes and bindings point back into
 * the tree) once: `assert.deepStrictEqual` renders both graphs on failure, which never ends.
 */
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

/** The addon's answer decoded without links against its JSON: the same keys in the same order. */
function json(name, source, b, entry, at) {
	const answer = native.parseAt(source, b, entry, at, false);
	const text = native.parseAtJson(source, b, entry, at, false);
	const tree = typeof answer === 'string' ? answer : JSON.stringify(decode(answer, source, native.constants, false));
	report(name, tree === text ? null : `differs from the JSON at ${[...tree].findIndex((c, i) => c !== text[i])}`);
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
			{ sourceType: 'module', typescript, erase: typescript },
			{ sourceType: 'module', typescript, scopes: true },
			{ typescript, preserveParens: true },
		]) {
			json(file, source, bits(options), 0, 0);
			report(`${file} wasm`, differ(outcome(() => wasm.parse(source, options)), outcome(() => node.parse(source, options))));
		}
	}
	// every brace in a component is somewhere an expression, a pattern or a statement might start
	if (svelte) {
		const options = { sourceType: 'module', typescript: /lang=["']?ts/.test(text), locations: true, comments: true, scopes: true };
		const held = new node.Source(text, options);
		const twin = new wasm.Source(text, options);
		for (const match of text.matchAll(brace_re)) {
			const at = match.index + 1;
			for (const [entry, index] of Object.entries(ENTRY)) {
				const method = `parse${entry[0].toUpperCase()}${entry.slice(1)}At`;
				json(`${file}@${at} ${entry}`, text, bits(options), index, at);
				report(`${file}@${at} ${entry} wasm`, differ(outcome(() => twin[method](at)), outcome(() => held[method](at))));
			}
		}
		twin.free();
	}
}
console.log(`${checked} answers compared, ${failed} differ`);
process.exit(failed ? 1 : 0);
