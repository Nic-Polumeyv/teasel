// `node scripts/test.ts interpret` runs the decoder without code generation, as a host forbidding it would
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import type { Entry, Options } from '../lib/api.js';
if (process.argv[2] === 'interpret') globalThis.Function = (() => { throw new EvalError('blocked'); }) as unknown as FunctionConstructor;
const node = await import('../node.js');
const wasm = await import('../wasm.js');
// the trees are poked as the stream shaped them, host nodes included, past what the types say
type Any = any;
const untyped = ({ Source, scopeOf, referenceOf, parentOf }: typeof node | typeof wasm) => ({
	open: (source: string, options?: Options): Any => new Source(source, options),
	scopeOf: (node: Any): Any => scopeOf(node),
	referenceOf: (node: Any): Any => referenceOf(node),
	parentOf: (node: Any): Any => parentOf(node),
});

for (const [name, m] of Object.entries({ node, wasm })) {
	const { open, scopeOf, referenceOf, parentOf } = untyped(m);
	const parse = (source: string, options?: Options): Any => open(source, options).parse();
	const program = (source: string, options?: Options): Any => parse(source, options).node;
	const at = (entry: Entry, source: string, offset: number, options?: Options, stopAt?: string[]): Any => open(source, options).parse(entry, offset, { stopAt });
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
	assert.throws(() => parse('class C { @dec #x = 1 }', { ...ts, decorators: 'legacy' }), (e: Any) => e.code === 'decorator_placement');
	assert.doesNotThrow(() => parse('class C { m(@dec p) {} }', { ...ts, decorators: 'legacy' }));
	assert.throws(() => parse('class C { m(@dec p) {} }', { ...ts, decorators: 'proposal' }), (e: Any) => e.code === 'decorator_placement');
	assert.doesNotThrow(() => parse('class C { @dec #x = 1 }', { ...ts, decorators: 'proposal' }));
	assert.doesNotThrow(() => parse('class C { @dec #x = 1 }', ts));
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
	assert.throws(() => at('expression', '{a}', 1, undefined, 'as' as unknown as string[]), TypeError);
	assert.throws(() => at('expression', '{a}', 1, undefined, ['a s']), TypeError);
	assert.throws(() => at('nonsense' as Entry, '{a}', 1), TypeError);

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

	assert.throws(() => parse('x = ;'), (e: Any) => e.code === 'unexpected_token' && e.pos === 4 && e.end === 5 && e.loc.column === 4 && e.message === 'Unexpected token' && e instanceof SyntaxError);
	assert.throws(() => parse('x = '), (e: Any) => e.code === 'unexpected_eof' && e.pos === 4 && e.end === 4);
	assert.throws(() => parse('/a', { locations: true }), (e: Any) => e.code === 'unterminated_regexp' && e.pos === 1);
	assert.throws(() => parse('x', { ranges: true } as Any), TypeError);
	assert.throws(() => parse('x', { ecmaVersion: 2020 } as Any), TypeError);
	assert.throws(() => parse('x', { preserveParens: true } as Any), TypeError);
	assert.throws(() => parse('return', { sourceType: 'module' }), SyntaxError);
	assert.equal(program('return', { allowReturnOutsideFunction: true }).body[0].type, 'ReturnStatement');
	{
		// a document's answer lists each piece of JavaScript the host read, with its share of the tables
		const host = readFileSync(new URL('../../crates/teasel/tests/hosts/svelte/host.grammar', import.meta.url), 'utf8');
		const answer = parse('<script>let a = 1;</script>{a + b}', { host, sourceType: 'module', scopes: true });
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
	assert.equal(source.parse('expression', 1).node.name, 'a');
	assert.equal(open('{xs as x}', ts).parse('expression', 1, { stopAt: ['as'] }).end, 3);
	assert.equal(source.parse('expression', 11).end, 20);
	assert.equal(source.parse('expression', 11).comments[0].loc.start.column, 13);
	assert.throws(() => source.parse('expression', 99), SyntaxError);
	assert.throws(() => open('𝒳 + y').parse('expression', 1), (e: Any) => /surrogate/.test(e.message));
	const erased = parse('import type T from "t"; export const x: T = (1 as any)!; enum E {}', { sourceType: 'module', typescript: 'erase' });
	assert.equal(erased.node.body.length, 2);
	assert.equal(erased.node.body[0].declaration.declarations[0].init.type, 'Literal');
	assert.equal('typeAnnotation' in erased.node.body[0].declaration.declarations[0].id, false);
	assert.deepEqual(erased.typescript.map((k: Any) => k.type), ['TSEnumDeclaration']);
	assert.throws(() => parse('let x: number = 1', { typescript: true, erase: true } as Any), TypeError);
	const template = open('<script>\n  let a = 1;\n</script>\n{a}', { sourceType: 'module', locations: true });
	const script = template.parse('program', 8, { end: 22 });
	assert.equal(script.node.start, 8);
	assert.equal(script.node.end, 22);
	assert.equal(script.end, 22);
	assert.equal(script.node.body[0].loc.start.line, 2);
	assert.throws(() => template.parse('program', 22, { end: 8 }), SyntaxError);
	assert.equal(template.parse('program', 24).node.body[0].type, 'ExpressionStatement');
	assert.equal(template.parse('expression', 33, { end: 34 }).node.name, 'a');
	assert.equal(program('"﻿a"; "bc"; zz').body[2].expression.name, 'zz');
	source[Symbol.dispose]();
	assert.throws(() => source.parse('expression', 1), TypeError);
	let escaped: Any;
	{
		using inner = open('x');
		escaped = inner;
		assert.equal(inner.parse('expression', 0).node.name, 'x');
	}
	assert.throws(() => escaped.parse('expression', 0), TypeError);
	assert.throws(() => parse('x', { locations: 1 } as Any), TypeError);
	assert.throws(() => parse('x', { typescript: 'yes' } as Any), TypeError);
	assert.throws(() => open('a;b;c').parse('program', 0, { end: -1 }), (e: Any) => e.code === 'invalid_request');
	assert.throws(() => open('a;b;c').parse('program', 0, { end: NaN }), (e: Any) => e.code === 'invalid_request');
	const wide = 'x;'.repeat(200000);
	assert.equal(program(wide).body.length, 200000);
	assert.equal(program('y;').body.length, 1);
	assert.equal(program(wide + wide).body.length, 400000);
	assert.equal(program('z;').body[0].expression.name, 'z');
	console.log(name, 'ok');
}

// a document of a host language: the host's nodes around the JavaScript ones, one tree
const grammar = readFileSync(new URL('../../crates/teasel/tests/hosts/svelte/host.grammar', import.meta.url), 'utf8');
for (const m of [node, wasm]) {
	const { open, scopeOf, referenceOf, parentOf } = untyped(m);
	const source = '<script lang="ts">\n\tlet items: string[] = [];\n</script>\n\n{#each items as item, i (item)}\n\t<p class:odd={i % 2} on:click={() => item}>{item}</p>\n{:else}\n\tnone\n{/each}\n';
	const doc = open(source, { host: grammar, sourceType: 'module', scopes: true, comments: true }).parse();
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
	assert.throws(() => open('x', { host: 'element div' }), /grammar line 1/);
	assert.throws(() => open('<div>', { host: grammar }).parse(), { code: 'unclosed', pos: 0 });
	// under recovery the tree is what could be read, the errors listed with it
	const loose: Any = open('<div>{#if }<Comp foo={bar}\n</div>', { host: grammar, errorRecovery: true }).parse();
	assert.deepEqual(loose.errors.map((e: Any) => e.code), ['unclosed', 'unexpected_token', 'expected']);
	const div = loose.node.fragment.nodes[0];
	assert.equal(div.end, 33);
	const block = div.fragment.nodes[0];
	assert.equal(block.test.name, '');
	assert.equal(block.consequent.nodes[0].name, 'Comp');
	assert.equal(block.consequent.nodes[0].end, 27);
}

// the answer follows the options the source was prepared with, and an entry is one of the names
for (const [label, m] of Object.entries({ node, wasm })) {
	const { open } = untyped(m);
	const options: Options = {};
	const source = open('x}', options);
	options.locations = true;
	assert.equal('loc' in source.parse('expression', 0).node, false, label);
	assert.throws(() => source.parse('toString' as Entry), TypeError, label);
}

// unfinished input under recovery is an answer, never a panic; a strict error is a SyntaxError
const grammars = Object.fromEntries(['svelte', 'vue'].map((name) => [name, readFileSync(new URL(`../../crates/teasel/tests/hosts/${name}/host.grammar`, import.meta.url), 'utf8')]));
for (const [label, m] of Object.entries({ node, wasm })) {
	const { open } = untyped(m);
	for (const text of ['<a x="', '<a /*', '<script>"</script>', '{#if', '<div class="{a']) {
		assert.equal(open(text, { host: grammars.svelte, errorRecovery: true, comments: true, scopes: true }).parse().node.type, 'Root', `${label} ${text}`);
	}
	assert.throws(() => open('<a @x="@"/>', { host: grammars.vue }).parse(), (e: Any) => e instanceof SyntaxError, `${label}`);
	const handler: Any = open('<button @click="let x = 1"/>', { host: grammars.vue, errorRecovery: true }).parse();
	assert.equal(handler.node.children[0].props[0].handler.type, 'Program', label);
	assert.deepEqual(handler.errors, [], label);
}
// a second host: the same walker, Vue's grammar
const vue = readFileSync(new URL('../../crates/teasel/tests/hosts/vue/host.grammar', import.meta.url), 'utf8');
for (const m of [node, wasm]) {
	const { open, parentOf } = untyped(m);
	const source = '<ul :class="{ on }">\n\t<li v-for="(item, i) in items" :key="item.id" @click.stop="select(item)">{{ item.name }} #{{ i }}</li>\n</ul>\n';
	const root: Any = open(source, { host: vue, sourceType: 'module' }).parse().node;
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
	assert.throws(() => open('<div v-for="x items">', { host: vue }).parse(), { code: 'expected', message: 'Expected in or of' });
}
