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
const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;
let checked = 0;
let failed = 0;
for (const file of files) {
	const text = readFileSync(file, 'utf8');
	const sources = file.endsWith('.svelte') ? [...text.matchAll(script_re)].map((m) => [m[2], /lang=["']?ts/.test(m[1] ?? '')]) : [[text, file.endsWith('.ts')]];
	for (const [source, typescript] of sources) {
		for (const options of [{ sourceType: 'module', typescript, locations: true, comments: true }, { sourceType: 'module', typescript, erase: typescript }]) {
			const json = native.parseJson(source, options);
			const answer = native.parse(source, options);
			if (typeof answer === 'string') {
				if (answer !== json) { failed++; console.log(`${file}: error differs`); }
				continue;
			}
			checked++;
			try {
				deepStrictEqual(decode(answer, source, native.constants), JSON.parse(json));
			} catch (e) {
				failed++;
				console.log(`${file}: ${e.message.split('\n').slice(0, 12).join('\n')}`);
			}
		}
	}
}
console.log(`${checked} trees compared, ${failed} differ`);
process.exit(failed ? 1 : 0);
