// node scripts/conform.ts [--kind KIND] [--file PATH] [DIR...]: every .svelte file under the directories parsed by the
// pinned svelte/compiler (`modern: true`) and by teasel's Svelte grammar, the two trees walked together and every
// difference kept under a kind: node type, key, what differs. Svelte's `loc` and `name_loc`, which teasel's tree leaves
// out, are derived from teasel's spans and `locate` and checked (see "Positions"). With no DIR the corpus is the repo's own Svelte fixtures
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
function attempt<T>(read: () => T, tree: (value: T) => unknown): { tree: Any; value: T } | { code: string } {
	try {
		const value = read();
		return { tree: plain(tree(value)), value };
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
type Locate = (offset: number) => { line: number; column: number };

// Positions. teasel's tree is Svelte's modern AST without `loc` and `name_loc`: every node keeps `start` and `end`, and
// the answer's `locate` gives an offset's line (from 1) and column (UTF-16 units from 0). So a `loc` or `name_loc` only
// Svelte has is no difference; it is derived from the paired teasel node, and what disagrees is one. This is the one
// place that knows how, read from svelte/src/compiler/phases/1-parse:
// - A node's `loc` is `locate(start)` to `locate(end)`. acorn makes it for JavaScript, locate-character for what the
//   markup reads itself (`read_identifier`, `read_tag`, `read_comment`, the simple expressions of read/expression.js),
//   and locate-character's positions also carry `character`, the offset itself.
// - A `<script>`'s Program: acorn gives it a `loc` from offset 0, and read/script.js overwrites its line and column with
//   those of the Script element around it, from `<script` to past `</script>`, while the Program's `start` and `end`
//   stay those of the script's content. So it is derived from the parent Script's span.
// - An `{#each}` expression in a TypeScript document (the first `<script>` with a `lang` says `ts`): acorn reads
//   `xs || ys as item` as `xs || (ys as item)`, and state/tag.js takes the assertion off and moves the root's `end`
//   back, but not its `loc.end`, which stays where the context ends. That happens when the root binds looser than
//   `as` (LOOSE), so it is derived from the root's type and the context's `end`; when the root is the assertion
//   itself, it is replaced by its operand, whose `loc` is right.
// - `name_loc` is the name as written (`read_tag`, element.js), a `loc` whose only offset is `character`: an element's
//   from `start + 1` for `name.length`, `svelte:` and dots included; an attribute's from `start` for `name.length`; a
//   shorthand `{name}`'s is its identifier's, which the value's expression holds (a shorthand is the one attribute
//   whose ExpressionTag value ends before the attribute does, at `}`); a directive's from `start` across its whole raw
//   name, `prefix:name|modifier|…`, the prefix following from the type (and a transition's from `intro` and `outro`).
// - Not derived: locate-character counts a line at `\n` only, read/script.js blanks the markup before a script to
//   spaces but for `\n`, and acorn, started inside the text, counts the lines before it at `\n` only and the rest as
//   JavaScript and `locate` do: at `\r\n`, a lone `\r`, U+2028 and U+2029. In a text with one of the last three,
//   Svelte's positions disagree with one another, and a difference there says so in its kind.
const PREFIX: Record<string, string> = { OnDirective: 'on', BindDirective: 'bind', ClassDirective: 'class', StyleDirective: 'style', UseDirective: 'use', AnimateDirective: 'animate', LetDirective: 'let' };
const LOOSE = (y: Any) => /^(Logical|Conditional|Assignment|ArrowFunction|Yield)Expression$/.test(y.type) || (y.type === 'BinaryExpression' && /^([!=]==?|[&^|])$/.test(y.operator));
function span(key: 'loc' | 'name_loc', y: Any, parent: Any, ts: boolean): [number, number] | string {
	if (key === 'loc') {
		if (y.type === 'Program' && parent?.type === 'Script') return [parent.start, parent.end];
		if (ts && parent?.type === 'EachBlock' && parent.expression === y && parent.context && LOOSE(y)) return [y.start, parent.context.end];
		return [y.start, y.end];
	}
	if (y.type === 'Attribute') {
		const v = y.value;
		if (v?.type === 'ExpressionTag' && v.end !== y.end) return [v.expression.start, v.expression.end];
		return [y.start, y.start + y.name.length];
	}
	if (y.type.endsWith('Directive')) {
		const prefix = y.type === 'TransitionDirective' ? (y.intro && y.outro ? 'transition' : y.intro ? 'in' : 'out') : PREFIX[y.type];
		if (prefix === undefined) return 'no rule';
		return [y.start, y.start + `${prefix}:${[y.name, ...(y.modifiers ?? [])].join('|')}`.length];
	}
	if ('fragment' in y && typeof y.name === 'string') return [y.start + 1, y.start + 1 + y.name.length];
	return 'no rule';
}
// the first script with a `lang`, as Svelte's Parser finds it in the text
function typescript(root: Any) {
	const scripts = [root.instance, root.module].filter(Boolean).sort((p, q) => p.start - q.start);
	const lang = scripts.map((s) => s.attributes.find((a: Any) => a.type === 'Attribute' && a.name === 'lang')).find(Boolean);
	return Array.isArray(lang?.value) && lang.value[0]?.raw === 'ts';
}
const BREAKS = /\r(?!\n)|[\u2028\u2029]/;
const checked = { loc: 0, name_loc: 0 }, agreed = { loc: 0, name_loc: 0 };
interface Where {
	locate: Locate;
	text: string;
	ts: boolean;
}
function verify(key: 'loc' | 'name_loc', x: Any, y: Any, parent: Any, { locate, text, ts }: Where, at: string, path: string, out: Difference[]) {
	checked[key]++;
	const before = out.length;
	const derived = span(key, y, parent, ts);
	const loc = x[key];
	if (typeof derived === 'string' || !derived.every((n) => Number.isInteger(n) && n >= 0 && n <= text.length))
		out.push({ kind: `${at}.${key}: not derived (${typeof derived === 'string' ? derived : 'no span in teasel'})`, path: `${path}.${key}`, svelte: loc, teasel: y });
	else if (shape(loc) !== 'object' || shape(loc.start) !== 'object' || shape(loc.end) !== 'object') out.push({ kind: `${at}.${key}: not a location`, path: `${path}.${key}`, svelte: loc });
	else {
		const mine: Difference[] = [];
		for (const [i, end] of (['start', 'end'] as const).entries()) {
			const s = loc[end], offset = derived[i];
			const t: Record<string, number> = { ...locate(offset), ...('character' in s ? { character: offset } : {}) };
			for (const k of new Set([...Object.keys(s), ...Object.keys(t)])) {
				if (s[k] === t[k]) continue;
				const what = !(k in t) ? 'only in svelte' : !(k in s) ? 'only derived' : 'differs';
				mine.push({ kind: `${at}.${key}.${end}.${k}: ${what}`, path: `${path}.${key}.${end}.${k}`, svelte: s[k], teasel: t[k] });
			}
		}
		// where Svelte disagrees with itself, one kind a node type says so
		if (mine.length && BREAKS.test(text)) out.push({ kind: `${at}.${key}: differs in a text with a line break other than \\n`, path: `${path}.${key}`, svelte: loc, teasel: mine.map((d) => `${d.path} ${d.teasel}`) });
		else out.push(...mine);
	}
	if (out.length === before) agreed[key]++;
}

// a kind names the nearest node's type and the keys from it; the path keeps the indices to find the place again
function compare(a: Any, b: Any, locate: Locate, text: string): Difference[] {
	const out: Difference[] = [];
	const context: Where = { locate, text, ts: typescript(b) };
	(function walk(x: Any, y: Any, where: string, path: string, parent: Any) {
		if (x === y) return;
		const sx = shape(x), sy = shape(y);
		if (sx !== sy) return void out.push({ kind: `${where}: svelte ${sx}, teasel ${sy}`, path, svelte: x, teasel: y });
		if (sx === 'array') {
			if (x.length !== y.length) return void out.push({ kind: `${where}: list length`, path, svelte: x, teasel: y });
			for (let i = 0; i < x.length; i++) walk(x[i], y[i], where, `${path}[${i}]`, parent);
			return;
		}
		if (sx !== 'object') return void out.push({ kind: `${where}: different ${sx}`, path, svelte: x, teasel: y });
		if (x.type !== y.type) return void out.push({ kind: `${where}: type ${x.type} vs ${y.type}`, path, svelte: x, teasel: y });
		const node = typeof x.type === 'string';
		const at = node ? x.type : where;
		for (const k of new Set([...Object.keys(x), ...Object.keys(y)])) {
			if (!(k in x)) out.push({ kind: `${at}.${k}: only in teasel`, path: `${path}.${k}`, teasel: y[k] });
			else if (!(k in y) && node && (k === 'loc' || k === 'name_loc')) verify(k, x, y, parent, context, at, path, out);
			else if (!(k in y)) out.push({ kind: `${at}.${k}: only in svelte`, path: `${path}.${k}`, svelte: x[k] });
			else walk(x[k], y[k], `${at}.${k}`, `${path}.${k}`, node ? y : parent);
		}
	})(a, b, 'Root', '$', undefined);
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
	const a = attempt(() => svelte(source, { modern: true }), (tree) => tree);
	const b = attempt(() => parse(source, { sourceType: 'module', comments: true }, grammar), (answer) => answer.node);
	let found: Difference[];
	if ('code' in a) {
		rejected++;
		if ('code' in b) bothReject++;
		found = 'code' in b ? [] : [{ kind: `rejects: only svelte (${a.code})`, path: '$', teasel: b.tree }];
	} else if ('code' in b) found = [{ kind: `rejects: only teasel (${b.code})`, path: '$', svelte: a.tree }];
	else {
		compared++;
		const answer = b.value;
		found = compare(a.tree, b.tree, (offset) => answer.locate(offset), source);
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
console.log(`positions derived from teasel's spans and locate: ${agreed.loc} of ${checked.loc} loc and ${agreed.name_loc} of ${checked.name_loc} name_loc agree`);
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
