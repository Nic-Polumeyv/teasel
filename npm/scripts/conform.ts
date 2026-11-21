// node scripts/conform.ts [--kind KIND] [--file PATH] [DIR...]: every .svelte file under the directories parsed by the
// pinned svelte/compiler (`modern: true`) and by teasel's Svelte grammar, the two trees walked together and every
// difference kept under a kind: node type, key, what differs. With no DIR the corpus is the repo's own Svelte fixtures
// and, when cloned (see ci.yml), Svelte's own test inputs; that run is held to conformance.json, which may only improve,
// and `UPDATE=1` rewrites it once it has. `--kind` lists the files showing one kind, `--file` every difference in one file, of that kind with both.
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { parse as svelte, VERSION } from 'svelte/compiler';
import { parse } from '../src/index.ts';
import grammar from './hosts/svelte.ts';

// both trees are read as plain JSON, past what either's types say
type Any = any;
const root = resolve(import.meta.dirname, '../..');
const fixtures = join(root, 'crates/teasel/tests/hosts/svelte');
const upstream = join(root, 'crates/teasel/tests/svelte/packages/svelte/tests');
const pin = join(fixtures, 'conformance.json');

const args = process.argv.slice(2);
const option = (name: string) => {
	const at = args.indexOf(name);
	return at === -1 ? undefined : args.splice(at, 2)[1];
};
const only = option('--kind');
const one = option('--file');
const fallback = args.length === 0;
const dirs = fallback ? [fixtures, ...(existsSync(upstream) ? [upstream] : [])] : args;

const files: { path: string; source: string }[] = [];
const seen = new Set<string>();
function walk(dir: string) {
	for (const name of readdirSync(dir).sort()) {
		if (name === 'node_modules' || name.startsWith('.')) continue;
		const path = join(dir, name);
		if (statSync(path).isDirectory()) walk(path);
		else if (name.endsWith('.svelte')) {
			const source = readFileSync(path, 'utf8');
			if (!seen.has(source)) seen.add(source), files.push({ path, source });
		}
	}
}
if (one === undefined) for (const dir of dirs) walk(dir);
else files.push({ path: resolve(one), source: readFileSync(one, 'utf8') });

const plain = (value: unknown) => JSON.parse(JSON.stringify(value, (_, v) => (typeof v === 'bigint' ? String(v) : v)));
const shape = (v: unknown) => (v === null ? 'null' : Array.isArray(v) ? 'array' : typeof v);
const show = (v: unknown) => {
	const text = JSON.stringify(v) ?? 'undefined';
	return text.length > 1500 ? `${text.slice(0, 1500)}…` : text;
};
function attempt(read: () => unknown): { tree: Any } | { code: string } {
	try {
		return { tree: plain(read()) };
	} catch (error) {
		return { code: String((error as { code?: unknown }).code ?? (error as Error).name) };
	}
}

interface Difference {
	kind: string;
	path: string;
	svelte?: unknown;
	teasel?: unknown;
}
// a kind names the nearest node's type and the keys from it; the path keeps the indices to find the place again
function compare(a: Any, b: Any): Difference[] {
	const out: Difference[] = [];
	(function walk(x: Any, y: Any, where: string, path: string) {
		if (x === y) return;
		const sx = shape(x), sy = shape(y);
		if (sx !== sy) return void out.push({ kind: `${where}: svelte ${sx}, teasel ${sy}`, path, svelte: x, teasel: y });
		if (sx === 'array') {
			if (x.length !== y.length) return void out.push({ kind: `${where}: list length`, path, svelte: x, teasel: y });
			for (let i = 0; i < x.length; i++) walk(x[i], y[i], where, `${path}[${i}]`);
			return;
		}
		if (sx !== 'object') return void out.push({ kind: `${where}: different ${sx}`, path, svelte: x, teasel: y });
		if (x.type !== y.type) return void out.push({ kind: `${where}: type ${x.type} vs ${y.type}`, path, svelte: x, teasel: y });
		const at = typeof x.type === 'string' ? x.type : where;
		for (const k of new Set([...Object.keys(x), ...Object.keys(y)])) {
			if (!(k in x)) out.push({ kind: `${at}.${k}: only in teasel`, path: `${path}.${k}`, teasel: y[k] });
			else if (!(k in y)) out.push({ kind: `${at}.${k}: only in svelte`, path: `${path}.${k}`, svelte: x[k] });
			else walk(x[k], y[k], `${at}.${k}`, `${path}.${k}`);
		}
	})(a, b, 'Root', '$');
	return out;
}

// the one place a kind is given its family; the first rule that matches wins
const TEMPLATE = /^(Root|Fragment|Text|Comment|Script|Attribute|(Regular|Slot|Title)Element|[A-Z]\w*(Directive|Block|Tag)|Component|Svelte[A-Z]\w*|ElseIf)\./;
const CSS = /^(Rule|Atrule|Block|Declaration|SelectorList|ComplexSelector|RelativeSelector|Combinator|\w*Selector|Nth|Percentage|StyleSheet)\./;
const families: [string, (kind: string) => boolean][] = [
	['rejections', (k) => k.startsWith('rejects')],
	['positions', (k) => /\.(loc|name_loc|start|end|line|column|character)\b/.test(k)],
	// a comment's type is Line or Block, and CSS has a Block too, holding no value
	['comments', (k) => /\.(leading|trailing|inner)?[cC]omments?\b|^(Line|Block)\.value\b/.test(k)],
	['whitespace and text', (k) => /^Fragment\.nodes: list length|^Text\./.test(k)],
	['css', (k) => CSS.test(k)],
	['template shape', (k) => TEMPLATE.test(k)],
	['typescript and estree shape', () => true],
];
const family = (kind: string) => families.find(([, test]) => test(kind))![0];

const name = (path: string) => (path.startsWith(root) ? relative(root, path) : path);
const kinds = new Map<string, Set<string>>();
let compared = 0, identical = 0, rejected = 0, bothReject = 0;
for (const { path, source } of files) {
	const a = attempt(() => svelte(source, { modern: true }));
	const b = attempt(() => parse(source, { sourceType: 'module', comments: true }, grammar).node);
	let found: Difference[];
	if ('code' in a) {
		rejected++;
		if ('code' in b) bothReject++;
		found = 'code' in b ? [] : [{ kind: `rejects: only svelte (${a.code})`, path: '$', teasel: b.tree }];
	} else if ('code' in b) found = [{ kind: `rejects: only teasel (${b.code})`, path: '$', svelte: a.tree }];
	else {
		compared++;
		found = compare(a.tree, b.tree);
		if (found.length === 0) identical++;
	}
	if (one !== undefined) {
		const shown = found.filter((d) => only === undefined || d.kind === only);
		for (const d of shown) {
			console.log(`${d.kind}\n  at ${d.path}`);
			if ('svelte' in d) console.log(`  svelte ${show(d.svelte)}`);
			if ('teasel' in d) console.log(`  teasel ${show(d.teasel)}`);
		}
		console.log(`${shown.length} differences`);
		process.exit(0);
	}
	for (const { kind } of found) {
		if (!kinds.has(kind)) kinds.set(kind, new Set());
		kinds.get(kind)!.add(name(path));
	}
}

if (only !== undefined) {
	const list = [...(kinds.get(only) ?? [])];
	console.log(list.join('\n'));
	console.log(`${list.length} files show ${JSON.stringify(only)}`);
	process.exit(0);
}

const ranked = [...kinds].sort((p, q) => q[1].size - p[1].size || (p[0] < q[0] ? -1 : 1));
console.log(`svelte ${VERSION}: ${files.length} files, ${compared} parsed by both, ${identical} identical`);
console.log(`svelte rejects ${rejected}: teasel rejects ${bothReject} of them`);
for (const [group] of families) {
	const mine = ranked.filter(([kind]) => family(kind) === group);
	if (mine.length === 0) continue;
	const showing = new Set(mine.flatMap(([, set]) => [...set]));
	console.log(`\n${group}: ${mine.length} kinds in ${showing.size} files`);
	for (const [kind, set] of mine) console.log(`${String(set.size).padStart(6)}  ${kind}  e.g. ${set.values().next().value}`);
}

if (!fallback) process.exit(0);
if (dirs.length === 1) {
	// CI always clones Svelte's tests, so there a missing clone is a broken step, not a smaller corpus
	if (process.env.CI) throw new Error(`no Svelte tests at ${relative(root, upstream)}: clone them as ci.yml does`);
	console.log(`\nthe baseline needs Svelte's tests at ${relative(root, upstream)} (see ci.yml); not checked`);
	process.exit(0);
}
const now = { svelte: VERSION, files: files.length, identical, kinds: Object.fromEntries(ranked.map(([kind, set]) => [kind, set.size])) };
const json = `${JSON.stringify(now, null, '\t')}\n`;
if (process.env.UPDATE) {
	writeFileSync(pin, json);
	process.exit(0);
}
const was: typeof now = JSON.parse(readFileSync(pin, 'utf8'));
const worse: string[] = [], better: string[] = [];
if (was.svelte !== now.svelte || was.files !== now.files) worse.push(`the corpus changed (svelte ${was.svelte} to ${now.svelte}, ${was.files} files to ${now.files}): run with UPDATE=1 if that is meant`);
if (now.identical < was.identical) worse.push(`identical files fell from ${was.identical} to ${now.identical}`);
if (now.identical > was.identical) better.push(`identical files rose from ${was.identical} to ${now.identical}`);
for (const kind of new Set([...Object.keys(was.kinds), ...Object.keys(now.kinds)])) {
	const before = was.kinds[kind] ?? 0, after = now.kinds[kind] ?? 0;
	if (after > before) worse.push(`${before === 0 ? 'new kind' : 'more files'}: ${kind} (${before} to ${after})`);
	else if (after < before) better.push(`fewer files: ${kind} (${before} to ${after})`);
}
if (worse.length) console.log(`\nfurther from svelte than ${relative(root, pin)}:\n  ${worse.join('\n  ')}`);
if (better.length) console.log(`\ncloser to svelte than ${relative(root, pin)}; run with UPDATE=1 to keep it:\n  ${better.join('\n  ')}`);
process.exit(worse.length || better.length ? 1 : 0);
