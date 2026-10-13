import { engine } from '#engine';
import type { Language } from './types.ts';

const grammars = new WeakMap<object, object>();

/** What the engine holds for a grammar: read on first use and kept while the grammar lives. */
export function compiled(grammar: Language<unknown> & { wire?: unknown }): object {
	let held = grammars.get(grammar);
	if (held === undefined) {
		if (!(grammar?.wire instanceof Uint8Array)) throw new TypeError('a parse takes `js`, one of its pieces, `css`, or a grammar made by @teasel/parser/grammar');
		held = engine.plan(grammar.wire);
		grammars.set(grammar, held);
	}
	return held;
}
