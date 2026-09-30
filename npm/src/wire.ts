// written by crates/teasel/src/host/grammar.rs; `cargo test` pins it

/** A host language: its document, its content, and the JavaScript inside them. */
export type Grammar = {
	name: string;
	document: DocumentRule;
	/** What opens and closes an expression in text, `{` and `}`. */
	delimiters: readonly [string, string];
	/** Attribute values hold expressions between the delimiters, as text does. */
	attributeExpressions: boolean;
	/** `{name}` among the attributes is `name={name}`. */
	attributeShorthand: boolean;
	sigils?: Sigils | null;
	/** An element the browser would close when another opens is closed there. */
	autoclose: boolean;
	/** Whitespace at the end of the source is not part of the document. */
	trim: boolean;
	void: ReadonlyArray<string>;
	/** A node wrapping every list of children, and its field: Svelte's `Fragment`. */
	fragment?: readonly [string, string] | null;
	/** Every list of children opens a scope of its own. */
	fragmentScope: boolean;
	elementFields: ElementFields;
	text: TextRule;
	comment: CommentRule;
	/** The attribute that makes an element's subtree verbatim: text and plain attributes only. */
	verbatim?: string | null;
	elements: ReadonlyArray<ElementRule>;
	script?: ScriptRule | null;
	style?: string | null;
	directiveSyntax?: DirectiveSyntax | null;
	shorthands: ReadonlyArray<Shorthand>;
	directives: ReadonlyArray<DirectiveRule>;
	spread?: string | null;
	blocks: ReadonlyArray<BlockRule>;
	tags: ReadonlyArray<TagRule>;
	declaration?: TagRule | null;
	expression?: TagRule | null;
};

export type DocumentRule = {
	ty: string;
	fields: ReadonlyArray<DocField>;
};

/** A field of the document's root, or a scope around fields. */
export type DocField =
	/** A field, what it holds, and whether it is left out rather than null when there is nothing. */
	| { field: { field: string; holds: RootField; omit: boolean } }
	| { scope: ReadonlyArray<DocField> };

/** What a field of the document's root holds. */
export type RootField =
	/** The document's nodes. */
	| 'fragment'
	/** The script, the module one when `module`. */
	| { script: { module: boolean } }
	| 'style'
	/** Every comment read. */
	| 'comments'
	| 'emptyList'
	| 'null';

/** The characters after the opening delimiter that make a tag a block, a branch, a close or a
 * special tag: `{#if}`, `{:else}`, `{/if}`, `{@html}`. */
export type Sigils = {
	open: string;
	branch: string;
	close: string;
	tag: string;
};

/** The fields of every element node. */
export type ElementFields = {
	name: string;
	attributes: string;
	children: string;
};

/** The type and fields of every text node: the text as read, and as written. */
export type TextRule = {
	ty: string;
	data: string;
	raw?: string | null;
};

export type CommentRule = {
	ty: string;
	data: string;
};

export type ElementRule = {
	name: Match;
	ty: string;
	/** The field that takes the expression of a `this` attribute, which leaves the attributes,
	 * and whether text is accepted there as a string. */
	this?: readonly [string, boolean] | null;
	root: boolean;
	once: boolean;
	inside?: string | null;
	/** Not when an enclosing element carries this attribute. */
	outside?: string | null;
	/** The content is text up to the closing tag, a script's say. */
	raw: boolean;
	/** The content is text with the host's expressions in it, a textarea's say. */
	rcdata: boolean;
};

export type Match =
	| { exact: string }
	/** A capitalized or dotted name. */
	| 'component'
	| 'any';

export type ScriptRule = {
	name: string;
	/** Attributes that make the script the module one, each with the text value it needs, if any. */
	module: ReadonlyArray<readonly [string, string | null]>;
	/** Attributes that make the document TypeScript, the same way. */
	typescript: ReadonlyArray<readonly [string, string | null]>;
};

/** How a directive's attribute name is spelled: `prefix name arg modifiers`, the name being the
 * directive's own when there is no prefix. */
export type DirectiveSyntax = {
	prefix?: string | null;
	/** What separates the argument, `:`. */
	arg: string;
	/** What separates the modifiers, `|` or `.`. */
	modifier: string;
	/** The brackets of an argument that is an expression, `[` `]`. */
	dynamic?: readonly [string, string] | null;
	nameField?: string | null;
	argField?: string | null;
	modifiersField?: string | null;
	rawField?: string | null;
	/** Every directive is unique by its whole attribute name. */
	unique: boolean;
};

/** A character standing for a directive's prefix and name, `:` for `v-bind`. */
export type Shorthand = {
	token: string;
	name: string;
	modifiers: ReadonlyArray<string>;
};

export type DirectiveRule = {
	name: Match;
	ty: string;
	value: DirectiveValue;
	flags: ReadonlyArray<readonly [string, boolean]>;
	unique: Unique;
	/** What the directive declares in the scope of its element: the fields of its form, or,
	 * with none named, its value. */
	declares?: ReadonlyArray<string> | null;
};

export type DirectiveValue =
	/** The one expression of the attribute value, `on:click={handler}`; `name` makes the
	 * directive's own argument the expression when there is no value: `bind:value`. */
	| { expression: { optional: boolean; name: boolean } }
	/** The one pattern of the attribute value, `let:item={{ id }}`. */
	| { pattern: { optional: boolean; name: boolean } }
	/** The attribute value as it is, text and expressions. */
	| 'value'
	/** The attribute value read by a form, `v-for="item in items"`. */
	| { form: Form };

export type Form = {
	items: ReadonlyArray<Item>;
	body?: Body | null;
};

/** One step of a form. */
export type Item =
	/** One of the host's words or punctuators. */
	| { literal: string }
	/** A JavaScript entry read into a field; `omit` leaves the field out when the entry was not
	 * read, where the plain form gives it null. */
	| { entry: { field: string; entry: Entry; omit: boolean } }
	/** Alternatives tried in order: at most one, or exactly one when `required`. */
	| { group: { alternatives: ReadonlyArray<Alternative>; required: boolean } };

/** A JavaScript entry inside a form. */
export type Entry =
	| 'expression'
	| 'pattern'
	| 'params'
	| 'identifier'
	| 'typeParameters'
	| 'statement'
	/** An expression, or, when what holds it is not one, its statements as a program. */
	| 'code'
	/** `pattern = expression`, a const declaration the host spells without the keyword. */
	| 'const'
	/** Identifiers separated by commas, possibly none. */
	| 'identifiers'
	/** The text up to the closing delimiter, unread, for a host that reads its expressions later. */
	| 'text';

export type Alternative = {
	items: ReadonlyArray<Item>;
	/** The body the block opens when this alternative was read, `[ then value=pattern -> then ]`. */
	body?: Body | null;
};

/** What a block's body is: the field that holds it, and what the body's scope declares. */
export type Body = {
	field: string;
	/** The field is left out of blocks that never opened this body; otherwise it is null there. */
	omit: boolean;
	/** A branch that nests a new block of the same kind into the field, `{:else if}`: the field
	 * of the nested block that its own body fills. */
	chain?: string | null;
	declares: ReadonlyArray<Declare>;
};

export type Declare = {
	field: string;
	/** Declared in the scope around the block rather than inside it: a snippet's name. */
	outside: boolean;
};

/** Which names a directive may not repeat on an element. */
export type Unique =
	| 'no'
	/** Its own argument, among directives of its kind. */
	| 'kind'
	/** Its argument, among the plain attributes too. */
	| 'attribute';

export type BlockRule = {
	name: string;
	ty: string;
	open: Form;
	branches: ReadonlyArray<BranchRule>;
	/** The boolean field that says the block was opened by a chained branch. */
	chainFlag?: string | null;
};

export type BranchRule = {
	words: ReadonlyArray<string>;
	form: Form;
};

export type TagRule = {
	name: string;
	ty: string;
	form: Form;
	/** The tag stands among an element's attributes rather than in content. */
	attribute: boolean;
};
