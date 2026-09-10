// Differential test of the scope facts: what every identifier of a component declares or refers
// to, by Svelte's own analysis versus teasel's, over every component of a Svelte checkout.
//
//   SVELTE_DIR=~/Projects/svelte bun scopes-host.js [--verbose] [--limit N] [filter]

import { readFileSync } from 'node:fs';
import { relative } from 'node:path';
import { args, capped, corpus, files, root } from './lib.js';
import { Source, bindingOf } from '../package/index.js';

// the parser itself, whose tree keeps the metadata the scope analysis reads
const { parse } = await import(`${root}/packages/svelte/src/compiler/phases/1-parse/index.js`);
const { ScopeRoot, create_scopes } = await import(`${root}/packages/svelte/src/compiler/phases/scope.js`);
const { verbose, limit, filter } = args();
const grammar = readFileSync(new URL('../hosts/svelte.grammar', import.meta.url), 'utf8');

const EMPTY_PROGRAM = () => ({ type: 'Program', sourceType: 'module', start: -1, end: -1, body: [] });

// Svelte's facts: every reference with what it resolves to, every declaration, by offset
function svelte(source) {
	const ast = parse(source);
	const scope_root = new ScopeRoot();
	const module = create_scopes(ast.module ? ast.module.content : EMPTY_PROGRAM(), scope_root, false, null);
	const instance = create_scopes(ast.instance ? ast.instance.content : EMPTY_PROGRAM(), scope_root, true, module.scope);
	const template = create_scopes(ast.fragment, scope_root, false, instance.scope);
	const depth = (scope) => (scope.parent ? depth(scope.parent) + 1 : 0);
	const refs = new Map();
	const decls = new Map();
	// the scopes of a component's slots live on its node rather than in the map
	const slot_scopes = [];
	(function walk(node) {
		if (Array.isArray(node)) return node.forEach(walk);
		if (!node || typeof node !== 'object') return;
		if (node.metadata?.scopes) slot_scopes.push(...Object.values(node.metadata.scopes));
		for (const [k, v] of Object.entries(node)) if (k !== 'metadata' && k !== 'loc') walk(v);
	})(ast.fragment);
	for (const { scope, scopes } of [module, instance, template]) {
		for (const s of new Set([scope, ...scopes.values(), ...slot_scopes])) {
			for (const binding of s.declarations.values()) {
				if (typeof binding.node.start === 'number') decls.set(binding.node.start, { name: binding.node.name, kind: binding.kind });
			}
			for (const [name, references] of s.references) {
				for (const { node } of references) {
					if (typeof node.start !== 'number') continue;
					const known = refs.get(node.start);
					if (known && known.depth >= depth(s)) continue;
					const binding = s.get(name);
					refs.set(node.start, { name, depth: depth(s), to: binding === null ? null : typeof binding.node.start === 'number' ? binding.node.start : 'synthetic' });
				}
			}
		}
	}
	return { refs, decls };
}

// teasel's facts, the same way
function teasel(source) {
	const tree = new Source(source, { host: grammar, sourceType: 'module', scopes: true }).parse().node;
	const refs = new Map();
	const decls = new Map();
	(function walk(node) {
		if (Array.isArray(node)) return node.forEach(walk);
		if (!node || typeof node !== 'object') return;
		if (node.type === 'Identifier') {
			const binding = bindingOf(node);
			if (binding === null) refs.set(node.start, { name: node.name, to: null });
			else if (binding !== undefined) {
				if (binding.node === node) decls.set(node.start, { name: node.name, kind: binding.kind });
				else refs.set(node.start, { name: node.name, to: binding.node ? binding.node.start : 'no node' });
			}
		}
		for (const [k, v] of Object.entries(node)) if (k !== 'loc') walk(v);
	})(tree);
	return { refs, decls };
}

const jobs = [];
for (const path of files(corpus, /\.svelte$/)) {
	const name = relative(corpus, path);
	if (filter && !name.includes(filter)) continue;
	if (name.includes('_actual') || name.includes('/output.svelte')) continue;
	jobs.push({ name, source: readFileSync(path, 'utf8') });
	if (capped(jobs, limit)) break;
}

const stats = { components: 0, skipped: 0, refs_agree: 0, refs_differ: 0, refs_missing: 0, decls_agree: 0, decls_missing: 0 };
const buckets = new Map();
function note(key, example) {
	if (!buckets.has(key)) buckets.set(key, { count: 0, examples: [] });
	const bucket = buckets.get(key);
	bucket.count++;
	if (bucket.examples.length < 3) bucket.examples.push(example);
}
const around = (source, at) => JSON.stringify(source.slice(Math.max(0, at - 30), at + 30));
for (const job of jobs) {
	let expected, actual;
	try {
		expected = svelte(job.source);
		actual = teasel(job.source);
	} catch (e) {
		stats.skipped++;
		if (stats.skipped <= 3) console.error(`${job.name}: ${e.stack?.split('\n').slice(0, 4).join(' | ') ?? e}`);
		continue;
	}
	stats.components++;
	for (const [start, ref] of expected.refs) {
		// Svelte lists the declaring identifiers among the references; `$` names are its stores and runes
		if (ref.name.startsWith('$') || expected.decls.has(start)) continue;
		const ours = actual.refs.get(start);
		if (ours === undefined) {
			stats.refs_missing++;
			const asDecl = actual.decls.get(start);
			note(asDecl ? `reference is a declaration for teasel` : ref.to === null ? `svelte reads a global where teasel sees no value name` : `reference has no fact in teasel`, `${job.name}@${start} ${ref.name}: ${around(job.source, start)}`);
		} else if (ref.to === 'synthetic' ? ours.to === null : ours.to !== ref.to && !(typeof ref.to === 'number' && typeof ours.to === 'number' && ours.to > ref.to && ours.to <= ref.to + 4 + ref.name.length)) {
			stats.refs_differ++;
			note(`reference resolves elsewhere: svelte ${ref.to === null ? 'global' : ref.to === 'synthetic' ? 'a synthetic binding' : 'a declaration'}, teasel ${ours.to === null ? 'global' : ours.to === 'no node' ? 'a binding without a node' : 'a declaration'}`, `${job.name}@${start} ${ref.name}: ${around(job.source, start)}`);
		} else stats.refs_agree++;
	}
	for (const [start, decl] of expected.decls) {
		if (decl.name.startsWith('$')) continue;
		// `let:x` without a value: Svelte makes the identifier up at the directive, teasel has it at the name
		const shifted = decl.kind === 'template' && [...actual.decls.entries()].some(([at, d]) => d.name === decl.name && at > start && at <= start + 4 + decl.name.length);
		if (actual.decls.has(start) || shifted) stats.decls_agree++;
		else {
			stats.decls_missing++;
			note(`declaration missing in teasel (svelte kind ${decl.kind})`, `${job.name}@${start} ${decl.name}: ${around(job.source, start)}`);
		}
	}
}
console.log(`${stats.components} components compared, ${stats.skipped} skipped`);
for (const [k, v] of Object.entries(stats)) if (k !== 'components' && k !== 'skipped') console.log(`  ${k.padEnd(14)} ${v}`);
const total = stats.refs_agree + stats.refs_differ + stats.refs_missing + stats.decls_agree + stats.decls_missing;
console.log(`  agreement      ${((100 * (stats.refs_agree + stats.decls_agree)) / total).toFixed(2)}%`);
for (const [key, { count, examples }] of [...buckets.entries()].sort((a, b) => b[1].count - a[1].count)) {
	console.log(`${count}  ${key}`);
	for (const example of examples.slice(0, verbose ? 3 : 2)) console.log('    ', example);
}
