import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { Source, init, parse, parseExpressionAt, parseParamsAt } from './index.js';

await init({ module_or_path: readFileSync(new URL('./pkg/teasel_bg.wasm', import.meta.url)) });
const program = parse('let x: number = 1; // done', { sourceType: 'module', typescript: true, comments: true });
assert.equal(program.body[0].declarations[0].id.typeAnnotation.typeAnnotation.type, 'TSNumberKeyword');
assert.equal(program.body[0].trailingComments[0].value, ' done');
assert.equal(parse('with (a) {}').body[0].type, 'WithStatement');
assert.equal(parseExpressionAt('"é" + x', 6).node.start, 6);
assert.equal(parseParamsAt('(a, b)', 0).end, 6);
assert.equal(parseExpressionAt('{(a) /* c */}', 1).end, 12);
assert.throws(() => parse('x = ;'), (e) => e instanceof SyntaxError && e.pos === 4);
assert.throws(() => parseExpressionAt('𝒳 + y', 1), SyntaxError);
const source = new Source('{a} {b}', { comments: true });
assert.equal(source.parseExpressionAt(5).node.name, 'b');
assert.equal(new Source('{xs as x}', { typescript: true }).parseExpressionAt(1, 'as').end, 3);
source.free();
console.log('ok');
