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
assert.equal(parse('let x: number = 1', { typescript: 'erase' }).body[0].declarations[0].id.typeAnnotation, undefined);
assert.equal(new Source('<script>let a = 1</script>').parse(8, 17).body[0].end, 17);
source.free();
console.log('ok');

{
	const program = parse('let x = 1; x = 2;', { sourceType: 'module', scopes: true });
	const [x] = program.bindings;
	assert.equal(x.node, program.body[0].declarations[0].id);
	assert.equal(x.references[0].write, true);
	assert.equal(program.scope.kind, 'module');
}
