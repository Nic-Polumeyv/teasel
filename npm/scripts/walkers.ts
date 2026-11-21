// node scripts/walkers.ts [fixtures|prefixes|svelte|svelte-prefixes]...: every input parsed by the Svelte grammar's walker
// and by the walker written for Svelte, under several option sets, and the complete answers compared: the
// tree, `end`, comments, scopes and their links, `typescript`, `errors`, or what was thrown. The first
// difference of an input is printed with its path; any difference fails. The inputs are the host's
// fixtures, every prefix of each of them, and Svelte's own tests when cloned into crates/teasel/tests/svelte.
// `npm run build:js` and a built engine first.
import { readdirSync, readFileSync, statSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import type { Options } from '../src/options.ts';

const { parse } = await import('../src/index.ts');
const { default: grammar } = await import('./hosts/svelte.ts');
const { svelte } = await import('../src/svelte.ts');
const { PARENT, SCOPE, REFERENCE } = await import('../src/decode.ts');

const root = new URL('../../crates/teasel/tests/', import.meta.url).pathname;
const sets: Record<string, Options> = {
	strict: {},
	full: { comments: true, scopes: true },
	recover: { errorRecovery: true, comments: true, scopes: true },
	erase: { typescript: 'erase', comments: true, scopes: true },
	module: { sourceType: 'module', parenthesized: true, scopes: true },
};
const cuts: Record<string, Options> = { strict: sets.full, recover: sets.recover };

const show = (value: unknown) => (typeof value === 'bigint' ? `${value}n` : JSON.stringify(value));
const LINKS = [PARENT, SCOPE, REFERENCE];

// the two answers as graphs, each object of one standing for one of the other; not assert.deepStrictEqual, which renders both graphs on failure and never ends on linked scopes
function differ(a: unknown, b: unknown, seen: Map<object, unknown>, path: string): string | null {
	if (a === b || (Number.isNaN(a) && Number.isNaN(b))) return null;
	if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return `${path}: ${show(a)} vs ${show(b)}`;
	if (seen.has(a)) return seen.get(a) === b ? null : `${path}: a different object`;
	seen.set(a, b);
	if (Array.isArray(a) !== Array.isArray(b) || Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return `${path}: of another kind`;
	const ka = Object.keys(a), kb = Object.keys(b);
	if (ka.join() !== kb.join()) return `${path}: keys ${ka} vs ${kb}`;
	for (const k of ka) {
		const r = differ((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k], seen, `${path}.${k}`);
		if (r) return r;
	}
	for (const link of LINKS) {
		const r = differ((a as Record<symbol, unknown>)[link], (b as Record<symbol, unknown>)[link], seen, `${path}[${link.description}]`);
		if (r) return r;
	}
	return null;
}

// what a parse answers, or what it throws
function outcome(text: string, options: Options, language: typeof svelte): unknown {
	try {
		return parse(text, options, language);
	} catch (error) {
		const e = error as Error & Record<string, unknown>;
		return { thrown: { name: e.name, message: e.message, code: e.code, args: e.args, pos: e.pos, end: e.end } };
	}
}

const tally: Record<string, { compared: number; differing: number }> = {};
let shown = 0;

function compare(group: string, label: string, text: string, options: Options, quiet = false) {
	const counts = (tally[group] ??= { compared: 0, differing: 0 });
	counts.compared++;
	const old = outcome(text, options, grammar as typeof svelte);
	const now = outcome(text, options, svelte);
	const found = differ(old, now, new Map(), '$');
	if (found === null) return true;
	counts.differing++;
	if (!quiet && shown++ < 60) console.log(`${group} ${label}: ${found}`);
	return false;
}

function files(dir: string, out: string[] = []) {
	for (const name of readdirSync(dir).sort()) {
		const path = join(dir, name);
		if (statSync(path).isDirectory()) files(path, out);
		else if (name.endsWith('.svelte')) out.push(path);
	}
	return out;
}

const wanted = process.argv.slice(2);
const runs = (what: string) => wanted.length === 0 || wanted.includes(what);
const fixtures = files(join(root, 'hosts/svelte'));

if (runs('fixtures')) {
	for (const file of fixtures) {
		const text = readFileSync(file, 'utf8');
		for (const [set, options] of Object.entries(sets)) compare(`fixtures ${set}`, file.slice(root.length), text, options);
	}
}

function prefixes(group: string, files: string[]) {
	for (const file of files) {
		const text = readFileSync(file, 'utf8');
		for (const [set, options] of Object.entries(cuts)) {
			// the first difference of an input is shown: the cuts after it mostly repeat it
			let quiet = false;
			for (let end = 0; end <= text.length; end++) {
				const code = text.charCodeAt(end - 1);
				if (code >= 0xd800 && code < 0xdc00) continue;
				if (!compare(`${group} ${set}`, `${file.slice(root.length)} cut at ${end}`, text.slice(0, end), options, quiet)) quiet = true;
			}
		}
	}
}

if (runs('prefixes')) prefixes('prefixes', fixtures);

if (runs('svelte')) {
	const dir = join(root, 'svelte/packages/svelte/tests');
	if (!existsSync(dir)) console.log(`no clone of Svelte's tests at ${dir}: skipped`);
	else {
		for (const file of files(dir)) {
			const text = readFileSync(file, 'utf8');
			for (const [set, options] of Object.entries(sets)) compare(`svelte ${set}`, file.slice(root.length), text, options);
		}
	}
}

// every prefix of every one of Svelte's tests: some three million parses, too long for every run
if (wanted.includes('svelte-prefixes')) prefixes('svelte prefixes', files(join(root, 'svelte/packages/svelte/tests')));

let failed = false;
for (const [group, { compared, differing }] of Object.entries(tally)) {
	console.log(`${group}: ${compared} compared, ${differing} differing`);
	failed ||= differing > 0;
}
if (failed) process.exit(1);
