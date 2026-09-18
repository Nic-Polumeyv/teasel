export interface Options {
	/** `script` by default, as in acorn. */
	sourceType?: 'script' | 'module';
	/**
	 * Parse TypeScript. `'erase'` parses it and emits JavaScript: annotations, type-only
	 * declarations and imports go, assertions give way to their expression, and what erasure
	 * cannot express (enums, namespaces with values, parameter properties, `export =`, `import =`)
	 * stays in the tree and is listed as `typescript` on the answer, as are the proposals
	 * JavaScript itself has: decorators and accessor fields (`AccessorProperty`).
	 */
	typescript?: boolean | 'erase';
	/**
	 * Which decorators are read. 'legacy' refuses decorators on private elements, class
	 * expressions and their members; 'proposal' refuses parameter decorators and decorators
	 * on abstract or declared fields. Unset reads both syntaxes.
	 */
	decorators?: 'legacy' | 'proposal';
	/** Attach `leadingComments`, `trailingComments` and `innerComments` to nodes, and list every comment read as `comments` on the answer. */
	comments?: boolean;
	/**
	 * Scope analysis: the answer lists `scopes`, `bindings` and `references`, and `scopeOf` and
	 * `referenceOf` answer for a node. The tree itself carries nothing, and a copy of a node
	 * carries no facts. TypeScript type positions bind nothing.
	 */
	scopes?: boolean;
	/** Add `loc` with line and column to every node, as in acorn; off by default. */
	locations?: boolean;
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

// `flag` of json.rs by bit
const FLAG = { module: 1, typescript: 2, erase: 4, comments: 8, scopes: 16, locations: 32, parenthesized: 64, legacyDecorators: 128, proposalDecorators: 256, allowReturnOutsideFunction: 512, allowAwaitOutsideFunction: 1024, allowSuperOutsideMethod: 2048, allowUndeclaredExports: 4096, errorRecovery: 8192 } as const;

const bit = (key: keyof Options & keyof typeof FLAG) => (value: unknown) => {
	if (typeof value !== 'boolean') throw new TypeError(`${key} must be a boolean, not ${JSON.stringify(value)}`);
	return value ? FLAG[key] : 0;
};
const one = (key: keyof Options, choices: Record<string, number>) => (value: unknown) => {
	if (typeof value !== 'string' || !Object.hasOwn(choices, value)) {
		throw new TypeError(`${key} must be ${Object.keys(choices).map((choice) => JSON.stringify(choice)).join(' or ')}, not ${JSON.stringify(value)}`);
	}
	return choices[value];
};
// what each option adds to the word the engine takes, one entry per key of `Options`
type On = { [K in keyof Options]-?: NonNullable<Options[K]> };
const WORD: { [K in keyof On]: (value: On[K]) => number } = {
	sourceType: one('sourceType', { script: 0, module: FLAG.module }),
	typescript: (value) => (value === 'erase' ? FLAG.typescript | FLAG.erase : bit('typescript')(value)),
	decorators: one('decorators', { legacy: FLAG.legacyDecorators, proposal: FLAG.proposalDecorators }),
	comments: bit('comments'),
	scopes: bit('scopes'),
	locations: bit('locations'),
	parenthesized: bit('parenthesized'),
	allowReturnOutsideFunction: bit('allowReturnOutsideFunction'),
	allowAwaitOutsideFunction: bit('allowAwaitOutsideFunction'),
	allowSuperOutsideMethod: bit('allowSuperOutsideMethod'),
	allowUndeclaredExports: bit('allowUndeclaredExports'),
	errorRecovery: bit('errorRecovery'),
};
const known = (key: string): key is keyof Options => Object.hasOwn(WORD, key);
const word = <K extends keyof On>(key: K, value: On[K]) => WORD[key](value);

/** The options that are on, as the word of bits the engine takes. */
export function flags(options: Options = {}): number {
	let on = 0;
	for (const key in options) {
		if (!known(key)) throw new TypeError(`${key} is not an option`);
		const value = options[key];
		if (value !== undefined) on |= word(key, value);
	}
	return on;
}

// `Entry` of parser/mod.rs by index
export const ENTRY = { program: 0, expression: 1, pattern: 2, params: 3, statement: 4, typeParameters: 5 } as const;
export type Entry = keyof typeof ENTRY;
