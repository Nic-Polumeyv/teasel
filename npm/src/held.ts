import { engine } from '#engine';
import type { Grammar } from './grammar.ts';

const grammars = new WeakMap<Grammar, object>();

/** What the engine holds for a grammar: read on first use and kept while the grammar lives. */
export function compiled(grammar: Grammar): object {
	let held = grammars.get(grammar);
	if (held === undefined) {
		if (!(grammar?.wire instanceof Uint8Array)) throw new TypeError('a parse takes `js`, one of its pieces, `css`, or a grammar made by @teasel/parser/grammar');
		held = engine.plan(grammar.wire);
		grammars.set(grammar, held);
	}
	return held;
}
