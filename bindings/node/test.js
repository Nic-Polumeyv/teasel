import { parse, parseExpressionAt, parseParamsAt } from './index.js';
import assert from 'node:assert/strict';

const program = parse('let x: number = 1; // done', { sourceType: 'module', typescript: true, comments: true, locations: true });
assert.equal(program.sourceType, 'module');
assert.equal(program.body[0].declarations[0].id.typeAnnotation.typeAnnotation.type, 'TSNumberKeyword');
assert.equal(program.body[0].trailingComments[0].value, ' done');
assert.equal(program.body[0].loc.end.column, 18);
assert.equal(parse('with (a) {}').body[0].type, 'WithStatement');
assert.equal('loc' in parse('x'), false);

const expression = parseExpressionAt('{a + b}', 1, { preserveParens: true });
assert.equal(expression.type, 'BinaryExpression');
assert.equal(expression.end, 6);

const params = parseParamsAt('(a, b = 1) => a', 0);
assert.equal(params.params.length, 2);
assert.equal(params.end, 10);

assert.throws(() => parse('x = ;'), (e) => e instanceof SyntaxError && e.pos === 4 && e.loc.column === 4 && e.message === 'Unexpected token (1:4)');
assert.throws(() => parse('x', { ranges: true }), TypeError);
assert.throws(() => parse('x', { sourceType: 'nonsense' }), TypeError);
assert.throws(() => parseExpressionAt('𝒳 + y', 1), (e) => e instanceof SyntaxError && /surrogate/.test(e.message));
assert.throws(() => parseExpressionAt('a + b', -1), SyntaxError);
assert.throws(() => parseExpressionAt('a + b', 99), SyntaxError);

const unicode = parseExpressionAt('"é" + x', 6);
assert.equal(unicode.type, 'Identifier');
assert.equal(unicode.start, 6);
console.log('ok');
