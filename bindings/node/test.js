import { parse, parseExpressionAt, parseParamsAt, parseStatementAt, isIdentifierStart, isIdentifierChar } from './index.js';
import assert from 'node:assert/strict';

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

const parens = parseExpressionAt('{(a) /* c */ } // d', 1, { comments: true });
assert.equal(parens.node.type, 'Identifier');
assert.equal(parens.end, 12);
assert.deepEqual(parens.comments.map((c) => c.value), [' c ']);
assert.equal(parens.node.trailingComments[0].start, 5);
assert.equal(parseStatementAt('{@const x = 1}', 2).end, 13);

const params = parseParamsAt('(a, b = 1) => a', 0);
assert.equal(params.params.length, 2);
assert.equal(params.end, 10);

assert.throws(() => parse('x = ;'), (e) => e instanceof SyntaxError && e.pos === 4 && e.loc.column === 4 && e.message === 'Unexpected token (1:4)');
assert.throws(() => parse('x', { ranges: true }), TypeError);
assert.throws(() => parse('x', { sourceType: 'nonsense' }), TypeError);
assert.throws(() => parseExpressionAt('𝒳 + y', 1), (e) => e instanceof SyntaxError && /surrogate/.test(e.message));
assert.throws(() => parseExpressionAt('a + b', -1), SyntaxError);
assert.throws(() => parseExpressionAt('a + b', 99), SyntaxError);

const unicode = parseExpressionAt('"é" + x', 6).node;
assert.equal(unicode.type, 'Identifier');
assert.equal(unicode.start, 6);
assert.equal(isIdentifierStart('a'.codePointAt(0)) && isIdentifierStart('é'.codePointAt(0)) && !isIdentifierStart('1'.codePointAt(0)), true);
assert.equal(isIdentifierChar('1'.codePointAt(0)) && !isIdentifierChar('-'.codePointAt(0)), true);
console.log('ok');
