//! The wire a grammar crosses on: JSON read into a tree, and the trait that reads each of the
//! grammar's types out of it. `wire!` defines a type once and derives its reader and the
//! TypeScript type the writer on the other side is checked against. A grammar carries no
//! numbers, so the reader takes none.

use std::fmt::Write;

/// A JSON value as read; an object keeps its keys in order.
#[derive(Debug)]
pub(crate) enum Json {
	Null,
	Bool(bool),
	String(String),
	Array(Vec<Json>),
	Object(Vec<(String, Json)>),
}

impl Json {
	pub(crate) fn parse(text: &str) -> Result<Json, String> {
		let mut reader = Reader { text, at: 0 };
		let value = reader.value(0)?;
		reader.space();
		if reader.at < text.len() {
			return Err(reader.error("text after the value"));
		}
		Ok(value)
	}

	/// The fields of an object whose keys are among `keys`, each spelled in camel case.
	pub(crate) fn object<'a>(&'a self, path: &str, keys: &[&str]) -> Result<Vec<(&'static str, &'a Json)>, String> {
		let Json::Object(fields) = self else {
			return Err(format!("{path}: not an object"));
		};
		fields
			.iter()
			.enumerate()
			.map(|(i, (key, value))| {
				if fields[..i].iter().any(|(before, _)| before == key) {
					return Err(format!("{path}: {key} twice"));
				}
				let known = keys.iter().find(|name| is_key(key, name)).map(|name| keep(name));
				known
					.map(|name| (name, value))
					.ok_or_else(|| format!("{path}: unknown field {key}"))
			})
			.collect()
	}
}

struct Reader<'a> {
	text: &'a str,
	at: usize,
}

impl Reader<'_> {
	fn error(&self, what: &str) -> String {
		format!("{what} at byte {} of the grammar", self.at)
	}

	fn peek(&self) -> Option<u8> {
		self.text.as_bytes().get(self.at).copied()
	}

	fn space(&mut self) {
		while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
			self.at += 1;
		}
	}

	fn eat(&mut self, c: u8) -> Result<(), String> {
		if self.peek() != Some(c) {
			return Err(self.error(&format!("expected {}", c as char)));
		}
		self.at += 1;
		Ok(())
	}

	fn value(&mut self, depth: usize) -> Result<Json, String> {
		self.space();
		if depth > 256 {
			return Err(self.error("nesting too deep"));
		}
		match self.peek() {
			Some(b'"') => Ok(Json::String(self.string()?)),
			Some(b'[') => {
				self.at += 1;
				let mut items = Vec::new();
				self.space();
				if self.peek() == Some(b']') {
					self.at += 1;
					return Ok(Json::Array(items));
				}
				loop {
					items.push(self.value(depth + 1)?);
					self.space();
					match self.peek() {
						Some(b',') => self.at += 1,
						Some(b']') => {
							self.at += 1;
							return Ok(Json::Array(items));
						}
						_ => return Err(self.error("expected , or ]")),
					}
				}
			}
			Some(b'{') => {
				self.at += 1;
				let mut fields = Vec::new();
				self.space();
				if self.peek() == Some(b'}') {
					self.at += 1;
					return Ok(Json::Object(fields));
				}
				loop {
					self.space();
					if self.peek() != Some(b'"') {
						return Err(self.error("expected a key"));
					}
					let key = self.string()?;
					self.space();
					self.eat(b':')?;
					fields.push((key, self.value(depth + 1)?));
					self.space();
					match self.peek() {
						Some(b',') => self.at += 1,
						Some(b'}') => {
							self.at += 1;
							return Ok(Json::Object(fields));
						}
						_ => return Err(self.error("expected , or }")),
					}
				}
			}
			Some(b't') => self.word("true", Json::Bool(true)),
			Some(b'f') => self.word("false", Json::Bool(false)),
			Some(b'n') => self.word("null", Json::Null),
			_ => Err(self.error("expected a value")),
		}
	}

	fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
		if !self.text[self.at..].starts_with(word) {
			return Err(self.error("expected a value"));
		}
		self.at += word.len();
		Ok(value)
	}

	/// A string after its opening quote, escapes decoded.
	fn string(&mut self) -> Result<String, String> {
		self.at += 1;
		let mut out = String::new();
		loop {
			let start = self.at;
			while !matches!(self.peek(), Some(b'"' | b'\\') | None) {
				self.at += 1;
			}
			out.push_str(&self.text[start..self.at]);
			if self.peek() == Some(b'"') {
				self.at += 1;
				return Ok(out);
			}
			self.at += 1;
			let escaped = self.peek().ok_or_else(|| self.error("unterminated string"))?;
			self.at += 1;
			out.push(match escaped {
				b'"' => '"',
				b'\\' => '\\',
				b'/' => '/',
				b'b' => '\u{8}',
				b'f' => '\u{c}',
				b'n' => '\n',
				b'r' => '\r',
				b't' => '\t',
				b'u' => {
					let high = self.hex4()?;
					let code = if (0xD800..0xDC00).contains(&high) {
						self.eat(b'\\')?;
						self.eat(b'u')?;
						let low = self.hex4()?;
						0x10000 + ((high - 0xD800) << 10) + low.wrapping_sub(0xDC00)
					} else {
						high
					};
					char::from_u32(code).ok_or_else(|| self.error("not a character"))?
				}
				_ => return Err(self.error("unknown escape")),
			});
		}
	}

	fn hex4(&mut self) -> Result<u32, String> {
		let digits = self
			.text
			.get(self.at..self.at + 4)
			.ok_or_else(|| self.error("expected four hex digits"))?;
		self.at += 4;
		u32::from_str_radix(digits, 16).map_err(|_| self.error("expected four hex digits"))
	}
}

/// A grammar's names reach the walker as `&'static str`, so they live for the process.
pub(crate) fn keep(s: &str) -> &'static str {
	Box::leak(s.to_owned().into_boxed_str())
}

/// Whether a key on the wire is a Rust field's name in camel case.
fn is_key(key: &str, field: &str) -> bool {
	let mut key = key.chars();
	let mut up = false;
	for c in field.chars() {
		if c == '_' {
			up = true;
			continue;
		}
		let want = if up { c.to_ascii_uppercase() } else { c };
		up = false;
		if key.next() != Some(want) {
			return false;
		}
	}
	key.next().is_none()
}

/// A Rust field's name in camel case, as the wire spells it.
pub(crate) fn key(field: &str) -> String {
	let mut out = String::with_capacity(field.len());
	let mut up = false;
	for c in field.chars() {
		if c == '_' {
			up = true;
			continue;
		}
		out.push(if up { c.to_ascii_uppercase() } else { c });
		up = false;
	}
	out
}

/// A variant's name as the wire spells it: `TypeParameters` is `typeParameters`.
pub(crate) fn variant(name: &str) -> String {
	let mut chars = name.chars();
	chars
		.next()
		.map(|c| c.to_ascii_lowercase())
		.into_iter()
		.chain(chars)
		.collect()
}

pub(crate) fn is_variant(key: &str, name: &str) -> bool {
	key.len() == name.len()
		&& key.as_bytes()[1..] == name.as_bytes()[1..]
		&& key.as_bytes()[0].eq_ignore_ascii_case(&name.as_bytes()[0])
}

/// A type read off the wire, and the TypeScript type it is written as.
pub(crate) trait Wire: Sized {
	/// A field of the type may be left out.
	const OPTIONAL: bool = false;

	/// Reads a value; `path` names it in the error.
	fn read(json: &Json, path: &str) -> Result<Self, String>;

	/// Reads a field of an object, given when the object has it.
	fn field(json: Option<&Json>, path: &str) -> Result<Self, String> {
		match json {
			Some(json) => Self::read(json, path),
			None => Err(format!("{path}: missing")),
		}
	}

	/// The TypeScript type, as a field's type is spelled.
	fn ts() -> String;

	/// Adds the `export type` the type and every type it holds need, each once, in the order of
	/// first use.
	fn definitions(_out: &mut Vec<(&'static str, String)>) {}
}

impl Wire for &'static str {
	fn read(json: &Json, path: &str) -> Result<Self, String> {
		match json {
			Json::String(s) => Ok(keep(s)),
			_ => Err(format!("{path}: not a string")),
		}
	}

	fn ts() -> String {
		"string".into()
	}
}

impl Wire for bool {
	fn read(json: &Json, path: &str) -> Result<Self, String> {
		match json {
			Json::Bool(b) => Ok(*b),
			_ => Err(format!("{path}: not a boolean")),
		}
	}

	fn ts() -> String {
		"boolean".into()
	}
}

impl<T: Wire> Wire for Option<T> {
	const OPTIONAL: bool = true;

	fn read(json: &Json, path: &str) -> Result<Self, String> {
		match json {
			Json::Null => Ok(None),
			_ => T::read(json, path).map(Some),
		}
	}

	fn field(json: Option<&Json>, path: &str) -> Result<Self, String> {
		json.map_or(Ok(None), |json| Self::read(json, path))
	}

	fn ts() -> String {
		format!("{} | null", T::ts())
	}

	fn definitions(out: &mut Vec<(&'static str, String)>) {
		T::definitions(out);
	}
}

impl<T: Wire> Wire for Vec<T> {
	fn read(json: &Json, path: &str) -> Result<Self, String> {
		let Json::Array(items) = json else {
			return Err(format!("{path}: not a list"));
		};
		items
			.iter()
			.enumerate()
			.map(|(i, item)| T::read(item, &format!("{path}[{i}]")))
			.collect()
	}

	fn ts() -> String {
		format!("ReadonlyArray<{}>", T::ts())
	}

	fn definitions(out: &mut Vec<(&'static str, String)>) {
		T::definitions(out);
	}
}

impl<A: Wire, B: Wire> Wire for (A, B) {
	fn read(json: &Json, path: &str) -> Result<Self, String> {
		match json {
			Json::Array(items) if items.len() == 2 => Ok((
				A::read(&items[0], &format!("{path}[0]"))?,
				B::read(&items[1], &format!("{path}[1]"))?,
			)),
			_ => Err(format!("{path}: not a pair")),
		}
	}

	fn ts() -> String {
		format!("readonly [{}, {}]", A::ts(), B::ts())
	}

	fn definitions(out: &mut Vec<(&'static str, String)>) {
		A::definitions(out);
		B::definitions(out);
	}
}

/// The fields of a type as TypeScript, one per line, with their docs; a variant's on one line.
pub(crate) fn fields(parts: &[(&str, &[&str], bool, String)], inline: bool) -> String {
	let mut lines = Vec::new();
	for (name, docs, optional, ty) in parts {
		let field = format!("{}{}: {ty}", key(name), if *optional { "?" } else { "" });
		if docs.is_empty() {
			lines.push(field);
		} else {
			lines.push(format!(
				"/**{} */{}{field}",
				docs.join("\n\t *"),
				if inline { " " } else { "\n\t" }
			));
		}
	}
	if inline {
		format!("{{ {} }}", lines.join("; "))
	} else {
		format!("{{\n\t{};\n}}", lines.join(";\n\t"))
	}
}

/// A union's alternatives as TypeScript, one per line, with their docs.
pub(crate) fn union(parts: &[(&[&str], String)]) -> String {
	let mut out = String::new();
	for (docs, ty) in parts {
		if !docs.is_empty() {
			write!(out, "\n\t/**{} */", docs.join("\n\t *")).unwrap();
		}
		write!(out, "\n\t| {ty}").unwrap();
	}
	out
}

/// Renders every definition reachable from `T` as a TypeScript module.
pub(crate) fn module<T: Wire>(root: &str) -> String {
	let mut out = format!("// written by {root}; `cargo test` pins it\n");
	let mut definitions = Vec::new();
	T::definitions(&mut definitions);
	for (name, docs, body) in definitions.into_iter().map(|(name, body)| split_docs(name, body)) {
		out.push('\n');
		if !docs.is_empty() {
			writeln!(out, "/**{docs} */").unwrap();
		}
		writeln!(
			out,
			"export type {name} ={}{body};",
			if body.starts_with('\n') { "" } else { " " }
		)
		.unwrap();
	}
	out
}

/// A definition's docs come first, ended by a NUL, so the module can place them.
fn split_docs(name: &'static str, body: String) -> (&'static str, String, String) {
	match body.split_once('\0') {
		Some((docs, body)) => (name, docs.to_string(), body.to_string()),
		None => (name, String::new(), body),
	}
}

/// A type on the wire, defined once: its Rust definition, its reader and its TypeScript type.
/// Docs come first on the type and on every field; a struct's `derived` fields are computed
/// after reading and are not on the wire.
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
			fn read(json: &$crate::host::wire::Json, path: &str) -> Result<Self, String> {
				let fields = json.object(path, &[$(stringify!($f)),*])?;
				let field = |name: &str| fields.iter().find(|(key, _)| *key == name).map(|(_, value)| *value);
				Ok($name {
					$($f: <$t as $crate::host::wire::Wire>::field(field(stringify!($f)), &format!("{path}.{}", $crate::host::wire::key(stringify!($f))))?,)*
					$($($d: Default::default(),)*)?
				})
			}

			fn ts() -> String {
				stringify!($name).into()
			}

			fn definitions(out: &mut Vec<(&'static str, String)>) {
				if out.iter().any(|(name, _)| *name == stringify!($name)) {
					return;
				}
				let body = $crate::host::wire::fields(&[
					$((stringify!($f), &[$($fdoc),*], <$t as $crate::host::wire::Wire>::OPTIONAL, <$t as $crate::host::wire::Wire>::ts()),)*
				], false);
				out.push((stringify!($name), format!("{}\0{body}", <[&str]>::join(&[$($doc),*], "\n *"))));
				$(<$t as $crate::host::wire::Wire>::definitions(out);)*
			}
		}
	};

	($(#[doc = $doc:literal])* $vis:vis enum $name:ident { $($body:tt)* }) => {
		wire!(@variants $name [$(#[doc = $doc])* $vis derive(Clone, Debug)] (json path out) [] [] [] [] $($body)*);
	};
	// an enum of names only is also Copy and compared
	($(#[doc = $doc:literal])* $vis:vis copy enum $name:ident { $($body:tt)* }) => {
		wire!(@variants $name [$(#[doc = $doc])* $vis derive(Clone, Copy, Debug, PartialEq, Eq)] (json path out) [] [] [] [] $($body)*);
	};

	// a unit variant: its name
	(@variants $name:ident $head:tt ($json:ident $path:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $v:ident $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($json $path $out)
			[$($def)* $(#[doc = $vdoc])* $v,]
			[$($read)* $crate::host::wire::Json::String(s) if $crate::host::wire::is_variant(s, stringify!($v)) => Ok($name::$v),]
			[$($ts)* (&[$($vdoc),*], format!("'{}'", $crate::host::wire::variant(stringify!($v)))),]
			[$($deps)*]
			$($($rest)*)?);
	};

	// a variant holding one value: `{ name: value }`
	(@variants $name:ident $head:tt ($json:ident $path:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $v:ident ( $t:ty ) $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($json $path $out)
			[$($def)* $(#[doc = $vdoc])* $v($t),]
			[$($read)* $crate::host::wire::Json::Object(fields) if fields.len() == 1 && $crate::host::wire::is_variant(&fields[0].0, stringify!($v)) => {
				Ok($name::$v(<$t as $crate::host::wire::Wire>::read(&fields[0].1, &format!("{}.{}", $path, $crate::host::wire::variant(stringify!($v))))?))
			}]
			[$($ts)* (&[$($vdoc),*], format!("{{ {}: {} }}", $crate::host::wire::variant(stringify!($v)), <$t as $crate::host::wire::Wire>::ts())),]
			[$($deps)* <$t as $crate::host::wire::Wire>::definitions($out);]
			$($($rest)*)?);
	};

	// a variant with fields: `{ name: { fields } }`
	(@variants $name:ident $head:tt ($json:ident $path:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($ts:tt)*] [$($deps:tt)*]
		$(#[doc = $vdoc:literal])* $v:ident { $($(#[doc = $fdoc:literal])* $f:ident : $t:ty),* $(,)? }
		$(derived { $($(#[doc = $ddoc:literal])* $d:ident : $dt:ty),* $(,)? })? $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $head ($json $path $out)
			[$($def)* $(#[doc = $vdoc])* $v { $($(#[doc = $fdoc])* $f: $t,)* $($($(#[doc = $ddoc])* $d: $dt,)*)? },]
			[$($read)* $crate::host::wire::Json::Object(outer) if outer.len() == 1 && $crate::host::wire::is_variant(&outer[0].0, stringify!($v)) => {
				let at = format!("{}.{}", $path, $crate::host::wire::variant(stringify!($v)));
				let fields = outer[0].1.object(&at, &[$(stringify!($f)),*])?;
				let field = |name: &str| fields.iter().find(|(key, _)| *key == name).map(|(_, value)| *value);
				Ok($name::$v {
					$($f: <$t as $crate::host::wire::Wire>::field(field(stringify!($f)), &format!("{at}.{}", $crate::host::wire::key(stringify!($f))))?,)*
					$($($d: Default::default(),)*)?
				})
			}]
			[$($ts)* (&[$($vdoc),*], format!("{{ {}: {} }}", $crate::host::wire::variant(stringify!($v)), $crate::host::wire::fields(&[
				$((stringify!($f), &[$($fdoc),*], <$t as $crate::host::wire::Wire>::OPTIONAL, <$t as $crate::host::wire::Wire>::ts()),)*
			], true))),]
			[$($deps)* $(<$t as $crate::host::wire::Wire>::definitions($out);)*]
			$($($rest)*)?);
	};

	(@variants $name:ident [$(#[doc = $doc:literal])* $vis:vis derive($($derive:ident),*)] ($json:ident $path:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($ts:tt)*] [$($deps:tt)*]) => {
		$(#[doc = $doc])*
		#[derive($($derive),*)]
		$vis enum $name { $($def)* }

		impl $crate::host::wire::Wire for $name {
			fn read($json: &$crate::host::wire::Json, $path: &str) -> Result<Self, String> {
				match $json {
					$($read)*
					_ => Err(format!("{}: not a {}", $path, stringify!($name))),
				}
			}

			fn ts() -> String {
				stringify!($name).into()
			}

			fn definitions($out: &mut Vec<(&'static str, String)>) {
				if $out.iter().any(|(name, _)| *name == stringify!($name)) {
					return;
				}
				let body = $crate::host::wire::union(&[$($ts)*]);
				$out.push((stringify!($name), format!("{}\0{body}", <[&str]>::join(&[$($doc),*], "\n *"))));
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
	fn reads_json() {
		let json = Json::parse(r#" {"a": [false, true, null, "xé😀\n"], "b": {}} "#).unwrap();
		let Json::Object(fields) = &json else { panic!() };
		assert_eq!(fields[0].0, "a");
		let Json::Array(items) = &fields[0].1 else { panic!() };
		assert!(matches!(items[3], Json::String(ref s) if s == "xé😀\n"));
		assert!(Json::parse("[true,]").is_err() && Json::parse("{} x").is_err() && Json::parse("1").is_err());
	}

	#[test]
	fn spells_keys() {
		assert_eq!(key("attribute_expressions"), "attributeExpressions");
		assert!(
			is_key("attributeExpressions", "attribute_expressions") && !is_key("attribute", "attribute_expressions")
		);
		assert_eq!(variant("TypeParameters"), "typeParameters");
		assert!(is_variant("typeParameters", "TypeParameters") && !is_variant("typeParameter", "TypeParameters"));
	}
}
