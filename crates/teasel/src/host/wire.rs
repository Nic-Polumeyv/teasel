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
\t#words: number[] = [];\n\
\t#strings = new Map<string, number>();\n\
\tword(word: number): void {{\n\
\t\tthis.#words.push(word);\n\
\t}}\n\
\tstr(s: string): void {{\n\
\t\tlet i = this.#strings.get(s);\n\
\t\tif (i === undefined) this.#strings.set(s, (i = this.#strings.size));\n\
\t\tthis.#words.push(i);\n\
\t}}\n\
\t/** Little-endian words: the count of record words, the count of strings, each string's offset and length, the record words; then the pool, UTF-8. */\n\
\tbytes(): Uint8Array {{\n\
\t\tconst encoder = new TextEncoder();\n\
\t\tconst strings = [...this.#strings.keys()].map((s) => encoder.encode(s));\n\
\t\tconst pool = strings.reduce((size, s) => size + s.length, 0);\n\
\t\tconst head = 2 + 2 * strings.length;\n\
\t\tconst out = new Uint8Array((head + this.#words.length) * 4 + pool);\n\
\t\tconst view = new DataView(out.buffer);\n\
\t\tview.setUint32(0, this.#words.length, true);\n\
\t\tview.setUint32(4, strings.length, true);\n\
\t\tlet offset = 0;\n\
\t\tstrings.forEach((s, i) => {{\n\
\t\t\tview.setUint32(8 + i * 8, offset, true);\n\
\t\t\tview.setUint32(12 + i * 8, s.length, true);\n\
\t\t\tout.set(s, (head + this.#words.length) * 4 + offset);\n\
\t\t\toffset += s.length;\n\
\t\t}});\n\
\t\tthis.#words.forEach((word, i) => view.setUint32((head + i) * 4, word, true));\n\
\t\treturn out;\n\
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

		impl $crate::host::wire::Wire for $name {
			fn read(c: &mut $crate::host::wire::Cursor) -> Result<Self, String> {
				Ok($name {
					$($f: <$t as $crate::host::wire::Wire>::read(c)?,)*
					$($($d: Default::default(),)*)?
				})
			}

			fn write(&self, w: &mut $crate::host::wire::Writer) {
				$(<$t as $crate::host::wire::Wire>::write(&self.$f, w);)*
			}

			fn ts() -> String {
				stringify!($name).into()
			}

			fn ts_write(value: &str) -> String {
				format!("write{}(w, {value});", stringify!($name))
			}

			fn definitions(out: &mut Vec<$crate::host::wire::Definition>) {
				if out.iter().any(|d| d.name == stringify!($name)) {
					return;
				}
				let fields = [$($crate::host::wire::Field {
					name: stringify!($f),
					docs: &[$($fdoc),*],
					optional: <$t as $crate::host::wire::Wire>::OPTIONAL,
					ty: <$t as $crate::host::wire::Wire>::ts_field(),
					write: <$t as $crate::host::wire::Wire>::ts_write(&format!("v.{}", $crate::host::wire::camel(stringify!($f)))),
				},)*];
				out.push($crate::host::wire::Definition {
					name: stringify!($name),
					docs: <[&str]>::join(&[$($doc),*], "\n *"),
					ty: $crate::host::wire::fields(&fields, false),
					write: $crate::host::wire::writes(&fields),
				});
				$(<$t as $crate::host::wire::Wire>::definitions(out);)*
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
			[$($ts)* $crate::host::wire::Variant { name: stringify!($variant), docs: &[$($vdoc),*], ty: None, write: String::new() },]
			[$($deps)*]
			$($($rest)*)?);
	};

	// a variant holding one value: `{ name: value }`
	(@variants $name:ident $head:tt ($c:ident $w:ident $v:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($is:tt)*] [$($write:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $variant:ident ( $t:ty ) $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($c $w $v $out)
			[$($def)* $(#[doc = $vdoc])* $variant($t),]
			[$($read)* |$c| Ok($name::$variant(<$t as $crate::host::wire::Wire>::read($c)?)),]
			[$($is)* matches!($v, $name::$variant(_)),]
			[$($write)* $name::$variant(value) => <$t as $crate::host::wire::Wire>::write(value, $w),]
			[$($ts)* $crate::host::wire::Variant {
				name: stringify!($variant),
				docs: &[$($vdoc),*],
				ty: Some(<$t as $crate::host::wire::Wire>::ts()),
				write: <$t as $crate::host::wire::Wire>::ts_write(&format!("v.{}", $crate::host::wire::camel(stringify!($variant)))),
			},]
			[$($deps)* <$t as $crate::host::wire::Wire>::definitions($out);]
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
				$($f: <$t as $crate::host::wire::Wire>::read($c)?,)*
				$($($d: Default::default(),)*)?
			}),]
			[$($is)* matches!($v, $name::$variant { .. }),]
			[$($write)* $name::$variant { $($f,)* .. } => { $(<$t as $crate::host::wire::Wire>::write($f, $w);)* }]
			[$($ts)* {
				let fields = [$($crate::host::wire::Field {
					name: stringify!($f),
					docs: &[$($fdoc),*],
					optional: <$t as $crate::host::wire::Wire>::OPTIONAL,
					ty: <$t as $crate::host::wire::Wire>::ts_field(),
					write: <$t as $crate::host::wire::Wire>::ts_write(&format!("v.{}.{}", $crate::host::wire::camel(stringify!($variant)), $crate::host::wire::camel(stringify!($f)))),
				},)*];
				$crate::host::wire::Variant {
					name: stringify!($variant),
					docs: &[$($vdoc),*],
					ty: Some($crate::host::wire::fields(&fields, true)),
					write: $crate::host::wire::writes(&fields),
				}
			},]
			[$($deps)* $(<$t as $crate::host::wire::Wire>::definitions($out);)*]
			$($($rest)*)?);
	};

	(@variants $name:ident [$(#[doc = $doc:literal])* $vis:vis derive($($derive:ident),*)] ($c:ident $w:ident $v:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($is:tt)*] [$($write:tt)*] [$($ts:tt)*] [$($deps:tt)*]) => {
		$(#[doc = $doc])*
		#[derive($($derive),*)]
		$vis enum $name { $($def)* }

		impl $crate::host::wire::Wire for $name {
			fn read($c: &mut $crate::host::wire::Cursor) -> Result<Self, String> {
				let readers: &[fn(&mut $crate::host::wire::Cursor) -> Result<Self, String>] = &[$($read)*];
				let tag = $c.word()?;
				let reader = readers.get(tag as usize).ok_or_else(|| format!("{tag} is not a {} on the grammar", stringify!($name)))?;
				reader($c)
			}

			fn write(&self, $w: &mut $crate::host::wire::Writer) {
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

			fn definitions($out: &mut Vec<$crate::host::wire::Definition>) {
				if $out.iter().any(|d| d.name == stringify!($name)) {
					return;
				}
				let (ty, write) = $crate::host::wire::union(&[$($ts)*]);
				$out.push($crate::host::wire::Definition {
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
pub(crate) use wire;

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
