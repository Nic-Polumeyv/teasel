// Differential test for scope analysis: eslint-scope over acorn's tree versus teasel, for every
// identifier the declaration it resolves to and whether it writes, and every declaration.
//
//   SVELTE_DIR=~/Projects/svelte bun scopes.js [--verbose] [--limit N] [filter]

import { readFileSync } from 'node:fs';
import { relative } from 'node:path';
import * as acorn from 'acorn';
import * as eslintScope from 'eslint-scope';
import { acorn_error, args, capped, compare, corpus, files, scripts, teasel } from './lib.js';

const { verbose, limit, filter } = args();

const jobs = [];
for (const path of files(corpus, /\.(svelte|js)$/)) {
	const name = relative(corpus, path);
	if (filter && !name.includes(filter)) continue;
	const text = readFileSync(path, 'utf8');
	if (path.endsWith('.js')) jobs.push({ name, source: text, mode: 'module+scopes' });
	else for (const { index, source } of scripts(text, false)) jobs.push({ name: `${name}#${index}`, source, mode: 'module+scopes' });
	if (capped(jobs, limit)) break;
}

/// What both sides reduce to: references as [start, declaration start | null | 'implicit', write,
/// writeExpr start | null] in source order, and declarations as [first declaring identifier start,
/// declaration node start]. eslint-scope reports `{ c = 1 } = o` twice, for the key and the value,
/// and a default in a pattern as a second write with the default as its expression; one identifier
/// is one reference here, assigned the whole right side, which comes last in the source.
function summary(references, declarations) {
	references.sort((a, b) => a[0] - b[0]);
	const once = [];
	for (const reference of references) {
		const last = once.at(-1);
		if (last && last[0] === reference[0]) {
			last[2] |= reference[2];
			if (reference[3] !== null && (last[3] === null || reference[3] > last[3])) last[3] = reference[3];
		} else once.push(reference);
	}
	declarations.sort((a, b) => a[0] - b[0]);
	return { references: once, declarations };
}

function expected(job) {
	let ast;
	try {
		ast = acorn.parse(job.source, { ecmaVersion: 16, sourceType: 'module', ranges: true });
	} catch (e) {
		return acorn_error(e, job.source, false);
	}
	const manager = eslintScope.analyze(ast, { ecmaVersion: 2022, sourceType: 'module' });
	const references = [];
	const declared = new Set();
	const declarations = [];
	// a class name is two variables to eslint-scope, outside and inside the class; one identifier is one binding here
	for (const scope of manager.scopes) {
		for (const variable of scope.variables) {
			const first = variable.defs[0];
			if (first && !declared.has(first.name.start)) declarations.push([first.name.start, first.node.start]);
			for (const def of variable.defs) declared.add(def.name.start);
		}
	}
	for (const scope of manager.scopes) {
		for (const ref of scope.references) {
			const start = ref.identifier.start;
			if (declared.has(start)) continue;
			const resolved = ref.resolved;
			const declaration = resolved === null ? null : resolved.defs.length === 0 ? 'implicit' : resolved.defs[0].name.start;
			references.push([start, declaration, ref.isWrite() ? 1 : 0, ref.isWrite() && ref.writeExpr ? ref.writeExpr.start : null]);
		}
	}
	return summary(references, declarations);
}

function actual(line) {
	const answer = JSON.parse(line);
	if (answer.error) return line;
	// a name declared twice has one binding and two declaring identifiers; the first is its position
	const declared = new Map();
	const declaration_of = new Map();
	const references = [];
	const walk = (node) => {
		if (!node || typeof node !== 'object') return;
		if (Array.isArray(node)) return node.forEach(walk);
		if (node.defines !== undefined) for (const b of node.defines) declaration_of.set(b, node.start);
		// a write names its references by their number in emission order, which is this walk's
		if (node.writes !== undefined) for (const i of node.writes) references[i][3] = node.start;
		if (node.type === 'Identifier') {
			if (node.declares !== undefined) {
				if (!declared.has(node.declares)) declared.set(node.declares, node.start);
			} else if (node.binding !== undefined) references.push([node.start, node.binding, node.write ? 1 : 0, null]);
		}
		for (const key in node) if (key !== 'loc' && key !== 'scopes' && key !== 'bindings') walk(node[key]);
	};
	walk(answer.body);
	const declarations = [...declared].map(([b, start]) => [start, declaration_of.get(b) ?? null]);
	for (const ref of references) {
		if (ref[1] === null) continue;
		const binding = answer.bindings[ref[1]];
		ref[1] = binding.kind === 'arguments' ? 'implicit' : declared.get(ref[1]);
	}
	return JSON.stringify(summary(references, declarations));
}

const lines = (await teasel(jobs)).map(actual);
process.exit(compare(jobs, expected, lines, { verbose, label: 'scope analysis' }) ? 0 : 1);
