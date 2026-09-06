// Differential test for scope analysis: eslint-scope over acorn's tree versus teasel, for every
// identifier the declaration it resolves to and whether it writes, and every declaration.
//
//   SVELTE_DIR=~/Projects/svelte bun scopes.js [--verbose] [--limit N] [filter]

import { readFileSync } from 'node:fs';
import { relative } from 'node:path';
import * as acorn from 'acorn';
import * as eslintScope from 'eslint-scope';
import { args, compare, corpus, files, teasel } from './lib.js';

const { verbose, limit, filter } = args();
const script_re = /<script((?:\s+(?:"[^"]*"|'[^']*'|[^>"'])*)?)>([\s\S]*?)<\/script>/g;

const jobs = [];
for (const path of files(corpus, /\.(svelte|js)$/)) {
	const name = relative(corpus, path);
	if (filter && !name.includes(filter)) continue;
	const text = readFileSync(path, 'utf8');
	if (path.endsWith('.js')) jobs.push({ name, source: text, mode: 'module+scopes' });
	else {
		for (const match of text.matchAll(script_re)) {
			if (/lang=["']?ts/.test(match[1] ?? '')) continue;
			jobs.push({ name: `${name}#${match.index}`, source: match[2], mode: 'module+scopes' });
		}
	}
	if (jobs.length >= limit) {
		jobs.length = limit;
		break;
	}
}

/// What both sides reduce to: references as [start, declaration start | null | 'implicit', write]
/// in source order, and declarations as their starts. eslint-scope reports `{ c = 1 } = o` twice,
/// for the key and the value; one identifier is one reference here.
function summary(references, declarations) {
	references.sort((a, b) => a[0] - b[0]);
	const once = [];
	for (const reference of references) {
		const last = once.at(-1);
		if (last && last[0] === reference[0]) last[2] |= reference[2];
		else once.push(reference);
	}
	declarations.sort((a, b) => a - b);
	return { references: once, declarations };
}

function expected(job) {
	let ast;
	try {
		ast = acorn.parse(job.source, { ecmaVersion: 16, sourceType: 'module', ranges: true });
	} catch (e) {
		return { error: { message: e.message.replace(/ \(\d+:\d+\)$/, ''), pos: e.pos, loc: { line: e.loc.line, column: e.loc.column } } };
	}
	const manager = eslintScope.analyze(ast, { ecmaVersion: 2022, sourceType: 'module' });
	const references = [];
	const declarations = new Set();
	for (const scope of manager.scopes) {
		for (const variable of scope.variables) for (const def of variable.defs) declarations.add(def.name.start);
	}
	for (const scope of manager.scopes) {
		for (const ref of scope.references) {
			const start = ref.identifier.start;
			if (declarations.has(start)) continue;
			const resolved = ref.resolved;
			const declaration = resolved === null ? null : resolved.defs.length === 0 ? 'implicit' : resolved.defs[0].name.start;
			references.push([start, declaration, ref.isWrite() ? 1 : 0]);
		}
	}
	return summary(references, [...declarations]);
}

function actual(line) {
	const answer = JSON.parse(line);
	if (answer.error) return line;
	// a name declared twice has one binding and two declaring identifiers; the first is its position
	const declared = new Map();
	const declarations = [];
	const references = [];
	const walk = (node) => {
		if (!node || typeof node !== 'object') return;
		if (Array.isArray(node)) return node.forEach(walk);
		if (node.type === 'Identifier') {
			if (node.declares !== undefined) {
				declarations.push(node.start);
				if (!declared.has(node.declares)) declared.set(node.declares, node.start);
			} else if (node.binding !== undefined) references.push([node.start, node.binding, node.write ? 1 : 0]);
		}
		for (const key in node) if (key !== 'loc' && key !== 'scopes' && key !== 'bindings') walk(node[key]);
	};
	walk(answer.body);
	for (const ref of references) {
		if (ref[1] === null) continue;
		const binding = answer.bindings[ref[1]];
		ref[1] = binding.kind === 'arguments' ? 'implicit' : declared.get(ref[1]);
	}
	return JSON.stringify(summary(references, declarations));
}

const lines = (await teasel(jobs)).map((line, i) => (jobs[i] ? actual(line) : line));
process.exit(compare(jobs, expected, lines, { verbose, label: 'scope analysis' }) ? 0 : 1);
