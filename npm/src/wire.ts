// written by crates/teasel/src/host/grammar.rs; `cargo test` pins it

/** The wire: words, then a pool of strings kept once. */
export class Writer {
	#words = new Uint32Array(1024);
	#count = 0;
	#strings = new Map<string, number>();
	#pool: string[] = [];
	word(word: number): void {
		if (this.#count === this.#words.length) {
			const more = new Uint32Array(this.#count * 2);
			more.set(this.#words);
			this.#words = more;
		}
		this.#words[this.#count++] = word;
	}
	str(s: string): void {
		let i = this.#strings.get(s);
		if (i === undefined) {
			this.#strings.set(s, (i = this.#pool.length));
			this.#pool.push(s);
		}
		this.word(i);
	}
	/** Little-endian words: the count of record words, the count of strings, each string's offset and length, the record words; then the pool, UTF-8. */
	bytes(): Uint8Array {
		const head = 2 + 2 * this.#pool.length;
		const poolAt = (head + this.#count) * 4;
		const out = new Uint8Array(poolAt + this.#pool.reduce((size, s) => size + s.length * 3, 0));
		const words = new Uint32Array(out.buffer, 0, head + this.#count);
		words[0] = this.#count;
		words[1] = this.#pool.length;
		const encoder = new TextEncoder();
		let offset = 0;
		this.#pool.forEach((s, i) => {
			const { written } = encoder.encodeInto(s, out.subarray(poolAt + offset));
			words[2 + 2 * i] = offset;
			words[3 + 2 * i] = written;
			offset += written;
		});
		words.set(this.#words.subarray(0, this.#count), head);
		// the engine reads little-endian words; a big-endian platform swaps them here
		if (new Uint8Array(new Uint32Array([1]).buffer)[0] !== 1) {
			const view = new DataView(out.buffer);
			words.forEach((word, i) => view.setUint32(i * 4, word, true));
		}
		return out.subarray(0, poolAt + offset);
	}
}

/** What crosses: the host's name and its definition. */
export type Host = {
	name: string;
	definition: Definition;
};
export function writeHost(w: Writer, v: Host): void {
	w.str(v.name);
	writeDefinition(w, v.definition);
}

/** A host language: its document, its content, and the JavaScript inside them. */
export type Definition = {
	document: Node;
	text: Node;
	comment: Node;
	fragment?: Node;
	delimiters: readonly [string, string];
	attributes?: Attributes;
	autoclose?: boolean;
	trim?: boolean;
	void?: ReadonlyArray<string>;
	verbatim?: string;
	elements: Elements;
	script?: Script;
	style?: string;
	directives?: Directives;
	spread?: string;
	sigils?: Sigils;
	declaration?: Node;
	expression?: Node;
};
export function writeDefinition(w: Writer, v: Definition): void {
	writeNode(w, v.document);
	writeNode(w, v.text);
	writeNode(w, v.comment);
	if (v.fragment === undefined) w.word(0);
	else {
		w.word(1);
		writeNode(w, v.fragment);
	}
	w.str(v.delimiters[0]);
	w.str(v.delimiters[1]);
	if (v.attributes === undefined) w.word(0);
	else {
		w.word(1);
		writeAttributes(w, v.attributes);
	}
	if (v.autoclose === undefined) w.word(0);
	else {
		w.word(1);
		w.word(v.autoclose ? 1 : 0);
	}
	if (v.trim === undefined) w.word(0);
	else {
		w.word(1);
		w.word(v.trim ? 1 : 0);
	}
	if (v.void === undefined) w.word(0);
	else {
		w.word(1);
		w.word(v.void.length);
		v.void.forEach((item) => {
			w.str(item);
		});
	}
	if (v.verbatim === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.verbatim);
	}
	writeElements(w, v.elements);
	if (v.script === undefined) w.word(0);
	else {
		w.word(1);
		writeScript(w, v.script);
	}
	if (v.style === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.style);
	}
	if (v.directives === undefined) w.word(0);
	else {
		w.word(1);
		writeDirectives(w, v.directives);
	}
	if (v.spread === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.spread);
	}
	if (v.sigils === undefined) w.word(0);
	else {
		w.word(1);
		writeSigils(w, v.sigils);
	}
	if (v.declaration === undefined) w.word(0);
	else {
		w.word(1);
		writeNode(w, v.declaration);
	}
	if (v.expression === undefined) w.word(0);
	else {
		w.word(1);
		writeNode(w, v.expression);
	}
}

/** A node type and the form its fields come from. */
export type Node = {
	type: string;
	items: ReadonlyArray<Item>;
};
export function writeNode(w: Writer, v: Node): void {
	w.str(v.type);
	w.word(v.items.length);
	v.items.forEach((item) => {
		writeItem(w, item);
	});
}

/** One step of a form: a host word, fields, or a group. */
export type Item =
	| string
	| { opt: ReadonlyArray<Item> }
	| { oneOf: ReadonlyArray<ReadonlyArray<Item>> }
	| { scope: ReadonlyArray<Item> }
	| { readonly [key: string]: Source };
export function writeItem(w: Writer, v: Item): void {
	if (typeof v === 'string') {
		w.word(0);
		w.str((v as string));
	} else if ('opt' in v) {
		w.word(1);
		w.word((v as { opt: ReadonlyArray<Item> }).opt.length);
		(v as { opt: ReadonlyArray<Item> }).opt.forEach((item) => {
			writeItem(w, item);
		});
	} else if ('oneOf' in v) {
		w.word(2);
		w.word((v as { oneOf: ReadonlyArray<ReadonlyArray<Item>> }).oneOf.length);
		(v as { oneOf: ReadonlyArray<ReadonlyArray<Item>> }).oneOf.forEach((item) => {
			w.word(item.length);
			item.forEach((item) => {
				writeItem(w, item);
			});
		});
	} else if ('scope' in v) {
		w.word(3);
		w.word((v as { scope: ReadonlyArray<Item> }).scope.length);
		(v as { scope: ReadonlyArray<Item> }).scope.forEach((item) => {
			writeItem(w, item);
		});
	} else {
		w.word(4);
		{
			const entries = Object.entries((v as { readonly [key: string]: Source }));
			w.word(entries.length);
			entries.forEach(([key, item]) => {
				w.str(key);
				writeSource(w, item);
			});
		}
	}
}

/** What fills a field: where it is read from, what reads it, and how. */
export type Source = {
	from: string;
	read: string;
	optional: boolean;
	bind: Bind;
	orArg: boolean;
	literal?: Literal;
};
export function writeSource(w: Writer, v: Source): void {
	w.str(v.from);
	w.str(v.read);
	w.word(v.optional ? 1 : 0);
	writeBind(w, v.bind);
	w.word(v.orArg ? 1 : 0);
	if (v.literal === undefined) w.word(0);
	else {
		w.word(1);
		writeLiteral(w, v.literal);
	}
}

export type Bind =
	| false
	| 'inside'
	| 'outside';
export function writeBind(w: Writer, v: Bind): void {
	switch (v) {
		case false:
			w.word(0);
			return;
		case 'inside':
			w.word(1);
			return;
		case 'outside':
			w.word(2);
			return;
	}
}

/** A value a literal source always writes. */
export type Literal =
	| true
	| false
	| null
	| readonly [];
export function writeLiteral(w: Writer, v: Literal): void {
	switch (v) {
		case true:
			w.word(0);
			return;
		case false:
			w.word(1);
			return;
		case null:
			w.word(2);
			return;
	}
	if (Array.isArray(v)) {
		w.word(3);
	}
}

export type Attributes = {
	expressions?: boolean;
	shorthand?: boolean;
};
export function writeAttributes(w: Writer, v: Attributes): void {
	if (v.expressions === undefined) w.word(0);
	else {
		w.word(1);
		w.word(v.expressions ? 1 : 0);
	}
	if (v.shorthand === undefined) w.word(0);
	else {
		w.word(1);
		w.word(v.shorthand ? 1 : 0);
	}
}

export type Elements = {
	fields: { readonly [key: string]: Source };
	rules?: { readonly [key: string]: Element };
	component?: Element;
	other?: Element;
};
export function writeElements(w: Writer, v: Elements): void {
	{
		const entries = Object.entries(v.fields);
		w.word(entries.length);
		entries.forEach(([key, item]) => {
			w.str(key);
			writeSource(w, item);
		});
	}
	if (v.rules === undefined) w.word(0);
	else {
		w.word(1);
		{
			const entries = Object.entries(v.rules);
			w.word(entries.length);
			entries.forEach(([key, item]) => {
				w.str(key);
				writeElement(w, item);
			});
		}
	}
	if (v.component === undefined) w.word(0);
	else {
		w.word(1);
		writeElement(w, v.component);
	}
	if (v.other === undefined) w.word(0);
	else {
		w.word(1);
		writeElement(w, v.other);
	}
}

export type Element = {
	node: Node;
	root: boolean;
	once: boolean;
	inside?: string;
	outside?: string;
	content?: Content;
};
export function writeElement(w: Writer, v: Element): void {
	writeNode(w, v.node);
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
	if (v.content === undefined) w.word(0);
	else {
		w.word(1);
		writeContent(w, v.content);
	}
}

export type Content =
	| 'raw'
	| 'rcdata';
export function writeContent(w: Writer, v: Content): void {
	switch (v) {
		case 'raw':
			w.word(0);
			return;
		case 'rcdata':
			w.word(1);
			return;
	}
}

export type Script = {
	element: string;
	module?: ReadonlyArray<readonly [string, string?]>;
	typescript?: ReadonlyArray<readonly [string, string?]>;
};
export function writeScript(w: Writer, v: Script): void {
	w.str(v.element);
	if (v.module === undefined) w.word(0);
	else {
		w.word(1);
		w.word(v.module.length);
		v.module.forEach((item) => {
			w.str(item[0]);
			if (item[1] === undefined) w.word(0);
			else {
				w.word(1);
				w.str(item[1]);
			}
		});
	}
	if (v.typescript === undefined) w.word(0);
	else {
		w.word(1);
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
}

export type Directives = {
	prefix?: string;
	arg?: string;
	modifier?: string;
	dynamic?: readonly [string, string];
	unique?: Raw;
	fields: { readonly [key: string]: Source };
	shorthands?: { readonly [key: string]: ReadonlyArray<string> };
	rules?: { readonly [key: string]: Directive };
	other?: Directive;
};
export function writeDirectives(w: Writer, v: Directives): void {
	if (v.prefix === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.prefix);
	}
	if (v.arg === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.arg);
	}
	if (v.modifier === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.modifier);
	}
	if (v.dynamic === undefined) w.word(0);
	else {
		w.word(1);
		w.str(v.dynamic[0]);
		w.str(v.dynamic[1]);
	}
	if (v.unique === undefined) w.word(0);
	else {
		w.word(1);
		writeRaw(w, v.unique);
	}
	{
		const entries = Object.entries(v.fields);
		w.word(entries.length);
		entries.forEach(([key, item]) => {
			w.str(key);
			writeSource(w, item);
		});
	}
	if (v.shorthands === undefined) w.word(0);
	else {
		w.word(1);
		{
			const entries = Object.entries(v.shorthands);
			w.word(entries.length);
			entries.forEach(([key, item]) => {
				w.str(key);
				w.word(item.length);
				item.forEach((item) => {
					w.str(item);
				});
			});
		}
	}
	if (v.rules === undefined) w.word(0);
	else {
		w.word(1);
		{
			const entries = Object.entries(v.rules);
			w.word(entries.length);
			entries.forEach(([key, item]) => {
				w.str(key);
				writeDirective(w, item);
			});
		}
	}
	if (v.other === undefined) w.word(0);
	else {
		w.word(1);
		writeDirective(w, v.other);
	}
}

export type Raw =
	| 'raw';
export function writeRaw(w: Writer, v: Raw): void {
	switch (v) {
		case 'raw':
			w.word(0);
			return;
	}
}

export type Directive = {
	node: Node;
	unique: Uniqueness;
};
export function writeDirective(w: Writer, v: Directive): void {
	writeNode(w, v.node);
	writeUniqueness(w, v.unique);
}

export type Uniqueness =
	| 'no'
	| 'kind'
	| 'attributes';
export function writeUniqueness(w: Writer, v: Uniqueness): void {
	switch (v) {
		case 'no':
			w.word(0);
			return;
		case 'kind':
			w.word(1);
			return;
		case 'attributes':
			w.word(2);
			return;
	}
}

export type Sigils = {
	open: string;
	branch: string;
	close: string;
	tag: string;
	blocks?: { readonly [key: string]: Block };
	tags?: { readonly [key: string]: Tag };
};
export function writeSigils(w: Writer, v: Sigils): void {
	w.str(v.open);
	w.str(v.branch);
	w.str(v.close);
	w.str(v.tag);
	if (v.blocks === undefined) w.word(0);
	else {
		w.word(1);
		{
			const entries = Object.entries(v.blocks);
			w.word(entries.length);
			entries.forEach(([key, item]) => {
				w.str(key);
				writeBlock(w, item);
			});
		}
	}
	if (v.tags === undefined) w.word(0);
	else {
		w.word(1);
		{
			const entries = Object.entries(v.tags);
			w.word(entries.length);
			entries.forEach(([key, item]) => {
				w.str(key);
				writeTag(w, item);
			});
		}
	}
}

export type Block = {
	node: Node;
	branches: { readonly [key: string]: Branch };
};
export function writeBlock(w: Writer, v: Block): void {
	writeNode(w, v.node);
	{
		const entries = Object.entries(v.branches);
		w.word(entries.length);
		entries.forEach(([key, item]) => {
			w.str(key);
			writeBranch(w, item);
		});
	}
}

export type Branch =
	| ReadonlyArray<Item>
	| Reopen;
export function writeBranch(w: Writer, v: Branch): void {
	if (Array.isArray(v)) {
		w.word(0);
		w.word((v as ReadonlyArray<Item>).length);
		(v as ReadonlyArray<Item>).forEach((item) => {
			writeItem(w, item);
		});
	} else {
		w.word(1);
		writeReopen(w, (v as Reopen));
	}
}

/** An `else if`: the block again, nested into this field, with this flag set on it. */
export type Reopen = {
	reopen: string;
	flag: string;
};
export function writeReopen(w: Writer, v: Reopen): void {
	w.str(v.reopen);
	w.str(v.flag);
}

export type Tag = {
	node: Node;
	among: Among;
};
export function writeTag(w: Writer, v: Tag): void {
	writeNode(w, v.node);
	writeAmong(w, v.among);
}

export type Among =
	| 'content'
	| 'attributes';
export function writeAmong(w: Writer, v: Among): void {
	switch (v) {
		case 'content':
			w.word(0);
			return;
		case 'attributes':
			w.word(1);
			return;
	}
}
