// Decodes every script under a directory both ways and diffs the trees, so the decoder is
// checked against the JSON the oracles are checked against.
//
//   node check.js DIR...
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import { deepStrictEqual } from 'node:assert';
import { decode } from './decode.js';

const native = createRequire(import.meta.url)('./binding.cjs');
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

/** @param {string} name @param {() => any} decoded @param {() => string} json */
function same(name, decoded, json) {
	let tree;
	let text;
	try {
		text = json();
		tree = decoded();
	} catch (e) {
		return;
	}
	if (typeof tree === 'string') {
		if (tree !== text) {
			failed++;
			console.log(`${name}: error differs`);
		}
		return;
	}
	checked++;
	try {
		deepStrictEqual(tree, JSON.parse(text));
		// the same keys in the same order, which deepStrictEqual does not mind
		if (JSON.stringify(tree) !== text) throw new Error('key order differs');
	} catch (e) {
		failed++;
		console.log(`${name}: ${e.message.split('\n').slice(0, 12).join('\n')}`);
	}
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
			{ typescript, preserveParens: true }
		]) {
			const answer = (value) => (typeof value === 'string' ? value : decode(value, source, native.constants, false));
			same(file, () => answer(native.parse(source, options)), () => native.parseJson(source, options));
		}
	}
	// every brace in a component is somewhere an expression, a pattern or a statement might start
	if (svelte) {
		const options = { sourceType: 'module', typescript: /lang=["']?ts/.test(text), locations: true, comments: true, scopes: true };
		const held = new native.Source(text, options);
		for (const match of text.matchAll(brace_re)) {
			const at = match.index + 1;
			const answer = (value) => (typeof value === 'string' ? value : decode(value, text, native.constants, false));
			same(`${file}@${at}`, () => answer(held.parseExpressionAt(at)), () => native.parseExpressionAtJson(text, at, options));
			same(`${file}@${at} pattern`, () => answer(held.parsePatternAt(at)), () => native.parsePatternAtJson(text, at, options));
			same(`${file}@${at} statement`, () => answer(held.parseStatementAt(at)), () => native.parseStatementAtJson(text, at, options));
		}
	}
}
console.log(`${checked} trees compared, ${failed} differ`);
process.exit(failed ? 1 : 0);
