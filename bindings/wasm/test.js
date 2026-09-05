import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { init, parse, parseExpressionAt, parseParamsAt } from './index.js';

await init({ module_or_path: readFileSync(new URL('./pkg/teasel_bg.wasm', import.meta.url)) });
const program = parse('let x: number = 1; // done', { sourceType: 'module', typescript: true, comments: true });
assert.equal(program.body[0].declarations[0].id.typeAnnotation.typeAnnotation.type, 'TSNumberKeyword');
assert.equal(program.body[0].trailingComments[0].value, ' done');
assert.equal(parse('with (a) {}').body[0].type, 'WithStatement');
assert.equal(parseExpressionAt('"é" + x', 6).start, 6);
assert.equal(parseParamsAt('(a, b)', 0).end, 6);
assert.throws(() => parse('x = ;'), (e) => e instanceof SyntaxError && e.pos === 4);
assert.throws(() => parseExpressionAt('𝒳 + y', 1), SyntaxError);
console.log('ok');
