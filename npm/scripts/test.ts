// `node scripts/test.ts interpret` runs the decoder without code generation, as a host forbidding it would
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import svelte from './hosts/svelte.ts';
import vue from './hosts/vue.ts';
import type { Expression, Identifier, Pattern } from 'estree';
import * as g from '../src/grammar.ts';
import type { Infer, NodeType } from '../src/grammar.ts';
import type * as api from '../src/index.ts';
import type { Options } from '../src/index.ts';
if (process.argv[2] === 'interpret') globalThis.Function = (() => { throw new EvalError('blocked'); }) as unknown as FunctionConstructor;
const name = process.execArgv.includes('--no-addons') ? 'wasm' : 'native';
const m = await import('../src/index.ts');
// the trees are poked as the recipes shape them, host nodes included, past what the types say
type Any = any;
const { Entry, ParseError } = m;
const untyped = ({ Source, scopeOf, referenceOf, parentOf }: typeof m) => ({
	open: (source: string, options?: Options): Any => new Source(source, options),
	scopeOf: (node: Any): Any => scopeOf(node),
	referenceOf: (node: Any): Any => referenceOf(node),
	parentOf: (node: Any): Any => parentOf(node),
});
const { open, scopeOf, referenceOf, parentOf } = untyped(m);

{
	const parse = (source: string, options?: Options): Any => open(source, options).parse();
	const program = (source: string, options?: Options): Any => parse(source, options).node;
	const at = (entry: Exclude<keyof typeof Entry, 'prototype'>, source: string, offset: number, options?: Options, stopAt?: string[]): Any => open(source, options).parse(stopAt === undefined ? Entry[entry] : Entry[entry].until(...stopAt), offset);
	const typed = parse('let x: number = 1; // done', { sourceType: 'module', typescript: true, comments: true, locations: true });
	assert.equal(typed.node.sourceType, 'module');
	assert.equal(typed.end, 26);
	assert.equal(typed.node.body[0].declarations[0].id.typeAnnotation.typeAnnotation.type, 'TSNumberKeyword');
	assert.equal(typed.node.body[0].trailingComments[0].value, ' done');
	assert.equal(typed.node.body[0].loc.end.column, 18);
	assert.equal(typed.comments.length, 1);
	assert.equal(typed.comments[0].loc.start.column, 19);
	assert.deepEqual(Object.keys(parse('x')), ['node', 'end']);
	assert.equal(program('with (a) {}').body[0].type, 'WithStatement');
	assert.equal('loc' in program('x'), false);

	const expression = at('expression', '{a + b}', 1);
	assert.equal(expression.node.type, 'BinaryExpression');
	assert.equal(expression.node.end, 6);
	assert.equal(expression.end, 6);

	const parens = at('expression', '{(a) /* c */ } // d', 1, { comments: true });
	assert.equal(parens.node.type, 'Identifier');
	assert.equal(parens.end, 12);
	assert.deepEqual(parens.comments.map((c: Any) => c.value), [' c ']);
	assert.equal(parens.node.trailingComments[0].start, 5);
	assert.equal(at('statement', '{@const x = 1}', 2).end, 13);
	const ts = { typescript: true };
	assert.doesNotThrow(() => parse('class C { @dec #x = 1; @dec declare y: number; m(@dec p) {} }', ts));
	assert.doesNotThrow(() => parse('const D = @dec class { @dec #x = 1 }', ts));
	assert.throws(() => parse('x', { decorators: 'legacy' } as Any), TypeError);
	assert.doesNotThrow(() => parse('class C { m(@dec p) {} }', ts));
	assert.equal(at('expression', '{items as item}', 1, ts).node.type, 'TSAsExpression');
	assert.equal(at('expression', '{items as item}', 1, ts, ['as']).end, 6);
	assert.equal(at('expression', '{f(x as T) as item}', 1, ts, ['as']).end, 10);
	assert.equal(at('expression', '{(xs as T) as item}', 1, ts, ['as']).end, 10);
	assert.equal(at('expression', '{xs as T[] as item}', 1, ts, ['as']).end, 10);
	assert.equal(at('expression', '{xs as T[] as item, i}', 1, ts, ['as', ',']).end, 10);
	assert.equal(at('expression', '{p.then(f) then r}', 1, undefined, ['then', 'catch']).end, 10);
	assert.equal(at('expression', '{xs as [a, b = 1]}', 1, ts, ['as']).end, 3);
	assert.equal(at('expression', '{f<A, B>(), i}', 1, ts, ['as', ',']).end, 10);
	assert.throws(() => at('expression', 'éé𝒳x', 3), (e: Any) => e.message === 'offset 3 is inside a surrogate pair');
	assert.equal(at('expression', '{obj. as item}', 1, undefined, ['as']).end, 8);
	assert.equal(at('expression', '{x. then y}', 1, ts).end, 8);
	assert.equal(at('expression', '{items, i}', 1, undefined, ['as', ',']).end, 6);
	assert.equal(at('expression', '{f(a, b), i}', 1, undefined, [',']).end, 8);
	assert.equal(at('expression', '{`${a}` as b}', 1, undefined, ['as']).end, 7);
	assert.equal(at('expression', '{{a:1} />', 1, undefined, ['/>']).end, 6);
	assert.equal(at('expression', '{x />', 1, undefined, ['/>']).end, 2);
	assert.equal(at('pattern', '{[a, b], i}', 1, undefined, [',']).end, 7);
	assert.throws(() => Entry.expression.until('a s'), TypeError);
	assert.throws(() => Entry.expression.until(), TypeError);
	assert.throws(() => open('{a}').parse('expression' as Any, 1), TypeError);
	assert.throws(() => open('{a}').parse(Entry.expression, '1' as Any), TypeError);
	assert.throws(() => open('{a}').parse(Entry.expression, [1] as Any), TypeError);
	assert.equal(Entry.expression.until('as').until(',').constructor, Entry);

	const loose = { errorRecovery: true };
	const recovered = at('expression', '{obj.}', 1, loose, ['}']);
	assert.equal(recovered.node.type, 'MemberExpression');
	assert.deepEqual(JSON.parse(JSON.stringify(recovered.node.property)), { type: 'Identifier', start: 5, end: 5, name: '' });
	assert.equal(recovered.end, 5);
	assert.deepEqual(recovered.errors, [{ code: 'unexpected_token', message: 'Unexpected token', pos: 5, end: 6, loc: { line: 1, column: 5 } }]);
	const unclosed = at('expression', '{f(a, }', 1, loose, ['}']);
	assert.deepEqual(JSON.parse(JSON.stringify(unclosed.node)), { type: 'Identifier', start: 6, end: 6, name: '' });
	assert.equal(unclosed.end, 6);
	const declaration = at('statement', '{let }', 1, { sourceType: 'module', errorRecovery: true }, ['}']);
	assert.equal(declaration.node.type, 'VariableDeclaration');
	assert.deepEqual(JSON.parse(JSON.stringify(declaration.node.declarations[0].id)), { type: 'Identifier', start: 5, end: 5, name: '' });
	assert.deepEqual(at('expression', '{a b}', 1, loose, ['}']).errors, []);
	const broken = parse('x = "abc\ny = ', { errorRecovery: true, locations: true });
	assert.deepEqual(broken.errors.map((e: Any) => [e.code, e.pos, e.loc.line]), [['unterminated_string', 4, 1], ['unexpected_eof', 13, 2]]);
	assert.deepEqual(parse('x', loose).errors, []);
	assert.equal('errors' in parse('x'), false);
	const scoped = parse('let = f(a, b)', { errorRecovery: true, scopes: true, sourceType: 'module' });
	assert.equal(scoped.bindings.length, 0);
	assert.equal(referenceOf(scoped.node.body[0].declarations[0].id), undefined);
	assert.equal(referenceOf(scoped.node.body[0].declarations[0].init.callee).binding, null);

	const generics = at('typeParameters', 'foo<T extends () => void>(x: T)', 3, ts);
	assert.equal(generics.node.type, 'TSTypeParameterDeclaration');
	assert.equal(generics.end, 25);
	assert.equal(at('typeParameters', "foo<T = '>'>()", 3, ts).end, 12);
	assert.throws(() => at('typeParameters', 'foo<T>()', 3), (e: Any) => e.code === 'not_typescript');
	const marked = { parenthesized: true };
	assert.equal(at('expression', '{(a, b)}', 1, marked).node.parenthesized, true);
	assert.equal(at('expression', '{((a))}', 1, marked).node.parenthesized, true);
	assert.equal(at('expression', '{f((a))}', 1, marked).node.arguments[0].parenthesized, true);
	assert.equal('parenthesized' in at('expression', '{(a) => a}', 1, marked).node, false);
	assert.equal('parenthesized' in at('expression', '{(a, b)}', 1).node, false);
	const params = at('params', '(a, b = 1) => a', 0);
	assert.equal(params.node.length, 2);
	assert.equal(params.end, 10);

	assert.throws(() => parse('x = ;'), (e: Any) => e.code === 'unexpected_token' && e.pos === 4 && e.end === 5 && e.loc.column === 4 && e.message === 'Unexpected token' && e instanceof ParseError && e instanceof SyntaxError);
	assert.ok(!('loc' in new ParseError({ message: 'Expected', code: 'expected', pos: 0, end: 0 })));
	assert.throws(() => parse('x = '), (e: Any) => e.code === 'unexpected_eof' && e.pos === 4 && e.end === 4);
	assert.throws(() => parse('/a', { locations: true }), (e: Any) => e.code === 'unterminated_regexp' && e.pos === 1);
	assert.throws(() => parse('x', { ranges: true } as Any), TypeError);
	assert.throws(() => parse('x', { ecmaVersion: 2020 } as Any), TypeError);
	assert.throws(() => parse('x', { preserveParens: true } as Any), TypeError);
	assert.throws(() => parse('return', { sourceType: 'module' }), SyntaxError);
	assert.equal(program('return', { allowReturnOutsideFunction: true }).body[0].type, 'ReturnStatement');
	assert.equal(program('await').body[0].expression.name, 'await');
	assert.equal(program('await x', { allowAwaitOutsideFunction: true }).body[0].expression.type, 'AwaitExpression');
	assert.throws(() => parse('super.x'), SyntaxError);
	assert.equal(program('super.x', { allowSuperOutsideMethod: true }).body[0].expression.object.type, 'Super');
	assert.throws(() => parse('export { x }', { sourceType: 'module' }), SyntaxError);
	assert.equal(program('export { x }', { sourceType: 'module', allowUndeclaredExports: true }).body[0].type, 'ExportNamedDeclaration');
	{
		// a document's answer lists each piece of JavaScript the host read, with its share of the tables
		const answer = open('<script>let a = 1;</script>{a + b}', { sourceType: 'module', scopes: true }).parse(svelte);
		const [script, expression] = answer.roots;
		assert.equal(answer.roots.length, 2);
		assert.equal(script.node.type, 'Program');
		assert.equal(script.scope, scopeOf(script.node));
		assert.deepEqual(script.bindings.map((b: Any) => b.name), ['a']);
		assert.equal(expression.node.type, 'BinaryExpression');
		assert.equal(expression.scope.kind, 'fragment');
		assert.deepEqual(expression.scopes, []);
		assert.deepEqual(expression.references.map((r: Any) => r.node.name), ['a', 'b']);
		assert.equal(expression.references[0].binding, script.bindings[0]);
		assert.equal(referenceOf(expression.node.left), expression.references[0]);
	}
	{
		const answer = parse('let x = 1; function f(y) { x = y; }', { sourceType: 'module', scopes: true });
		const tree = answer.node;
		const [x, f, y] = answer.bindings;
		assert.equal(scopeOf(tree), answer.scopes[0]);
		assert.equal(x.node, tree.body[0].declarations[0].id);
		const [written] = answer.references.filter((r: Any) => r.binding === x);
		assert.equal(written.node.start, 27);
		// the binding is the reference its declaring identifier makes
		assert.equal(referenceOf(x.node), x);
		assert.deepEqual([x.binding, x.declares, x.write, x.read, x.mutate], [x, true, true, false, false]);
		assert.equal(x.writeExpr, tree.body[0].declarations[0].init);
		assert.deepEqual([written.declares, written.write], [false, true]);
		assert.equal(referenceOf(written.node).binding, x);
		assert.equal(f.scope.kind, 'module');
		assert.equal(y.scope.node, tree.body[1]);
		assert.deepEqual(Object.keys(answer.scopes[0]), ['kind', 'parent', 'topLevelAwait', 'node']);
		assert.deepEqual(Object.keys(x), ['name', 'kind', 'scope', 'write', 'node', 'declaration', 'binding', 'declares', 'read', 'mutate', 'writeExpr']);
		assert.deepEqual(answer.bindings.map((b: Any) => b.name), ['x', 'f', 'y']);
		assert.equal(x.declaration, tree.body[0].declarations[0]);
		assert.equal(f.declaration, tree.body[1]);
		assert.equal(y.declaration, tree.body[1]);
		const reads = answer.references.filter((r: Any) => r.binding === y);
		assert.deepEqual([y.write, y.writeExpr], [true, null]);
		assert.equal(written.writeExpr, reads[0].node);
		assert.equal(answer.scopes[0].topLevelAwait, false);
		const top = parse('g = await 1; g++;', { sourceType: 'module', scopes: true });
		assert.equal(top.scopes[0].topLevelAwait, true);
		const [assigned, updated] = top.node.body.map((s: Any) => referenceOf(s.expression.left ?? s.expression.argument));
		assert.equal(assigned.writeExpr, top.node.body[0].expression.right);
		assert.equal(updated.writeExpr, null);
		assert.equal(assigned.binding, null);
		assert.deepEqual([assigned.read, updated.read, written.read, reads[0].read], [false, true, false, true]);
		assert.equal(assigned.scope, top.scopes[0]);
		assert.deepEqual(top.references, [assigned, updated]);
		assert.equal(parentOf(assigned.node), top.node.body[0].expression);
		assert.equal(parentOf(top.node.body[0]), top.node);
		assert.equal(parentOf(top.node), undefined);
		assert.equal(parentOf(program('`x${1}`').body[0].expression.quasis[0].value), undefined);
		const literal = program('let r = /a/g, t = `x${1}y`;', { scopes: true }).body[0].declarations;
		assert.equal(Object.getPrototypeOf(literal[0].init.regex), Object.prototype);
		assert.deepEqual(literal[1].init.quasis[0].value, { raw: 'x', cooked: 'x' });
		assert.equal('references' in literal[0].init.regex, false);
		assert.equal('scope' in tree, false);
		assert.equal('binding' in x.node, false);
		const fragment = at('expression', 'a + b', 0, { scopes: true });
		assert.equal(referenceOf(fragment.node.left).binding, null);
		assert.equal(referenceOf(fragment.node), undefined);
		assert.equal(fragment.scopes[0].kind, 'fragment');
		const bare = at('expression', '{count}', 1, { scopes: true });
		assert.equal(referenceOf(bare.node).binding, null);
		assert.equal(scopeOf(bare.node).kind, 'fragment');
		assert.doesNotThrow(() => JSON.stringify(tree.body));
		assert.deepEqual(Object.keys(x.node), ['type', 'start', 'end', 'name']);
		assert.equal(scopeOf(tree.body[1]).kind, 'function');
		const mutated = parse('let o = {}; o.x = 1; g = 2; h.k = 3;', { sourceType: 'module', scopes: true });
		const [o] = mutated.bindings;
		const [of_o] = mutated.references.filter((r: Any) => r.binding === o);
		assert.deepEqual([o.write, o.writeExpr], [true, mutated.node.body[0].declarations[0].init]);
		assert.equal(of_o.mutate, true);
		assert.equal(of_o.write, false);
		const g = mutated.node.body[2].expression.left;
		assert.equal(referenceOf(g).binding, null);
		assert.deepEqual(referenceOf(g), { scope: mutated.scopes[0], binding: null, write: true, read: false, mutate: false, declares: false, node: g, writeExpr: mutated.node.body[2].expression.right });
		assert.equal(referenceOf(mutated.node.body[3].expression.left.object).mutate, true);
		assert.equal(referenceOf(o.node), o);
		// declared again: a reference that writes, the binding staying the first declaration
		const twice = parse('var x; var x = 1; function x() {}', { scopes: true });
		assert.equal(twice.bindings.length, 1);
		assert.deepEqual([twice.bindings[0].write, twice.bindings[0].writeExpr], [false, null]);
		assert.deepEqual(twice.references.map((r: Any) => [r.node.start, r.declares, r.write, r.writeExpr?.start ?? null, r.binding]), [[11, true, true, 15, twice.bindings[0]], [27, true, true, null, twice.bindings[0]]]);
		assert.equal(referenceOf(twice.node.body[1].declarations[0].id).binding, twice.bindings[0]);
		assert.equal(referenceOf(null), undefined);
		assert.equal(referenceOf(at('pattern', '[a, b]', 0, { scopes: true }).node.elements[0]).binding.kind, 'pattern');
		const list = at('params', '(a, b)', 0, { scopes: true });
		assert.equal(referenceOf(list.node[1]).binding.kind, 'param');
		assert.equal(list.scopes[0].node, null);
	}
	assert.throws(() => parse('x', { sourceType: 'nonsense' } as Any), TypeError);
	assert.throws(() => at('expression', '𝒳 + y', 1), (e: Any) => /surrogate/.test(e.message) && e instanceof SyntaxError);
	assert.throws(() => at('expression', 'a + b', -1), SyntaxError);
	assert.throws(() => at('expression', 'a + b', 99), SyntaxError);

	const unicode = at('expression', '"é" + x', 6).node;
	assert.equal(unicode.type, 'Identifier');
	assert.equal(unicode.start, 6);
	const source = open('{a} {"é"} {b /* c */}', { locations: true, comments: true });
	assert.equal(source.parse(Entry.expression, 1).node.name, 'a');
	assert.equal(open('{xs as x}', ts).parse(Entry.expression.until('as'), 1).end, 3);
	assert.equal(source.parse(Entry.expression, 11).end, 20);
	assert.equal(source.parse(Entry.expression, 11).comments[0].loc.start.column, 13);
	assert.throws(() => source.parse(Entry.expression, 99), SyntaxError);
	assert.throws(() => open('𝒳 + y').parse(Entry.expression, 1), (e: Any) => /surrogate/.test(e.message));
	const erased = parse('import type T from "t"; export const x: T = (1 as any)!; enum E {}', { sourceType: 'module', typescript: 'erase' });
	assert.equal(erased.node.body.length, 2);
	assert.equal(erased.node.body[0].declaration.declarations[0].init.type, 'Literal');
	assert.equal('typeAnnotation' in erased.node.body[0].declaration.declarations[0].id, false);
	assert.deepEqual(erased.typescript.map((k: Any) => k.type), ['TSEnumDeclaration']);
	assert.throws(() => parse('let x: number = 1', { typescript: true, erase: true } as Any), TypeError);
	const template = open('<script>\n  let a = 1;\n</script>\n{a}', { sourceType: 'module', locations: true });
	const script = template.parse(Entry.program, [8, 22]);
	assert.equal(script.node.start, 8);
	assert.equal(script.node.end, 22);
	assert.equal(script.end, 22);
	assert.equal(script.node.body[0].loc.start.line, 2);
	assert.throws(() => template.parse(Entry.program, [22, 8]), SyntaxError);
	assert.equal(template.parse(Entry.program, 24).node.body[0].type, 'ExpressionStatement');
	assert.equal(template.parse(Entry.expression, [33, 34]).node.name, 'a');
	assert.equal(program('"﻿a"; "bc"; zz').body[2].expression.name, 'zz');
	source[Symbol.dispose]();
	assert.throws(() => source.parse(Entry.expression, 1), TypeError);
	let escaped: Any;
	{
		using inner = open('x');
		escaped = inner;
		assert.equal(inner.parse(Entry.expression, 0).node.name, 'x');
	}
	assert.throws(() => escaped.parse(Entry.expression, 0), TypeError);
	assert.throws(() => parse('x', { locations: 1 } as Any), TypeError);
	assert.throws(() => parse('x', { typescript: 'yes' } as Any), TypeError);
	assert.throws(() => open('a;b;c').parse(Entry.program, [0, -1]), (e: Any) => e.code === 'invalid_request');
	assert.throws(() => open('a;b;c').parse(Entry.program, [0, NaN]), (e: Any) => e.code === 'invalid_request');
	const wide = 'x;'.repeat(200000);
	assert.equal(program(wide).body.length, 200000);
	assert.equal(program('y;').body.length, 1);
	assert.equal(program(wide + wide).body.length, 400000);
	assert.equal(program('z;').body[0].expression.name, 'z');
	// a source that grows the engine's memory while the tree's buffers stay where they are
	assert.equal(program(`/*${'c'.repeat(1 << 25)}*/ w;`).body[0].expression.name, 'w');
	assert.equal(program('v;').body[0].expression.name, 'v');
	console.log(name, 'ok');
}

// a document of a host language: the host's nodes around the JavaScript ones, one tree
{
	const source = '<script lang="ts">\n\tlet items: string[] = [];\n</script>\n\n{#each items as item, i (item)}\n\t<p class:odd={i % 2} on:click={() => item}>{item}</p>\n{:else}\n\tnone\n{/each}\n';
	const doc = open(source, { sourceType: 'module', scopes: true, comments: true }).parse(svelte);
	const root = doc.node;
	assert.equal(root.type, 'Root');
	assert.equal(root.end, source.length);
	assert.equal(doc.end, source.length);
	assert.deepEqual(doc.comments, []);
	assert.equal(root.instance.context, 'default');
	assert.equal(root.instance.content.body[0].declarations[0].id.typeAnnotation.type, 'TSTypeAnnotation');
	assert.equal('module' in root, false);
	const each = root.fragment.nodes.find((n: Any) => n.type === 'EachBlock');
	assert.equal(each.index.name, 'i');
	assert.equal(each.context.name, 'item');
	assert.equal(each.key.name, 'item');
	assert.equal(each.fallback.nodes[0].data, '\n\tnone\n');
	const p = each.body.nodes[1];
	assert.equal(p.type, 'RegularElement');
	assert.deepEqual(p.attributes.map((a: Any) => a.type), ['ClassDirective', 'OnDirective']);
	assert.equal(p.attributes[0].expression.type, 'BinaryExpression');
	assert.equal(parentOf(p.attributes[0]), p);
	assert.equal(parentOf(each.context), each);
	// the block declares its context and index; the script declares the list
	const tag = p.fragment.nodes[0];
	assert.equal(tag.type, 'ExpressionTag');
	assert.equal(referenceOf(tag.expression).binding.node, each.context);
	assert.equal(referenceOf(p.attributes[0].expression.left).binding.name, 'i');
	assert.equal(referenceOf(each.expression).binding.kind, 'let');
	// every fragment is a scope of its own; the block's body declares its context and index
	assert.equal(scopeOf(each.body).node, each.body);
	assert.equal(referenceOf(tag.expression).binding.scope, scopeOf(each.body));
	assert.equal(scopeOf(each.body).parent, scopeOf(root.fragment));
	// the template sees the instance script, which sees the module script, which is the root's
	assert.equal(scopeOf(root.fragment).parent, scopeOf(root.instance.content));
	assert.equal(scopeOf(root.instance.content).parent, scopeOf(root));
	assert.equal(scopeOf(each.fallback).parent, scopeOf(root.fragment));
	assert.throws(() => open('<div>').parse('host x' as Any), TypeError);
	assert.throws(() => open('<div>').parse({ wire: 'host x' } as Any), TypeError);
	assert.throws(() => open('<div>').parse(svelte), { code: 'unclosed', pos: 0 });
	// under recovery the tree is what could be read, the errors listed with it
	const loose: Any = open('<div>{#if }<Comp foo={bar}\n</div>', { errorRecovery: true }).parse(svelte);
	assert.deepEqual(loose.errors.map((e: Any) => e.code), ['unclosed', 'unexpected_token', 'expected']);
	const div = loose.node.fragment.nodes[0];
	assert.equal(div.end, 33);
	const block = div.fragment.nodes[0];
	assert.equal(block.test.name, '');
	assert.equal(block.consequent.nodes[0].name, 'Comp');
	assert.equal(block.consequent.nodes[0].end, 27);
}

// the answer follows the options the source was prepared with
{
	const options: Options = {};
	const source = open('x}', options);
	options.locations = true;
	assert.equal('loc' in source.parse(Entry.expression, 0).node, false, name);
}

// unfinished input under recovery is an answer, never a panic; a strict error is a SyntaxError
{
	for (const text of ['<a x="', '<a /*', '<script>"</script>', '{#if', '<div class="{a']) {
		assert.equal(open(text, { errorRecovery: true, comments: true, scopes: true }).parse(svelte).node.type, 'Root', `${name} ${text}`);
	}
	assert.throws(() => open('<a @x="@"/>').parse(vue), (e: Any) => e instanceof SyntaxError, `${name}`);
	const handler: Any = open('<button @click="let x = 1"/>', { errorRecovery: true }).parse(vue);
	assert.equal(handler.node.children[0].props[0].handler.type, 'Program', name);
	assert.deepEqual(handler.errors, [], name);
}
// the grammar the Rust tests read is the typed definition on its wire, pinned beside its documents
for (const [host, definition] of [['svelte', svelte], ['vue', vue]] as const) {
	const pin = new URL(`../../crates/teasel/tests/hosts/${host}/host.wire`, import.meta.url);
	if (process.env.UPDATE) writeFileSync(pin, definition.wire);
	else assert.ok(readFileSync(pin).equals(definition.wire), `${name} ${host}/host.wire changed; run with UPDATE=1 once the change is meant`);
}
// every node's parent is the node it sits in, objects without a type passed through
{
	const wrong: string[] = [];
	const walk = (holder: Any, value: Any) => {
		if (value === null || typeof value !== 'object') return;
		if (Array.isArray(value)) return value.forEach((item) => walk(holder, item));
		const node = typeof value.type === 'string';
		if (node && parentOf(value) !== holder) wrong.push(`${value.type} at ${value.start}`);
		for (const key of Object.keys(value)) walk(node ? value : holder, value[key]);
	};
	const decoder = readFileSync(new URL('../src/decode.ts', import.meta.url), 'utf8');
	for (const typescript of [true, 'erase'] as const) walk(undefined, open(decoder, { sourceType: 'module', typescript, comments: true, scopes: true }).parse().node);
	for (const [host, grammar] of [['svelte', svelte], ['vue', vue]] as const) {
		const dir = new URL(`../../crates/teasel/tests/hosts/${host}/`, import.meta.url);
		for (const file of readdirSync(dir).filter((f) => !/\.(json|wire)$/.test(f))) {
			walk(undefined, open(readFileSync(new URL(file, dir), 'utf8'), { sourceType: 'module', comments: true, scopes: true, errorRecovery: true }).parse(grammar).node);
		}
	}
	assert.deepEqual(wrong, [], `${name} parents`);
}

// a second host: the same walker, Vue's grammar
{
	const source = '<ul :class="{ on }">\n\t<li v-for="(item, i) in items" :key="item.id" @click.stop="select(item)">{{ item.name }} #{{ i }}</li>\n</ul>\n';
	const root: Any = open(source, { sourceType: 'module' }).parse(vue).node;
	assert.equal(root.type, 'Root');
	const ul = root.children[0];
	assert.equal(ul.tag, 'ul');
	assert.deepEqual(ul.props[0], { ...ul.props[0], type: 'Directive', name: 'bind', rawName: ':class', arg: 'class', modifiers: [] });
	assert.equal(ul.props[0].exp.type, 'ObjectExpression');
	const li = ul.children[1];
	const [each, key, click] = li.props;
	assert.equal(each.name, 'for');
	assert.equal(each.value.name, 'item');
	assert.equal(each.key.name, 'i');
	assert.equal(each.source.name, 'items');
	assert.equal(key.exp.type, 'MemberExpression');
	assert.deepEqual(click.modifiers, ['stop']);
	assert.equal(click.handler.type, 'CallExpression');
	assert.equal(li.children[0].type, 'Interpolation');
	assert.equal(li.children[0].content.property.name, 'name');
	assert.equal(li.children[1].content, ' #');
	assert.equal(parentOf(li.children[2].content), li.children[2]);
	assert.throws(() => open('<div v-for="x items">').parse(vue), { code: 'expected', message: 'Expected in or of' });
}

// a lone surrogate escape is the code unit it names, as JavaScript keeps it
{
	const values = (source: string): string[] => open(source, { sourceType: 'module' }).parse().node.body.map((s: Any) => s.expression.value ?? s.expression.quasis.map((q: Any) => q.value.cooked).join('|'));
	assert.deepEqual(values("'\\ud83d'; '\\ude00x\\u{dbff}'; '\\ufffd'; '\\ud83d\\ude00'; `a\\ud83d${b}\\udc00`;"), ['\ud83d', '\ude00x\udbff', '�', '😀', 'a\ud83d|\udc00']);
	assert.deepEqual(values("'\\ud83d'; '\\ufffd'; 'é\\udc00'; '\\ud83d';"), ['\ud83d', '�', 'é\udc00', '\ud83d']);
	assert.deepEqual(values("'\\ud800😀\\ud800é\\udc00 \\udc00'; 'a\\ud800';"), ['\ud800😀\ud800é\udc00 \udc00', 'a\ud800']);
	assert.throws(() => open("export { x as '\\ud800' };", { sourceType: 'module' }).parse(), { code: 'lone_surrogate_in_module_name', pos: 14 });
	assert.throws(() => open("import { '\\udc00' as y } from 'm';", { sourceType: 'module' }).parse(), { code: 'lone_surrogate_in_module_name', pos: 9 });
}

// a tree too deep for a walk is a parse error on every backend, and the engine reads the next source
{
	const deep = { code: 'nesting_depth' };
	for (const scopes of [false, true]) {
		assert.throws(() => open('a' + '.b'.repeat(9_999), { scopes }).parse(), deep);
		assert.throws(() => open('new '.repeat(9_999) + 'x', { scopes }).parse(), deep);
		assert.throws(() => open('type A = ' + 'B<'.repeat(999) + 'C' + '>'.repeat(999), { scopes, typescript: true }).parse(), deep);
		assert.throws(() => open('<a>'.repeat(40_000) + '</a>'.repeat(40_000), { scopes }).parse(svelte), deep);
		assert.equal(open('x', { scopes }).parse().node.body.length, 1);
	}
}

// the tests read the source; the published build must answer the same once its specifiers are rewritten
{
	const built = await import('../dist/index.js');
	await import('../dist/grammar.js');
	using source = new built.Source('let x = 1');
	assert.equal(source.parse().node.body.length, 1, `${name} dist`);
}

// /writing-a-grammar builds a grammar in steps; what each step answers for the page's template is pinned beside the page
{
	const template = '<ul>\n\t{{#repeat item, i in items by item.id}}\n\t\t<Card title={{ item.name }} index={{ i }} />\n\t{{:empty}}\n\t\t<li>No items</li>\n\t{{/repeat}}\n</ul>\n';
	const html = {
		document: g.node('Template', { children: g.content }),
		text: g.node('Text', { data: g.text.data }),
		comment: g.node('Comment', { data: g.text.data }),
		delimiters: ['{{', '}}'] as const,
		elements: {
			fields: { name: g.element.tag, attributes: g.element.attributes, children: g.content },
			component: g.element(g.node('Component')),
			other: g.element(g.node('Element')),
		},
	};
	const expressions = { ...html, attributes: { expressions: true } as const, expression: g.node('Expression', { expression: g.js.expression }) };
	const sigils = { open: '#', branch: ':', close: '/', tag: '@' };
	const item = { item: g.bind(g.js.pattern) };
	const index = g.opt(',', { index: g.optional(g.bind(g.js.identifier)) });
	const list = { list: g.js.expression };
	const key = g.opt('by', { key: g.optional(g.js.expression) });
	const repeat = (head: Any[], branches?: Any): Any => ({ ...expressions, sigils: { ...sigils, blocks: { repeat: g.block(g.node('RepeatBlock', ...head, { body: g.content }), branches) } } });
	const steps: Any[] = [
		html,
		expressions,
		{ ...expressions, sigils: { ...sigils, blocks: {} } },
		repeat([item, 'in', list]),
		repeat([item, index, 'in', list]),
		repeat([item, index, 'in', list, key]),
		repeat([item, index, 'in', list, key], { branches: { empty: [{ fallback: g.optional(g.content) }] } }),
	];
	// an identifier that refers to a binding declared elsewhere carries where that declaration is
	const shape = (value: Any): Any => {
		if (Array.isArray(value)) return value.map(shape);
		if (value === null || typeof value !== 'object') return value;
		const out: Any = {};
		for (const field of Object.keys(value)) if (field !== 'loc') out[field] = shape(value[field]);
		const binding = value.type === 'Identifier' ? referenceOf(value)?.binding : undefined;
		if (binding && binding.node !== value) out.refers = [binding.node.start, binding.node.end];
		return out;
	};
	const answers = steps.map((definition) => {
		try {
			return { tree: shape(open(template, { scopes: true }).parse(g.grammar('tpl', definition)).node) };
		} catch (e) {
			const { code, message, pos, end } = e as Any;
			return { error: { code, message, pos, end } };
		}
	});
	const pin = new URL('../../docs/content/03-examples/writing-a-grammar.json', import.meta.url);
	const pinned = `${JSON.stringify({ text: template, steps: answers }, null, '\t')}\n`;
	if (process.env.UPDATE) writeFileSync(pin, pinned);
	else assert.equal(readFileSync(pin, 'utf8'), pinned, `${name} writing-a-grammar.json changed; run with UPDATE=1 once the change is meant`);
	// the page's whole grammar is the last step's
	const page = readFileSync(new URL('../../docs/content/03-examples/03-writing-a-grammar.md', import.meta.url), 'utf8');
	const whole = [...page.matchAll(/```js tpl\.js\n(import \* as g[\s\S]*?)```/g)].at(-1)![1];
	const builders = JSON.stringify(new URL('../dist/grammar.js', import.meta.url).href);
	const written = await import(`data:text/javascript,${encodeURIComponent(whole.replace("'@teasel/parser/grammar'", builders))}`);
	assert.deepEqual(written.tpl.wire, g.grammar('tpl', steps.at(-1)).wire, `${name} the whole grammar on /writing-a-grammar is the last step's`);
}

// tsc checks what the types promise here and node never calls it, since the refused definitions throw at runtime
function types(source: api.Source, definition: typeof svelte) {
	type Equal<A, B> = (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
	const expect = <T extends true>() => {};
	type Required<T> = { [K in keyof T]-?: {} extends Pick<T, K> ? never : K }[keyof T];
	type Svelte = typeof svelte;
	type Fragment = NodeType<Svelte, 'Fragment'>;

	type Each = NodeType<Svelte, 'EachBlock'>;
	expect<Equal<Required<Each>, 'type' | 'start' | 'end' | 'expression' | 'context' | 'body'>>();
	expect<Equal<Each['context'], Pattern | null>>();
	expect<Equal<Each['index'], Identifier | undefined>>();
	expect<Equal<Each['key'], Expression | undefined>>();
	expect<Equal<Each['body'], Fragment>>();
	expect<Equal<Each['fallback'], Fragment | undefined>>();

	expect<Equal<NodeType<Svelte, 'IfBlock'>['alternate'], Fragment | null>>();
	expect<Equal<NodeType<Svelte, 'IfBlock'>['elseif'], boolean>>();
	expect<Equal<NodeType<Svelte, 'AwaitBlock'>['pending'], Fragment | null>>();
	expect<Equal<NodeType<Svelte, 'SnippetBlock'>['typeParams'], string | undefined>>();
	expect<Equal<keyof Fragment, 'type' | 'nodes'>>();
	expect<Equal<NodeType<Svelte, 'BindDirective'>['expression'], Expression>>();
	expect<Equal<NodeType<Svelte, 'OnDirective'>['expression'], Expression | null>>();
	expect<Equal<NodeType<Svelte, 'TransitionDirective'>['intro'], boolean>>();
	expect<Equal<NodeType<Svelte, 'StyleDirective'>['value'], NodeType<Svelte, 'Attribute'>['value']>>();
	expect<Equal<Infer<Svelte>['instance'], NodeType<Svelte, 'Script'> | undefined>>();
	expect<Equal<Infer<Svelte>['fragment'], Fragment>>();

	type Vue = typeof vue;
	const f = {} as Extract<NodeType<Vue, 'Directive'>, { source: unknown }>;
	expect<Equal<typeof f.source, Expression | null>>();
	expect<Equal<typeof f.value, Pattern | undefined>>();
	expect<Equal<typeof f.arg, string | Expression | null>>();
	expect<Equal<Infer<Vue>['children'][number]['type'], 'Element' | 'Text' | 'Comment' | 'Interpolation' | 'Slot' | 'Template' | 'Component'>>();

	// @ts-expect-error a field may not be named type
	g.node('X', { type: g.js.expression });
	// @ts-expect-error a field may not take a group's name
	g.node('X', { scope: g.js.expression });
	// @ts-expect-error only what reads a pattern, an identifier or parameters can declare
	g.bind(g.js.expression);
	// @ts-expect-error a directive's value is null when missing, never left out
	g.optional(g.value.expression);
	// @ts-expect-error an argument stands in only for a directive's value
	g.orArg(g.js.expression);
	// @ts-expect-error a tag has no body
	g.tag(g.node('T', { body: g.content }));
	// @ts-expect-error a tag opens no scope to declare in
	g.tag(g.node('T', { name: g.bind(g.js.identifier) }));
	// @ts-expect-error only a block declares around itself
	g.directive(g.node('D', { name: g.bind.outside(g.js.identifier) }));
	g.grammar('x', {
		document: g.node('Root', { children: g.content }),
		text: g.node('Text', { data: g.text.data }),
		comment: g.node('Comment', { data: g.text.data }),
		delimiters: ['{', '}'],
		elements: {
			fields: { name: g.element.tag, attributes: g.element.attributes, children: g.content },
			// @ts-expect-error inside names an element rule
			rules: { head: g.element(g.node('Head')), title: g.element(g.node('Title'), { inside: 'haed' }) },
		},
	});

	const doc = source.parse(definition);
	expect<Equal<typeof doc.node, Infer<Svelte>>>();
	expect<Equal<ReturnType<typeof source.parse<Infer<Svelte>>>['node'], Infer<Svelte>>>();
	// @ts-expect-error a grammar reads the whole source
	source.parse(definition, 1);
}
