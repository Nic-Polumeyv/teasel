import { parse, parseExpressionAt, parseParamsAt } from './index.js';
import assert from 'node:assert/strict';

const program = parse('let x: number = 1; // done', { typescript: true, comments: true });
assert.equal(program.body[0].declarations[0].id.typeAnnotation.typeAnnotation.type, 'TSNumberKeyword');
assert.equal(program.body[0].trailingComments[0].value, ' done');

const expression = parseExpressionAt('{a + b}', 1, { preserveParens: true });
assert.equal(expression.type, 'BinaryExpression');
assert.equal(expression.end, 6);

assert.equal(parseParamsAt('(a, b = 1) => a', 0).length, 2);

assert.throws(() => parse('x = ;'), (e) => e instanceof SyntaxError && e.pos === 4 && e.loc.column === 4);

const unicode = parseExpressionAt('"é" + x', 6);
assert.equal(unicode.type, 'Identifier');
assert.equal(unicode.start, 6);
console.log('ok');
