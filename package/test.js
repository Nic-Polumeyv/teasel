import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import * as node from './index.js';
import * as wasm from './wasm.js';

await wasm.init(readFileSync(new URL('./teasel.wasm', import.meta.url)));

for (const [name, { Source, parse, parseExpressionAt, parseParamsAt, parseStatementAt, isIdentifierStart, isIdentifierChar }] of [['node', node], ['wasm', wasm]]) {
	const program = parse('let x: number = 1; // done', { sourceType: 'module', typescript: true, comments: true, locations: true });
	assert.equal(program.sourceType, 'module');
	assert.equal(program.body[0].declarations[0].id.typeAnnotation.typeAnnotation.type, 'TSNumberKeyword');
	assert.equal(program.body[0].trailingComments[0].value, ' done');
	assert.equal(program.body[0].loc.end.column, 18);
	assert.equal(program.comments.length, 1);
	assert.equal(program.comments[0].loc.start.column, 19);
	assert.equal('comments' in parse('x'), false);
	assert.equal(parse('with (a) {}').body[0].type, 'WithStatement');
	assert.equal('loc' in parse('x'), false);

	const expression = parseExpressionAt('{a + b}', 1, { preserveParens: true });
	assert.equal(expression.node.type, 'BinaryExpression');
	assert.equal(expression.node.end, 6);
	assert.equal(expression.end, 6);
	assert.equal(parseExpressionAt('{(a)}', 1, { preserveParens: true }).node.type, 'ParenthesizedExpression');

	const parens = parseExpressionAt('{(a) /* c */ } // d', 1, { comments: true });
	assert.equal(parens.node.type, 'Identifier');
	assert.equal(parens.end, 12);
	assert.deepEqual(parens.comments.map((c) => c.value), [' c ']);
	assert.equal(parens.node.trailingComments[0].start, 5);
	assert.equal(parseStatementAt('{@const x = 1}', 2).end, 13);
	assert.equal(parseExpressionAt('{items as item}', 1, { typescript: true }).node.type, 'TSAsExpression');
	assert.equal(parseExpressionAt('{items as item}', 1, { typescript: true, until: 'as' }).end, 6);
	assert.equal(parseExpressionAt('{f(x as T) as item}', 1, { typescript: true, until: 'as' }).end, 10);
	assert.equal(parseExpressionAt('{xs as T[] as item}', 1, { typescript: true, until: 'as' }).end, 10);
	assert.equal(parseExpressionAt('{xs as unknown as T[] as item: T, i}', 1, { typescript: true, until: 'as' }).end, 21);
	assert.equal(parseExpressionAt('{xs as [a, b = 1]}', 1, { typescript: true, until: 'as' }).end, 3);
	assert.throws(() => parseExpressionAt('éé𝒳x', 3), (e) => e.message === 'offset 3 is inside a surrogate pair');
	assert.equal(parseExpressionAt('{xs as T === y as item}', 1, { typescript: true, until: 'as' }).end, 14);
	assert.equal(parseExpressionAt('{xs as const as item}', 1, { typescript: true, until: 'as' }).end, 12);
	assert.throws(() => parseExpressionAt('{a}', 1, { until: 'in' }), TypeError);

	const params = parseParamsAt('(a, b = 1) => a', 0);
	assert.equal(params.params.length, 2);
	assert.equal(params.end, 10);

	assert.throws(() => parse('x = ;'), (e) => e instanceof SyntaxError && e.code === 'unexpected_token' && e.pos === 4 && e.end === 5 && e.loc.column === 4 && e.message === 'Unexpected token');
	assert.throws(() => parse('x = '), (e) => e.code === 'unexpected_eof' && e.pos === 4 && e.end === 4);
	assert.throws(() => parse('/a', { locations: true }), (e) => e.code === 'unterminated_regexp' && e.pos === 1);
	assert.throws(() => parse('x', { ranges: true }), TypeError);
	assert.throws(() => parse('return', { sourceType: 'module' }), SyntaxError);
	assert.equal(parse('return', { allowReturnOutsideFunction: true }).body[0].type, 'ReturnStatement');
	{
		const program = parse('let x = 1; function f(y) { x = y; }', { sourceType: 'module', scopes: true });
		const [x, f, y] = program.bindings;
		assert.equal(program.scope, program.scopes[0]);
		assert.equal(x.node, program.body[0].declarations[0].id);
		assert.deepEqual(x.references.map((r) => r.start), [27]);
		assert.equal(x.references[0].write, true);
		assert.equal(f.scope.kind, 'module');
		assert.equal(y.scope.node, program.body[1]);
		assert.equal(y.scope.through[0], x);
		assert.equal(program.scopes[0].declarations.get('f'), f);
		const at = parseExpressionAt('a + b', 0, { scopes: true });
		assert.equal(at.node.left.binding, null);
		assert.equal(at.scopes[0].kind, 'fragment');
	}
	assert.throws(() => parse('x', { sourceType: 'nonsense' }), TypeError);
	assert.throws(() => parseExpressionAt('𝒳 + y', 1), (e) => e instanceof SyntaxError && /surrogate/.test(e.message));
	assert.throws(() => parseExpressionAt('a + b', -1), SyntaxError);
	assert.throws(() => parseExpressionAt('a + b', 99), SyntaxError);

	const unicode = parseExpressionAt('"é" + x', 6).node;
	assert.equal(unicode.type, 'Identifier');
	assert.equal(unicode.start, 6);
	assert.equal(isIdentifierStart('a'.codePointAt(0)) && isIdentifierStart('é'.codePointAt(0)) && !isIdentifierStart('1'.codePointAt(0)), true);
	assert.equal(isIdentifierChar('1'.codePointAt(0)) && !isIdentifierChar('-'.codePointAt(0)), true);
	const source = new Source('{a} {"é"} {b /* c */}', { locations: true, comments: true });
	assert.equal(source.parseExpressionAt(1).node.name, 'a');
	assert.equal(new Source('{xs as x}', { typescript: true }).parseExpressionAt(1, 'as').end, 3);
	assert.equal(source.parseExpressionAt(11).end, 20);
	assert.equal(source.parseExpressionAt(11).comments[0].loc.start.column, 13);
	assert.throws(() => source.parseExpressionAt(99), SyntaxError);
	assert.throws(() => new Source('𝒳 + y').parseExpressionAt(1), (e) => /surrogate/.test(e.message));
	const erased = parse('import type T from "t"; export const x: T = (1 as any)!; enum E {}', { sourceType: 'module', typescript: 'erase' });
	assert.equal(erased.body.length, 2);
	assert.equal(erased.body[0].declaration.declarations[0].init.type, 'Literal');
	assert.equal('typeAnnotation' in erased.body[0].declaration.declarations[0].id, false);
	assert.deepEqual(erased.typescript.map((k) => k.type), ['TSEnumDeclaration']);
	assert.equal('typeAnnotation' in parse('let x: number = 1', { typescript: true, erase: true }).body[0].declarations[0].id, false);
	const template = new Source('<script>\n  let a = 1;\n</script>\n{a}', { sourceType: 'module', locations: true });
	const script = template.parse(8, 22);
	assert.equal(script.start, 8);
	assert.equal(script.end, 22);
	assert.equal(script.body[0].loc.start.line, 2);
	assert.throws(() => template.parse(22, 8), SyntaxError);
	assert.equal(template.parse(24).body[0].type, 'ExpressionStatement');
	assert.equal(parse('"﻿a"; "bc"; zz').body[2].expression.name, 'zz');
	source.free();
	assert.throws(() => source.parseExpressionAt(1), TypeError);
	assert.equal(new Source('{xs as x}', { typescript: true, until: 'as' }).parseExpressionAt(1).end, 3);
	assert.throws(() => parse('x', { locations: 1 }), TypeError);
	assert.throws(() => parse('x', { typescript: 'yes' }), TypeError);
	assert.throws(() => new Source('a;b;c').parse(0, -1), (e) => e.code === 'invalid_request');
	assert.throws(() => new Source('a;b;c').parse(0, NaN), (e) => e.code === 'invalid_request');
	const wide = 'x;'.repeat(200000);
	assert.equal(parse(wide).body.length, 200000);
	assert.equal(parse('y;').body.length, 1);
	assert.equal(parse(wide + wide).body.length, 400000);
	assert.equal(parse('z;').body[0].expression.name, 'z');
	console.log(name, 'ok');
}
