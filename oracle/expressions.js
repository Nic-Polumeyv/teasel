// Differential test over every template expression in a Svelte checkout, parsed the way the
// Svelte compiler does it: acorn.parseExpressionAt from the expression's offset in the whole file.
//
//   SVELTE_DIR=~/Projects/svelte bun expressions.js [--verbose] [--limit N] [filter]

import { acorn_expression, args, capped, compare, components, teasel } from './lib.js';

const { verbose, limit, filter } = args();

const estree = /^(Identifier|Literal|[A-Za-z]+Expression|SpreadElement|RestElement|TemplateElement|ObjectPattern|ArrayPattern|AssignmentPattern|TemplateLiteral|Super|MetaProperty|Property|PrivateIdentifier|VariableDeclaration)$/;

// Svelte parses these through a wrapper (patterns, snippet parameters, @const declarations) or
// synthesizes them (shorthand directives, string tags), not through parseExpressionAt.
function skipped(node, parent, key, source) {
	if (key === 'context' || key === 'parameters' || key === 'declaration') return true;
	if (parent.type === 'AwaitBlock' && (key === 'value' || key === 'error')) return true;
	if (parent.type === 'SnippetBlock' && key === 'expression') return true;
	if (parent.type === 'SvelteElement' && key === 'tag' && node.type === 'Literal') return true;
	if (parent.type.endsWith('Directive') && !source.slice(parent.start, parent.end).includes('{')) return true;
	return false;
}

function* roots(node, key, parent, parent_is_estree, source) {
	if (Array.isArray(node)) {
		for (const item of node) yield* roots(item, key, parent, parent_is_estree, source);
		return;
	}
	if (!node || typeof node !== 'object') return;
	const is_estree = typeof node.type === 'string' && estree.test(node.type);
	if (is_estree && !parent_is_estree) {
		if (!skipped(node, parent, key, source)) yield node;
		return;
	}
	for (const [k, v] of Object.entries(node)) {
		if (k === 'loc' || k === 'metadata') continue;
		yield* roots(v, k, node, is_estree, source);
	}
}

const jobs = [];
const { each, stats } = await components(filter);
for (const { name, source, ast, ts, byte } of each) {
	for (const node of roots(ast.fragment, 'fragment', ast, false, source)) {
		jobs.push({ name: `${name}@${node.start}`, source, mode: `${ts ? 'ts-' : ''}expr+parenthesized:${byte(node.start)}`, offset: node.start, ts });
	}
	if (capped(jobs, limit)) break;
}

const lines = await teasel(jobs);
process.exit(compare(jobs, (job) => acorn_expression(job.source, job.offset, job.ts), lines, { verbose, skipped: stats.skipped }) ? 0 : 1);
