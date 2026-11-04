// node scripts/erase.ts --tsc TYPESCRIPT_DIR DIR...: every TypeScript conformance case under the directories, a
// case of several files split at its `// @filename:` lines, erased by teasel and emitted by TypeScript without
// `verbatimModuleSyntax`, the two compared as trees. Beyond erasure TypeScript drops an import no value reads,
// which teasel's references predict here, and a clause written empty, which teasel keeps and the comparison
// leaves out. A case either refuses, or one erasure keeps something in, is counted and passed over.
// `npm run build` first.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import { parse, referenceOf } from '../dist/index.js';

const args = process.argv.slice(2);
const at = args.indexOf('--tsc');
if (at < 0 || args.length < at + 3) {
	console.error('usage: node scripts/erase.ts --tsc TYPESCRIPT_DIR DIR...');
	process.exit(2);
}
const ts = createRequire(import.meta.url)(args[at + 1]);
const dirs = args.filter((_, i) => i !== at && i !== at + 1);

const DROP = new Set(['start', 'end', 'loc', 'range', 'raw', 'leadingComments', 'trailingComments', 'innerComments', 'comments', 'directive', 'bigint', 'regex', 'sourceType']);
const MEMBER = new Set(['MethodDefinition', 'PropertyDefinition']);
const empty_export = (n: any) => n.type === 'ExportNamedDeclaration' && !n.declaration && !n.source && !n.specifiers?.length;
const prologue = (n: any) => n.type === 'ExpressionStatement' && n.expression?.type === 'Literal' && n.expression.value === 'use strict';
/** The tree as TypeScript prints it: a quoted member name plain, an anonymous default export unnamed. */
function norm(node: any): any {
	if (Array.isArray(node)) return node.filter((n) => !(n && typeof n === 'object' && (empty_export(n) || prologue(n)))).map(norm);
	if (node === null || typeof node !== 'object') return node;
	if (node.type === 'Literal') {
		const v = node.value;
		return { type: 'Literal', value: typeof v === 'bigint' ? v.toString() + 'n' : v instanceof RegExp ? `/${v.source}/${v.flags}` : v };
	}
	if (MEMBER.has(node.type) && !node.computed && node.key?.type === 'Literal' && typeof node.key.value === 'string') {
		node = { ...node, key: { type: 'Identifier', name: node.key.value } };
	}
	if (node.type === 'ExportDefaultDeclaration' && /^default_\d+$/.test(node.declaration?.id?.name ?? '')) {
		node = { ...node, declaration: { ...node.declaration, id: null } };
	}
	const out: any = {};
	for (const key of Object.keys(node).sort()) {
		if (DROP.has(key)) continue;
		const v = node[key];
		if (v === false || v == null) continue;
		const nv = norm(v);
		if (Array.isArray(nv) && nv.length === 0) continue;
		out[key] = nv;
	}
	return out;
}
function first_diff(a: any, b: any, path = ''): string | null {
	if (a === b) return null;
	if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return `${path || '/'}: ${JSON.stringify(a)?.slice(0, 50)} vs ${JSON.stringify(b)?.slice(0, 50)}`;
	if (Array.isArray(a) !== Array.isArray(b)) return `${path}: ${Array.isArray(a) ? 'array' : a.type} vs ${Array.isArray(b) ? 'array' : b.type}`;
	if (Array.isArray(a)) {
		for (let i = 0; i < Math.max(a.length, b.length); i++) {
			if (i >= a.length || i >= b.length) return `${path}[${i}]: ${a[i]?.type ?? '(none)'} vs ${b[i]?.type ?? '(none)'}`;
			const d = first_diff(a[i], b[i], `${path}[${i}]`);
			if (d) return d;
		}
		return null;
	}
	if (a.type !== b.type) return `${path}: ${a.type} vs ${b.type}`;
	for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) {
		if (!(key in a) || !(key in b)) return `${path}.${key}: ${key in a ? (a[key]?.type ?? 'set') : '(none)'} vs ${key in b ? (b[key]?.type ?? 'set') : '(none)'}`;
		const d = first_diff(a[key], b[key], `${path}.${key}`);
		if (d) return d;
	}
	return null;
}

function* cases(dir: string): Generator<string> {
	for (const name of readdirSync(dir).sort()) {
		const path = join(dir, name);
		if (statSync(path).isDirectory()) yield* cases(path);
		else if (name.endsWith('.ts') && !name.endsWith('.d.ts')) yield path;
	}
}
function chunks(file: string): { name: string; src: string }[] {
	const src = readFileSync(file, 'utf8');
	const parts = src.split(/^(?=\/\/ ?@[Ff]ile[Nn]ame: )/m);
	if (parts.length === 1) return [{ name: '', src }];
	const out = [];
	for (const part of parts) {
		const m = part.match(/^\/\/ ?@[Ff]ile[Nn]ame: *(\S+)/);
		if (m && /\.(ts|mts|cts)$/.test(m[1]) && !/\.d\.[mc]?ts$/.test(m[1])) out.push({ name: m[1], src: part });
	}
	return out;
}

const WRITTEN_EMPTY = /^(import|export)\s*\{\s*\}/;
// a reference made from inside an ambient block counts as a read, where TypeScript sees none
const AMBIENT_BLOCK = /\bdeclare\s+(global|module)\b/;
/** teasel's erased tree, with what TypeScript drops beyond erasure taken out: an import no value reads, and a clause written empty. */
function erased(src: string): { body: any[]; written_empty: number } | { skipped: string } {
	if (AMBIENT_BLOCK.test(src)) return { skipped: 'an ambient block' };
	let answer;
	try {
		answer = parse(src, { sourceType: 'module', typescript: 'erase', scopes: true });
	} catch {
		return { skipped: 'teasel refuses' };
	}
	if (answer.typescript?.length) return { skipped: 'erasure keeps ' + answer.typescript[0].type };
	const read = new Set(answer.references!.map((reference) => reference.binding));
	let written_empty = 0;
	const body = [];
	for (const node of answer.node.body) {
		if ((node.type === 'ImportDeclaration' || node.type === 'ExportNamedDeclaration') && WRITTEN_EMPTY.test(src.slice(node.start, node.end))) {
			written_empty++;
			continue;
		}
		if (node.type === 'ImportDeclaration' && node.specifiers.length > 0) {
			const specifiers = node.specifiers.filter((specifier: any) => read.has(referenceOf(specifier.local) as any));
			if (specifiers.length === 0) continue;
			body.push({ ...node, specifiers });
			continue;
		}
		body.push(node);
	}
	return { body, written_empty };
}
function emitted(src: string): { body: any[] } | { skipped: string } {
	let result;
	try {
		result = ts.transpileModule(src, { fileName: 'case.ts', reportDiagnostics: true, compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext, isolatedModules: true, useDefineForClassFields: true, removeComments: true } });
	} catch {
		return { skipped: 'TypeScript throws' };
	}
	if (result.diagnostics?.length) return { skipped: 'TypeScript refuses' };
	try {
		return { body: parse(result.outputText, { sourceType: 'module' }).node.body };
	} catch {
		return { skipped: 'TypeScript emits what is not JavaScript' };
	}
}

let inputs = 0, compared = 0, written_empty = 0;
const skipped = new Map<string, number>();
const differ: string[] = [];
for (const dir of dirs) {
	for (const file of cases(dir)) {
		for (const { name, src } of chunks(file)) {
			inputs++;
			const label = name ? `${file}#${name}` : file;
			const t = erased(src);
			if ('skipped' in t) { skipped.set(t.skipped, (skipped.get(t.skipped) ?? 0) + 1); continue; }
			const c = emitted(src);
			if ('skipped' in c) { skipped.set(c.skipped, (skipped.get(c.skipped) ?? 0) + 1); continue; }
			compared++;
			written_empty += t.written_empty;
			const d = first_diff(norm(t.body), norm(c.body));
			if (d) differ.push(`${label}\n    ${d}`);
		}
	}
}
console.log(`${inputs} inputs, ${compared} compared, ${written_empty} clauses written empty left out`);
for (const [why, n] of [...skipped].sort((a, b) => b[1] - a[1])) console.log(`  ${String(n).padStart(5)} skipped: ${why}`);
console.log(`${differ.length} differ`);
for (const d of differ) console.log(d);
process.exit(differ.length ? 1 : 0);
