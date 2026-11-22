import { LANGUAGE, type HostNode, type Language } from './types.ts';
import { engine } from '#engine';
import { children as builtin, frozen } from './children.ts';

// what the engine tells of the walker written for Svelte, beside the tree's layout, as `about` tells of a grammar
const about: { children: Record<string, string[]>; names: string[] } = JSON.parse(engine.layout()).svelte;

/**
 * Svelte, read by the walker written for it instead of a grammar: the tree the Svelte grammar's
 * walker answers with. Entry 8 of the engine; not exported from the package yet.
 */
export const svelte: Language<HostNode> = Object.freeze({
	children: frozen({ ...builtin, ...about.children }),
	[LANGUAGE]: Object.freeze({ entry: 8, names: Object.freeze(about.names) }),
});
