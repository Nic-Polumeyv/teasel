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

#[derive(Clone, Debug)]
pub struct BlockRule {
	pub name: &'static str,
	pub ty: &'static str,
	pub open: Form,
	pub branches: Vec<BranchRule>,
	/// The boolean fields of the branches that reopen the block, each true when that branch did.
	pub chain_flags: Vec<&'static str>,

	/// Every entry the block's forms can read, and every body they can open.
	pub entries: Vec<(&'static str, bool)>,
	pub bodies: Vec<(&'static str, bool)>,
}

#[derive(Clone, Debug)]
pub struct BranchRule {
	pub words: Vec<&'static str>,
	pub form: Form,
}

#[derive(Clone, Debug)]
pub struct TagRule {
	pub name: &'static str,
	pub ty: &'static str,
	pub form: Form,
	/// The tag stands among an element's attributes rather than in content.
	pub attribute: bool,
}

impl TagRule {
	/// The one field of a tag that reads one thing.
	pub fn field(&self) -> &'static str {
		match self.form.items.as_slice() {
			[Item::Entry { field, .. }] => field,
			_ => unreachable!("checked when the grammar was read"),
		}
	}
}

/// The characters after the opening delimiter that make a tag a block, a branch, a close or a
/// special tag: `{#if}`, `{:else}`, `{/if}`, `{@html}`.
#[derive(Clone, Debug)]
pub struct Sigils {
	pub open: &'static str,
	pub branch: &'static str,
	pub close: &'static str,
	pub tag: &'static str,
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
	/// What opens and closes an expression in text, `{` and `}`.
	pub delimiters: (&'static str, &'static str),
	/// Attribute values hold expressions between the delimiters, as text does.
	pub attribute_expressions: bool,
	/// `{name}` among the attributes is `name={name}`.
	pub attribute_shorthand: bool,
	pub sigils: Option<Sigils>,
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
	pub spread: Option<&'static str>,
	pub blocks: Vec<BlockRule>,
	pub tags: Vec<TagRule>,
	pub declaration: Option<TagRule>,
	pub expression: Option<TagRule>,
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
			pub r#type: &'static str,
			pub items: Vec<Item>,
		}
	}

	wire! {
		/// An `else if`: the block again, nested into this field, with this flag set on it.
		pub struct Reopen {
			pub reopen: &'static str,
			pub flag: &'static str,
		}
	}

	wire! {
		pub enum Branch {
			Form(Vec<Item>) = "Array.isArray(v)",
			Reopen(Reopen) = "",
		}
	}

	wire! {
		pub struct Block {
			pub node: Node,
			pub branches: Record<Branch>,
		}
	}

	wire! {
		pub copy enum Among {
			Content,
			Attributes,
		}
	}

	wire! {
		pub struct Tag {
			pub node: Node,
			pub among: Among,
		}
	}

	wire! {
		pub copy enum Uniqueness {
			No,
			Kind,
			Attributes,
		}
	}

	wire! {
		pub struct Directive {
			pub node: Node,
			pub unique: Uniqueness,
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
			pub node: Node,
			pub root: bool,
			pub once: bool,
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
		pub struct Sigils {
			pub open: &'static str,
			pub branch: &'static str,
			pub close: &'static str,
			pub tag: &'static str,
			pub blocks: Option<Record<Block>>,
			pub tags: Option<Record<Tag>>,
		}
	}

	wire! {
		pub struct Attributes {
			pub expressions: Option<bool>,
			pub shorthand: Option<bool>,
		}
	}

	wire! {
		/// A host language: its document, its content, and the JavaScript inside them.
		pub struct Definition {
			pub document: Node,
			pub text: Node,
			pub comment: Node,
			pub fragment: Option<Node>,
			pub delimiters: (&'static str, &'static str),
			pub attributes: Option<Attributes>,
			pub autoclose: Option<bool>,
			pub trim: Option<bool>,
			pub void: Option<Vec<&'static str>>,
			pub verbatim: Option<&'static str>,
			pub elements: Elements,
			pub script: Option<Script>,
			pub style: Option<&'static str>,
			pub directives: Option<Directives>,
			pub spread: Option<&'static str>,
			pub sigils: Option<Sigils>,
			pub declaration: Option<Node>,
			pub expression: Option<Node>,
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

use definition::{Bind, Branch, Host, Literal, Source};

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
	let this = match rule.node.items.as_slice() {
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
				rule.node.r#type
			));
		}
	};
	Ok(ElementRule {
		name,
		ty: rule.node.r#type,
		this,
		root: rule.root,
		once: rule.once,
		inside: rule.inside,
		outside: rule.outside,
		raw: rule.content == Some(definition::Content::Raw),
		rcdata: rule.content == Some(definition::Content::Rcdata),
	})
}

fn directive(name: Match, rule: &definition::Directive) -> Result<DirectiveRule, String> {
	let grouped = rule.node.items.iter().any(|item| match item {
		definition::Item::Opt(inner) | definition::Item::Scope(inner) => has_literal(inner),
		definition::Item::OneOf(list) => list.iter().any(|inner| has_literal(inner)),
		_ => false,
	});
	if grouped {
		return Err(format!(
			"{}: a flag stands outside groups, since every node of it has one",
			rule.node.r#type
		));
	}
	let mut flags = Vec::new();
	for (field, s) in sources(&rule.node.items).iter().filter(|(_, s)| s.from == "literal") {
		match s.literal {
			Some(Literal::True) => flags.push((*field, true)),
			Some(Literal::False) => flags.push((*field, false)),
			_ => return Err(format!("{}: {field} is a flag: true or false", rule.node.r#type)),
		}
	}
	let rest: Vec<&definition::Item> = rule
		.node
		.items
		.iter()
		.filter(|item| match item {
			definition::Item::Fields(record) => record.0.iter().any(|(_, s)| s.from != "literal"),
			_ => true,
		})
		.collect();
	let unique = match rule.unique {
		definition::Uniqueness::No => Unique::No,
		definition::Uniqueness::Kind => Unique::Kind,
		definition::Uniqueness::Attributes => Unique::Attribute,
	};
	let ty = rule.node.r#type;
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

fn block(name: &'static str, rule: &definition::Block) -> Result<BlockRule, String> {
	let ty = rule.node.r#type;
	let mut chain_flags = Vec::new();
	let mut branches = Vec::new();
	for (words, branch) in &rule.branches.0 {
		if words.is_empty() {
			return Err(format!("{ty}: a branch needs words"));
		}
		let form = match branch {
			Branch::Form(items) => form(ty, items, At::Block, &mut Vec::new())?,
			Branch::Reopen(r) => {
				if !chain_flags.contains(&r.flag) {
					chain_flags.push(r.flag);
				}
				let mut head = form(ty, &rule.node.items, At::Block, &mut Vec::new())?;
				let own = head
					.body
					.take()
					.ok_or_else(|| format!("{ty}: a reopened form ends in its body"))?;
				head.body = Some(Body {
					field: r.reopen,
					omit: false,
					chain: Some((own.field, r.flag)),
					declares: own.declares,
				});
				head
			}
		};
		if !closed(&form.items, form.body.as_ref()) {
			return Err(format!("{ty}: the {words} branch ends in no body"));
		}
		branches.push(BranchRule {
			words: words.split(' ').collect(),
			form,
		});
	}
	let mut bound = Vec::new();
	let open = form(ty, &rule.node.items, At::Block, &mut bound)?;
	if !closed(&open.items, open.body.as_ref()) {
		return Err(format!("{ty} ends in no body"));
	}
	if let Some(unread) = bound.first() {
		return Err(format!(
			"{ty}: {} binds after the body, so nothing declares it",
			unread.field
		));
	}
	Ok(BlockRule {
		name,
		ty,
		open,
		branches,
		chain_flags,
		entries: Vec::new(),
		bodies: Vec::new(),
	})
}

fn tag(name: &'static str, node: &definition::Node, attribute: bool) -> Result<TagRule, String> {
	Ok(TagRule {
		name,
		ty: node.r#type,
		form: form(node.r#type, &node.items, At::Tag, &mut Vec::new())?,
		attribute,
	})
}

/// The node of a tag that reads one thing: the declaration's statement, the expression's expression.
fn one(node: &definition::Node, read: &str) -> Result<TagRule, String> {
	let rule = tag("", node, false)?;
	match rule.form.items.as_slice() {
		[Item::Entry { entry, .. }] if *entry == entry_of(read)? => Ok(rule),
		_ => Err(format!("{} holds one field, its {read}", node.r#type)),
	}
}

fn lower(host: Host) -> Result<Grammar, String> {
	let d = host.definition;
	let texts = |node: &definition::Node,
	             reads: &[&str]|
	 -> Result<(&'static str, &'static str, Option<&'static str>), String> {
		let record = sources(&node.items);
		let fields_only = node
			.items
			.iter()
			.all(|item| matches!(item, definition::Item::Fields(_)));
		if !fields_only || record.iter().any(|(_, s)| s.from != "text" || !reads.contains(&s.read)) {
			return Err(format!(
				"{} holds {} and nothing else",
				node.r#type,
				reads.join(" and ")
			));
		}
		let by = |read: &str| record.iter().find(|(_, s)| s.read == read).map(|(field, _)| *field);
		Ok((
			node.r#type,
			by("data").ok_or_else(|| format!("{} needs a data field", node.r#type))?,
			by("raw"),
		))
	};
	let (text_ty, text_data, text_raw) = texts(&d.text, &["data", "raw"])?;
	let (comment_ty, comment_data, _) = texts(&d.comment, &["data"])?;
	let fragment = match &d.fragment {
		Some(node) => {
			let (scope, items) = match node.items.as_slice() {
				[definition::Item::Scope(inner)] => (true, inner.as_slice()),
				items => (false, items),
			};
			let ([(field, source)], [_]) = (&sources(items)[..], items) else {
				return Err(format!("{} holds one field, its nodes", node.r#type));
			};
			if source.from != "nodes" {
				return Err(format!("{} holds one field, its nodes", node.r#type));
			}
			let field = *field;
			Some((node.r#type, field, scope))
		}
		None => None,
	};
	token("the open delimiter", d.delimiters.0)?;
	token("the close delimiter", d.delimiters.1)?;
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
	let sigils = d.sigils.as_ref();
	if let Some(s) = sigils {
		let marks = [
			("open", s.open),
			("branch", s.branch),
			("close", s.close),
			("tag", s.tag),
		];
		for (i, (what, mark)) in marks.iter().enumerate() {
			token(&format!("the {what} sigil"), mark)?;
			if let Some((other, _)) = marks[..i].iter().find(|(_, m)| m == mark) {
				return Err(format!("the {other} and {what} sigils are both {mark}"));
			}
		}
	}
	let mut blocks = Vec::new();
	for (name, rule) in sigils.and_then(|s| s.blocks.as_ref()).iter().flat_map(|r| &r.0) {
		blocks.push(block(name, rule)?);
	}
	let mut tags = Vec::new();
	for (name, rule) in sigils.and_then(|s| s.tags.as_ref()).iter().flat_map(|r| &r.0) {
		tags.push(tag(name, &rule.node, rule.among == definition::Among::Attributes)?);
	}
	let mut reads = Vec::new();
	let document = DocumentRule {
		ty: d.document.r#type,
		fields: scoped(&d.document.items, &mut reads)?,
	};
	if !reads.contains(&"fragment") {
		return Err(format!("{} holds no content", d.document.r#type));
	}
	Ok(Grammar {
		name: host.name,
		document,
		delimiters: d.delimiters,
		attribute_expressions: d.attributes.as_ref().is_some_and(|a| a.expressions == Some(true)),
		attribute_shorthand: d.attributes.as_ref().is_some_and(|a| a.shorthand == Some(true)),
		sigils: sigils.map(|s| Sigils {
			open: s.open,
			branch: s.branch,
			close: s.close,
			tag: s.tag,
		}),
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
		spread: d.spread,
		blocks,
		tags,
		declaration: d.declaration.as_ref().map(|node| one(node, "statement")).transpose()?,
		expression: d.expression.as_ref().map(|node| one(node, "expression")).transpose()?,
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
		let close = &[self.delimiters.1];
		for rule in &mut self.directives {
			if let DirectiveValue::Form(form) = &mut rule.value {
				form.finish(&[])?;
			}
		}
		for rule in self
			.tags
			.iter_mut()
			.chain(&mut self.declaration)
			.chain(&mut self.expression)
		{
			rule.form.finish(close)?;
		}
		for block in &mut self.blocks {
			block.open.finish(close)?;
			let mut entries = block.open.entries.clone();
			let mut bodies = Vec::new();
			collect_bodies(&block.open, &mut bodies);
			for branch in &mut block.branches {
				branch.form.finish(close)?;
				entries.extend_from_slice(&branch.form.entries);
				collect_bodies(&branch.form, &mut bodies);
			}
			block.entries = entries;
			block.bodies = bodies;
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

	pub fn block(&self, name: &str) -> Option<&BlockRule> {
		self.blocks.iter().find(|rule| rule.name == name)
	}

	pub fn tag(&self, name: &str) -> Option<&TagRule> {
		self.tags.iter().find(|rule| rule.name == name)
	}

	/// Whether an element of the name has no content: the grammar's list, and a doctype.
	pub fn is_void(&self, name: &str) -> bool {
		name.starts_with('!') || self.void.contains(&name)
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
