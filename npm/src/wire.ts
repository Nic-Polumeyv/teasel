// written by crates/teasel/src/host/grammar.rs; `cargo test` pins it

/** The wire: words, then a pool of strings kept once. */
export class Writer {
	#words: number[] = [];
	#strings = new Map<string, number>();
	word(word: number): void {
		this.#words.push(word);
	}
	str(s: string): void {
		let i = this.#strings.get(s);
		if (i === undefined) this.#strings.set(s, (i = this.#strings.size));
		this.#words.push(i);
	}
	/** Little-endian words: the count of record words, the count of strings, each string's offset and length, the record words; then the pool, UTF-8. */
	bytes(): Uint8Array {
		const encoder = new TextEncoder();
		const strings = [...this.#strings.keys()].map((s) => encoder.encode(s));
		const pool = strings.reduce((size, s) => size + s.length, 0);
		const head = 2 + 2 * strings.length;
		const out = new Uint8Array((head + this.#words.length) * 4 + pool);
		const view = new DataView(out.buffer);
		view.setUint32(0, this.#words.length, true);
		view.setUint32(4, strings.length, true);
		let offset = 0;
		strings.forEach((s, i) => {
			view.setUint32(8 + i * 8, offset, true);
			view.setUint32(12 + i * 8, s.length, true);
			out.set(s, (head + this.#words.length) * 4 + offset);
			offset += s.length;
		});
		this.#words.forEach((word, i) => view.setUint32((head + i) * 4, word, true));
		return out;
	}
}

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
	sigils?: Sigils;
	/** An element the browser would close when another opens is closed there. */
	autoclose: boolean;
	/** Whitespace at the end of the source is not part of the document. */
	trim: boolean;
	void: ReadonlyArray<string>;
	/** A node wrapping every list of children, and its field: Svelte's `Fragment`. */
	fragment?: readonly [string, string];
	/** Every list of children opens a scope of its own. */
	fragmentScope: boolean;
	elementFields: ElementFields;
	text: TextRule;
	comment: CommentRule;
	/** The attribute that makes an element's subtree verbatim: text and plain attributes only. */
	verbatim?: string;
	elements: ReadonlyArray<ElementRule>;
	script?: ScriptRule;
	style?: string;
	directiveSyntax?: DirectiveSyntax;
	shorthands: ReadonlyArray<Shorthand>;
	directives: ReadonlyArray<DirectiveRule>;
	spread?: string;
	blocks: ReadonlyArray<BlockRule>;
	tags: ReadonlyArray<TagRule>;
	declaration?: TagRule;
	expression?: TagRule;
};
export function writeGrammar(w: Writer, v: Grammar): void {
	w.str(v.name);
	writeDocumentRule(w, v.document);
	w.str(v.delimiters[0]);
	w.str(v.delimiters[1]);
	w.word(v.attributeExpressions ? 1 : 0);
	w.word(v.attributeShorthand ? 1 : 0);
	if (v.sigils === undefined) w.word(0);
	else {
		w.word(1);
		writeSigils(w, v.sigils);
	}
	w.word(v.autoclose ? 1 : 0);
	w.word(v.trim ? 1 : 0);
	w.word(v.void.length);
	v.void.forEach((item) => {
		w.str(item);
	});
	if (v.fragment === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.fragment[0]);
		w.str(v.fragment[1]);
	}
	w.word(v.fragmentScope ? 1 : 0);
	writeElementFields(w, v.elementFields);
	writeTextRule(w, v.text);
	writeCommentRule(w, v.comment);
	if (v.verbatim === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.verbatim);
	}
	w.word(v.elements.length);
	v.elements.forEach((item) => {
		writeElementRule(w, item);
	});
	if (v.script === undefined) w.word(0);
	else {
		w.word(1);
		writeScriptRule(w, v.script);
	}
	if (v.style === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.style);
	}
	if (v.directiveSyntax === undefined) w.word(0);
	else {
		w.word(1);
		writeDirectiveSyntax(w, v.directiveSyntax);
	}
	w.word(v.shorthands.length);
	v.shorthands.forEach((item) => {
		writeShorthand(w, item);
	});
	w.word(v.directives.length);
	v.directives.forEach((item) => {
		writeDirectiveRule(w, item);
	});
	if (v.spread === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.spread);
	}
	w.word(v.blocks.length);
	v.blocks.forEach((item) => {
		writeBlockRule(w, item);
	});
	w.word(v.tags.length);
	v.tags.forEach((item) => {
		writeTagRule(w, item);
	});
	if (v.declaration === undefined) w.word(0);
	else {
		w.word(1);
		writeTagRule(w, v.declaration);
	}
	if (v.expression === undefined) w.word(0);
	else {
		w.word(1);
		writeTagRule(w, v.expression);
	}
}

export type DocumentRule = {
	ty: string;
	fields: ReadonlyArray<DocField>;
};
export function writeDocumentRule(w: Writer, v: DocumentRule): void {
	w.str(v.ty);
	w.word(v.fields.length);
	v.fields.forEach((item) => {
		writeDocField(w, item);
	});
}

/** A field of the document's root, or a scope around fields. */
export type DocField =
	/** A field, what it holds, and whether it is left out rather than null when there is nothing. */
	| { field: { field: string; holds: RootField; omit: boolean } }
	| { scope: ReadonlyArray<DocField> };
export function writeDocField(w: Writer, v: DocField): void {
	if ('field' in v) {
		w.word(0);
		w.str(v.field.field);
		writeRootField(w, v.field.holds);
		w.word(v.field.omit ? 1 : 0);
	} else {
		w.word(1);
		w.word(v.scope.length);
		v.scope.forEach((item) => {
			writeDocField(w, item);
		});
	}
}

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
export function writeRootField(w: Writer, v: RootField): void {
	if (typeof v === 'string') {
		switch (v) {
			case 'fragment':
				w.word(0);
				break;
			case 'style':
				w.word(2);
				break;
			case 'comments':
				w.word(3);
				break;
			case 'emptyList':
				w.word(4);
				break;
			case 'null':
				w.word(5);
				break;
		}
	} else {
		w.word(1);
		w.word(v.script.module ? 1 : 0);
	}
}

/** The characters after the opening delimiter that make a tag a block, a branch, a close or a
 * special tag: `{#if}`, `{:else}`, `{/if}`, `{@html}`. */
export type Sigils = {
	open: string;
	branch: string;
	close: string;
	tag: string;
};
export function writeSigils(w: Writer, v: Sigils): void {
	w.str(v.open);
	w.str(v.branch);
	w.str(v.close);
	w.str(v.tag);
}

/** The fields of every element node. */
export type ElementFields = {
	name: string;
	attributes: string;
	children: string;
};
export function writeElementFields(w: Writer, v: ElementFields): void {
	w.str(v.name);
	w.str(v.attributes);
	w.str(v.children);
}

/** The type and fields of every text node: the text as read, and as written. */
export type TextRule = {
	ty: string;
	data: string;
	raw?: string;
};
export function writeTextRule(w: Writer, v: TextRule): void {
	w.str(v.ty);
	w.str(v.data);
	if (v.raw === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.raw);
	}
}

export type CommentRule = {
	ty: string;
	data: string;
};
export function writeCommentRule(w: Writer, v: CommentRule): void {
	w.str(v.ty);
	w.str(v.data);
}

export type ElementRule = {
	name: Match;
	ty: string;
	/** The field that takes the expression of a `this` attribute, which leaves the attributes,
	 * and whether text is accepted there as a string. */
	this?: readonly [string, boolean];
	root: boolean;
	once: boolean;
	inside?: string;
	/** Not when an enclosing element carries this attribute. */
	outside?: string;
	/** The content is text up to the closing tag, a script's say. */
	raw: boolean;
	/** The content is text with the host's expressions in it, a textarea's say. */
	rcdata: boolean;
};
export function writeElementRule(w: Writer, v: ElementRule): void {
	writeMatch(w, v.name);
	w.str(v.ty);
	if (v.this === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.this[0]);
		w.word(v.this[1] ? 1 : 0);
	}
	w.word(v.root ? 1 : 0);
	w.word(v.once ? 1 : 0);
	if (v.inside === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.inside);
	}
	if (v.outside === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.outside);
	}
	w.word(v.raw ? 1 : 0);
	w.word(v.rcdata ? 1 : 0);
}

export type Match =
	| { exact: string }
	/** A capitalized or dotted name. */
	| 'component'
	| 'any';
export function writeMatch(w: Writer, v: Match): void {
	if (typeof v === 'string') {
		switch (v) {
			case 'component':
				w.word(1);
				break;
			case 'any':
				w.word(2);
				break;
		}
	} else {
		w.word(0);
		w.str(v.exact);
	}
}

export type ScriptRule = {
	name: string;
	/** Attributes that make the script the module one, each with the text value it needs, if any. */
	module: ReadonlyArray<readonly [string, string | undefined]>;
	/** Attributes that make the document TypeScript, the same way. */
	typescript: ReadonlyArray<readonly [string, string | undefined]>;
};
export function writeScriptRule(w: Writer, v: ScriptRule): void {
	w.str(v.name);
	w.word(v.module.length);
	v.module.forEach((item) => {
		w.str(item[0]);
		if (item[1] === undefined) w.word(0);
		else {
			w.word(1);
			w.str(item[1]);
		}
	});
	w.word(v.typescript.length);
	v.typescript.forEach((item) => {
		w.str(item[0]);
		if (item[1] === undefined) w.word(0);
		else {
			w.word(1);
			w.str(item[1]);
		}
	});
}

/** How a directive's attribute name is spelled: `prefix name arg modifiers`, the name being the
 * directive's own when there is no prefix. */
export type DirectiveSyntax = {
	prefix?: string;
	/** What separates the argument, `:`. */
	arg: string;
	/** What separates the modifiers, `|` or `.`. */
	modifier: string;
	/** The brackets of an argument that is an expression, `[` `]`. */
	dynamic?: readonly [string, string];
	nameField?: string;
	argField?: string;
	modifiersField?: string;
	rawField?: string;
	/** Every directive is unique by its whole attribute name. */
	unique: boolean;
};
export function writeDirectiveSyntax(w: Writer, v: DirectiveSyntax): void {
	if (v.prefix === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.prefix);
	}
	w.str(v.arg);
	w.str(v.modifier);
	if (v.dynamic === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.dynamic[0]);
		w.str(v.dynamic[1]);
	}
	if (v.nameField === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.nameField);
	}
	if (v.argField === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.argField);
	}
	if (v.modifiersField === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.modifiersField);
	}
	if (v.rawField === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.rawField);
	}
	w.word(v.unique ? 1 : 0);
}

/** A character standing for a directive's prefix and name, `:` for `v-bind`. */
export type Shorthand = {
	token: string;
	name: string;
	modifiers: ReadonlyArray<string>;
};
export function writeShorthand(w: Writer, v: Shorthand): void {
	w.str(v.token);
	w.str(v.name);
	w.word(v.modifiers.length);
	v.modifiers.forEach((item) => {
		w.str(item);
	});
}

export type DirectiveRule = {
	name: Match;
	ty: string;
	value: DirectiveValue;
	flags: ReadonlyArray<readonly [string, boolean]>;
	unique: Unique;
	/** What the directive declares in the scope of its element: the fields of its form, or,
	 * with none named, its value. */
	declares?: ReadonlyArray<string>;
};
export function writeDirectiveRule(w: Writer, v: DirectiveRule): void {
	writeMatch(w, v.name);
	w.str(v.ty);
	writeDirectiveValue(w, v.value);
	w.word(v.flags.length);
	v.flags.forEach((item) => {
		w.str(item[0]);
		w.word(item[1] ? 1 : 0);
	});
	writeUnique(w, v.unique);
	if (v.declares === undefined) w.word(0);
	else {
		w.word(1);
		w.word(v.declares.length);
		v.declares.forEach((item) => {
			w.str(item);
		});
	}
}

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
export function writeDirectiveValue(w: Writer, v: DirectiveValue): void {
	if (typeof v === 'string') {
		switch (v) {
			case 'value':
				w.word(2);
				break;
		}
	} else if ('expression' in v) {
		w.word(0);
		w.word(v.expression.optional ? 1 : 0);
		w.word(v.expression.name ? 1 : 0);
	} else if ('pattern' in v) {
		w.word(1);
		w.word(v.pattern.optional ? 1 : 0);
		w.word(v.pattern.name ? 1 : 0);
	} else {
		w.word(3);
		writeForm(w, v.form);
	}
}

export type Form = {
	items: ReadonlyArray<Item>;
	body?: Body;
};
export function writeForm(w: Writer, v: Form): void {
	w.word(v.items.length);
	v.items.forEach((item) => {
		writeItem(w, item);
	});
	if (v.body === undefined) w.word(0);
	else {
		w.word(1);
		writeBody(w, v.body);
	}
}

/** One step of a form. */
export type Item =
	/** One of the host's words or punctuators. */
	| { literal: string }
	/** A JavaScript entry read into a field; `omit` leaves the field out when the entry was not
	 * read, where the plain form gives it null. */
	| { entry: { field: string; entry: Entry; omit: boolean } }
	/** Alternatives tried in order: at most one, or exactly one when `required`. */
	| { group: { alternatives: ReadonlyArray<Alternative>; required: boolean } };
export function writeItem(w: Writer, v: Item): void {
	if ('literal' in v) {
		w.word(0);
		w.str(v.literal);
	} else if ('entry' in v) {
		w.word(1);
		w.str(v.entry.field);
		writeEntry(w, v.entry.entry);
		w.word(v.entry.omit ? 1 : 0);
	} else {
		w.word(2);
		w.word(v.group.alternatives.length);
		v.group.alternatives.forEach((item) => {
			writeAlternative(w, item);
		});
		w.word(v.group.required ? 1 : 0);
	}
}

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
export function writeEntry(w: Writer, v: Entry): void {
	if (typeof v === 'string') {
		switch (v) {
			case 'expression':
				w.word(0);
				break;
			case 'pattern':
				w.word(1);
				break;
			case 'params':
				w.word(2);
				break;
			case 'identifier':
				w.word(3);
				break;
			case 'typeParameters':
				w.word(4);
				break;
			case 'statement':
				w.word(5);
				break;
			case 'code':
				w.word(6);
				break;
			case 'const':
				w.word(7);
				break;
			case 'identifiers':
				w.word(8);
				break;
			case 'text':
				w.word(9);
				break;
		}
	}
}

export type Alternative = {
	items: ReadonlyArray<Item>;
	/** The body the block opens when this alternative was read, `[ then value=pattern -> then ]`. */
	body?: Body;
};
export function writeAlternative(w: Writer, v: Alternative): void {
	w.word(v.items.length);
	v.items.forEach((item) => {
		writeItem(w, item);
	});
	if (v.body === undefined) w.word(0);
	else {
		w.word(1);
		writeBody(w, v.body);
	}
}

/** What a block's body is: the field that holds it, and what the body's scope declares. */
export type Body = {
	field: string;
	/** The field is left out of blocks that never opened this body; otherwise it is null there. */
	omit: boolean;
	/** A branch that nests a new block of the same kind into the field, `{:else if}`: the field
	 * of the nested block that its own body fills. */
	chain?: string;
	declares: ReadonlyArray<Declare>;
};
export function writeBody(w: Writer, v: Body): void {
	w.str(v.field);
	w.word(v.omit ? 1 : 0);
	if (v.chain === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.chain);
	}
	w.word(v.declares.length);
	v.declares.forEach((item) => {
		writeDeclare(w, item);
	});
}

export type Declare = {
	field: string;
	/** Declared in the scope around the block rather than inside it: a snippet's name. */
	outside: boolean;
};
export function writeDeclare(w: Writer, v: Declare): void {
	w.str(v.field);
	w.word(v.outside ? 1 : 0);
}

/** Which names a directive may not repeat on an element. */
export type Unique =
	| 'no'
	/** Its own argument, among directives of its kind. */
	| 'kind'
	/** Its argument, among the plain attributes too. */
	| 'attribute';
export function writeUnique(w: Writer, v: Unique): void {
	if (typeof v === 'string') {
		switch (v) {
			case 'no':
				w.word(0);
				break;
			case 'kind':
				w.word(1);
				break;
			case 'attribute':
				w.word(2);
				break;
		}
	}
}

export type BlockRule = {
	name: string;
	ty: string;
	open: Form;
	branches: ReadonlyArray<BranchRule>;
	/** The boolean field that says the block was opened by a chained branch. */
	chainFlag?: string;
};
export function writeBlockRule(w: Writer, v: BlockRule): void {
	w.str(v.name);
	w.str(v.ty);
	writeForm(w, v.open);
	w.word(v.branches.length);
	v.branches.forEach((item) => {
		writeBranchRule(w, item);
	});
	if (v.chainFlag === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.chainFlag);
	}
}

export type BranchRule = {
	words: ReadonlyArray<string>;
	form: Form;
};
export function writeBranchRule(w: Writer, v: BranchRule): void {
	w.word(v.words.length);
	v.words.forEach((item) => {
		w.str(item);
	});
	writeForm(w, v.form);
}

export type TagRule = {
	name: string;
	ty: string;
	form: Form;
	/** The tag stands among an element's attributes rather than in content. */
	attribute: boolean;
};
export function writeTagRule(w: Writer, v: TagRule): void {
	w.str(v.name);
	w.str(v.ty);
	writeForm(w, v.form);
	w.word(v.attribute ? 1 : 0);
}
