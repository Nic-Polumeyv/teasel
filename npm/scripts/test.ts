// `node scripts/test.ts [web] [interpret]`: `web` takes the browser loader, which no Node condition reaches; `interpret` runs the decoder without code generation, as a host forbidding it would
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { registerHooks } from 'node:module';
import type { Expression, Identifier, Pattern, Program, VariableDeclaration } from 'estree';
import type { AST, Options } from '../src/index.ts';
import type { Addon } from '../src/engine/addon.ts';
if (process.argv.includes('interpret')) globalThis.Function = (() => { throw new EvalError('blocked'); }) as unknown as FunctionConstructor;
const name = process.argv.includes('web') ? 'web' : process.execArgv.includes('--no-addons') ? 'wasm' : 'native';
// a native run that fell back to WebAssembly would pass as native, and --no-addons takes WebAssembly without a warning
process.on('warning', (warning) => {
	if ((warning as { code?: string }).code === 'wasm_fallback') throw warning;
});
// the hook must be in place before anything imports the engine, and the Function swap before anything imports the decoder, so those come in below
if (name === 'web') registerHooks({ resolve: (specifier, context, next) => (specifier === '#engine' ? { url: new URL('../dist/engine/web.js', import.meta.url).href, shortCircuit: true } : next(specifier, context)) });
const { generate } = await import('./children.ts');
const { load } = await import('../dist/engine/addon.js');
const m = await import('../src/index.ts');
// the trees are poked as the recipes shape them, past what the types say
type Any = any;
// a component, JavaScript and CSS, each by its own function
const read: (text: string, options?: Any) => Any = m.parse;
const js: (text: string, options?: Any) => Any = m.parseModule;
const css: (text: string, options?: Any) => Any = m.parseCss;
const tables = await import('../src/children.ts');
const { scopeOf, referenceOf, parentOf }: Record<'scopeOf' | 'referenceOf' | 'parentOf', (node: Any) => Any> = m;
// a component's script tag that makes it TypeScript
const TS = '<script lang="ts"></script>';
// the last node of a component's fragment
const last = (source: string, options?: Options): Any => read(source, options).node.fragment.nodes.at(-1);
const text = (source: string, node: Any) => source.slice(node.start, node.end);

{
	const parse = (source: string, options?: Options): Any => js(source, options);
	const program = (source: string, options?: Options): Any => parse(source, options).node;
	const typed = parse('let x: number = 1; // done', { typescript: true, comments: true });
	assert.equal(typed.node.sourceType, 'module');
	assert.equal(typed.node.body[0].declarations[0].id.typeAnnotation.typeAnnotation.type, 'TSNumberKeyword');
	assert.equal(typed.node.body[0].trailingComments[0].value, ' done');
	assert.equal(typed.comments.length, 1);
	assert.deepEqual(Object.keys(typed), ['node', 'comments']);
	assert.deepEqual(typed.locate(26), { line: 1, column: 26 });
	assert.equal('loc' in typed.node.body[0] || 'loc' in typed.comments[0], false);
	assert.deepEqual(Object.keys(parse('x')), ['node']);
	assert.equal(read('x').node.type, 'Root');
	assert.equal(read('x').node.fragment.nodes[0].data, 'x');

	// where a component's expression ends: before the block's own words, `as`, `then`, `,`, outside every bracket it opened
	const each = (source: string): Any => [text(source, last(source).expression), last(source).context && text(source, last(source).context)];
	assert.deepEqual(each(`${TS}{#each items as item}{/each}`), ['items', 'item']);
	assert.deepEqual(each(`${TS}{#each f(x as T) as item}{/each}`), ['f(x as T)', 'item']);
	assert.deepEqual(each(`${TS}{#each (xs as T) as item}{/each}`), ['xs as T', 'item']);
	// a TypeScript `as` is the block's unless another `as` follows the assertion
	assert.deepEqual(each(`${TS}{#each xs as T[] as item}{/each}`), ['xs as T[]', 'item']);
	assert.deepEqual(each(`${TS}{#each xs as T[] as item, i}{/each}`), ['xs as T[]', 'item']);
	assert.deepEqual(each(`${TS}{#each xs as [a, b = 1]}{/each}`), ['xs', '[a, b = 1]']);
	assert.deepEqual(each(`${TS}{#each f<A, B>(), i}{/each}`), ['f<A, B>()', null]);
	assert.deepEqual(each('{#each items, i}{/each}'), ['items', null]);
	assert.deepEqual(each('{#each f(a, b), i}{/each}'), ['f(a, b)', null]);
	assert.deepEqual(each('{#each `${a}` as b}{/each}'), ['`${a}`', 'b']);
	assert.deepEqual(each('{#each xs as [a, b], i}{/each}'), ['xs', '[a, b]']);
	assert.equal(text('{#await p.then(f) then r}{/await}', last('{#await p.then(f) then r}{/await}').expression), 'p.then(f)');
	// after `.` a word is a property name
	assert.deepEqual(each(`${TS}{#each x. then as y}{/each}`), ['x. then', 'y']);
	assert.throws(() => read('{#each obj. as item}{/each}'), (e: Any) => e.code === 'expected' && e.pos === 15);

	const loose = { errorRecovery: true };
	const recovered = read('{obj.}', loose);
	const member = recovered.node.fragment.nodes[0].expression;
	assert.equal(member.type, 'MemberExpression');
	assert.deepEqual(JSON.parse(JSON.stringify(member.property)), { type: 'Identifier', start: 5, end: 5, name: '' });
	assert.deepEqual(recovered.errors, [{ code: 'unexpected_token', args: [], pos: 5, end: 6, line: 1, column: 5 }]);
	const call = read('{f(a, }', loose).node.fragment.nodes[0].expression;
	assert.equal(text('{f(a, }', call), 'f(a, ');
	const declaration = read('{let }', { errorRecovery: true }).node.fragment.nodes[0].declaration;
	assert.equal(declaration.type, 'VariableDeclaration');
	assert.deepEqual(JSON.parse(JSON.stringify(declaration.declarations[0].id)), { type: 'Identifier', start: 5, end: 5, name: '' });
	const broken = parse('x = "abc\ny = ', { errorRecovery: true });
	assert.deepEqual(broken.errors.map((e: Any) => [e.code, e.pos, e.line]), [['unterminated_string', 4, 1], ['unexpected_eof', 13, 2]]);
	assert.deepEqual(parse('x', loose).errors, []);
	assert.equal('errors' in parse('x'), false);
	const codes = (source: string, options: Options) => parse(source, { ...options, errorRecovery: true }).errors.map((e: Any) => [e.code, ...e.args]);
	assert.deepEqual(codes('let a; let a; /a{2,1}/u; eval = 1;', {}), [
		['redeclaration', 'a'],
		['regexp_quantifier_order'],
		['strict_binding', 'eval'],
	]);
	assert.deepEqual(codes('class C { static public x }', { typescript: true }), [['modifier_order', 'public', 'static'], ['unexpected_token']]);
	const scoped = parse('let = f(a, b)', { errorRecovery: true, scopes: true });
	assert.equal(scoped.bindings.length, 0);
	assert.equal(referenceOf(scoped.node.body[0].declarations[0].id), undefined);
	assert.equal(referenceOf(scoped.node.body[0].declarations[0].init.callee).binding, null);

	// a snippet's type parameters, TypeScript only, and its parameters
	const snippet = (source: string): Any => last(source);
	assert.equal(snippet(`${TS}{#snippet foo<T extends () => void>(x: T)}{/snippet}`).typeParams, 'T extends () => void');
	assert.equal(snippet(`${TS}{#snippet foo<T = '>'>()}{/snippet}`).typeParams, "T = '>'");
	assert.throws(() => read('{#snippet foo<T>()}{/snippet}'), (e: Any) => e.code === 'not_typescript');
	assert.equal(snippet('{#snippet s(a, b = 1)}{/snippet}').parameters.length, 2);
	const marked = { parenthesized: true };
	const tag = (source: string, options?: Options): Any => read(source, options).node.fragment.nodes[0].expression;
	assert.equal(tag('{(a, b)}', marked).parenthesized, true);
	assert.equal(tag('{((a))}', marked).parenthesized, true);
	assert.equal(tag('{f((a))}', marked).arguments[0].parenthesized, true);
	assert.equal('parenthesized' in tag('{(a) => a}', marked), false);
	assert.equal('parenthesized' in tag('{(a, b)}'), false);

	assert.throws(() => parse('x = ;'), (e: Any) => e.code === 'unexpected_token' && e.pos === 4 && e.end === 5 && e.column === 4 && e.message === 'unexpected_token' && e.args.length === 0 && Object.getPrototypeOf(e) === SyntaxError.prototype);
	assert.throws(() => parse('x = '), (e: Any) => e.code === 'unexpected_eof' && e.pos === 4 && e.end === 4);
	assert.throws(() => parse('/a'), (e: Any) => e.code === 'unterminated_regexp' && e.pos === 1);
	assert.throws(() => parse('x', { ranges: true } as Any), TypeError);
	assert.throws(() => parse('x', { ecmaVersion: 2020 } as Any), TypeError);
	assert.throws(() => parse('x', { preserveParens: true } as Any), TypeError);
	assert.throws(() => parse('return'), SyntaxError);
	assert.throws(() => parse('super.x'), SyntaxError);
	assert.throws(() => parse('export { x }'), SyntaxError);
	// a text is always a module, and the engine's other dials are not the package's to offer
	for (const option of ['sourceType', 'allowReturnOutsideFunction', 'allowAwaitOutsideFunction', 'allowSuperOutsideMethod', 'allowUndeclaredExports']) {
		assert.throws(() => parse('x', { [option]: true } as Any), TypeError);
		assert.throws(() => read('x', { [option]: true } as Any), TypeError);
	}
	assert.throws(() => css('a {}', { comments: true } as Any), TypeError);
	const ts = { typescript: true };
	assert.doesNotThrow(() => parse('class C { @dec #x = 1; @dec declare y: number; m(@dec p) {} }', ts));
	assert.doesNotThrow(() => parse('const D = @dec class { @dec #x = 1 }', ts));
	assert.throws(() => parse('x', { decorators: 'legacy' } as Any), TypeError);
	assert.doesNotThrow(() => parse('class C { m(@dec p) {} }', ts));
	{
		// a component's answer lists each piece of JavaScript it holds, with its share of the tables
		const answer = read('<script>let a = 1;</script>{a + b}', { scopes: true });
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
		const answer = parse('let x = 1; function f(y) { x = y; }', { scopes: true });
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
		const top = parse('g = await 1; g++;', { scopes: true });
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
		const commented = parse('/* a */ x', { comments: true });
		assert.equal(commented.comments[0], commented.node.body[0].leadingComments[0]);
		assert.equal(parentOf(commented.comments[0]), commented.node.body[0]);
		assert.equal(parentOf(program('`x${1}`').body[0].expression.quasis[0].value), undefined);
		const literal = program('let r = /a/g, t = `x${1}y`;', { scopes: true }).body[0].declarations;
		assert.equal(Object.getPrototypeOf(literal[0].init.regex), Object.prototype);
		assert.deepEqual([literal[0].init.value instanceof RegExp, String(literal[0].init.value)], [true, '/a/g']);
		const big = program('0x1_Fn').body[0].expression;
		assert.deepEqual([big.value, big.raw, big.bigint], [31n, '0x1_Fn', '31']);
		assert.deepEqual(literal[1].init.quasis[0].value, { raw: 'x', cooked: 'x' });
		assert.equal('references' in literal[0].init.regex, false);
		assert.equal('scope' in tree, false);
		assert.equal('binding' in x.node, false);
		// an expression of the markup reads in the fragment's scope, a name declared nowhere a global
		const fragment = read('{a + b}', { scopes: true });
		const sum = fragment.node.fragment.nodes[0].expression;
		assert.equal(referenceOf(sum.left).binding, null);
		assert.equal(referenceOf(sum), undefined);
		assert.equal(scopeOf(fragment.node.fragment).kind, 'fragment');
		assert.equal(referenceOf(sum.left).scope, scopeOf(fragment.node.fragment));
		assert.doesNotThrow(() => JSON.stringify(tree.body));
		assert.deepEqual(Object.keys(x.node), ['type', 'start', 'end', 'name']);
		assert.equal(scopeOf(tree.body[1]).kind, 'function');
		const mutated = parse('let o = {}; o.x = 1; g = 2; h.k = 3;', { scopes: true });
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
		const twice = parse('function f() { var x; var x = 1; function x() {} }', { scopes: true });
		const [, again] = twice.bindings;
		assert.equal(twice.bindings.length, 2);
		assert.deepEqual([again.write, again.writeExpr], [false, null]);
		assert.deepEqual(twice.references.map((r: Any) => [r.node.start, r.declares, r.write, r.writeExpr?.start ?? null, r.binding]), [[26, true, true, 30, again], [42, true, true, null, again]]);
		assert.equal(referenceOf(twice.node.body[0].body.body[1].declarations[0].id).binding, again);
		assert.equal(referenceOf(null), undefined);
		// what the markup declares is a `pattern`, no declaration of JavaScript's naming it
		const context = last('{#each xs as [a, b]}{/each}', { scopes: true }).context;
		assert.deepEqual([referenceOf(context.elements[0]).binding.kind, referenceOf(context.elements[0]).binding.declaration], ['pattern', null]);
	}
	{
		// a namespace declared twice reopens its scope: both read it, and it names the last
		const [first, second] = js('namespace A { let a; } namespace A { let b; }', { typescript: true, scopes: true }).node.body;
		assert.equal(scopeOf(first), scopeOf(second));
		assert.equal(scopeOf(second).node, second);
	}
	assert.throws(() => parse('x', { typescript: 'nonsense' } as Any), TypeError);

	// positions are UTF-16 offsets into the whole component
	const braces = '{a} {"é"} {b /* c */}', located: Options = { comments: true };
	const spaced = read(braces, located);
	assert.deepEqual(spaced.node.fragment.nodes.map((n: Any) => [n.type, n.start, n.end]), [['ExpressionTag', 0, 3], ['Text', 3, 4], ['ExpressionTag', 4, 9], ['Text', 9, 10], ['ExpressionTag', 10, 21]]);
	assert.equal(spaced.comments[0].start, 13);
	assert.deepEqual(spaced.locate(13), { line: 1, column: 13 });
	const erased = parse('import type T from "t"; export const x: T = (1 as any)!; enum E {}', { typescript: 'erase' });
	assert.equal(erased.node.body.length, 2);
	assert.equal(erased.node.body[0].declaration.declarations[0].init.type, 'Literal');
	assert.equal('typeAnnotation' in erased.node.body[0].declaration.declarations[0].id, false);
	assert.deepEqual(erased.typescript.map((k: Any) => k.type), ['TSEnumDeclaration']);
	assert.throws(() => parse('let x: number = 1', { typescript: true, erase: true } as Any), TypeError);
	const template = '<script>\n  let a = 1;\n</script>\n{a}';
	const placed = read(template);
	const script = placed.node.instance.content;
	assert.deepEqual([script.start, script.end], [8, 22]);
	assert.deepEqual([9, 22].map((at) => placed.locate(at)), [{ line: 2, column: 0 }, { line: 3, column: 0 }]);
	const sheet = css('/* top */ div, span { color: red; }');
	assert.equal(sheet.node.type, 'StyleSheet');
	assert.equal(sheet.node.end, 35);
	assert.deepEqual(sheet.node.children.map((rule: Any) => rule.type), ['Rule']);
	assert.deepEqual(JSON.parse(JSON.stringify(sheet.node.comments)), [{ type: 'CSSComment', value: ' top ', start: 0, end: 9 }]);
	assert.throws(() => css('div { color: }'), (e: Any) => e.code === 'expected' && e.pos === 6);
	const half = css('a { b: c } div { color: } p { q: r }', { errorRecovery: true });
	assert.deepEqual(half.errors.map((e: Any) => [e.code, e.pos]), [['expected', 17]]);
	assert.deepEqual(half.node.children.map((rule: Any) => rule.end), [10]);
	assert.equal(program('"﻿a"; "bc"; zz').body[2].expression.name, 'zz');
	assert.throws(() => read(42 as Any), TypeError);
	const twice = 'let a = 1; f(a)';
	const whole = JSON.stringify(parse(twice).node);
	assert.equal(read(`{${twice.slice(11)}}`).node.fragment.nodes[0].expression.type, 'CallExpression');
	assert.equal(JSON.stringify(parse(twice).node), whole);
	assert.throws(() => parse('x', { locations: true } as Any), TypeError);
	assert.throws(() => parse('x', { typescript: 'yes' } as Any), TypeError);
	const { engine } = await import('#engine');
	for (const language of [3, -1, 1.5, NaN]) {
		assert.throws(() => engine.parse('a;', 0, language), / is no language/);
	}
	if (name === 'native') assert.throws(() => (load() as Addon).parse(new Uint8Array([0xff]), 0, 0), /the source is not UTF-8/);
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

// regular expressions written from their grammar, some then damaged: the engine and the host's `RegExp` agree on which are valid
{
	let seed = 1;
	const below = (n: number) => ((seed ^= seed << 13), (seed ^= seed >>> 17), (seed ^= seed << 5), (seed >>>= 0) % n);
	const pick = (list: string[]) => list[below(list.length)];
	const maybe = (percent: number) => below(100) < percent;
	const NAMES = ['a', 'b', 'aa', '$', '_x', 'é', '𝒜', '\\u0061', '\\u{62}', '\\ud835\\udc9c', '1', '', 'a-'];
	const CHARS = ['a', 'b', 'c', 'k', 'u', 'x', 'z', '0', '1', '8', '9', '_', ' ', ',', '-', '=', '!', ':', '<', '>', '&', '#', '~', '@', '%', ';', '`', 'é', '😀', '😁', '{', '}', ']', '^', '$', '.', '*', '+', '?', '|', '(', ')', '['];
	const ESCAPES = ['\\d', '\\D', '\\s', '\\S', '\\w', '\\W', '\\t', '\\n', '\\v', '\\f', '\\r', '\\b', '\\B', '\\0', '\\00', '\\01', '\\08', '\\1', '\\2', '\\3', '\\9', '\\10', '\\12', '\\377', '\\400', '\\8', '\\cA', '\\cz', '\\c', '\\c1', '\\c_', '\\x41', '\\x4', '\\xg0', '\\u0041', '\\u004', '\\u{41}', '\\u{1F600}', '\\u{110000}', '\\u{}', '\\u{4', '\\uD83D', '\\uDE00', '\\uD83D\\uDE00', '\\uDE00\\uD83D', '\\k', '\\k<a>', '\\k<b>', '\\k<aa>', '\\k<', '\\k<>', '\\k<𝒜>', '\\p{L}', '\\P{L}', '\\p{Lu}', '\\p{ASCII}', '\\p{Script=Greek}', '\\p{sc=Grek}', '\\p{scx=Latin}', '\\p{gc=L}', '\\p{General_Category=Letter}', '\\p{RGI_Emoji}', '\\P{RGI_Emoji}', '\\p{Basic_Emoji}', '\\p{Nope}', '\\p{sc=Nope}', '\\p{L', '\\p', '\\p{}', '\\p{=L}', '\\p{gc=}', '\\p{L1}', '\\p{Any}', '\\-', '\\/', '\\\\', '\\.', '\\*', '\\(', '\\)', '\\[', '\\]', '\\{', '\\}', '\\|', '\\^', '\\$', '\\?', '\\+', '\\a', '\\e', '\\_', '\\ ', '\\é', '\\😀', '\\&', '\\!', '\\#', '\\%', '\\,', '\\:', '\;', '\\<', '\\=', '\\>', '\\@', '\\`', '\\~', '\\q{ab|c}', '\\q{a}', '\\q{}', '\\q{a|}', '\\q', '\\q{a'];
	const QUANT = ['*', '+', '?', '*?', '+?', '??', '{1}', '{1,}', '{1,2}', '{2,1}', '{0}', '{01,1}', '{1,01}', '{10,9}', '{9,10}', '{,2}', '{', '{1', '{1,', '{a}', '{}', '{1}?', '{1,2}?', '}', '{99999999999999999999,1}', '{1,99999999999999999999}'];
	const MODS = ['i', 'm', 's', 'im', 'ims', 'ii', '', 'x', 'i-m', '-i', 'i-', '-', 'i-i', 'm-im', '-ii', 'ims-'];
	const SPLICE = ['(', ')', '[', ']', '{', '}', '|', '\\', '-', '&&', '--', '?', '*', '(?<a>', '(?<', '(?', '\\k<a>', '\\k', '^', ',', '<', '>', ':', '=', 'k', '0', '😀'];
	const FLAGS = ['', '', '', '', 'u', 'u', 'u', 'v', 'v', 'v', 'i', 'gimsyd', 'gu', 'dv', 'uv', 'gg', 'x', 'uu', 'vu', 'gx'];
	const char = () => pick(CHARS.slice(0, 32));
	const atom = (depth: number): string => {
		const r = below(100);
		if (r < 30) return char();
		if (r < 34) return '.';
		if (r < 52) return pick(ESCAPES);
		if (r < 68) return set(depth);
		if (depth > 4) return char();
		if (r < 76) return `(${disjunction(depth + 1)})`;
		if (r < 82) return `(?:${disjunction(depth + 1)})`;
		if (r < 90) return `(?<${pick(NAMES)}>${disjunction(depth + 1)})`;
		if (r < 94) return `(?${pick(MODS)}:${disjunction(depth + 1)})`;
		return pick(CHARS);
	};
	const term = (depth: number) => {
		const r = below(100);
		if (r < 5) return pick(['^', '$', '\\b', '\\B']) + (maybe(15) ? pick(QUANT) : '');
		if (r < 12 && depth <= 4) return `(?${pick(['=', '!', '<=', '<!'])}${disjunction(depth + 1)})` + (maybe(30) ? pick(QUANT) : '');
		return atom(depth) + (maybe(30) ? pick(QUANT) : '');
	};
	const disjunction = (depth: number) => {
		let out = '';
		for (let alternatives = maybe(35) ? 2 + below(2) : 1; alternatives > 0; alternatives--) {
			for (let terms = below(4); terms > 0; terms--) out += term(depth);
			if (alternatives > 1) out += '|';
		}
		return out;
	};
	const member = () => (maybe(45) ? char() : maybe(20) ? pick(CHARS) : pick(ESCAPES));
	const set = (depth: number) => {
		let out = maybe(20) ? '[^' : '[';
		const style = below(10);
		for (let i = below(4); i > 0; i--) {
			out += maybe(30) ? `${member()}-${member()}` : maybe(17) && depth < 4 ? set(depth + 1) : member();
			if (i > 1) out += style < 2 ? '&&' : style < 4 ? '--' : maybe(8) ? pick(['&&', '--', '&', '-']) : '';
		}
		return out + (maybe(10) ? '-]' : ']');
	};
	const damage = (pattern: string) => {
		const chars = [...pattern];
		for (let edits = 1 + below(2); edits > 0; edits--) {
			const at = below(chars.length + 1);
			if (maybe(25)) chars.splice(at, 1);
			else chars.splice(at, 0, maybe(33) && chars.length > 0 ? chars[below(chars.length)] : pick(SPLICE));
		}
		return chars.join('');
	};
	// a body the literal ends where it is meant to: no bare `/` outside a class, nothing left open
	const closed = (pattern: string) => {
		let escaped = false, bracket = false;
		for (const c of pattern) {
			if (escaped) escaped = false;
			else if (c === '\\') escaped = true;
			else if (c === '[' || c === ']') bracket = c === '[';
			else if (c === '/' && !bracket) return false;
		}
		return !bracket && !escaped && pattern !== '' && pattern[0] !== '*' && pattern[0] !== '/';
	};
	const valid = (read: () => unknown) => {
		try {
			read();
			return true;
		} catch (e) {
			if (!(e instanceof SyntaxError)) throw e;
			return false;
		}
	};
	// an older host does not read modifiers or a name given twice
	for (let cases = valid(() => new RegExp('(?i:a)|(?<a>x)|(?<a>y)')) ? 50_000 : 0; cases > 0; ) {
		const pattern = maybe(40) ? damage(disjunction(0)) : disjunction(0);
		if (!closed(pattern)) continue;
		cases--;
		const flags = pick(FLAGS);
		assert.equal(valid(() => js(`/${pattern}/${flags}`)), valid(() => new RegExp(pattern, flags)), `/${pattern}/${flags}`);
	}
}

// a component: its markup's nodes around the JavaScript ones, one tree
{
	const source = '<script lang="ts">\n\tlet items: string[] = [];\n</script>\n\n{#each items as item, i (item)}\n\t<p class:odd={i % 2} on:click={() => item}>{item}</p>\n{:else}\n\tnone\n{/each}\n';
	const doc = read(source, { scopes: true, comments: true });
	const root = doc.node;
	assert.equal(root.type, 'Root');
	assert.equal(root.end, source.length);
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
	assert.throws(() => read('<div>'), { code: 'unclosed', pos: 0 });
	// under recovery the tree is what could be read, the errors listed with it
	const loose = read('<div>{#if }<Comp foo={bar}\n</div>', { errorRecovery: true });
	assert.deepEqual(loose.errors.map((e: Any) => e.code), ['unclosed', 'unexpected_token', 'expected']);
	const div = loose.node.fragment.nodes[0];
	assert.equal(div.end, 33);
	const block = div.fragment.nodes[0];
	assert.equal(block.test.name, '');
	assert.equal(block.consequent.nodes[0].name, 'Comp');
	assert.equal(block.consequent.nodes[0].end, 27);
}

// unfinished input under recovery is an answer, never a panic; a strict error is a SyntaxError
{
	for (const text of ['<a x="', '<a /*', '<script>"</script>', '{#if', '<div class="{a']) {
		assert.equal(read(text, { errorRecovery: true, comments: true, scopes: true }).node.type, 'Root', `${name} ${text}`);
	}
	// JavaScript that fails on its first token
	assert.throws(() => read('<a on:x={)}/>'), (e: Any) => e instanceof SyntaxError && (e as Any).code === 'unexpected_token' && (e as Any).pos === 9, `${name}`);
}
// every node's parent is the node it sits in, objects without a type passed through, and a listed
// comment's the node it is attached to; a list born at its size has no holes
{
	const wrong: string[] = [];
	const taken = (c: Any) => ['leadingComments', 'trailingComments', 'innerComments'].some((key) => parentOf(c)?.[key]?.includes(c));
	const walk = (holder: Any, value: Any, listed = false) => {
		if (value === null || typeof value !== 'object') return;
		if (Array.isArray(value)) {
			for (let i = 0; i < value.length; i++) {
				if (!Object.hasOwn(value, i)) wrong.push(`hole at ${i}`);
				walk(holder, value[i], listed);
			}
			return;
		}
		const node = typeof value.type === 'string';
		if (node && parentOf(value) !== holder && !(listed && taken(value))) wrong.push(`${value.type} at ${value.start}`);
		for (const key of Object.keys(value)) walk(node ? value : holder, value[key], key === 'comments');
	};
	const decoder = readFileSync(new URL('../src/decode.ts', import.meta.url), 'utf8');
	for (const typescript of [true, 'erase'] as const) walk(undefined, js(decoder, { typescript, comments: true, scopes: true }).node);
	walk(undefined, js('[1, , 3]; function f(this: T, a: number, b?: string) {} /* c */ // d', { typescript: 'erase', comments: true }).node);
	const dir = new URL('../../crates/teasel/tests/hosts/svelte/', import.meta.url);
	for (const file of readdirSync(dir).filter((f) => !f.endsWith('.json'))) {
		walk(undefined, read(readFileSync(new URL(file, dir), 'utf8'), { comments: true, scopes: true, errorRecovery: true }).node);
	}
	assert.deepEqual(wrong, [], `${name} parents`);
}

// `locate`: the line and column of every node, comment and error, in any order, as a scan of the text from its start finds them
{
	const scan = (text: string, offset: number) => {
		let line = 1, start = 0;
		for (let i = 0; i < offset; i++) {
			const c = text[i];
			if (c === '\n' || c === '\u2028' || c === '\u2029' || (c === '\r' && text[i + 1] !== '\n')) [line, start] = [line + 1, i + 1];
		}
		return { line, column: offset - start };
	};
	const texts: [string, Any][] = [
		['a\r\nb\rc\u2028d\u2029e\n"𝒳" // é\r\n/* x\r\ny\u2028*/ f\r', js],
		['<script>\r\n\tlet a = "𝒳"; // é\r\n</script>\r\n<p class="x\ry">{a}\u2028{b}<!-- c\r\n --></p>\r<p>{a /* 𝒳 */}\r\n</p>\r<style>\n\tp { color: red } /* \r */\n</style>\n', read],
		['p {\r\n\tcolor: red;\r}\n/* 𝒳\r\n */ a { b: c }', css],
		['a\nb\nc\nd\ne', js],
	];
	for (const [text, language] of texts) {
		const answer = language(text, language === css ? {} : { comments: true });
		const offsets: number[] = [];
		const visit = (value: Any) => {
			if (value === null || typeof value !== 'object') return;
			if (Array.isArray(value)) return value.forEach(visit);
			if (typeof value.start === 'number') offsets.push(value.start, value.end);
			for (const key of Object.keys(value)) visit(value[key]);
		};
		visit(answer.node);
		visit(answer.comments);
		assert.ok(offsets.length > 8);
		for (const at of [...offsets, ...offsets.reverse(), ...Array.from({ length: text.length + 1 }, (_, i) => (i * 7) % (text.length + 1))]) assert.deepEqual(answer.locate(at), scan(text, at), `${text} at ${at}`);
	}
	const recovered = js('x = "a\u2028\r\ny = \r(\n', { errorRecovery: true });
	assert.ok(recovered.errors.length > 1);
	for (const error of recovered.errors) assert.deepEqual(recovered.locate(error.pos), { line: error.line, column: error.column });
	const thrown = '\r\n\u2028\r = ;';
	assert.throws(() => js(thrown), (e: Any) => e.line === 4 && JSON.stringify({ line: e.line, column: e.column }) === JSON.stringify(scan(thrown, e.pos)));
	assert.deepEqual(read('{a}\n\n{b}').locate(6), { line: 3, column: 1 });
	const answer = read('a\nb');
	for (const offset of [-1, 4, 1.5, NaN, '1' as Any]) assert.throws(() => answer.locate(offset), RangeError);
}

// a tag under `svelte:` needs a rule of its own; under any other prefix it reads as any other
assert.throws(() => read('<svelte:foo></svelte:foo>'), (e: Any) => e.code === 'invalid_name' && e.pos === 1);
assert.equal(read('<svelte:head></svelte:head>').node.fragment.nodes[0].type, 'SvelteHead');
assert.equal(read('<x:foo></x:foo>').node.fragment.nodes[0].type, 'RegularElement');

// a lone surrogate escape is the code unit it names, as JavaScript keeps it
{
	const values = (source: string) => js(source).node.body.map((s: Any) => s.expression.value ?? s.expression.quasis.map((q: Any) => q.value.cooked).join('|'));
	assert.deepEqual(values("'\\ud83d'; '\\ude00x\\u{dbff}'; '\\ufffd'; '\\ud83d\\ude00'; `a\\ud83d${b}\\udc00`;"), ['\ud83d', '\ude00x\udbff', '�', '😀', 'a\ud83d|\udc00']);
	assert.deepEqual(values("'\\ud83d'; '\\ufffd'; 'é\\udc00'; '\\ud83d';"), ['\ud83d', '�', 'é\udc00', '\ud83d']);
	assert.deepEqual(values("'\\ud800😀\\ud800é\\udc00 \\udc00'; 'a\\ud800';"), ['\ud800😀\ud800é\udc00 \udc00', 'a\ud800']);
	assert.throws(() => js("export { x as '\\ud800' };"), { code: 'lone_surrogate_in_module_name', pos: 14 });
	assert.throws(() => js("import { '\\udc00' as y } from 'm';"), { code: 'lone_surrogate_in_module_name', pos: 9 });
}

// a tree too deep for a walk is a parse error on every backend, and the engine reads the next source
{
	const deep = { code: 'nesting_depth' };
	for (const scopes of [false, true]) {
		assert.throws(() => js('a' + '.b'.repeat(9_999), { scopes }), deep);
		assert.throws(() => js('new '.repeat(9_999) + 'x', { scopes }), deep);
		assert.throws(() => js('type A = ' + 'B<'.repeat(999) + 'C' + '>'.repeat(999), { scopes, typescript: true }), deep);
		assert.throws(() => read('<a>'.repeat(40_000) + '</a>'.repeat(40_000), { scopes }), deep);
		assert.equal(js('x', { scopes }).node.body.length, 1);
	}
	// tried: 501 rules deep in each sheet, a tree the decoder without code generation holds only once V8 has optimized it
	const sheets = `<style>${'a {'.repeat(1000)}</style><style>a {}</style>`;
	assert.deepEqual(read(sheets, { errorRecovery: true }).errors.map((e: Any) => e.code), ['expected', 'unexpected_close']);
}

// a component parses after the engine started over
if (name !== 'native') {
	let restarted = false;
	// a RangeError inside the engine is taken for a wasm stack overflow: exhaust the stack, then parse on the way back up
	(function deep() {
		try {
			deep();
		} catch {
			if (!restarted) {
				try {
					read('<a>{x}</a>');
				} catch (e: Any) {
					restarted = /started over/.test(e.message);
					// an overflow before the engine is reached says nothing: the next frame up tries again
					if (!restarted) throw e;
				}
			}
		}
	})();
	assert.ok(restarted, `${name} the engine started over`);
	assert.equal(read('<a>{x}</a>').node.fragment.nodes[0].name, 'a');
	assert.equal(css('a { b: c }').node.children[0].type, 'Rule');
	// a source that grows the memory past 256 MiB leaves it to the instance the next parse replaces
	const { engine } = await import('#engine');
	const memory = () => (engine.tree(false, true).find((view) => typeof view === 'object') as Uint32Array).buffer.byteLength;
	assert.equal(js(`/*${'c'.repeat(1 << 27)}*/`).node.body.length, 0);
	assert.ok(memory() > 1 << 28, `${name} grew to ${memory()}`);
	assert.equal(read('<a>{x}</a>').node.fragment.nodes[0].name, 'a');
	assert.ok(memory() < 1 << 28, `${name} shrank to ${memory()}`);
}

// the tests read the source; the published build must answer the same once its specifiers are rewritten
{
	const built = await import('../dist/index.js');
	assert.equal(built.parseModule('let x = 1').node.body.length, 1, `${name} dist`);
	assert.equal((built.parse('<p>{x}</p>').node.fragment.nodes[0] as Any).name, 'p', `${name} dist`);
}

// src/children.ts is written from the engine's layout by scripts/children.ts: the two must agree
{
	const { engine } = await import('#engine');
	const made = generate(JSON.parse(engine.layout()));
	assert.deepEqual(JSON.parse(JSON.stringify(tables.children)), made.children, 'run node scripts/children.ts');
	assert.deepEqual([...m.extras], made.extras, 'run node scripts/children.ts');
	assert.deepEqual(JSON.parse(JSON.stringify(m.children)), { ...made.children, ...made.svelte }, 'run node scripts/children.ts');
}

// what children names is what the answers hold: over every fixture, a key whose value is a node or a
// list of nodes is one of the type's children, and every child named is seen holding one somewhere
{
	const seen = new Map<string, Set<string>>();
	// every key holding a node or a list, empty or not: what the reverse check counts as covered
	const listed = new Map<string, Set<string>>();
	const observe = (value: Any) => {
		if (value === null || typeof value !== 'object') return;
		if (Array.isArray(value)) return value.forEach(observe);
		if (typeof value.type !== 'string') return;
		// a comment is not a node; a stylesheet's `Block` is one
		if ((value.type === 'Line' || value.type === 'Block') && typeof value.value === 'string') return;
		let keys = seen.get(value.type);
		if (keys === undefined) seen.set(value.type, (keys = new Set()));
		let lists = listed.get(value.type);
		if (lists === undefined) listed.set(value.type, (lists = new Set()));
		for (const key in value) {
			const v = value[key];
			if (v === null || typeof v !== 'object') continue;
			const node = (x: Any) => x !== null && typeof x === 'object' && typeof x.type === 'string' && !((x.type === 'Line' || x.type === 'Block') && typeof x.value === 'string');
			if (Array.isArray(v) ? v.some(node) : node(v)) keys.add(key);
			if (Array.isArray(v) || node(v)) lists.add(key);
			observe(v);
		}
	};
	const fixtures = new URL('../../crates/teasel/tests/', import.meta.url);
	const files = (dir: string, ext: string) => readdirSync(new URL(dir, fixtures)).filter((file) => file.endsWith(ext)).map((file) => readFileSync(new URL(`${dir}${file}`, fixtures), 'utf8'));
	const check = (table: typeof m.children, what: string, byHand = false) => {
		for (const [type, keys] of seen) {
			assert.ok(type in table, `${what}: ${type} is not in children`);
			for (const key of keys) assert.ok(table[type].includes(key) || m.extras.includes(key), `${what}: ${type}.${key} holds nodes but children does not name it`);
		}
		// js's table comes from the layout, so a stale entry there is impossible; css's and Svelte's are spelled by hand
		const unseen = Object.entries(table).flatMap(([type, keys]) => (listed.has(type) && !(byHand && type in tables.children) ? keys.filter((key) => !listed.get(type)!.has(key)).map((key) => `${type}.${key}`) : []));
		if (byHand) assert.deepEqual(unseen, [], `${what}: children names fields no fixture holds a node or a list in`);
		seen.clear();
		listed.clear();
	};
	const parses = (text: string, options: Options) => {
		try {
			return js(text, options).node;
		} catch (e) {
			if (e instanceof SyntaxError) return null;
			throw e;
		}
	};
	for (const text of files('fixtures/js/', '.js')) observe(parses(text, { comments: true, errorRecovery: true }));
	check(tables.children, 'js fixtures');
	for (const text of files('fixtures/ts/', '.ts')) observe(parses(text, { typescript: true, comments: true, errorRecovery: true }));
	check(tables.children, 'ts fixtures');
	for (const text of files('hosts/svelte/', '.svelte')) {
		try {
			observe(read(text, { comments: true, scopes: true, errorRecovery: true }).node);
		} catch (e) {
			if (!(e instanceof SyntaxError)) throw e;
		}
	}
	// a stylesheet read on its own is walked by the same table
	for (const text of files('fixtures/css/', '.css')) {
		try {
			observe(css(text).node);
		} catch (e) {
			if (!(e instanceof SyntaxError)) throw e;
		}
	}
	check(m.children, 'svelte and css fixtures', true);
	assert.ok('Rule' in m.children && 'Identifier' in m.children && 'EachBlock' in m.children);
	assert.ok(!('Host' in tables.children) && !('EachBlock' in tables.children));
	assert.ok(Object.isFrozen(m.children) && Object.isFrozen(m.children.Program) && Object.isFrozen(m.children.Rule));
	assert.deepEqual(m.children.StyleSheet, ['attributes', 'children', 'comments']);
	const cut = read('<script lang="ts"></script>{#snippet x<T,}{/snippet}', { errorRecovery: true }).node.fragment.nodes[0];
	assert.equal(cut.typeParams, 'T,');
}

// every node of a type has the same keys in the same order, but for what acorn adds where it applies and, erasing, a decorator
{
	const fixtures = new URL('../../crates/teasel/tests/fixtures/', import.meta.url);
	const sources = (dir: string) => readdirSync(new URL(dir, fixtures)).filter((file) => !file.endsWith('.json')).map((file) => readFileSync(new URL(`${dir}${file}`, fixtures), 'utf8'));
	const acorn = ['regex', 'bigint', 'directive'];
	for (const [dir, options, where] of [
		['js/', { scopes: true }, acorn],
		['ts/', { typescript: true, scopes: true }, ['regex', 'bigint']],
		['ts/', { typescript: 'erase', scopes: true }, [...acorn, 'decorators']],
	] as const) {
		const shapes = new Map<string, string>();
		const walk = (value: Any) => {
			if (value === null || typeof value !== 'object') return;
			if (Array.isArray(value)) return value.forEach(walk);
			if (typeof value.type === 'string') {
				const keys = Object.keys(value).filter((key) => !(where as readonly string[]).includes(key)).join();
				assert.equal(keys, shapes.get(value.type) ?? (shapes.set(value.type, keys), keys), `${dir} ${options.typescript ?? false}: a second shape of ${value.type}`);
			}
			for (const key in value) walk(value[key]);
		};
		for (const text of sources(dir)) {
			try {
				walk(js(text, options).node);
			} catch (e) {
				if (!(e instanceof SyntaxError)) throw e;
			}
		}
	}
}

// tsc checks what the types promise here and node never calls it
function types(text: string) {
	type Equal<A, B> = (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
	const expect = <T extends true>() => {};
	type Required<T> = { [K in keyof T]-?: {} extends Pick<T, K> ? never : K }[keyof T];
	type Fragment = AST.Fragment;

	type Each = AST.EachBlock;
	expect<Equal<Required<Each>, 'type' | 'start' | 'end' | 'expression' | 'context' | 'body'>>();
	expect<Equal<Each['context'], Pattern | null>>();
	expect<Equal<Each['index'], Identifier | undefined>>();
	expect<Equal<Each['key'], Expression | undefined>>();
	expect<Equal<Each['body'], Fragment>>();
	expect<Equal<Each['fallback'], Fragment | undefined>>();

	expect<Equal<AST.IfBlock['alternate'], Fragment | null>>();
	expect<Equal<AST.IfBlock['branches'], AST.ElseIf[]>>();
	expect<Equal<Required<AST.ElseIf>, 'type' | 'start' | 'end' | 'test' | 'consequent'>>();
	expect<Equal<AST.ElseIf['consequent'], Fragment>>();
	expect<Equal<AST.AwaitBlock['pending'], Fragment | null>>();
	expect<Equal<AST.SnippetBlock['typeParams'], string | undefined>>();
	expect<Equal<AST.SnippetBlock['fragment'], Fragment>>();
	expect<Equal<AST.KeyBlock['fragment'], Fragment>>();
	expect<Equal<AST.RegularElement['fragment'], Fragment>>();
	expect<Equal<AST.Text['raw'], string>>();
	expect<Equal<keyof Fragment, 'type' | 'nodes'>>();
	expect<Equal<AST.OnDirective['name'], string>>();
	expect<Equal<AST.OnDirective['modifiers'], string[]>>();
	expect<Equal<AST.BindDirective['expression'], Expression>>();
	expect<Equal<AST.OnDirective['expression'], Expression | null>>();
	expect<Equal<AST.TransitionDirective['intro'], boolean>>();
	expect<Equal<AST.StyleDirective['value'], AST.Attribute['value']>>();
	expect<Equal<AST.ConstTag['declaration'], VariableDeclaration>>();
	expect<Equal<Extract<AST.Attribute['value'], unknown[]>[number], AST.Text | AST.ExpressionTag>>();
	expect<Equal<AST.Root['instance'], AST.Script | undefined>>();
	expect<Equal<AST.Root['fragment'], Fragment>>();
	expect<Equal<AST.Script['content'], Program>>();

	// a component, a module and a stylesheet, each by its function
	const doc = m.parse(text, {});
	expect<Equal<typeof doc.node, AST.Root>>();
	const program = m.parseModule(text);
	expect<Equal<typeof program.node, Program>>();
	doc.node.fragment.nodes[0]?.type satisfies AST.FragmentNode['type'] | undefined;
	const sheet = m.parseCss(text);
	expect<Equal<typeof sheet.node.type, 'StyleSheet'>>();
	// @ts-expect-error a stylesheet read on its own has no element around it
	sheet.node.attributes;
	// the accessors take an attached comment and a component's nodes, and a script's program has a component's node for a parent
	m.parentOf(m.parseModule(text).node.body[0].leadingComments![0]);
	m.scopeOf(doc.node);
	m.parentOf(doc.node.instance!.content)?.type === 'Script';
	// @ts-expect-error a component is always a module
	m.parse(text, { sourceType: 'module' });
	// @ts-expect-error a stylesheet has no comments to attach
	m.parseCss(text, { comments: true });
}
