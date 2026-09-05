import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import init, { parse, parseExpressionAt } from './pkg/teasel.js';

await init(readFileSync(new URL('./pkg/teasel_bg.wasm', import.meta.url)));
const program = JSON.parse(parse('let x: number = 1; // done', JSON.stringify({ typescript: true, comments: true })));
assert.equal(program.body[0].declarations[0].id.typeAnnotation.typeAnnotation.type, 'TSNumberKeyword');
assert.equal(program.body[0].trailingComments[0].value, ' done');
assert.equal(JSON.parse(parseExpressionAt('"é" + x', 6, '{}')).start, 6);
assert.equal(JSON.parse(parse('x = ;', '{}')).error.pos, 4);
console.log('ok');
