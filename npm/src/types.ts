// The types the package answers with and the contract its engines meet, each defined once and
// read by the entry, the decoder, the engines and the grammar builders.
import type { Expression, Identifier, Node, SourceLocation } from 'estree';
import type { Code } from './codes.js';

/** A scope, as one of `scopes` on the answer. */
export interface Scope {
	kind:
		| 'module'
		| 'script'
		| 'function'
		| 'function-name'
		| 'class'
		| 'block'
		| 'catch'
		| 'for'
		| 'switch'
		| 'static-block'
		| 'with'
		| 'namespace'
		| 'enum'
		| 'fragment';
	/** The node that opens it; null for a function-name scope and for the scope around a parameter list parsed on its own. */
	node: Node | null;
	parent: Scope | null;
	/** An `await` or `for await` runs directly in it, no function around; only a program or fragment scope can say so. */
	topLevelAwait: boolean;
}

/** A binding, as one of `bindings` on the answer: one an identifier declares, or the `arguments` a function reads. */
export type Binding = Declared | Arguments;

/** A binding an identifier declares. It is the reference that identifier makes, the first of its own: `referenceOf` answers with it, and its `binding` is itself. */
export interface Declared extends Reference {
	name: string;
	/**
	 * What declared it. `function-name` and `class-name` are the name a function expression or a class
	 * expression has inside itself, `const f = function g() {}` declaring `g`. `pattern` is a name that
	 * a `Plan.pattern` piece declares, parsed on its own; a `Plan.params` piece declares `param`s.
	 */
	kind:
		| 'var'
		| 'let'
		| 'const'
		| 'using'
		| 'await using'
		| 'function'
		| 'class'
		| 'param'
		| 'catch'
		| 'import'
		| 'function-name'
		| 'class-name'
		| 'enum'
		| 'enum-member'
		| 'namespace'
		| 'pattern';
	/** The identifier that declares it. */
	node: Identifier;
	/** The scope it is declared in. */
	scope: Scope;
	/** What declares it: the declarator, function, class, import specifier, catch clause or enum, as eslint-scope's definition node; null for a pattern or parameter list parsed on its own. */
	declaration: Node | null;
	binding: Declared;
	declares: true;
	/** The declaration binds a value: an initializer, a parameter, a function, a class, an import; not a bare `let x;`. */
	write: boolean;
	read: false;
	mutate: false;
	/** The initializer of a declarator, `1` in `let x = 1`; null otherwise, the iterated expression of a `for-of` and a parameter's default being on the tree. */
	writeExpr: Expression | null;
}

/** The `arguments` of a function that reads it: bound by the call, declared by no identifier. */
export interface Arguments {
	name: 'arguments';
	kind: 'arguments';
	scope: Scope;
	node: null;
	declaration: null;
	binding: Arguments;
	declares: true;
	write: true;
	read: false;
	mutate: false;
	writeExpr: null;
}

/** A piece of JavaScript a host read on its own, as one of `roots` on a document's answer, with what the tables hold for it. */
export interface Root {
	node: Node;
	/** The scope the piece sits in. */
	scope: Scope;
	/** The scopes opened inside it, the bindings declared and the references made there. */
	scopes: Scope[];
	bindings: Binding[];
	references: Reference[];
}

/** A reference, as one of `references` on the answer: an identifier using a name, or declaring it again. A binding is one too, the reference its declaring identifier makes. */
export interface Reference {
	node: Identifier;
	/** The scope the reference is made from. */
	scope: Scope;
	/** Null for a global. */
	binding: Binding | null;
	/** The identifier is assigned to, updated or bound by a destructuring assignment. */
	write: boolean;
	/** A member of the identifier's value is assigned to, updated or deleted. */
	mutate: boolean;
	/** The identifier's value is read: every reference but a declaration, a plain assignment's target or a destructuring one's; a compound assignment or an update reads and writes. */
	read: boolean;
	/** What a write assigns: the right side of the assignment, the iterated expression of a `for-in` or `for-of`, or what a declaration is initialized with, as eslint-scope's `writeExpr`; null for an update. */
	writeExpr: Expression | null;
	/** The identifier declares its binding: the binding itself for the first declaration, and a reference of its own for a name declared again, `var x` twice, which writes when a value is bound there. */
	declares: boolean;
}

/** A range of the source, with `loc` when `locations` is on. */
export interface Span {
	start: number;
	end: number;
	loc?: { start: { line: number; column: number }; end: { line: number; column: number } };
}

export interface Comment extends Span {
	type: 'Line' | 'Block';
	value: string;
}

/** A node erasure left in place, by type. */
export interface Kept extends Span {
	type: string;
}

/** A recovered error: what the thrown `SyntaxError` carries, as a plain object. */
export interface Recovered {
	code: Code;
	message: string;
	pos: number;
	end: number;
	loc: { line: number; column: number };
}

/** What a parse returns: the node, or the patterns of a parameter list, and what the options add; a key is there exactly when its option is on. */
export interface Parsed<T> {
	node: T;
	/** The offset after everything the parse consumed: the node, its closing parens and the comments after it; a program's is the end it was given. */
	end: number;
	/** Every comment read, in source order; with `comments`. */
	comments?: Comment[];
	/** What erasure left in place; with `typescript: 'erase'`. */
	typescript?: Kept[];
	/** The errors recovered from, in source order; with `errorRecovery`. */
	errors?: Recovered[];
	/** With `scopes`. */
	scopes?: Scope[];
	bindings?: Binding[];
	references?: Reference[];
	/** With `scopes`, for a document read by a host grammar: its pieces of JavaScript in source order. */
	roots?: Root[];
}

/**
 * A node of a host language, as its grammar names the type and the fields; the JavaScript under
 * it is ESTree. The node a grammar wraps children in has no span.
 */
export interface HostNode {
	type: string;
	start?: number;
	end?: number;
	loc?: SourceLocation;
	[field: string]: unknown;
}

// ── the engine

export type View = Uint32Array | Float64Array | Uint8Array;
/** Whether the tree is the TypeScript one, then each view of the layout's `views`, as long as its buffer's room; `undefined` for a table no parse filled yet. */
export type Tree = readonly (View | number | undefined)[];

/** What parses, as the reader sees it. */
export interface Views {
	/** The tree's memory layout, the names of its views and the recipes, as JSON. */
	readonly layout: () => string;
	/** The views of the last parse's tree, JavaScript's or TypeScript's; `moved` when a buffer of it has since this reader last took them. The same array as long as no view in it changed, `moved` aside. */
	readonly tree: (typescript: boolean, moved: boolean) => Tree;
}

/** What the engine holds: a prepared source, or a host language's grammar read once. */
export interface Held {
	readonly free: () => void;
}

/** A source the engine prepared: it parses at an entry and offset, cut at `end`, the stop tokens as one string, the whole source as a document by a grammar `plan` holds; the answer is its words, or an error as JSON. */
export interface Prepared extends Held {
	readonly parse: (entry: number, offset: number, end: number | undefined, stop: string, plan: Held | undefined) => Uint32Array | string;
}

/** What parses: the addon or the WebAssembly module. */
export interface Engine extends Views {
	readonly create: (source: string, flags: number) => Prepared;
	/** The grammar of a host language on its wire, read once. */
	readonly plan: (grammar: Uint8Array) => Held;
}
