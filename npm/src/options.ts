export interface Options {
	/** @default 'script' */
	sourceType?: 'script' | 'module';
	/**
	 * Parse TypeScript. `'erase'` parses it and emits JavaScript: annotations, type-only
	 * declarations and imports go, assertions give way to their expression, and what erasure
	 * cannot express (enums, namespaces with values, parameter properties, `export =`, `import =`)
	 * stays in the tree and is listed as `typescript` on the answer, as are the proposals
	 * JavaScript itself has: decorators and accessor fields (`AccessorProperty`).
	 */
	typescript?: boolean | 'erase';
	/** Attach `leadingComments`, `trailingComments` and `innerComments` to nodes, and list every comment read as `comments` on the answer. */
	comments?: boolean;
	/**
	 * Scope analysis: the answer lists `scopes`, `bindings` and `references`, and `scopeOf` and
	 * `referenceOf` answer for a node. The tree itself carries nothing, and a copy of a node
	 * carries no facts. TypeScript type positions bind nothing.
	 */
	scopes?: boolean;
	/**
	 * Add `loc` with line and column to every node. `'js'` adds it to the JavaScript nodes and
	 * comments only: a host's own nodes, a stylesheet's included, keep `start` and `end`.
	 * @default false
	 */
	locations?: boolean | 'js';
	/** Mark a node the source wraps in parens with `parenthesized: true`, absent otherwise. */
	parenthesized?: boolean;
	allowReturnOutsideFunction?: boolean;
	allowAwaitOutsideFunction?: boolean;
	allowSuperOutsideMethod?: boolean;
	allowUndeclaredExports?: boolean;
	/**
	 * List syntax errors on the answer as `errors` instead of throwing the first: a missing
	 * operand, name or pattern is an `Identifier` named `''` of no width where it was expected,
	 * and a statement or entry that cannot be read is skipped to the next stop token or
	 * unmatched closing bracket, an empty identifier standing for it. Placeholders are neither
	 * bindings nor references.
	 */
	errorRecovery?: boolean;
}

// the engine's word: two bits per option, in this order, holding the index of its value; `flag` in json.rs lays it out the same way
const ACCEPTED: { [K in keyof Options]-?: readonly NonNullable<Options[K]>[] } = {
	sourceType: ['script', 'module'],
	typescript: [false, true, 'erase'],
	comments: [false, true],
	scopes: [false, true],
	locations: [false, true, 'js'],
	parenthesized: [false, true],
	allowReturnOutsideFunction: [false, true],
	allowAwaitOutsideFunction: [false, true],
	allowSuperOutsideMethod: [false, true],
	allowUndeclaredExports: [false, true],
	errorRecovery: [false, true],
};
const SLOT = Object.fromEntries(Object.keys(ACCEPTED).map((key, slot) => [key, slot])) as Record<keyof Options, number>;
const known = (key: string): key is keyof Options => Object.hasOwn(ACCEPTED, key);

/** The options that are on, as the word the engine takes. */
export function flags(options: Options = {}): number {
	let on = 0;
	for (const key in options) {
		if (!known(key)) throw new TypeError(`${key} is not an option`);
		const value: unknown = options[key];
		if (value === undefined) continue;
		const accepted: readonly unknown[] = ACCEPTED[key];
		const index = accepted.indexOf(value);
		if (index < 0) throw new TypeError(`${key} must be ${accepted.map((a) => JSON.stringify(a)).join(' or ')}, not ${JSON.stringify(value)}`);
		on |= index << (2 * SLOT[key]);
	}
	return on;
}
