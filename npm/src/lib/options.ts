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
	/**
	 * Add `loc` with line and column to every node.
	 * @default false
	 */
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

// what each accepted value of an option adds to the word the engine takes, the bits of `flag` in json.rs
type Word<K extends keyof Options> = readonly (readonly [NonNullable<Options[K]>, number])[];
const yes = (bit: number) => [[false, 0], [true, bit]] as const;
const WORD: { [K in keyof Options]-?: Word<K> } = {
	sourceType: [['script', 0], ['module', 1]],
	typescript: [[false, 0], [true, 2], ['erase', 2 | 4]],
	comments: yes(8),
	scopes: yes(16),
	locations: yes(32),
	parenthesized: yes(64),
	decorators: [['legacy', 128], ['proposal', 256]],
	allowReturnOutsideFunction: yes(512),
	allowAwaitOutsideFunction: yes(1024),
	allowSuperOutsideMethod: yes(2048),
	allowUndeclaredExports: yes(4096),
	errorRecovery: yes(8192),
};
const known = (key: string): key is keyof Options => Object.hasOwn(WORD, key);

/** The options that are on, as the word of bits the engine takes. */
export function flags(options: Options = {}): number {
	let on = 0;
	for (const key in options) {
		if (!known(key)) throw new TypeError(`${key} is not an option`);
		const value: unknown = options[key];
		if (value === undefined) continue;
		const words: Word<keyof Options> = WORD[key];
		const found = words.find(([accepted]) => accepted === value);
		if (found === undefined) throw new TypeError(`${key} must be ${words.map(([accepted]) => JSON.stringify(accepted)).join(' or ')}, not ${JSON.stringify(value)}`);
		on |= found[1];
	}
	return on;
}

// `Entry` of parser/mod.rs by index
export const ENTRY = { program: 0, expression: 1, pattern: 2, params: 3, statement: 4, typeParameters: 5 } as const;
export type Entry = keyof typeof ENTRY;
