import type { Comment, Node, Program } from 'estree';
import { decode, PARENT, REFERENCE, SCOPE } from './decode.ts';
import type { HostNode, Parsed, Recovered, Reference, Scope } from './types.ts';
import { flags, MODULE, OFFERED, OFFERED_CSS, type CssOptions, type Options } from './options.ts';
import { engine } from '#engine';
import { svelte as own } from './children.ts';
import type { Root, SvelteNode } from './svelte.ts';

export type { CssOptions, Options } from './options.ts';
export type { Arguments, Binding, Code, Comment, Declared, HostNode, JsRoot, Kept, Parsed, Recovered, Reference, Scope, Span } from './types.ts';
export type * as AST from './svelte.ts';

// what the decoder hangs on a node, under keys JSON and enumeration skip
const slot = (node: object | null | undefined, key: symbol) => (node as Record<symbol, any> | null | undefined)?.[key];

/** The node `node` is a child of: for a comment, the node it is attached to, else the node that lists it; undefined for the root of an answer and for a comment no node holds. A literal's `regex` and a template element's `value` are not nodes and have none. */
export function parentOf(node: Node | SvelteNode | HostNode | Comment | null | undefined): Node | SvelteNode | HostNode | undefined {
	return slot(node, PARENT);
}
/** With `scopes`: the scope `node` opens, when it opens one. */
export function scopeOf(node: Node | SvelteNode | HostNode | null | undefined): Scope | undefined {
	return slot(node, SCOPE);
}
/** With `scopes`: the reference an identifier makes, the binding itself for the identifier that declares it; a global's too, which no binding lists. Undefined when the identifier names no value, a property key say. */
export function referenceOf(node: Node | null | undefined): Reference | undefined {
	return slot(node, REFERENCE);
}

/** For every type of node a parse answers with, the fields that hold a node or a list of nodes: what a walk follows. */
export const children = own;
/** The fields TypeScript may add to a node of any type, holding nodes: annotations, type parameters and arguments, what a class implements, decorators. */
export { extras } from './children.ts';

// the engine's numbers for what it reads
const JAVASCRIPT = 0, CSS = 1, SVELTE = 2;

function read<T>(text: string, word: number, language: number): Parsed<T> {
	if (typeof text !== 'string') throw new TypeError('a parse takes a string');
	const words = engine.parse(text, word, language);
	let error: Omit<Recovered, 'line' | 'column'>;
	try {
		const parsed = decode(words, text, engine) as Parsed<T>;
		if (parsed.node !== null) return parsed;
		error = parsed.errors![0];
	} catch (thrown) {
		// a tree deeper than the caller's stack has room for overflowed the decoder
		if (!(thrown instanceof RangeError)) throw thrown;
		error = { code: 'nesting_depth', args: [], pos: 0, end: 0 };
	}
	throw Object.assign(new SyntaxError(error.args.reduce((text: string, arg) => `${text} '${arg}'`, error.code)), error);
}

/**
 * A Svelte component, the whole of `text`: its `Root`, the markup's nodes and the JavaScript
 * inside them in one tree, its scripts read as modules, in TypeScript when a script tag says
 * `lang="ts"`. Positions are UTF-16 offsets. A lone surrogate in the text reaches the parser as
 * U+FFFD, the way `TextEncoder` encodes it, so string values, template cooked and raw text and
 * regex patterns carry U+FFFD there while spans and `locate` are right.
 */
export function parse(text: string, options?: Options): Parsed<Root> {
	return read(text, flags(options, OFFERED), SVELTE);
}

/** JavaScript or TypeScript, the whole of `text` as a module: its `Program`. */
export function parseModule(text: string, options?: Options): Parsed<Program> {
	return read(text, flags(options, OFFERED) | MODULE, JAVASCRIPT);
}

/** CSS, the whole of `text` as a stylesheet: a `StyleSheet` of rules and at-rules with its comments listed. */
export function parseCss(text: string, options?: CssOptions): Parsed<HostNode> {
	return read(text, flags(options, OFFERED_CSS), CSS);
}
