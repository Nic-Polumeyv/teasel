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
	pool: usize,
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
		let at = 2 + 2 * strings;
		let end = at + records;
		if bytes.len() < end * 4 {
			return Err("the grammar ends early".into());
		}
		Ok(Cursor {
			bytes,
			at,
			end,
			strings,
			pool: end * 4,
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
		let bytes = self
			.bytes
			.get(self.pool + offset..self.pool + offset + len)
			.ok_or("the grammar ends early")?;
		Ok(keep(
			std::str::from_utf8(bytes).map_err(|_| "a string on the grammar is not UTF-8")?,
		))
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
	fn definitions(_out: &mut Vec<Definition>) {}
}

/// A named type on the wire as TypeScript: its type and the function that writes it.
pub(crate) struct Definition {
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

	fn definitions(out: &mut Vec<Definition>) {
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
		format!(
			"w.word({value}.length);\n{value}.forEach((item) => {{\n\t{}\n}});",
			indent(&T::ts_write("item"))
		)
	}

	fn definitions(out: &mut Vec<Definition>) {
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
		format!("readonly [{}, {}]", A::ts(), B::ts())
	}

	fn ts_write(value: &str) -> String {
		format!(
			"{}\n{}",
			A::ts_write(&format!("{value}[0]")),
			B::ts_write(&format!("{value}[1]"))
		)
	}

	fn definitions(out: &mut Vec<Definition>) {
		A::definitions(out);
		B::definitions(out);
	}
}

fn indent(text: &str) -> String {
	text.replace('\n', "\n\t")
}

/// A field as TypeScript: its name, docs, and the type and writer of its value.
pub(crate) struct Field {
	pub(crate) name: &'static str,
	pub(crate) docs: &'static [&'static str],
	pub(crate) optional: bool,
	pub(crate) ty: String,
	pub(crate) write: String,
}

/// A variant as TypeScript: a name, or a key holding a payload.
pub(crate) struct Variant {
	pub(crate) name: &'static str,
	pub(crate) docs: &'static [&'static str],
	pub(crate) ty: Option<String>,
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

/// A union as TypeScript, one alternative per line with its docs, and the statements that write
/// a value of it: its variant's index, then the payload.
pub(crate) fn union(variants: &[Variant]) -> (String, String) {
	let mut ty = String::new();
	let mut names = Vec::new();
	let mut keyed = Vec::new();
	for (tag, v) in variants.iter().enumerate() {
		if !v.docs.is_empty() {
			ty.push_str(&format!("\n\t/**{} */", v.docs.join("\n\t *")));
		}
		let key = camel(v.name);
		match &v.ty {
			None => {
				ty.push_str(&format!("\n\t| '{key}'"));
				names.push(format!("case '{key}':\n\tw.word({tag});\n\tbreak;"));
			}
			Some(payload) => {
				ty.push_str(&format!("\n\t| {{ {key}: {payload} }}"));
				keyed.push((key, format!("w.word({tag});\n{}", v.write)));
			}
		}
	}
	let mut write = String::new();
	if !names.is_empty() {
		write.push_str(&format!(
			"if (typeof v === 'string') {{\n\tswitch (v) {{\n\t\t{}\n\t}}\n}}",
			indent(&indent(&names.join("\n")))
		));
	}
	for (i, (key, body)) in keyed.iter().enumerate() {
		let last = i == keyed.len() - 1;
		let head = if !write.is_empty() && last {
			String::new()
		} else {
			format!("if ('{key}' in v) ")
		};
		if !write.is_empty() {
			write.push_str(" else ");
		}
		write.push_str(&format!("{head}{{\n\t{}\n}}", indent(body)));
	}
	(ty, write)
}

/// Renders every definition reachable from `T` as a TypeScript module: the types, the functions
/// that write them, and the writer they write to.
pub(crate) fn module<T: Wire>(root: &str) -> String {
	let mut out = format!(
		"// written by {root}; `cargo test` pins it\n\n\
/** The wire: words, then a pool of strings kept once. */\n\
export class Writer {{\n\
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
\t\tconst out = new Uint8Array(poolAt + this.#pool.reduce((size, s) => size + s.length * 3, 0));\n\
\t\tconst words = new Uint32Array(out.buffer, 0, head + this.#count);\n\
\t\twords[0] = this.#count;\n\
\t\twords[1] = this.#pool.length;\n\
\t\tconst encoder = new TextEncoder();\n\
\t\tlet offset = 0;\n\
\t\tthis.#pool.forEach((s, i) => {{\n\
\t\t\tconst {{ written }} = encoder.encodeInto(s, out.subarray(poolAt + offset));\n\
\t\t\twords[2 + 2 * i] = offset;\n\
\t\t\twords[3 + 2 * i] = written;\n\
\t\t\toffset += written;\n\
\t\t}});\n\
\t\twords.set(this.#words.subarray(0, this.#count), head);\n\
\t\t// the engine reads little-endian words; a big-endian platform swaps them here\n\
\t\tif (new Uint8Array(new Uint32Array([1]).buffer)[0] !== 1) {{\n\
\t\t\tconst view = new DataView(out.buffer);\n\
\t\t\twords.forEach((word, i) => view.setUint32(i * 4, word, true));\n\
\t\t}}\n\
\t\treturn out.subarray(0, poolAt + offset);\n\
\t}}\n\
}}\n"
	);
	let mut definitions = Vec::new();
	T::definitions(&mut definitions);
	for d in definitions {
		out.push('\n');
		if !d.docs.is_empty() {
			out.push_str(&format!("/**{} */\n", d.docs));
		}
		out.push_str(&format!(
			"export type {} ={}{};\n",
			d.name,
			if d.ty.starts_with('\n') { "" } else { " " },
			d.ty
		));
		out.push_str(&format!(
			"export function write{}(w: Writer, v: {}): void {{\n\t{}\n}}\n",
			d.name,
			d.name,
			indent(&d.write)
		));
	}
	out
}

/// A type on the wire, defined once: its Rust definition, its reader and writer, and the
/// TypeScript type and writer. Docs come first on the type and on every field; a struct's
/// `derived` fields are computed after reading and are not on the wire; a `copy enum` holds
/// names only.
macro_rules! wire {
	(
		$(#[doc = $doc:literal])* $vis:vis struct $name:ident {
			$($(#[doc = $fdoc:literal])* $fvis:vis $f:ident : $t:ty),* $(,)?
		}
		$(derived { $($(#[doc = $ddoc:literal])* $dvis:vis $d:ident : $dt:ty),* $(,)? })?
	) => {
		$(#[doc = $doc])*
		#[derive(Clone, Debug)]
		$vis struct $name {
			$($(#[doc = $fdoc])* $fvis $f: $t,)*
			$($($(#[doc = $ddoc])* $dvis $d: $dt,)*)?
		}

		impl $crate::host::grammar::Wire for $name {
			fn read(c: &mut $crate::host::grammar::Cursor) -> Result<Self, String> {
				Ok($name {
					$($f: <$t as $crate::host::grammar::Wire>::read(c)?,)*
					$($($d: Default::default(),)*)?
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

			fn definitions(out: &mut Vec<$crate::host::grammar::Definition>) {
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
				out.push($crate::host::grammar::Definition {
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

	// a variant that is a name
	(@variants $name:ident $head:tt ($c:ident $w:ident $v:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($is:tt)*] [$($write:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $variant:ident $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($c $w $v $out)
			[$($def)* $(#[doc = $vdoc])* $variant,]
			[$($read)* |_| Ok($name::$variant),]
			[$($is)* matches!($v, $name::$variant),]
			[$($write)* $name::$variant => {}]
			[$($ts)* $crate::host::grammar::Variant { name: stringify!($variant), docs: &[$($vdoc),*], ty: None, write: String::new() },]
			[$($deps)*]
			$($($rest)*)?);
	};

	// a variant holding one value: `{ name: value }`
	(@variants $name:ident $head:tt ($c:ident $w:ident $v:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($is:tt)*] [$($write:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $variant:ident ( $t:ty ) $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($c $w $v $out)
			[$($def)* $(#[doc = $vdoc])* $variant($t),]
			[$($read)* |$c| Ok($name::$variant(<$t as $crate::host::grammar::Wire>::read($c)?)),]
			[$($is)* matches!($v, $name::$variant(_)),]
			[$($write)* $name::$variant(value) => <$t as $crate::host::grammar::Wire>::write(value, $w),]
			[$($ts)* $crate::host::grammar::Variant {
				name: stringify!($variant),
				docs: &[$($vdoc),*],
				ty: Some(<$t as $crate::host::grammar::Wire>::ts()),
				write: <$t as $crate::host::grammar::Wire>::ts_write(&format!("v.{}", $crate::host::grammar::camel(stringify!($variant)))),
			},]
			[$($deps)* <$t as $crate::host::grammar::Wire>::definitions($out);]
			$($($rest)*)?);
	};

	// a variant with fields: `{ name: { fields } }`
	(@variants $name:ident $head:tt ($c:ident $w:ident $v:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($is:tt)*] [$($write:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $variant:ident { $($(#[doc = $fdoc:literal])* $f:ident : $t:ty),* $(,)? }
		$(derived { $($(#[doc = $ddoc:literal])* $d:ident : $dt:ty),* $(,)? })? $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($c $w $v $out)
			[$($def)* $(#[doc = $vdoc])* $variant { $($(#[doc = $fdoc])* $f: $t,)* $($($(#[doc = $ddoc])* $d: $dt,)*)? },]
			[$($read)* |$c| Ok($name::$variant {
				$($f: <$t as $crate::host::grammar::Wire>::read($c)?,)*
				$($($d: Default::default(),)*)?
			}),]
			[$($is)* matches!($v, $name::$variant { .. }),]
			[$($write)* $name::$variant { $($f,)* .. } => { $(<$t as $crate::host::grammar::Wire>::write($f, $w);)* }]
			[$($ts)* {
				let fields = [$($crate::host::grammar::Field {
					name: stringify!($f),
					docs: &[$($fdoc),*],
					optional: <$t as $crate::host::grammar::Wire>::OPTIONAL,
					ty: <$t as $crate::host::grammar::Wire>::ts_field(),
					write: <$t as $crate::host::grammar::Wire>::ts_write(&format!("v.{}.{}", $crate::host::grammar::camel(stringify!($variant)), $crate::host::grammar::camel(stringify!($f)))),
				},)*];
				$crate::host::grammar::Variant {
					name: stringify!($variant),
					docs: &[$($vdoc),*],
					ty: Some($crate::host::grammar::fields(&fields, true)),
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

			fn definitions($out: &mut Vec<$crate::host::grammar::Definition>) {
				if $out.iter().any(|d| d.name == stringify!($name)) {
					return;
				}
				let (ty, write) = $crate::host::grammar::union(&[$($ts)*]);
				$out.push($crate::host::grammar::Definition {
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

wire! {
	/// A JavaScript entry inside a form.
	pub copy enum Entry {
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
}

wire! {
	/// One step of a form.
	pub enum Item {
		/// One of the host's words or punctuators.
		Literal(&'static str),
		/// A JavaScript entry read into a field; `omit` leaves the field out when the entry was not
		/// read, where the plain form gives it null.
		Entry {
			field: &'static str,
			entry: Entry,
			omit: bool,
		} derived {
			stops: Stops,
		},
		/// Alternatives tried in order: at most one, or exactly one when `required`.
		Group {
			alternatives: Vec<Alternative>,
			required: bool,
		} derived {
			/// The literals that may follow the group.
			after: &'static [&'static str],
		},
	}
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

wire! {
	pub struct Alternative {
		pub items: Vec<Item>,
		/// The body the block opens when this alternative was read, `[ then value=pattern -> then ]`.
		pub body: Option<Body>,
	}
}

wire! {
	/// What a block's body is: the field that holds it, and what the body's scope declares.
	pub struct Body {
		pub field: &'static str,
		/// The field is left out of blocks that never opened this body; otherwise it is null there.
		pub omit: bool,
		/// A branch that nests a new block of the same kind into the field, `{:else if}`: the field
		/// of the nested block that its own body fills.
		pub chain: Option<&'static str>,
		pub declares: Vec<Declare>,
	}
}

wire! {
	pub struct Declare {
		pub field: &'static str,
		/// Declared in the scope around the block rather than inside it: a snippet's name.
		pub outside: bool,
	}
}

wire! {
	pub struct Form {
		pub items: Vec<Item>,
		pub body: Option<Body>,
	} derived {
		/// Every entry the form can read, and whether its field is left out when it was not.
		pub entries: Vec<(&'static str, bool)>,
	}
}

wire! {
	pub copy enum Match {
		Exact(&'static str),
		/// A capitalized or dotted name.
		Component,
		Any,
	}
}

wire! {
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
}

wire! {
	pub struct ScriptRule {
		pub name: &'static str,
		/// Attributes that make the script the module one, each with the text value it needs, if any.
		pub module: Vec<(&'static str, Option<&'static str>)>,
		/// Attributes that make the document TypeScript, the same way.
		pub typescript: Vec<(&'static str, Option<&'static str>)>,
	}
}

wire! {
	/// How a directive's attribute name is spelled: `prefix name arg modifiers`, the name being the
	/// directive's own when there is no prefix.
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
}

wire! {
	/// A character standing for a directive's prefix and name, `:` for `v-bind`.
	pub struct Shorthand {
		pub token: &'static str,
		pub name: &'static str,
		pub modifiers: Vec<&'static str>,
	}
}

wire! {
	pub enum DirectiveValue {
		/// The one expression of the attribute value, `on:click={handler}`; `name` makes the
		/// directive's own argument the expression when there is no value: `bind:value`.
		Expression {
			optional: bool,
			name: bool,
		},
		/// The one pattern of the attribute value, `let:item={{ id }}`.
		Pattern {
			optional: bool,
			name: bool,
		},
		/// The attribute value as it is, text and expressions.
		Value,
		/// The attribute value read by a form, `v-for="item in items"`.
		Form(Form),
	}
}

wire! {
	/// Which names a directive may not repeat on an element.
	pub copy enum Unique {
		No,
		/// Its own argument, among directives of its kind.
		Kind,
		/// Its argument, among the plain attributes too.
		Attribute,
	}
}

wire! {
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
}

wire! {
	pub struct BlockRule {
		pub name: &'static str,
		pub ty: &'static str,
		pub open: Form,
		pub branches: Vec<BranchRule>,
		/// The boolean field that says the block was opened by a chained branch.
		pub chain_flag: Option<&'static str>,
	} derived {
		/// Every entry the block's forms can read, and every body they can open.
		pub entries: Vec<(&'static str, bool)>,
		pub bodies: Vec<(&'static str, bool)>,
	}
}

wire! {
	pub struct BranchRule {
		pub words: Vec<&'static str>,
		pub form: Form,
	}
}

wire! {
	pub struct TagRule {
		pub name: &'static str,
		pub ty: &'static str,
		pub form: Form,
		/// The tag stands among an element's attributes rather than in content.
		pub attribute: bool,
	}
}

wire! {
	/// The characters after the opening delimiter that make a tag a block, a branch, a close or a
	/// special tag: `{#if}`, `{:else}`, `{/if}`, `{@html}`.
	pub struct Sigils {
		pub open: &'static str,
		pub branch: &'static str,
		pub close: &'static str,
		pub tag: &'static str,
	}
}

wire! {
	/// What a field of the document's root holds.
	pub copy enum RootField {
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
}

wire! {
	/// A field of the document's root, or a scope around fields.
	pub enum DocField {
		/// A field, what it holds, and whether it is left out rather than null when there is nothing.
		Field {
			field: &'static str,
			holds: RootField,
			omit: bool,
		},
		Scope(Vec<DocField>),
	}
}

wire! {
	pub struct DocumentRule {
		pub ty: &'static str,
		pub fields: Vec<DocField>,
	}
}

wire! {
	/// The fields of every element node.
	pub struct ElementFields {
		pub name: &'static str,
		pub attributes: &'static str,
		pub children: &'static str,
	}
}

wire! {
	/// The type and fields of every text node: the text as read, and as written.
	pub struct TextRule {
		pub ty: &'static str,
		pub data: &'static str,
		pub raw: Option<&'static str>,
	}
}

wire! {
	pub struct CommentRule {
		pub ty: &'static str,
		pub data: &'static str,
	}
}

wire! {
	/// A host language: its document, its content, and the JavaScript inside them.
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

impl Form {
	// what the walker takes from a form is computed once, here
	fn finish(&mut self) -> Result<(), String> {
		resolve(&mut self.items, &[]);
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
	/// Reads a grammar off its wire, the bytes `wire` writes.
	pub fn read(bytes: &[u8]) -> Result<Grammar, String> {
		let mut cursor = Cursor::new(bytes)?;
		let mut grammar = <Grammar as Wire>::read(&mut cursor)?;
		if !cursor.done() {
			return Err("words after the grammar".into());
		}
		grammar.finish()?;
		Ok(grammar)
	}

	/// The grammar on its wire, as the JavaScript side writes it.
	pub fn wire(&self) -> Vec<u8> {
		let mut writer = Writer::default();
		self.write(&mut writer);
		writer.bytes()
	}

	/// The TypeScript module of every type on the wire, for the side that writes it.
	pub fn wire_types() -> String {
		module::<Grammar>("crates/teasel/src/host/grammar.rs")
	}

	fn finish(&mut self) -> Result<(), String> {
		if self.name.is_empty() {
			return Err("a grammar names its host".into());
		}
		for rule in &mut self.directives {
			if let DirectiveValue::Form(form) = &mut rule.value {
				form.finish()?;
			}
		}
		for rule in self
			.tags
			.iter_mut()
			.chain(&mut self.declaration)
			.chain(&mut self.expression)
		{
			rule.form.finish()?;
		}
		for block in &mut self.blocks {
			block.open.finish()?;
			let mut entries = block.open.entries.clone();
			let mut bodies = Vec::new();
			collect_bodies(&block.open, &mut bodies);
			for branch in &mut block.branches {
				branch.form.finish()?;
				if branch.form.body.is_none() {
					return Err(format!(
						"the {} branch of {} needs a body",
						branch.words.join(" "),
						block.name
					));
				}
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
