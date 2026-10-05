//! A host grammar: what a template language puts around the JavaScript it embeds, read once
//! from the wire a host hands over. Every form is a sequence of the host's own words and
//! punctuators around JavaScript entries; what may follow an entry is what ends it.
//!
//! The wire a grammar crosses on: words, then a pool of strings. Nothing is spelled: a value is
//! what sits at its position, in the order the types declare, and both ends of the crossing come
//! from those declarations. `wire!` defines a type once and derives its reader, its writer, and
//! the TypeScript type and writer the other end is checked against.
//!
//! Little-endian 32-bit words: the count of record words, the count of strings, each string's
//! offset and length in the pool, the record words; then the pool, UTF-8. A string is the index
//! of one in the pool, a boolean 0 or 1, an option 0 or 1 and then the value, a list its length
//! and then the items, a struct its fields, an enum the index of its variant and then the payload.

/// Reads the record words of a wire in order.
pub(crate) struct Cursor<'a> {
	bytes: &'a [u8],
	at: usize,
	end: usize,
	strings: usize,
	/// The pool, kept for the process once: every name is a slice of it.
	pool: &'static str,
}

impl<'a> Cursor<'a> {
	pub(crate) fn new(bytes: &'a [u8]) -> Result<Cursor<'a>, String> {
		let word = |i: usize| {
			bytes
				.get(i * 4..i * 4 + 4)
				.map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
		};
		let (Some(records), Some(strings)) = (word(0), word(1)) else {
			return Err("the grammar has no header".into());
		};
		let at = strings.checked_mul(2).and_then(|n| n.checked_add(2));
		let end = at.and_then(|at| at.checked_add(records));
		let (Some(at), Some(end)) = (at, end) else {
			return Err("the grammar ends early".into());
		};
		if end.checked_mul(4).is_none_or(|len| len > bytes.len()) {
			return Err("the grammar ends early".into());
		}
		let pool = std::str::from_utf8(&bytes[end * 4..]).map_err(|_| "the grammar's strings are not UTF-8")?;
		Ok(Cursor {
			bytes,
			at,
			end,
			strings,
			pool: keep(pool),
		})
	}

	fn word_at(&self, i: usize) -> u32 {
		u32::from_le_bytes(self.bytes[i * 4..i * 4 + 4].try_into().unwrap())
	}

	pub(crate) fn word(&mut self) -> Result<u32, String> {
		if self.at >= self.end {
			return Err("the grammar ends early".into());
		}
		self.at += 1;
		Ok(self.word_at(self.at - 1))
	}

	pub(crate) fn str(&mut self) -> Result<&'static str, String> {
		let i = self.word()? as usize;
		if i >= self.strings {
			return Err(format!("no string {i} on the grammar"));
		}
		let (offset, len) = (self.word_at(2 + 2 * i) as usize, self.word_at(3 + 2 * i) as usize);
		self.pool
			.get(offset..offset + len)
			.ok_or_else(|| format!("string {i} on the grammar is cut"))
	}

	pub(crate) fn done(&self) -> bool {
		self.at == self.end
	}
}

/// Writes a wire, strings kept once.
#[derive(Default)]
pub(crate) struct Writer {
	words: Vec<u32>,
	strings: Vec<String>,
}

impl Writer {
	pub(crate) fn word(&mut self, word: u32) {
		self.words.push(word);
	}

	pub(crate) fn str(&mut self, s: &str) {
		let i = self.strings.iter().position(|known| known == s).unwrap_or_else(|| {
			self.strings.push(s.to_owned());
			self.strings.len() - 1
		});
		self.words.push(i as u32);
	}

	pub(crate) fn bytes(self) -> Vec<u8> {
		let mut head = vec![self.words.len() as u32, self.strings.len() as u32];
		let mut offset = 0;
		for s in &self.strings {
			head.extend([offset, s.len() as u32]);
			offset += s.len() as u32;
		}
		let mut out: Vec<u8> = head.iter().chain(&self.words).flat_map(|w| w.to_le_bytes()).collect();
		for s in &self.strings {
			out.extend_from_slice(s.as_bytes());
		}
		out
	}
}

/// A grammar's names reach the walker as `&'static str`, so they live for the process.
pub(crate) fn keep(s: &str) -> &'static str {
	Box::leak(s.to_owned().into_boxed_str())
}

/// A Rust name in camel case, as TypeScript spells it: `attribute_expressions`, `TypeParameters`.
pub(crate) fn camel(name: &str) -> String {
	let name = name.strip_prefix("r#").unwrap_or(name);
	let mut out = String::with_capacity(name.len());
	let mut up = false;
	for (i, c) in name.chars().enumerate() {
		if c == '_' {
			up = true;
			continue;
		}
		out.push(if up {
			c.to_ascii_uppercase()
		} else if i == 0 {
			c.to_ascii_lowercase()
		} else {
			c
		});
		up = false;
	}
	out
}

/// A type on the wire, and the TypeScript that writes it.
pub(crate) trait Wire: Sized {
	/// A field of the type may be left out.
	const OPTIONAL: bool = false;

	fn read(c: &mut Cursor) -> Result<Self, String>;

	fn write(&self, w: &mut Writer);

	/// The TypeScript type.
	fn ts() -> String;

	/// The TypeScript type of a field, whose `?` says what `OPTIONAL` does.
	fn ts_field() -> String {
		Self::ts()
	}

	/// TypeScript statements that write `value`, an expression of the type, to `w`.
	fn ts_write(value: &str) -> String;

	/// Adds the definitions the type and every type it holds need, each once, in the order of
	/// first use.
	fn definitions(_out: &mut Vec<Declaration>) {}
}

/// A named type on the wire as TypeScript: its type and the function that writes it.
pub(crate) struct Declaration {
	pub(crate) name: &'static str,
	pub(crate) docs: String,
	pub(crate) ty: String,
	pub(crate) write: String,
}

impl Wire for &'static str {
	fn read(c: &mut Cursor) -> Result<Self, String> {
		c.str()
	}

	fn write(&self, w: &mut Writer) {
		w.str(self);
	}

	fn ts() -> String {
		"string".into()
	}

	fn ts_write(value: &str) -> String {
		format!("w.str({value});")
	}
}

impl Wire for bool {
	fn read(c: &mut Cursor) -> Result<Self, String> {
		match c.word()? {
			0 => Ok(false),
			1 => Ok(true),
			other => Err(format!("{other} is not a boolean on the grammar")),
		}
	}

	fn write(&self, w: &mut Writer) {
		w.word(u32::from(*self));
	}

	fn ts() -> String {
		"boolean".into()
	}

	fn ts_write(value: &str) -> String {
		format!("w.word({value} ? 1 : 0);")
	}
}

impl<T: Wire> Wire for Option<T> {
	const OPTIONAL: bool = true;

	fn read(c: &mut Cursor) -> Result<Self, String> {
		match c.word()? {
			0 => Ok(None),
			1 => T::read(c).map(Some),
			other => Err(format!("{other} is not an option on the grammar")),
		}
	}

	fn write(&self, w: &mut Writer) {
		match self {
			None => w.word(0),
			Some(value) => {
				w.word(1);
				value.write(w);
			}
		}
	}

	fn ts() -> String {
		format!("{} | undefined", T::ts())
	}

	fn ts_field() -> String {
		T::ts()
	}

	fn ts_write(value: &str) -> String {
		format!(
			"if ({value} === undefined) w.word(0);\nelse {{\n\tw.word(1);\n\t{}\n}}",
			indent(&T::ts_write(value))
		)
	}

	fn definitions(out: &mut Vec<Declaration>) {
		T::definitions(out);
	}
}

impl<T: Wire> Wire for Vec<T> {
	fn read(c: &mut Cursor) -> Result<Self, String> {
		(0..c.word()?).map(|_| T::read(c)).collect()
	}

	fn write(&self, w: &mut Writer) {
		w.word(self.len() as u32);
		for item in self {
			item.write(w);
		}
	}

	fn ts() -> String {
		format!("ReadonlyArray<{}>", T::ts())
	}

	fn ts_write(value: &str) -> String {
		let (i, item) = (fresh(value, "i"), fresh(value, "item"));
		format!(
			"w.word({value}.length);\nfor (let {i} = 0; {i} < {value}.length; {i}++) {{\n\tconst {item} = {value}[{i}];\n\t{}\n}}",
			indent(&T::ts_write(&item))
		)
	}

	fn definitions(out: &mut Vec<Declaration>) {
		T::definitions(out);
	}
}

impl<A: Wire, B: Wire> Wire for (A, B) {
	fn read(c: &mut Cursor) -> Result<Self, String> {
		Ok((A::read(c)?, B::read(c)?))
	}

	fn write(&self, w: &mut Writer) {
		self.0.write(w);
		self.1.write(w);
	}

	fn ts() -> String {
		format!(
			"readonly [{}, {}{}]",
			A::ts(),
			B::ts_field(),
			if B::OPTIONAL { "?" } else { "" }
		)
	}

	fn ts_write(value: &str) -> String {
		format!(
			"{}\n{}",
			A::ts_write(&format!("{value}[0]")),
			B::ts_write(&format!("{value}[1]"))
		)
	}

	fn definitions(out: &mut Vec<Declaration>) {
		A::definitions(out);
		B::definitions(out);
	}
}

/// An object of the definition: its keys in order, each holding a `T`.
#[derive(Clone, Debug, Default)]
pub struct Record<T>(pub Vec<(&'static str, T)>);

impl<T: Wire> Wire for Record<T> {
	fn read(c: &mut Cursor) -> Result<Self, String> {
		(0..c.word()?)
			.map(|_| Ok((c.str()?, T::read(c)?)))
			.collect::<Result<_, String>>()
			.map(Record)
	}

	fn write(&self, w: &mut Writer) {
		w.word(self.0.len() as u32);
		for (key, value) in &self.0 {
			w.str(key);
			value.write(w);
		}
	}

	fn ts() -> String {
		format!("{{ readonly [key: string]: {} }}", T::ts())
	}

	fn ts_write(value: &str) -> String {
		let (keys, i, key) = (fresh(value, "keys"), fresh(value, "i"), fresh(value, "key"));
		format!(
			"{{\n\tconst {keys} = Object.keys({value});\n\tw.word({keys}.length);\n\tfor (let {i} = 0; {i} < {keys}.length; {i}++) {{\n\t\tconst {key} = {keys}[{i}];\n\t\tw.str({key});\n\t\t{}\n\t}}\n}}",
			indent(&indent(&T::ts_write(&format!("{value}[{key}]"))))
		)
	}

	fn definitions(out: &mut Vec<Declaration>) {
		T::definitions(out);
	}
}

fn indent(text: &str) -> String {
	text.replace('\n', "\n\t")
}

/// A loop variable's name that the looped value does not mention, so the loop never shadows
/// what it reads.
fn fresh(value: &str, base: &str) -> String {
	let mut name = base.to_string();
	while value.contains(&name) {
		name.push('_');
	}
	name
}

/// A field as TypeScript: its name, docs, and the type and writer of its value.
pub(crate) struct Field {
	pub(crate) name: &'static str,
	pub(crate) docs: &'static [&'static str],
	pub(crate) optional: bool,
	pub(crate) ty: String,
	pub(crate) write: String,
}

/// The fields of a type as TypeScript, one per line, with their docs; a variant's on one line.
pub(crate) fn fields(fields: &[Field], inline: bool) -> String {
	let lines: Vec<String> = fields
		.iter()
		.map(|f| {
			let field = format!("{}{}: {}", camel(f.name), if f.optional { "?" } else { "" }, f.ty);
			if f.docs.is_empty() {
				field
			} else {
				format!(
					"/**{} */{}{field}",
					f.docs.join("\n\t *"),
					if inline { " " } else { "\n\t" }
				)
			}
		})
		.collect();
	if inline {
		format!("{{ {} }}", lines.join("; "))
	} else {
		format!("{{\n\t{};\n}}", lines.join(";\n\t"))
	}
}

/// The statements writing a struct's fields in order.
pub(crate) fn writes(fields: &[Field]) -> String {
	fields.iter().map(|f| f.write.clone()).collect::<Vec<_>>().join("\n")
}

/// How the writer tells a variant: a `case` on its literal, or a test on `v`; an empty test is
/// the `else`.
pub(crate) enum Tell {
	Case(String),
	Test(String),
}

/// A variant as TypeScript: the alternative's type, how it is told, and what writes its payload.
pub(crate) struct Variant {
	pub(crate) docs: &'static [&'static str],
	pub(crate) ty: String,
	pub(crate) tell: Tell,
	pub(crate) write: String,
}

/// A union as TypeScript, one alternative per line with its docs, and the statements that write
/// a value of it: its variant's index, then the payload.
pub(crate) fn union(variants: &[Variant]) -> (String, String) {
	let mut ty = String::new();
	let mut cases = Vec::new();
	let mut tests = Vec::new();
	for (tag, v) in variants.iter().enumerate() {
		if !v.docs.is_empty() {
			ty.push_str(&format!("\n\t/**{} */", v.docs.join("\n\t *")));
		}
		ty.push_str(&format!("\n\t| {}", v.ty));
		let payload = if v.write.is_empty() {
			String::new()
		} else {
			format!("\n{}", v.write)
		};
		match &v.tell {
			Tell::Case(literal) => cases.push(format!(
				"case {literal}:\n\tw.word({tag});{}\n\treturn;",
				indent(&payload)
			)),
			Tell::Test(test) => tests.push((test.clone(), format!("w.word({tag});{payload}"))),
		}
	}
	let mut write = String::new();
	if !cases.is_empty() {
		write.push_str(&format!("switch (v) {{\n\t{}\n}}", indent(&cases.join("\n"))));
	}
	for (i, (test, body)) in tests.iter().enumerate() {
		if i > 0 {
			write.push_str(" else ");
		} else if !write.is_empty() {
			write.push('\n');
		}
		let head = if test.is_empty() {
			String::new()
		} else {
			format!("if ({test}) ")
		};
		write.push_str(&format!("{head}{{\n\t{}\n}}", indent(body)));
	}
	(ty, write)
}

/// Renders every definition reachable from `T` as a TypeScript module that exports the root
/// type and `wire`, which writes one: the other types, their writers and the writer they write
/// to stay inside.
pub(crate) fn module<T: Wire>(root: &str) -> String {
	let mut out = format!(
		"// written by {root}; `cargo test` pins it\n\n\
/** The wire: words, then a pool of strings kept once. */\n\
class Writer {{\n\
\t#words = new Uint32Array(1024);\n\
\t#count = 0;\n\
\t#strings = new Map<string, number>();\n\
\t#pool: string[] = [];\n\
\tword(word: number): void {{\n\
\t\tif (this.#count === this.#words.length) {{\n\
\t\t\tconst more = new Uint32Array(this.#count * 2);\n\
\t\t\tmore.set(this.#words);\n\
\t\t\tthis.#words = more;\n\
\t\t}}\n\
\t\tthis.#words[this.#count++] = word;\n\
\t}}\n\
\tstr(s: string): void {{\n\
\t\tlet i = this.#strings.get(s);\n\
\t\tif (i === undefined) {{\n\
\t\t\tthis.#strings.set(s, (i = this.#pool.length));\n\
\t\t\tthis.#pool.push(s);\n\
\t\t}}\n\
\t\tthis.word(i);\n\
\t}}\n\
\t/** Little-endian words: the count of record words, the count of strings, each string's offset and length, the record words; then the pool, UTF-8. */\n\
\tbytes(): Uint8Array {{\n\
\t\tconst head = 2 + 2 * this.#pool.length;\n\
\t\tconst poolAt = (head + this.#count) * 4;\n\
\t\tconst joined = this.#pool.join('');\n\
\t\tconst out = new Uint8Array(poolAt + joined.length * 3);\n\
\t\tconst words = new Uint32Array(out.buffer, 0, head + this.#count);\n\
\t\twords[0] = this.#count;\n\
\t\twords[1] = this.#pool.length;\n\
\t\tconst encoder = new TextEncoder();\n\
\t\tlet {{ written }} = encoder.encodeInto(joined, out.subarray(poolAt));\n\
\t\tif (written === joined.length) {{\n\
\t\t\t// every string is ASCII, so its bytes are its characters\n\
\t\t\tlet offset = 0;\n\
\t\t\tfor (let i = 0; i < this.#pool.length; i++) {{\n\
\t\t\t\twords[2 + 2 * i] = offset;\n\
\t\t\t\twords[3 + 2 * i] = this.#pool[i]!.length;\n\
\t\t\t\toffset += this.#pool[i]!.length;\n\
\t\t\t}}\n\
\t\t}} else {{\n\
\t\t\twritten = 0;\n\
\t\t\tfor (let i = 0; i < this.#pool.length; i++) {{\n\
\t\t\t\tconst bytes = encoder.encodeInto(this.#pool[i]!, out.subarray(poolAt + written)).written;\n\
\t\t\t\twords[2 + 2 * i] = written;\n\
\t\t\t\twords[3 + 2 * i] = bytes;\n\
\t\t\t\twritten += bytes;\n\
\t\t\t}}\n\
\t\t}}\n\
\t\twords.set(this.#words.subarray(0, this.#count), head);\n\
\t\t// the engine reads little-endian words; a big-endian platform swaps them here\n\
\t\tif (new Uint8Array(new Uint32Array([1]).buffer)[0] !== 1) {{\n\
\t\t\tconst view = new DataView(out.buffer);\n\
\t\t\twords.forEach((word, i) => view.setUint32(i * 4, word, true));\n\
\t\t}}\n\
\t\treturn out.subarray(0, poolAt + written);\n\
\t}}\n\
}}\n"
	);
	let mut definitions = Vec::new();
	T::definitions(&mut definitions);
	let top = T::ts();
	for d in definitions {
		out.push('\n');
		if !d.docs.is_empty() {
			out.push_str(&format!("/**{} */\n", d.docs));
		}
		out.push_str(&format!(
			"{}type {} ={}{};\n",
			if d.name == top { "export " } else { "" },
			d.name,
			if d.ty.starts_with('\n') { "" } else { " " },
			d.ty
		));
		out.push_str(&format!(
			"function write{}(w: Writer, v: {}): void {{\n\t{}\n}}\n",
			d.name,
			d.name,
			indent(&d.write)
		));
	}
	out.push_str(&format!(
		"\n/** A {top} on its wire, as the engine reads it. */\nexport function wire(v: {top}): Uint8Array {{\n\tconst w = new Writer();\n\twrite{top}(w, v);\n\treturn w.bytes();\n}}\n"
	));
	out
}

/// A type on the wire, defined once: its Rust definition, its reader and writer, and the
/// TypeScript type and writer. Docs come first on the type and on every field. A `copy enum`
/// holds names only. A variant is told apart in TypeScript by its name or a literal it is
/// given, or by a test on `v` when its value is bare: `Word(&'static str) = "typeof v === 'string'"`,
/// an empty test being the `else`.
macro_rules! wire {
	(
		$(#[doc = $doc:literal])* $vis:vis struct $name:ident {
			$($(#[doc = $fdoc:literal])* $fvis:vis $f:ident : $t:ty),* $(,)?
		}
	) => {
		$(#[doc = $doc])*
		#[derive(Clone, Debug)]
		$vis struct $name {
			$($(#[doc = $fdoc])* $fvis $f: $t,)*
		}

		impl $crate::host::grammar::Wire for $name {
			fn read(c: &mut $crate::host::grammar::Cursor) -> Result<Self, String> {
				Ok($name {
					$($f: <$t as $crate::host::grammar::Wire>::read(c)?,)*
				})
			}

			fn write(&self, w: &mut $crate::host::grammar::Writer) {
				$(<$t as $crate::host::grammar::Wire>::write(&self.$f, w);)*
			}

			fn ts() -> String {
				stringify!($name).into()
			}

			fn ts_write(value: &str) -> String {
				format!("write{}(w, {value});", stringify!($name))
			}

			fn definitions(out: &mut Vec<$crate::host::grammar::Declaration>) {
				if out.iter().any(|d| d.name == stringify!($name)) {
					return;
				}
				let fields = [$($crate::host::grammar::Field {
					name: stringify!($f),
					docs: &[$($fdoc),*],
					optional: <$t as $crate::host::grammar::Wire>::OPTIONAL,
					ty: <$t as $crate::host::grammar::Wire>::ts_field(),
					write: <$t as $crate::host::grammar::Wire>::ts_write(&format!("v.{}", $crate::host::grammar::camel(stringify!($f)))),
				},)*];
				out.push($crate::host::grammar::Declaration {
					name: stringify!($name),
					docs: <[&str]>::join(&[$($doc),*], "\n *"),
					ty: $crate::host::grammar::fields(&fields, false),
					write: $crate::host::grammar::writes(&fields),
				});
				$(<$t as $crate::host::grammar::Wire>::definitions(out);)*
			}
		}
	};

	($(#[doc = $doc:literal])* $vis:vis enum $name:ident { $($body:tt)* }) => {
		wire!(@variants $name [$(#[doc = $doc])* $vis derive(Clone, Debug)] (c w v out) [] [] [] [] [] [] $($body)*);
	};
	($(#[doc = $doc:literal])* $vis:vis copy enum $name:ident { $($body:tt)* }) => {
		wire!(@variants $name [$(#[doc = $doc])* $vis derive(Clone, Copy, Debug, PartialEq, Eq)] (c w v out) [] [] [] [] [] [] $($body)*);
	};

	// a name told by a test, written as the type given: `List: "readonly []" = "Array.isArray(v)"`
	(@variants $name:ident $head:tt ($c:ident $w:ident $v:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($is:tt)*] [$($write:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $variant:ident : $ty:literal = $test:literal $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($c $w $v $out)
			[$($def)* $(#[doc = $vdoc])* $variant,]
			[$($read)* |_| Ok($name::$variant),]
			[$($is)* matches!($v, $name::$variant),]
			[$($write)* $name::$variant => {}]
			[$($ts)* $crate::host::grammar::Variant {
				docs: &[$($vdoc),*],
				ty: $ty.into(),
				tell: $crate::host::grammar::Tell::Test($test.into()),
				write: String::new(),
			},]
			[$($deps)*]
			$($($rest)*)?);
	};

	// a name, written as `'name'` or as the literal given: `No = false`
	(@variants $name:ident $head:tt ($c:ident $w:ident $v:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($is:tt)*] [$($write:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $variant:ident $(= $literal:tt)? $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($c $w $v $out)
			[$($def)* $(#[doc = $vdoc])* $variant,]
			[$($read)* |_| Ok($name::$variant),]
			[$($is)* matches!($v, $name::$variant),]
			[$($write)* $name::$variant => {}]
			[$($ts)* {
				let literal = [$(stringify!($literal),)* &format!("'{}'", $crate::host::grammar::camel(stringify!($variant)))][0].to_string();
				$crate::host::grammar::Variant {
					docs: &[$($vdoc),*],
					ty: literal.clone(),
					tell: $crate::host::grammar::Tell::Case(literal),
					write: String::new(),
				}
			},]
			[$($deps)*]
			$($($rest)*)?);
	};

	// a variant holding one value: `{ name: value }`, or the bare value told by the test given
	(@variants $name:ident $head:tt ($c:ident $w:ident $v:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($is:tt)*] [$($write:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $variant:ident ( $t:ty ) $(= $test:literal)? $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($c $w $v $out)
			[$($def)* $(#[doc = $vdoc])* $variant($t),]
			[$($read)* |$c| Ok($name::$variant(<$t as $crate::host::grammar::Wire>::read($c)?)),]
			[$($is)* matches!($v, $name::$variant(_)),]
			[$($write)* $name::$variant(value) => <$t as $crate::host::grammar::Wire>::write(value, $w),]
			[$($ts)* {
				let key = $crate::host::grammar::camel(stringify!($variant));
				let payload = <$t as $crate::host::grammar::Wire>::ts();
				let test: Option<&str> = [$(Some($test),)* None][0];
				match test {
					Some(test) => $crate::host::grammar::Variant {
						docs: &[$($vdoc),*],
						ty: payload.clone(),
						tell: $crate::host::grammar::Tell::Test(test.into()),
						write: <$t as $crate::host::grammar::Wire>::ts_write(&format!("(v as {payload})")),
					},
					None => $crate::host::grammar::Variant {
						docs: &[$($vdoc),*],
						ty: format!("{{ {key}: {payload} }}"),
						tell: $crate::host::grammar::Tell::Test(format!("'{key}' in v")),
						write: <$t as $crate::host::grammar::Wire>::ts_write(&format!("(v as {{ {key}: {payload} }}).{key}")),
					},
				}
			},]
			[$($deps)* <$t as $crate::host::grammar::Wire>::definitions($out);]
			$($($rest)*)?);
	};

	// a variant with fields: `{ name: { fields } }`, or the bare record told by the test given
	(@variants $name:ident $head:tt ($c:ident $w:ident $v:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($is:tt)*] [$($write:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $variant:ident { $($(#[doc = $fdoc:literal])* $f:ident : $t:ty),* $(,)? } $(= $test:literal)? $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($c $w $v $out)
			[$($def)* $(#[doc = $vdoc])* $variant { $($(#[doc = $fdoc])* $f: $t,)* },]
			[$($read)* |$c| Ok($name::$variant {
				$($f: <$t as $crate::host::grammar::Wire>::read($c)?,)*
			}),]
			[$($is)* matches!($v, $name::$variant { .. }),]
			[$($write)* $name::$variant { $($f,)* } => { $(<$t as $crate::host::grammar::Wire>::write($f, $w);)* }]
			[$($ts)* {
				let key = $crate::host::grammar::camel(stringify!($variant));
				let test: Option<&str> = [$(Some($test),)* None][0];
				let shape = $crate::host::grammar::fields(&[$($crate::host::grammar::Field {
					name: stringify!($f),
					docs: &[$($fdoc),*],
					optional: <$t as $crate::host::grammar::Wire>::OPTIONAL,
					ty: <$t as $crate::host::grammar::Wire>::ts_field(),
					write: String::new(),
				},)*], true);
				let value = match test {
					Some(_) => format!("(v as {shape})"),
					None => format!("(v as {{ {key}: {shape} }}).{key}"),
				};
				let fields = [$($crate::host::grammar::Field {
					name: stringify!($f),
					docs: &[$($fdoc),*],
					optional: <$t as $crate::host::grammar::Wire>::OPTIONAL,
					ty: <$t as $crate::host::grammar::Wire>::ts_field(),
					write: <$t as $crate::host::grammar::Wire>::ts_write(&format!("{value}.{}", $crate::host::grammar::camel(stringify!($f)))),
				},)*];
				$crate::host::grammar::Variant {
					docs: &[$($vdoc),*],
					ty: match test { Some(_) => shape.clone(), None => format!("{{ {key}: {shape} }}") },
					tell: $crate::host::grammar::Tell::Test(test.map_or_else(|| format!("'{key}' in v"), str::to_string)),
					write: $crate::host::grammar::writes(&fields),
				}
			},]
			[$($deps)* $(<$t as $crate::host::grammar::Wire>::definitions($out);)*]
			$($($rest)*)?);
	};

	(@variants $name:ident [$(#[doc = $doc:literal])* $vis:vis derive($($derive:ident),*)] ($c:ident $w:ident $v:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($is:tt)*] [$($write:tt)*] [$($ts:tt)*] [$($deps:tt)*]) => {
		$(#[doc = $doc])*
		#[derive($($derive),*)]
		$vis enum $name { $($def)* }

		impl $crate::host::grammar::Wire for $name {
			fn read($c: &mut $crate::host::grammar::Cursor) -> Result<Self, String> {
				let readers: &[fn(&mut $crate::host::grammar::Cursor) -> Result<Self, String>] = &[$($read)*];
				let tag = $c.word()?;
				let reader = readers.get(tag as usize).ok_or_else(|| format!("{tag} is not a {} on the grammar", stringify!($name)))?;
				reader($c)
			}

			fn write(&self, $w: &mut $crate::host::grammar::Writer) {
				let $v = self;
				$w.word([$($is)*].iter().position(|is| *is).unwrap() as u32);
				match $v { $($write)* }
			}

			fn ts() -> String {
				stringify!($name).into()
			}

			fn ts_write(value: &str) -> String {
				format!("write{}(w, {value});", stringify!($name))
			}

			fn definitions($out: &mut Vec<$crate::host::grammar::Declaration>) {
				if $out.iter().any(|d| d.name == stringify!($name)) {
					return;
				}
				let (ty, write) = $crate::host::grammar::union(&[$($ts)*]);
				$out.push($crate::host::grammar::Declaration {
					name: stringify!($name),
					docs: <[&str]>::join(&[$($doc),*], "\n *"),
					ty,
					write,
				});
				$($deps)*
			}
		}
	};
}

/// A JavaScript entry inside a form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
	Expression,
	Pattern,
	Params,
	Identifier,
	TypeParameters,
	Statement,
	/// An expression, or, when what holds it is not one, its statements as a program.
	Code,
	/// `pattern = expression`, a const declaration the host spells without the keyword.
	Const,
	/// Identifiers separated by commas, possibly none.
	Identifiers,
	/// The text up to the closing delimiter, unread, for a host that reads its expressions later.
	Text,
}

/// One step of a form.
#[derive(Clone, Debug)]
pub enum Item {
	/// One of the host's words or punctuators.
	Literal(&'static str),
	/// A JavaScript entry read into a field; `omit` leaves the field out when the entry was not
	/// read, where the plain form gives it null.
	Entry {
		field: &'static str,
		entry: Entry,
		omit: bool,

		stops: Stops,
	},
	/// Alternatives tried in order: at most one, or exactly one when `required`.
	Group {
		alternatives: Vec<Alternative>,
		required: bool,

		/// The literals that may follow the group.
		after: &'static [&'static str],
	},
}

/// The literals that may follow an entry, as the parser takes them: all of them, and for an
/// expression without the tokens that continue one, which are JavaScript's before the host's.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stops {
	pub list: &'static [&'static str],
	pub joined: &'static str,
	pub expression: &'static str,
	/// The expression's stops up to each group that can follow it, in the rule's order, when more
	/// than one can: all but the last, which `expression` is.
	pub tiers: &'static [&'static str],
}

impl Stops {
	fn of(groups: Vec<Vec<&'static str>>) -> Stops {
		let continues = |s: &&str| !matches!(*s, "(" | "[" | "." | "?." | "`");
		let list: Vec<&'static str> = groups.iter().flatten().copied().collect();
		let ending: Vec<Vec<&'static str>> = groups
			.iter()
			.map(|group| group.iter().copied().filter(continues).collect::<Vec<_>>())
			.filter(|group| !group.is_empty())
			.collect();
		let mut so_far: Vec<&'static str> = Vec::new();
		let mut tiers = Vec::new();
		for group in ending.iter().take(ending.len().saturating_sub(1)) {
			so_far.extend(group);
			tiers.push(keep(&so_far.join(" ")));
		}
		let expression: Vec<&str> = list.iter().copied().filter(continues).collect();
		Stops {
			joined: keep(&list.join(" ")),
			expression: keep(&expression.join(" ")),
			list: Vec::leak(list),
			tiers: Vec::leak(tiers),
		}
	}
}

/// Gives every entry and group of `items` what may follow it, `follow` following them all.
fn resolve(items: &mut [Item], follow: &[&'static str]) {
	for i in 0..items.len() {
		let (head, rest) = items.split_at_mut(i + 1);
		match head.last_mut().unwrap() {
			Item::Literal(_) => {}
			Item::Entry { stops, .. } => *stops = Stops::of(first_groups(rest, follow)),
			Item::Group {
				alternatives, after, ..
			} => {
				let following = first_literals(rest, follow);
				for alternative in alternatives.iter_mut() {
					resolve(&mut alternative.items, &following);
				}
				*after = Vec::leak(following);
			}
		}
	}
}

/// The literals that can start what `items` read, then `follow` if they can read nothing.
pub(super) fn first_literals(items: &[Item], follow: &[&'static str]) -> Vec<&'static str> {
	first_groups(items, follow).into_iter().flatten().collect()
}

/// `first_literals` group by group, in the rule's order.
fn first_groups(items: &[Item], follow: &[&'static str]) -> Vec<Vec<&'static str>> {
	let mut out = Vec::new();
	for item in items {
		match item {
			Item::Literal(literal) => {
				out.push(vec![*literal]);
				return out;
			}
			Item::Entry { .. } => return out,
			Item::Group {
				alternatives, required, ..
			} => {
				out.push(
					alternatives
						.iter()
						.flat_map(|alternative| first_literals(&alternative.items, &[]))
						.collect(),
				);
				if *required {
					return out;
				}
			}
		}
	}
	out.push(follow.to_vec());
	out
}

fn collect_entries(items: &[Item], out: &mut Vec<(&'static str, bool)>) {
	for item in items {
		match item {
			Item::Entry { field, omit, .. } => out.push((field, *omit)),
			Item::Group { alternatives, .. } => {
				for alternative in alternatives {
					collect_entries(&alternative.items, out);
				}
			}
			Item::Literal(_) => {}
		}
	}
}

fn collect_alternative_bodies<'a>(items: &'a [Item], out: &mut Vec<&'a Body>) {
	for item in items {
		if let Item::Group { alternatives, .. } = item {
			for alternative in alternatives {
				if let Some(body) = &alternative.body {
					out.push(body);
				}
				collect_alternative_bodies(&alternative.items, out);
			}
		}
	}
}

fn collect_bodies(form: &Form, out: &mut Vec<(&'static str, bool)>) {
	let mut push = |body: &Body| {
		if !out.iter().any(|(f, _)| *f == body.field) {
			out.push((body.field, body.omit));
		}
	};
	if let Some(body) = &form.body {
		push(body);
	}
	for item in &form.items {
		if let Item::Group { alternatives, .. } = item {
			for alternative in alternatives {
				if let Some(body) = &alternative.body {
					push(body);
				}
			}
		}
	}
}

#[derive(Clone, Debug)]
pub struct Alternative {
	pub items: Vec<Item>,
	/// The body the block opens when this alternative was read, `[ then value=pattern -> then ]`.
	pub body: Option<Body>,
}

/// What a block's body is: the field that holds it, and what the body's scope declares.
#[derive(Clone, Debug)]
pub struct Body {
	pub field: &'static str,
	/// The field is left out of blocks that never opened this body; otherwise it is null there.
	pub omit: bool,
	/// A branch that nests a new block of the same kind into the field, `{:else if}`: the field
	/// of the nested block that its own body fills, and the flag set on it.
	pub chain: Option<(&'static str, &'static str)>,
	pub declares: Vec<Declare>,
}

#[derive(Clone, Debug)]
pub struct Declare {
	pub field: &'static str,
	/// Declared in the scope around the block rather than inside it: a snippet's name.
	pub outside: bool,
}

#[derive(Clone, Debug)]
pub struct Form {
	pub items: Vec<Item>,
	pub body: Option<Body>,

	/// Every entry the form can read, and whether its field is left out when it was not.
	pub entries: Vec<(&'static str, bool)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
	Exact(&'static str),
	/// A capitalized or dotted name.
	Component,
	Any,
}

#[derive(Clone, Debug)]
pub struct ElementRule {
	pub name: Match,
	pub ty: &'static str,
	/// The field that takes the expression of a `this` attribute, which leaves the attributes,
	/// and whether text is accepted there as a string.
	pub this: Option<(&'static str, bool)>,
	pub root: bool,
	pub once: bool,
	pub inside: Option<&'static str>,
	/// Not when an enclosing element carries this attribute.
	pub outside: Option<&'static str>,
	/// The content is text up to the closing tag, a script's say.
	pub raw: bool,
	/// The content is text with the host's expressions in it, a textarea's say.
	pub rcdata: bool,
}

#[derive(Clone, Debug)]
pub struct ScriptRule {
	pub name: &'static str,
	/// Attributes that make the script the module one, each with the text value it needs, if any.
	pub module: Vec<(&'static str, Option<&'static str>)>,
	/// Attributes that make the document TypeScript, the same way.
	pub typescript: Vec<(&'static str, Option<&'static str>)>,
}

/// How a directive's attribute name is spelled: `prefix name arg modifiers`, the name being the
/// directive's own when there is no prefix.
#[derive(Clone, Debug)]
pub struct DirectiveSyntax {
	pub prefix: Option<&'static str>,
	/// What separates the argument, `:`.
	pub arg: &'static str,
	/// What separates the modifiers, `|` or `.`.
	pub modifier: &'static str,
	/// The brackets of an argument that is an expression, `[` `]`.
	pub dynamic: Option<(&'static str, &'static str)>,
	pub name_field: Option<&'static str>,
	pub arg_field: Option<&'static str>,
	pub modifiers_field: Option<&'static str>,
	pub raw_field: Option<&'static str>,
	/// Every directive is unique by its whole attribute name.
	pub unique: bool,
}

/// A character standing for a directive's prefix and name, `:` for `v-bind`.
#[derive(Clone, Debug)]
pub struct Shorthand {
	pub token: &'static str,
	pub name: &'static str,
	pub modifiers: Vec<&'static str>,
}

#[derive(Clone, Debug)]
pub enum DirectiveValue {
	/// The one expression of the attribute value, `on:click={handler}`; `name` makes the
	/// directive's own argument the expression when there is no value: `bind:value`.
	Expression { optional: bool, name: bool },
	/// The one pattern of the attribute value, `let:item={{ id }}`.
	Pattern { optional: bool, name: bool },
	/// The attribute value as it is, text and expressions.
	Value,
	/// The attribute value read by a form, `v-for="item in items"`.
	Form(Form),
}

/// Which names a directive may not repeat on an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unique {
	No,
	/// Its own argument, among directives of its kind.
	Kind,
	/// Its argument, among the plain attributes too.
	Attribute,
}

#[derive(Clone, Debug)]
pub struct DirectiveRule {
	pub name: Match,
	pub ty: &'static str,
	pub value: DirectiveValue,
	pub flags: Vec<(&'static str, bool)>,
	pub unique: Unique,
	/// What the directive declares in the scope of its element: the fields of its form, or,
	/// with none named, its value.
	pub declares: Option<Vec<&'static str>>,
}

/// Where a construct may stand.
pub use definition::Place;

/// A marker and what follows it: a construct's open, one of its branches, or its close.
#[derive(Clone, Debug)]
pub struct Piece {
	pub marker: Vec<&'static str>,
	/// Whitespace must follow the marker.
	pub space: bool,
	pub form: Form,
	/// The marker and the form's last word, as messages name the piece: `{:else}`.
	pub display: &'static str,
	/// The form's last word before any body: where the piece's tag ends.
	pub end: &'static str,
}

/// A tag, or with a close a block.
#[derive(Clone, Debug)]
pub struct Construct {
	pub name: &'static str,
	pub ty: &'static str,
	pub places: Vec<Place>,
	pub open: Piece,
	pub branches: Vec<Piece>,
	pub close: Option<Piece>,
	/// The boolean fields of the branches that reopen the block, each true when that branch did.
	pub chain_flags: Vec<&'static str>,

	/// Every entry the construct's forms can read, and every body they can open.
	pub entries: Vec<(&'static str, bool)>,
	pub bodies: Vec<(&'static str, bool)>,
}

impl Construct {
	pub fn is_block(&self) -> bool {
		self.close.is_some()
	}

	pub fn stands(&self, place: Place) -> bool {
		self.places.contains(&place)
	}

	/// The one field of a tag that reads one thing, its entry.
	pub fn single(&self) -> Option<(&'static str, Entry)> {
		match self.open.form.items.as_slice() {
			[Item::Entry { field, entry, .. }, Item::Literal(_)] if self.close.is_none() => Some((field, *entry)),
			_ => None,
		}
	}
}

/// What a field of the document's root holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootField {
	/// The document's nodes.
	Fragment,
	/// The script, the module one when `module`.
	Script {
		module: bool,
	},
	Style,
	/// Every comment read.
	Comments,
	EmptyList,
	Null,
}

/// A field of the document's root, or a scope around fields.
#[derive(Clone, Debug)]
pub enum DocField {
	/// A field, what it holds, and whether it is left out rather than null when there is nothing.
	Field {
		field: &'static str,
		holds: RootField,
		omit: bool,
	},
	Scope(Vec<DocField>),
}

#[derive(Clone, Debug)]
pub struct DocumentRule {
	pub ty: &'static str,
	pub fields: Vec<DocField>,
}

/// The fields of every element node.
#[derive(Clone, Debug)]
pub struct ElementFields {
	pub name: &'static str,
	pub attributes: &'static str,
	pub children: &'static str,
}

/// The type and fields of every text node: the text as read, and as written.
#[derive(Clone, Debug)]
pub struct TextRule {
	pub ty: &'static str,
	pub data: &'static str,
	pub raw: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct CommentRule {
	pub ty: &'static str,
	pub data: &'static str,
}

/// A host language: its document, its content, and the JavaScript inside them.
#[derive(Clone, Debug)]
pub struct Grammar {
	pub name: &'static str,
	pub document: DocumentRule,
	/// `{name}` among the attributes is `name={name}`: the marker and the closing word around it.
	pub shorthand: Option<(&'static str, &'static str)>,
	/// An element the browser would close when another opens is closed there.
	pub autoclose: bool,
	/// Whitespace at the end of the source is not part of the document.
	pub trim: bool,
	pub void: Vec<&'static str>,
	/// A node wrapping every list of children, and its field: Svelte's `Fragment`.
	pub fragment: Option<(&'static str, &'static str)>,
	/// Every list of children opens a scope of its own.
	pub fragment_scope: bool,
	pub element_fields: ElementFields,
	pub text: TextRule,
	pub comment: CommentRule,
	/// The attribute that makes an element's subtree verbatim: text and plain attributes only.
	pub verbatim: Option<&'static str>,
	pub elements: Vec<ElementRule>,
	pub script: Option<ScriptRule>,
	pub style: Option<&'static str>,
	pub directive_syntax: Option<DirectiveSyntax>,
	pub shorthands: Vec<Shorthand>,
	pub directives: Vec<DirectiveRule>,
	pub constructs: Vec<Construct>,
	/// The first bytes of every marker, where a construct may start.
	pub starts: [bool; 256],
}

/// A component name: capitalized, or a dotted path of identifiers.
pub fn component_name(name: &str) -> bool {
	use crate::lexer::unicode::{is_id_continue, is_id_start};
	let mut chars = name.chars();
	let Some(first) = chars.next() else { return false };
	if first.is_uppercase() {
		return chars.all(|c| is_id_continue(c) || c == '.');
	}
	if !is_id_start(first) || !name.contains('.') {
		return false;
	}
	let mut parts = name.split('.');
	parts.next().is_some_and(|part| part.chars().all(is_id_continue))
		&& parts.all(|part| !part.is_empty() && part.chars().all(is_id_continue))
}

/// A grammar as the builders make it: what crosses the wire, lowered here into what the walker
/// reads. The TypeScript side is checked against these types.
pub mod definition {
	use super::{Cursor, Record, Wire, Writer};

	wire! {
		/// What fills a field: where it is read from, what reads it, and how.
		pub struct Source {
			pub from: &'static str,
			pub read: &'static str,
			pub optional: bool,
			pub bind: Bind,
			pub or_arg: bool,
			pub literal: Option<Literal>,
		}
	}

	wire! {
		pub copy enum Bind {
			No = false,
			Inside,
			Outside,
		}
	}

	wire! {
		/// A value a literal source always writes.
		pub copy enum Literal {
			True = true,
			False = false,
			Null = null,
			List: "readonly []" = "Array.isArray(v)",
		}
	}

	wire! {
		/// One step of a form: a host word, fields, or a group.
		pub enum Item {
			Word(&'static str) = "typeof v === 'string'",
			Opt(Vec<Item>),
			OneOf(Vec<Vec<Item>>),
			Scope(Vec<Item>),
			Fields(Record<Source>) = "",
		}
	}

	wire! {
		/// A node type and the form its fields come from.
		pub struct Node {
			pub node: &'static str,
			pub form: Option<Vec<Item>>,
		}
	}

	wire! {
		pub copy enum Uniqueness {
			Kind,
			Attributes,
		}
	}

	wire! {
		pub struct Directive {
			pub node: &'static str,
			pub form: Option<Vec<Item>>,
			pub unique: Option<Uniqueness>,
		}
	}

	wire! {
		pub copy enum Content {
			Raw,
			Rcdata,
		}
	}

	wire! {
		pub struct Element {
			pub node: &'static str,
			pub form: Option<Vec<Item>>,
			pub root: Option<bool>,
			pub once: Option<bool>,
			pub inside: Option<&'static str>,
			pub outside: Option<&'static str>,
			pub content: Option<Content>,
		}
	}

	wire! {
		pub struct Elements {
			pub fields: Record<Source>,
			pub rules: Option<Record<Element>>,
			pub component: Option<Element>,
			pub other: Option<Element>,
		}
	}

	wire! {
		pub struct Script {
			pub element: &'static str,
			pub module: Option<Vec<(&'static str, Option<&'static str>)>>,
			pub typescript: Option<Vec<(&'static str, Option<&'static str>)>>,
		}
	}

	wire! {
		pub copy enum Raw {
			Raw,
		}
	}

	wire! {
		pub struct Directives {
			pub prefix: Option<&'static str>,
			pub arg: Option<&'static str>,
			pub modifier: Option<&'static str>,
			pub dynamic: Option<(&'static str, &'static str)>,
			pub unique: Option<Raw>,
			pub fields: Record<Source>,
			pub shorthands: Option<Record<Vec<&'static str>>>,
			pub rules: Option<Record<Directive>>,
			pub other: Option<Directive>,
		}
	}

	wire! {
		/// Where a construct may stand: in content, in an attribute value, among attributes, in the
		/// text of an element whose content is rcdata.
		pub copy enum Place {
			Content,
			Value,
			Attributes,
			Rcdata,
		}
	}

	wire! {
		/// What a construct opens or closes with: its marker, then what its form reads.
		pub struct Piece {
			pub marker: Vec<&'static str>,
			/// Whitespace must follow the marker.
			pub space: Option<bool>,
			pub form: Vec<Item>,
		}
	}

	wire! {
		/// A branch of a block; `reopen` nests the block again into that field, with that flag true on it.
		pub struct Branch {
			pub marker: Vec<&'static str>,
			pub space: Option<bool>,
			pub form: Vec<Item>,
			pub reopen: Option<(&'static str, &'static str)>,
		}
	}

	wire! {
		/// A tag, or with `close` a block.
		pub struct Construct {
			pub node: &'static str,
			pub r#in: Option<Vec<Place>>,
			pub open: Piece,
			pub branches: Option<Vec<Branch>>,
			pub close: Option<Piece>,
		}
	}

	wire! {
		pub struct Attributes {
			/// `{name}` among the attributes is `name={name}`: the marker and the closing word around the name.
			pub shorthand: Option<(&'static str, &'static str)>,
		}
	}

	wire! {
		/// A host language: its document, its content, and the JavaScript inside them.
		pub struct Definition {
			pub document: Node,
			pub text: Node,
			pub comment: Node,
			pub fragment: Option<Node>,
			pub attributes: Option<Attributes>,
			pub autoclose: Option<bool>,
			pub trim: Option<bool>,
			pub void: Option<Vec<&'static str>>,
			pub verbatim: Option<&'static str>,
			pub elements: Elements,
			pub script: Option<Script>,
			pub style: Option<&'static str>,
			pub directives: Option<Directives>,
			pub constructs: Option<Record<Construct>>,
		}
	}

	wire! {
		/// What crosses: the host's name and its definition.
		pub struct Host {
			pub name: &'static str,
			pub definition: Definition,
		}
	}

	impl Host {
		pub fn read(bytes: &[u8]) -> Result<Host, String> {
			let mut cursor = Cursor::new(bytes)?;
			let host = <Host as Wire>::read(&mut cursor)?;
			if !cursor.done() {
				return Err("words after the grammar".into());
			}
			Ok(host)
		}

		/// The host on its wire, as the JavaScript side writes it.
		pub fn wire(&self) -> Vec<u8> {
			let mut writer = Writer::default();
			self.write(&mut writer);
			writer.bytes()
		}
	}
}

// ── the definition lowered into the walker's rules

use definition::{Bind, Host, Literal, Source};

#[derive(Clone, Copy, PartialEq, Eq)]
enum At {
	Block,
	Tag,
	Directive,
}

fn entry_of(read: &str) -> Result<Entry, String> {
	Ok(match read {
		"expression" => Entry::Expression,
		"pattern" => Entry::Pattern,
		"params" => Entry::Params,
		"identifier" => Entry::Identifier,
		"typeParameters" => Entry::TypeParameters,
		"statement" => Entry::Statement,
		"code" => Entry::Code,
		"const" => Entry::Const,
		"identifiers" => Entry::Identifiers,
		"text" => Entry::Text,
		other => return Err(format!("no entry reads {other}")),
	})
}

/// The fields among items, in order.
fn sources(items: &[definition::Item]) -> Vec<(&'static str, &Source)> {
	items
		.iter()
		.filter_map(|item| match item {
			definition::Item::Fields(record) => Some(record.0.iter().map(|(field, source)| (*field, source))),
			_ => None,
		})
		.flatten()
		.collect()
}

/// Whether a body stands anywhere among `items`, groups included.
fn has_body(items: &[definition::Item]) -> bool {
	items.iter().any(|item| match item {
		definition::Item::Fields(record) => record.0.iter().any(|(_, s)| s.from == "content"),
		definition::Item::Opt(inner) | definition::Item::Scope(inner) => has_body(inner),
		definition::Item::OneOf(list) => list.iter().any(|inner| has_body(inner)),
		definition::Item::Word(_) => false,
	})
}

/// The fields `items` can read, each once; sibling alternatives may read the same field.
fn fields_in(ty: &str, items: &[definition::Item]) -> Result<Vec<&'static str>, String> {
	let mut seen: Vec<&'static str> = Vec::new();
	for item in items {
		let own = match item {
			definition::Item::Fields(record) => record.0.iter().map(|(field, _)| *field).collect(),
			definition::Item::Opt(inner) | definition::Item::Scope(inner) => fields_in(ty, inner)?,
			definition::Item::OneOf(list) => {
				let mut union: Vec<&'static str> = Vec::new();
				for inner in list {
					for field in fields_in(ty, inner)? {
						if !union.contains(&field) {
							union.push(field);
						}
					}
				}
				union
			}
			definition::Item::Word(_) => Vec::new(),
		};
		for field in own {
			if seen.contains(&field) {
				return Err(format!("{ty} reads {field} twice"));
			}
			seen.push(field);
		}
	}
	Ok(seen)
}

/// Whether a read of the items always ends in a body: the last item is one, or a required group
/// whose every alternative ends in one.
fn closed(items: &[Item], body: Option<&Body>) -> bool {
	body.is_some()
		|| matches!(items.last(), Some(Item::Group { alternatives, required: true, .. })
			if alternatives.iter().all(|a| closed(&a.items, a.body.as_ref())))
}

/// The field among `fields` read by `read`.
fn part(fields: &Record<Source>, read: &str) -> Option<&'static str> {
	fields.0.iter().find(|(_, s)| s.read == read).map(|(field, _)| *field)
}

/// The fields of a site that reads each of them once and never leaves one out.
fn once(what: &str, fields: &Record<Source>) -> Result<(), String> {
	let mut reads = Vec::new();
	for (field, s) in &fields.0 {
		if s.optional {
			return Err(format!("{field} on {what} is never left out"));
		}
		if reads.contains(&s.read) {
			return Err(format!("{what} reads {} twice", s.read));
		}
		reads.push(s.read);
	}
	Ok(())
}

fn token(what: &str, s: &str) -> Result<(), String> {
	if s.is_empty() {
		return Err(format!("{what} is empty"));
	}
	Ok(())
}

/// Whether a literal stands anywhere in `items`, groups included.
fn has_literal(items: &[definition::Item]) -> bool {
	items.iter().any(|item| match item {
		definition::Item::Fields(record) => record.0.iter().any(|(_, s)| s.from == "literal"),
		definition::Item::Opt(inner) | definition::Item::Scope(inner) => has_literal(inner),
		definition::Item::OneOf(list) => list.iter().any(|inner| has_literal(inner)),
		definition::Item::Word(_) => false,
	})
}

fn form(ty: &str, items: &[definition::Item], site: At, bound: &mut Vec<Declare>) -> Result<Form, String> {
	fields_in(ty, items)?;
	let (items, body) = sequence(ty, items, site, bound)?;
	Ok(Form {
		items,
		body,
		entries: Vec::new(),
	})
}

fn sequence(
	ty: &str,
	items: &[definition::Item],
	site: At,
	bound: &mut Vec<Declare>,
) -> Result<(Vec<Item>, Option<Body>), String> {
	let mut out = Vec::new();
	let mut body: Option<Body> = None;
	for (at, item) in items.iter().enumerate() {
		match item {
			definition::Item::Word(word) => out.push(Item::Literal(word)),
			definition::Item::OneOf(_) | definition::Item::Opt(_) => {
				let required = matches!(item, definition::Item::OneOf(_));
				let one;
				let list = match item {
					definition::Item::OneOf(list) => list.as_slice(),
					definition::Item::Opt(inner) if inner.is_empty() => return Err(format!("{ty}: `opt` needs items")),
					definition::Item::Opt(inner) => match inner.as_slice() {
						[definition::Item::OneOf(list)] => list.as_slice(),
						_ => {
							one = [inner.clone()];
							&one
						}
					},
					_ => unreachable!(),
				};
				let alternatives = alternatives(ty, list, site, bound)?;
				if list.iter().any(|items| has_body(items)) {
					let last = site == At::Block && at == items.len() - 1;
					if !last || !required || !alternatives.iter().all(|a| closed(&a.items, a.body.as_ref())) {
						return Err(format!(
							"{ty}: a group ending in bodies is the last item of a block's form, required, with every alternative ending in a body"
						));
					}
					bound.clear();
				}
				out.push(Item::Group {
					alternatives,
					required,
					after: &[],
				});
			}
			definition::Item::Scope(_) => return Err(format!("{ty}: a scope belongs to the document or the fragment")),
			definition::Item::Fields(record) => {
				for (field, source) in &record.0 {
					match source.from {
						"literal" if site == At::Directive => continue,
						"content" => {
							if site != At::Block || at != items.len() - 1 {
								return Err(format!("{ty}: {field} is a body, which only ends a block's form"));
							}
							if let Some(first) = &body {
								return Err(format!("{ty}: {field} is a second body after {}", first.field));
							}
							body = Some(Body {
								field,
								omit: source.optional,
								chain: None,
								declares: std::mem::take(bound),
							});
							continue;
						}
						"js" => {}
						from => {
							return Err(format!(
								"{ty}: {field} reads {from} {}, which a form cannot",
								source.read
							));
						}
					}
					if site == At::Tag && source.bind != Bind::No {
						return Err(format!("{ty}: {field} binds, but a tag opens no scope"));
					}
					let outside = source.bind == Bind::Outside;
					if source.bind != Bind::No && !bound.iter().any(|d| d.field == *field && d.outside == outside) {
						bound.push(Declare { field, outside });
					}
					out.push(Item::Entry {
						field,
						entry: entry_of(source.read)?,
						omit: source.optional,
						stops: Stops::default(),
					});
				}
			}
		}
	}
	Ok((out, body))
}

fn alternatives(
	ty: &str,
	list: &[Vec<definition::Item>],
	site: At,
	bound: &mut Vec<Declare>,
) -> Result<Vec<Alternative>, String> {
	if list.is_empty() {
		return Err(format!("{ty}: `oneOf` needs alternatives"));
	}
	list.iter()
		.map(|items| {
			if items.is_empty() {
				return Err(format!("{ty}: an alternative needs items"));
			}
			// an alternative ending in a body declares what was bound before the group too
			let mut own = bound.clone();
			let bound = if has_body(items) { &mut own } else { &mut *bound };
			let (items, body) = sequence(ty, items, site, bound)?;
			Ok(Alternative { items, body })
		})
		.collect()
}

fn holds(field: &str, source: &Source) -> Result<RootField, String> {
	Ok(match (source.from, source.read, source.literal) {
		("literal", _, Some(Literal::Null)) => RootField::Null,
		("literal", _, Some(Literal::List)) => RootField::EmptyList,
		("literal", _, _) => return Err(format!("{field} on the document holds null or a list, not a boolean")),
		(_, "script", _) => RootField::Script { module: false },
		(_, "script:module", _) => RootField::Script { module: true },
		(_, "fragment", _) => RootField::Fragment,
		(_, "style", _) => RootField::Style,
		(_, "comments", _) => RootField::Comments,
		(_, read, _) => return Err(format!("{field} on the document cannot hold {read}")),
	})
}

fn scoped(items: &[definition::Item], reads: &mut Vec<&'static str>) -> Result<Vec<DocField>, String> {
	let mut out = Vec::new();
	for item in items {
		match item {
			definition::Item::Scope(inner) => out.push(DocField::Scope(scoped(inner, reads)?)),
			definition::Item::Fields(record) => {
				for (field, source) in &record.0 {
					if source.from != "literal" {
						if reads.contains(&source.read) {
							return Err(format!("the document reads {} twice", source.read));
						}
						reads.push(source.read);
					}
					out.push(DocField::Field {
						field,
						holds: holds(field, source)?,
						omit: source.optional,
					});
				}
			}
			_ => return Err("the document holds fields and scopes only".into()),
		}
	}
	Ok(out)
}

fn element(name: Match, rule: &definition::Element) -> Result<ElementRule, String> {
	let this = match rule.form.as_deref().unwrap_or_default() {
		[] => None,
		[definition::Item::Fields(Record(fields))]
			if fields.len() == 1
				&& fields[0].1.from == "element"
				&& matches!(fields[0].1.read, "this" | "this:text") =>
		{
			Some((fields[0].0, fields[0].1.read == "this:text"))
		}
		_ => {
			return Err(format!(
				"{} reads one `this` field at most, and nothing else",
				rule.node
			));
		}
	};
	Ok(ElementRule {
		name,
		ty: rule.node,
		this,
		root: rule.root == Some(true),
		once: rule.once == Some(true),
		inside: rule.inside,
		outside: rule.outside,
		raw: rule.content == Some(definition::Content::Raw),
		rcdata: rule.content == Some(definition::Content::Rcdata),
	})
}

fn directive(name: Match, rule: &definition::Directive) -> Result<DirectiveRule, String> {
	let items = rule.form.as_deref().unwrap_or_default();
	let ty = rule.node;
	let grouped = items.iter().any(|item| match item {
		definition::Item::Opt(inner) | definition::Item::Scope(inner) => has_literal(inner),
		definition::Item::OneOf(list) => list.iter().any(|inner| has_literal(inner)),
		_ => false,
	});
	if grouped {
		return Err(format!(
			"{ty}: a flag stands outside groups, since every node of it has one"
		));
	}
	let mut flags = Vec::new();
	for (field, s) in sources(items).iter().filter(|(_, s)| s.from == "literal") {
		match s.literal {
			Some(Literal::True) => flags.push((*field, true)),
			Some(Literal::False) => flags.push((*field, false)),
			_ => return Err(format!("{ty}: {field} is a flag: true or false")),
		}
	}
	let rest: Vec<&definition::Item> = items
		.iter()
		.filter(|item| match item {
			definition::Item::Fields(record) => record.0.iter().any(|(_, s)| s.from != "literal"),
			_ => true,
		})
		.collect();
	let unique = match rule.unique {
		None => Unique::No,
		Some(definition::Uniqueness::Kind) => Unique::Kind,
		Some(definition::Uniqueness::Attributes) => Unique::Attribute,
	};
	if let [only] = rest.as_slice() {
		let wrapped = match only {
			definition::Item::Opt(inner) if inner.len() == 1 => Some(inner.as_slice()),
			_ => None,
		};
		let mut single = match wrapped {
			Some(inner) => sources(inner),
			None => sources(std::slice::from_ref(*only)),
		};
		single.retain(|(_, s)| s.from != "literal");
		if let [(field, s)] = single.as_slice()
			&& s.from == "value"
		{
			let fixed = if s.read == "value" { "value" } else { "expression" };
			if *field != fixed {
				return Err(format!("{ty}: a directive's value is read into {fixed}"));
			}
			if s.optional {
				return Err(format!(
					"{ty}: {field} is a directive's value: `opt` makes it null when missing, it is never left out"
				));
			}
			let (optional, name_too) = (wrapped.is_some() || s.or_arg, s.or_arg);
			let value = match s.read {
				"value" => DirectiveValue::Value,
				"pattern" => DirectiveValue::Pattern {
					optional,
					name: name_too,
				},
				_ => DirectiveValue::Expression {
					optional,
					name: name_too,
				},
			};
			return Ok(DirectiveRule {
				name,
				ty,
				value,
				flags,
				unique,
				declares: (s.bind != Bind::No).then(Vec::new),
			});
		}
	}
	let mut bound = Vec::new();
	let items: Vec<definition::Item> = rest.into_iter().cloned().collect();
	let value = DirectiveValue::Form(form(ty, &items, At::Directive, &mut bound)?);
	Ok(DirectiveRule {
		name,
		ty,
		value,
		flags,
		unique,
		declares: (!bound.is_empty()).then(|| bound.iter().map(|d| d.field).collect()),
	})
}

/// The last word a form reads before any body, on its first path.
fn last_word(items: &[Item]) -> Option<&'static str> {
	match items.last()? {
		Item::Literal(word) => Some(word),
		Item::Group { alternatives, .. } => alternatives.first().and_then(|a| last_word(&a.items)),
		Item::Entry { .. } => None,
	}
}

/// A marker and a word as messages write them: run together, a space where two letters meet.
fn display(marker: &[&str], end: &str) -> String {
	let letter = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
	let mut out = String::new();
	for part in marker.iter().chain([&end]) {
		if out.ends_with(letter) && part.starts_with(letter) {
			out.push(' ');
		}
		out.push_str(part);
	}
	out
}

fn piece(
	ty: &str,
	marker: &[&'static str],
	space: Option<bool>,
	items: &[definition::Item],
	site: At,
	bound: &mut Vec<Declare>,
) -> Result<Piece, String> {
	if marker.is_empty() || marker.iter().any(|part| part.is_empty()) {
		return Err(format!("{ty}: a marker needs parts, none of them empty"));
	}
	let form = form(ty, items, site, bound)?;
	let end = last_word(&form.items).ok_or_else(|| format!("{ty}: a marker's form ends its tag with a word"))?;
	Ok(Piece {
		display: keep(&display(marker, end)),
		marker: marker.to_vec(),
		space: space == Some(true),
		end,
		form,
	})
}

fn construct(name: &'static str, rule: &definition::Construct) -> Result<Construct, String> {
	let ty = rule.node;
	let block = rule.close.is_some();
	let places = match &rule.r#in {
		Some(list) if list.is_empty() => return Err(format!("{ty} stands nowhere")),
		Some(list) => list.clone(),
		None => vec![Place::Content],
	};
	let mut chain_flags = Vec::new();
	let mut branches = Vec::new();
	for branch in rule.branches.iter().flatten() {
		if !block {
			return Err(format!("{ty} has branches, so it closes"));
		}
		let mut piece = piece(
			ty,
			&branch.marker,
			branch.space,
			&branch.form,
			At::Block,
			&mut Vec::new(),
		)?;
		if let Some((field, flag)) = branch.reopen {
			if !chain_flags.contains(&flag) {
				chain_flags.push(flag);
			}
			let own = piece.form.body.take().ok_or_else(|| {
				format!(
					"{ty}: the {} branch reopens the block, so it reads that block's body",
					piece.display
				)
			})?;
			piece.form.body = Some(Body {
				field,
				omit: false,
				chain: Some((own.field, flag)),
				declares: own.declares,
			});
		}
		if !closed(&piece.form.items, piece.form.body.as_ref()) {
			return Err(format!("{ty}: the {} branch ends in no body", piece.display));
		}
		branches.push(piece);
	}
	let mut bound = Vec::new();
	let site = if block { At::Block } else { At::Tag };
	let open = piece(
		ty,
		&rule.open.marker,
		rule.open.space,
		&rule.open.form,
		site,
		&mut bound,
	)?;
	if block && !closed(&open.form.items, open.form.body.as_ref()) {
		return Err(format!("{ty} ends in no body"));
	}
	if let Some(unread) = bound.first() {
		return Err(format!(
			"{ty}: {} binds after the body, so nothing declares it",
			unread.field
		));
	}
	let close = match &rule.close {
		Some(close) => {
			let piece = piece(ty, &close.marker, close.space, &close.form, At::Tag, &mut Vec::new())?;
			if !piece.form.items.iter().all(|item| matches!(item, Item::Literal(_))) {
				return Err(format!("{ty}: a close reads words only"));
			}
			Some(piece)
		}
		None => None,
	};
	Ok(Construct {
		name,
		ty,
		places,
		open,
		branches,
		close,
		chain_flags,
		entries: Vec::new(),
		bodies: Vec::new(),
	})
}

fn lower(host: Host) -> Result<Grammar, String> {
	let d = host.definition;
	let texts = |node: &definition::Node,
	             reads: &[&str]|
	 -> Result<(&'static str, &'static str, Option<&'static str>), String> {
		let items = node.form.as_deref().unwrap_or_default();
		let record = sources(items);
		let fields_only = items.iter().all(|item| matches!(item, definition::Item::Fields(_)));
		if !fields_only || record.iter().any(|(_, s)| s.from != "text" || !reads.contains(&s.read)) {
			return Err(format!("{} holds {} and nothing else", node.node, reads.join(" and ")));
		}
		let by = |read: &str| record.iter().find(|(_, s)| s.read == read).map(|(field, _)| *field);
		Ok((
			node.node,
			by("data").ok_or_else(|| format!("{} needs a data field", node.node))?,
			by("raw"),
		))
	};
	let (text_ty, text_data, text_raw) = texts(&d.text, &["data", "raw"])?;
	let (comment_ty, comment_data, _) = texts(&d.comment, &["data"])?;
	let fragment = match &d.fragment {
		Some(node) => {
			let (scope, items) = match node.form.as_deref().unwrap_or_default() {
				[definition::Item::Scope(inner)] => (true, inner.as_slice()),
				items => (false, items),
			};
			let ([(field, source)], [_]) = (&sources(items)[..], items) else {
				return Err(format!("{} holds one field, its nodes", node.node));
			};
			if source.from != "nodes" {
				return Err(format!("{} holds one field, its nodes", node.node));
			}
			let field = *field;
			Some((node.node, field, scope))
		}
		None => None,
	};
	once("an element", &d.elements.fields)?;
	let element_fields =
		|read: &str| part(&d.elements.fields, read).ok_or_else(|| format!("elements need a field read by {read}"));
	let mut elements = Vec::new();
	for (name, rule) in d.elements.rules.iter().flat_map(|r| &r.0) {
		elements.push(element(Match::Exact(name), rule)?);
	}
	if let Some(rule) = &d.elements.component {
		elements.push(element(Match::Component, rule)?);
	}
	if let Some(rule) = &d.elements.other {
		elements.push(element(Match::Any, rule)?);
	}
	let x = d.directives.as_ref();
	if let Some(x) = x {
		once("a directive", &x.fields)?;
		for (what, mark) in [
			("prefix", x.prefix),
			("argument mark", x.arg),
			("modifier mark", x.modifier),
		] {
			token(&format!("the directive {what}"), mark.unwrap_or("-"))?;
		}
		if let Some((open, close)) = x.dynamic {
			token("the dynamic argument's open", open)?;
			token("the dynamic argument's close", close)?;
		}
	}
	let directive_syntax = x.map(|x| DirectiveSyntax {
		prefix: x.prefix,
		arg: x.arg.unwrap_or(":"),
		modifier: x.modifier.unwrap_or("|"),
		dynamic: x.dynamic,
		name_field: part(&x.fields, "name"),
		arg_field: part(&x.fields, "arg"),
		modifiers_field: part(&x.fields, "modifiers"),
		raw_field: part(&x.fields, "raw"),
		unique: x.unique.is_some(),
	});
	let shorthands = x
		.and_then(|x| x.shorthands.as_ref())
		.map(|s| &s.0[..])
		.unwrap_or_default()
		.iter()
		.map(|(token, list)| Shorthand {
			token,
			name: list.first().copied().unwrap_or(""),
			modifiers: list.get(1..).unwrap_or_default().to_vec(),
		})
		.collect();
	let mut directives = Vec::new();
	for (name, rule) in x.and_then(|x| x.rules.as_ref()).iter().flat_map(|r| &r.0) {
		directives.push(directive(Match::Exact(name), rule)?);
	}
	if let Some(other) = x.and_then(|x| x.other.as_ref()) {
		directives.push(directive(Match::Any, other)?);
	}
	let mut constructs = Vec::new();
	for (name, rule) in d.constructs.iter().flat_map(|r| &r.0) {
		constructs.push(construct(name, rule)?);
	}
	let shorthand = d.attributes.as_ref().and_then(|a| a.shorthand);
	if let Some((open, close)) = shorthand {
		token("the shorthand's marker", open)?;
		token("the shorthand's closing word", close)?;
	}
	let mut reads = Vec::new();
	let document = DocumentRule {
		ty: d.document.node,
		fields: scoped(d.document.form.as_deref().unwrap_or_default(), &mut reads)?,
	};
	if !reads.contains(&"fragment") {
		return Err(format!("{} holds no content", d.document.node));
	}
	Ok(Grammar {
		name: host.name,
		document,
		shorthand,
		autoclose: d.autoclose == Some(true),
		trim: d.trim == Some(true),
		void: d.void.clone().unwrap_or_default(),
		fragment: fragment.map(|(ty, field, _)| (ty, field)),
		fragment_scope: fragment.is_some_and(|(_, _, scope)| scope),
		element_fields: ElementFields {
			name: element_fields("name")?,
			attributes: element_fields("attributes")?,
			children: element_fields("fragment")?,
		},
		text: TextRule {
			ty: text_ty,
			data: text_data,
			raw: text_raw,
		},
		comment: CommentRule {
			ty: comment_ty,
			data: comment_data,
		},
		verbatim: d.verbatim,
		elements,
		script: d.script.as_ref().map(|s| ScriptRule {
			name: s.element,
			module: s.module.clone().unwrap_or_default(),
			typescript: s.typescript.clone().unwrap_or_default(),
		}),
		style: d.style,
		directive_syntax,
		shorthands,
		directives,
		constructs,
		starts: [false; 256],
	})
}

impl Form {
	/// `follow` is what ends the form: the close delimiter, nothing for an attribute value.
	fn finish(&mut self, follow: &[&'static str]) -> Result<(), String> {
		resolve(&mut self.items, follow);
		let mut entries = Vec::new();
		collect_entries(&self.items, &mut entries);
		let mut bodies = Vec::new();
		if let Some(body) = &self.body {
			bodies.push(body);
		}
		collect_alternative_bodies(&self.items, &mut bodies);
		for body in bodies {
			for declare in &body.declares {
				if !entries.iter().any(|(field, _)| *field == declare.field) {
					return Err(format!(
						"`{}` declares `{}`, which no entry of the form reads",
						body.field, declare.field
					));
				}
			}
		}
		self.entries = entries;
		Ok(())
	}
}

impl Grammar {
	/// A grammar with nothing in it, behind a stylesheet read on its own.
	pub(crate) fn empty() -> Grammar {
		Grammar {
			name: "",
			document: DocumentRule {
				ty: "Document",
				fields: vec![DocField::Field {
					field: "children",
					holds: RootField::Fragment,
					omit: false,
				}],
			},
			shorthand: None,
			autoclose: false,
			trim: false,
			void: Vec::new(),
			fragment: None,
			fragment_scope: false,
			element_fields: ElementFields {
				name: "name",
				attributes: "attributes",
				children: "children",
			},
			text: TextRule {
				ty: "Text",
				data: "data",
				raw: None,
			},
			comment: CommentRule {
				ty: "Comment",
				data: "data",
			},
			verbatim: None,
			elements: Vec::new(),
			script: None,
			style: None,
			directive_syntax: None,
			shorthands: Vec::new(),
			directives: Vec::new(),
			constructs: Vec::new(),
			starts: [false; 256],
		}
	}

	/// Reads a grammar off its wire: the definition as the builders made it, lowered.
	pub fn read(bytes: &[u8]) -> Result<Grammar, String> {
		let mut grammar = lower(Host::read(bytes)?)?;
		grammar.finish()?;
		Ok(grammar)
	}

	/// The TypeScript module of every type on the wire, for the side that writes it.
	pub fn wire_types() -> String {
		module::<Host>("crates/teasel/src/host/grammar.rs")
	}

	fn finish(&mut self) -> Result<(), String> {
		if self.name.is_empty() {
			return Err("a grammar names its host".into());
		}
		for rule in &mut self.directives {
			if let DirectiveValue::Form(form) = &mut rule.value {
				form.finish(&[])?;
			}
		}
		let mut starts = [false; 256];
		for construct in &mut self.constructs {
			construct.open.form.finish(&[])?;
			let mut entries = construct.open.form.entries.clone();
			let mut bodies = Vec::new();
			collect_bodies(&construct.open.form, &mut bodies);
			for branch in &mut construct.branches {
				branch.form.finish(&[])?;
				entries.extend_from_slice(&branch.form.entries);
				collect_bodies(&branch.form, &mut bodies);
			}
			if let Some(close) = &mut construct.close {
				close.form.finish(&[])?;
			}
			construct.entries = entries;
			construct.bodies = bodies;
			for piece in std::iter::once(&construct.open)
				.chain(&construct.branches)
				.chain(&construct.close)
			{
				starts[piece.marker[0].as_bytes()[0] as usize] = true;
			}
		}
		if let Some((open, _)) = self.shorthand {
			starts[open.as_bytes()[0] as usize] = true;
		}
		self.starts = starts;
		for (ty, _) in self.own_children() {
			if crate::recipe::names_type(ty) {
				return Err(format!("a node type named {ty} is JavaScript's"));
			}
			if self.style.is_some() && super::css::CHILDREN.iter().any(|(css, _)| *css == ty) {
				return Err(format!("a node type named {ty} is a stylesheet's"));
			}
		}
		Ok(())
	}

	pub fn element(&self, name: &str) -> Option<&ElementRule> {
		self.elements.iter().find(|rule| match rule.name {
			Match::Exact(exact) => exact == name,
			Match::Component => component_name(name),
			Match::Any => true,
		})
	}

	pub fn component(&self) -> Option<&ElementRule> {
		self.elements.iter().find(|rule| rule.name == Match::Component)
	}

	/// The rule of a directive by its name; `*` when the grammar takes any.
	pub fn directive(&self, name: &str) -> Option<&DirectiveRule> {
		self.directives.iter().find(|rule| match rule.name {
			Match::Exact(exact) => exact == name,
			_ => true,
		})
	}

	/// The construct that reads values written as one expression: an attribute's chunk, a shorthand's value.
	pub fn value_expression(&self) -> Option<&Construct> {
		self.constructs
			.iter()
			.find(|c| c.stands(Place::Value) && matches!(c.single(), Some((_, Entry::Expression))))
	}

	pub fn is_void(&self, name: &str) -> bool {
		name.starts_with('!') || self.void.contains(&name)
	}
}

impl Grammar {
	/// Every type of node a document of this grammar holds, each with the fields that hold nodes or
	/// lists of them: what a walk over a document follows. The attribute, script and stylesheet
	/// nodes are the walker's own; their fields are named here beside the grammar's.
	pub fn children(&self) -> Vec<(&'static str, Vec<&'static str>)> {
		let mut out = self.own_children();
		if self.style.is_some() {
			out.extend(super::css::children(true));
		}
		out
	}

	fn own_children(&self) -> Vec<(&'static str, Vec<&'static str>)> {
		let mut out: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
		let mut add = |ty: &'static str, fields: &[&'static str]| {
			let at = match out.iter().position(|(known, _)| *known == ty) {
				Some(at) => at,
				None => {
					out.push((ty, Vec::new()));
					out.len() - 1
				}
			};
			for &field in fields {
				if !out[at].1.contains(&field) {
					out[at].1.push(field);
				}
			}
		};
		fn body(body: &Body, into: &mut Vec<&'static str>) {
			into.push(body.field);
			into.extend(body.chain.map(|(field, _)| field));
		}
		fn items(list: &[Item], into: &mut Vec<&'static str>) {
			for item in list {
				match item {
					Item::Literal(_) => {}
					// text and type parameters are answered as text
					Item::Entry { field, entry, .. } => {
						if !matches!(entry, Entry::Text | Entry::TypeParameters) {
							into.push(field);
						}
					}
					Item::Group { alternatives, .. } => {
						for alternative in alternatives {
							items(&alternative.items, into);
							alternative.body.iter().for_each(|b| body(b, into));
						}
					}
				}
			}
		}
		fn form(form: &Form, into: &mut Vec<&'static str>) {
			items(&form.items, into);
			form.body.iter().for_each(|b| body(b, into));
		}
		fn document(fields: &[DocField], into: &mut Vec<&'static str>) {
			for field in fields {
				match field {
					// comments are not nodes: a walk does not follow them
					DocField::Field { field, holds, .. } => {
						if !matches!(holds, RootField::Null | RootField::Comments) {
							into.push(field);
						}
					}
					DocField::Scope(inner) => document(inner, into),
				}
			}
		}
		let mut fields = Vec::new();
		document(&self.document.fields, &mut fields);
		add(self.document.ty, &fields);
		if let Some((ty, field)) = self.fragment {
			add(ty, &[field]);
		}
		add(self.text.ty, &[]);
		add(self.comment.ty, &[]);
		for rule in &self.elements {
			fields.clear();
			fields.extend([self.element_fields.attributes, self.element_fields.children]);
			fields.extend(rule.this.map(|(field, _)| field));
			add(rule.ty, &fields);
		}
		add("Attribute", &["value"]);
		if self.script.is_some() {
			add("Script", &["content", "attributes"]);
		}
		// a dynamic argument, `:[expression]`, is a node in the argument's field
		let dynamic = self
			.directive_syntax
			.as_ref()
			.and_then(|syntax| syntax.dynamic.and(syntax.arg_field));
		for rule in &self.directives {
			fields.clear();
			fields.extend(dynamic);
			match &rule.value {
				DirectiveValue::Expression { .. } | DirectiveValue::Pattern { .. } => fields.push("expression"),
				DirectiveValue::Value => fields.push("value"),
				DirectiveValue::Form(f) => form(f, &mut fields),
			}
			add(rule.ty, &fields);
		}
		for rule in &self.constructs {
			fields.clear();
			form(&rule.open.form, &mut fields);
			rule.branches.iter().for_each(|branch| form(&branch.form, &mut fields));
			add(rule.ty, &fields);
		}
		out
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// cargo test --release -p teasel --lib read_phases -- --ignored --nocapture
	#[test]
	#[ignore]
	fn read_phases() {
		let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/hosts/svelte/host.wire")).unwrap();
		let time = |label: &str, f: &dyn Fn()| {
			let t = std::time::Instant::now();
			for _ in 0..1000 {
				f();
			}
			println!("{label}: {:?}", t.elapsed() / 1000);
		};
		time("Host::read", &|| {
			std::hint::black_box(Host::read(&bytes).unwrap());
		});
		let host = Host::read(&bytes).unwrap();
		time("lower", &|| {
			std::hint::black_box(lower(host.clone()).unwrap());
		});
		let grammar = lower(host.clone()).unwrap();
		time("finish", &|| {
			let mut g = grammar.clone();
			g.finish().unwrap();
			std::hint::black_box(g);
		});
	}

	#[test]
	fn words_round_trip() {
		let mut w = Writer::default();
		Some("a").write(&mut w);
		vec![("b", true), ("a", false)].write(&mut w);
		Option::<&str>::None.write(&mut w);
		let bytes = w.bytes();
		assert_eq!(bytes.len(), (2 + 4 + 8) * 4 + 2);
		let mut c = Cursor::new(&bytes).unwrap();
		assert_eq!(Option::<&str>::read(&mut c).unwrap(), Some("a"));
		assert_eq!(Vec::<(&str, bool)>::read(&mut c).unwrap(), [("b", true), ("a", false)]);
		assert_eq!(Option::<&str>::read(&mut c).unwrap(), None);
		assert!(c.done() && c.word().is_err());
		let mut short = Cursor::new(&bytes[..bytes.len() - 1]).unwrap();
		assert!(Option::<&str>::read(&mut short).is_ok() && Vec::<(&str, bool)>::read(&mut short).is_err());
		assert!(Cursor::new(&bytes[..12]).is_err() && Cursor::new(&[]).is_err());
	}

	#[test]
	fn spells_names() {
		assert_eq!(camel("attribute_expressions"), "attributeExpressions");
		assert_eq!(camel("TypeParameters"), "typeParameters");
	}
}
