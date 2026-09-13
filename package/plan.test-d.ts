// Misuse the plan helpers on purpose; `bun run types` fails if any line below stops erroring.
import { rule, seq, js, region, declare, incoming, type Infer, type Slot } from './plan.js';

const each = rule('EachBlock', { expression: 'null', context: 'null', index: 'omit' })
	.form((f) => seq(js('expression', f.expression), js('pattern', f.context), js('bindingIdentifier', f.index)))
	.regions((f) => [region('iteration', incoming, [f.context, f.index])])
	.declares((f) => [declare([f.context, f.index], 'iteration')]);

type EachBlock = Infer<typeof each>;
const node: EachBlock = { type: 'EachBlock', start: 0, end: 1, expression: null, context: null };
// @ts-expect-error an omitted field is optional, never a string
const wrongOmitted: EachBlock = { ...node, index: 'nope' };
// @ts-expect-error a null field is required
const missingNull: EachBlock = { type: 'EachBlock', start: 0, end: 1, expression: null };

// @ts-expect-error `b` is not a field of this rule
rule('X', { a: 'null' }).form((f) => js('expression', f.b));
rule('Y', { a: 'null', p: 'null' })
	.form((f) => seq(js('expression', f.a), js('pattern', f.p)))
	.regions((f) => [region('r', incoming, [f.p])])
	// @ts-expect-error `a` was read as an expression, so it cannot be declared
	.declares((f) => [declare([f.a], 'r')]);
rule('Z', { p: 'null' })
	.form((f) => js('pattern', f.p))
	.regions((f) => [region('r', incoming, [f.p])])
	// @ts-expect-error no region named `q`
	.declares((f) => [declare([f.p], 'q')]);
rule('Q', { p: 'null' })
	.form((f) => js('pattern', f.p))
	// @ts-expect-error a slot of another rule cannot be covered here
	.regions(() => [region('r', incoming, [null! as Slot<'other'>])]);
export {};
