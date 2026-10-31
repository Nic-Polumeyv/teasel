//! A host grammar: what a template language puts around the JavaScript it embeds, read once
//! from the definition a host hands over. Every form is a sequence of the host's own words and
//! punctuators around JavaScript entries; what may follow an entry is what ends it.
//!
//! A definition crosses as the JSON `JSON.stringify` writes of it: no whitespace, no numbers,
//! keys in camel case, a field that is absent left out. `wire!` defines a type once and derives
//! its reader, which takes the type's value off the text where it stands, and its writer. A
//! struct's keys come in any order, each once; an enum's variant is told by the shape of its
//! value. The text is kept for the process, and every name is a slice of it unless it was
//! escaped; every list is a slice of the arena the definition is read into.

use crate::ast::HostValue;

/// Reads a definition's JSON in one pass, each value straight into its type.
pub(crate) struct Reader<'a> {
	/// Every name without an escape is a slice of it.
	text: &'static str,
	at: usize,
	depth: u32,
	arena: &'a Arena,
}

impl<'a> Reader<'a> {
	pub(crate) fn new(text: &'static str, arena: &'a Arena) -> Reader<'a> {
		Reader {
			text,
			at: 0,
			depth: 0,
			arena,
		}
	}

	/// Guards the descent into objects and lists, as the parser guards its own.
	fn descend(&mut self) -> Result<(), String> {
		self.depth += 1;
		if self.depth > crate::parser::MAX_DEPTH {
			return Err(format!(
				"the grammar nests deeper than {} at byte {}",
				crate::parser::MAX_DEPTH,
				self.at
			));
		}
		Ok(())
	}

	pub(crate) fn peek(&self) -> Option<u8> {
		self.text.as_bytes().get(self.at).copied()
	}

	/// What the reader wanted where it stands, as the error says it.
	#[cold]
	pub(crate) fn expected(&self, what: &str) -> String {
		match self.peek() {
			None => format!("the grammar ends early: {what} expected at byte {}", self.at),
			Some(b' ' | b'\t' | b'\n' | b'\r') => format!(
				"{what} expected at byte {} of the grammar, which has no whitespace: it is written as JSON.stringify writes it",
				self.at
			),
			Some(_) => format!("{what} expected at byte {} of the grammar", self.at),
		}
	}

	/// Steps over `byte` when the text goes on with it.
	pub(crate) fn eat(&mut self, byte: u8) -> bool {
		let found = self.peek() == Some(byte);
		self.at += usize::from(found);
		found
	}

	pub(crate) fn expect(&mut self, byte: u8) -> Result<(), String> {
		if self.eat(byte) {
			Ok(())
		} else {
			Err(self.expected_byte(byte))
		}
	}

	#[cold]
	fn expected_byte(&self, byte: u8) -> String {
		self.expected(&format!("`{}`", byte as char))
	}

	/// Steps over `text` when the text goes on with it: a literal, or a name as `spelled!` spells it.
	pub(crate) fn eat_text(&mut self, text: &str) -> bool {
		let found = self.text.as_bytes()[self.at..].starts_with(text.as_bytes());
		if found {
			self.at += text.len();
		}
		found
	}

	pub(crate) fn str(&mut self) -> Result<&'static str, String> {
		if !self.eat(b'"') {
			return Err(self.expected("a string"));
		}
		let start = self.at;
		let rest = &self.text.as_bytes()[start..];
		self.at += rest
			.iter()
			.position(|&b| matches!(b, b'"' | b'\\' | ..0x20))
			.unwrap_or(rest.len());
		match self.peek() {
			Some(b'"') => {
				self.at += 1;
				Ok(&self.text[start..self.at - 1])
			}
			Some(b'\\') => self.escaped(start),
			_ => Err(self.expected("`\"`")),
		}
	}

	/// The rest of a string that holds an escape, decoded into a copy of its own.
	fn escaped(&mut self, start: usize) -> Result<&'static str, String> {
		let mut out = String::new();
		let mut plain = start;
		loop {
			match self.peek() {
				Some(b'"') => {
					out.push_str(&self.text[plain..self.at]);
					self.at += 1;
					return Ok(keep(&out));
				}
				Some(b'\\') => {
					out.push_str(&self.text[plain..self.at]);
					self.at += 1;
					out.push(self.escape()?);
					plain = self.at;
				}
				Some(0x20..) => self.at += 1,
				_ => return Err(self.expected("`\"`")),
			}
		}
	}

	fn escape(&mut self) -> Result<char, String> {
		let c = match self.peek() {
			Some(b'"') => '"',
			Some(b'\\') => '\\',
			Some(b'/') => '/',
			Some(b'b') => '\u{8}',
			Some(b'f') => '\u{c}',
			Some(b'n') => '\n',
			Some(b'r') => '\r',
			Some(b't') => '\t',
			Some(b'u') => {
				let digits = self
					.text
					.get(self.at + 1..self.at + 5)
					.filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()));
				let Some(digits) = digits else {
					self.at += 1;
					return Err(self.expected("four hex digits"));
				};
				self.at += 5;
				let code = u32::from_str_radix(digits, 16).unwrap();
				// a pair, as a writer that escapes everything beyond ASCII spells one character
				if (0xD800..0xDC00).contains(&code)
					&& let Some(low) = self.text.get(self.at..self.at + 6).and_then(|t| t.strip_prefix("\\u"))
					&& let Ok(low) = u32::from_str_radix(low, 16)
					&& (0xDC00..0xE000).contains(&low)
				{
					self.at += 6;
					return Ok(char::from_u32(0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00)).unwrap());
				}
				// `JSON.stringify` escapes a surrogate only when it is lone, which UTF-8 cannot
				// hold: it becomes U+FFFD, as the encoder makes it in a source
				return Ok(char::from_u32(code).unwrap_or('\u{FFFD}'));
			}
			_ => return Err(self.expected("an escape")),
		};
		self.at += 1;
		Ok(c)
	}

	/// Reads an object, `entry` reading each key and its value in order.
	// `dyn` keeps one loop for every object: a copy for each struct, its keys inlined into it, made
	// the WebAssembly module's first read 1.3 ms longer to compile
	pub(crate) fn object(
		&mut self,
		entry: &mut dyn FnMut(&mut Reader<'a>) -> Result<(), String>,
	) -> Result<(), String> {
		self.expect(b'{')?;
		self.descend()?;
		if !self.eat(b'}') {
			loop {
				entry(self)?;
				if self.eat(b'}') {
					break;
				}
				if !self.eat(b',') {
					return Err(self.expected("`,` or `}`"));
				}
			}
		}
		self.depth -= 1;
		Ok(())
	}

	/// Reads a list, `item` reading each item in order.
	pub(crate) fn list(&mut self, mut item: impl FnMut(&mut Reader<'a>) -> Result<(), String>) -> Result<(), String> {
		self.expect(b'[')?;
		self.descend()?;
		if !self.eat(b']') {
			loop {
				item(self)?;
				if self.eat(b']') {
					break;
				}
				if !self.eat(b',') {
					return Err(self.expected("`,` or `]`"));
				}
			}
		}
		self.depth -= 1;
		Ok(())
	}

	/// Steps over a key the struct has no field for and its value: a definition may carry more
	/// than the engine reads, and the types cannot refuse it.
	pub(crate) fn skip_entry(&mut self) -> Result<(), String> {
		self.str()?;
		self.expect(b':')?;
		self.skip()
	}

	fn skip(&mut self) -> Result<(), String> {
		match self.peek() {
			Some(b'"') => self.str().map(|_| ()),
			Some(b'{') => self.object(&mut Reader::skip_entry),
			Some(b'[') => self.list(Reader::skip),
			Some(b'-' | b'0'..=b'9') => {
				while self
					.peek()
					.is_some_and(|b| matches!(b, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
				{
					self.at += 1;
				}
				Ok(())
			}
			_ if self.eat_text("true") || self.eat_text("false") || self.eat_text("null") => Ok(()),
			_ => Err(self.expected("a value")),
		}
	}

	/// Reads the value of a struct's key into its slot, which a key read before has not filled.
	pub(crate) fn once<T: Wire<'a>>(&mut self, slot: &mut Option<T>, key: &str, ty: &str) -> Result<(), String> {
		if slot.is_some() {
			return Err(self.twice(key, ty));
		}
		*slot = Some(T::read(self)?);
		Ok(())
	}

	#[cold]
	fn twice(&self, key: &str, ty: &str) -> String {
		format!("a second {key} on a {ty} at byte {} of the grammar", self.at)
	}

	/// The error for a struct whose keys, each with whether it was read, leave out one it needs.
	#[cold]
	pub(crate) fn missing(&self, ty: &str, keys: &[(&str, bool)]) -> String {
		let key = keys.iter().find(|(_, read)| !read).map_or("", |(key, _)| key);
		format!("a {ty} needs {key}, before byte {} of the grammar", self.at)
	}

	pub(crate) fn end(&mut self) -> Result<(), String> {
		// a file ends in a newline
		while self.peek().is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r')) {
			self.at += 1;
		}
		if self.at == self.text.len() {
			Ok(())
		} else {
			Err(format!("text after the grammar at byte {}", self.at))
		}
	}
}

/// A grammar's names reach the walker as `&'static str`, so they live for the process.
pub(crate) fn keep(s: &str) -> &'static str {
	Box::leak(s.to_owned().into_boxed_str())
}

/// Holds a definition's lists until it is lowered: each is bumped onto the chunk being filled,
/// and all go with the arena. A list of its own from the allocator, freed on its own, made
/// reading the Svelte grammar 60% slower.
pub struct Arena {
	chunks: std::cell::RefCell<Vec<Vec<std::mem::MaybeUninit<u64>>>>,
	/// The free words of the last chunk.
	free: std::cell::Cell<(*mut std::mem::MaybeUninit<u64>, usize)>,
}

impl Arena {
	/// Room for `words` words.
	fn words(&self, words: usize) -> *mut std::mem::MaybeUninit<u64> {
		let (at, left) = self.free.get();
		if words <= left {
			self.free.set((at.wrapping_add(words), left - words));
			return at;
		}
		let mut chunks = self.chunks.borrow_mut();
		let size = words.max(chunks.last().map_or(1024, |chunk| chunk.capacity() * 2));
		let mut chunk = Vec::with_capacity(size);
		let at = chunk.as_mut_ptr();
		chunks.push(chunk);
		self.free.set((at.wrapping_add(words), size - words));
		at
	}

	/// Whether the list at `at`, `words` words long, takes `more` where it is: it ends what the
	/// chunk has filled, and the chunk has the room.
	fn grow(&self, at: *mut std::mem::MaybeUninit<u64>, words: usize, more: usize) -> bool {
		let (free, left) = self.free.get();
		let grows = at.wrapping_add(words) == free && more <= left;
		if grows {
			self.free.set((free.wrapping_add(more), left - more));
		}
		grows
	}
}

impl Default for Arena {
	fn default() -> Arena {
		Arena {
			chunks: Default::default(),
			free: std::cell::Cell::new((std::ptr::null_mut(), 0)),
		}
	}
}

/// A list being read into an arena.
struct Items<'a, T> {
	arena: &'a Arena,
	at: *mut T,
	len: usize,
	room: usize,
}

impl<'a, T: Copy> Items<'a, T> {
	const fn words(items: usize) -> usize {
		const { assert!(size_of::<T>() > 0 && align_of::<T>() <= align_of::<u64>()) };
		(items * size_of::<T>()).div_ceil(size_of::<u64>())
	}

	fn new(arena: &'a Arena) -> Items<'a, T> {
		Items {
			arena,
			at: std::ptr::NonNull::dangling().as_ptr(),
			len: 0,
			room: 0,
		}
	}

	fn push(&mut self, item: T) {
		if self.len == self.room {
			let room = (self.room * 2).max(4);
			let (old, new) = (Self::words(self.room), Self::words(room));
			if !(self.room > 0 && self.arena.grow(self.at.cast(), old, new - old)) {
				let at = self.arena.words(new).cast::<T>();
				// SAFETY: the new room is the list's own, apart from the old, which holds `len` items
				unsafe { std::ptr::copy_nonoverlapping(self.at, at, self.len) };
				self.at = at;
			}
			self.room = room;
		}
		// SAFETY: the room for `room` items at `at` is the list's
		unsafe { self.at.add(self.len).write(item) };
		self.len += 1;
	}

	fn list(self) -> &'a [T] {
		// SAFETY: `len` items were written at `at`, which the arena holds as long as it lives
		unsafe { std::slice::from_raw_parts(self.at, self.len) }
	}
}

/// The length of what `camel` spells.
pub(crate) const fn camel_len(open: &str, name: &str, close: &str) -> usize {
	let name = name.as_bytes();
	let mut i = if name.len() > 2 && name[0] == b'r' && name[1] == b'#' {
		2
	} else {
		0
	};
	let mut len = open.len() + close.len();
	while i < name.len() {
		if name[i] != b'_' {
			len += 1;
		}
		i += 1;
	}
	len
}

/// A Rust name in camel case, as the definition spells it, between `open` and `close`: `or_arg`
/// is `orArg`, `OneOf` is `oneOf`, `r#in` is `in`.
pub(crate) const fn camel<const N: usize>(open: &str, name: &str, close: &str) -> [u8; N] {
	let (open, name, close) = (open.as_bytes(), name.as_bytes(), close.as_bytes());
	let mut out = [0; N];
	let mut len = 0;
	while len < open.len() {
		out[len] = open[len];
		len += 1;
	}
	let mut i = if name.len() > 2 && name[0] == b'r' && name[1] == b'#' {
		2
	} else {
		0
	};
	let mut up = false;
	while i < name.len() {
		if name[i] == b'_' {
			up = true;
		} else {
			out[len] = if up {
				name[i].to_ascii_uppercase()
			} else if len == open.len() {
				name[i].to_ascii_lowercase()
			} else {
				name[i]
			};
			len += 1;
			up = false;
		}
		i += 1;
	}
	i = 0;
	while i < close.len() {
		out[len + i] = close[i];
		i += 1;
	}
	out
}

/// A Rust name as the definition spells it, worked out when compiling; with `open` and `close`,
/// the text around it too: `"inside"`, `{"opt":`.
macro_rules! spelled {
	($name:expr) => {
		spelled!("", $name, "")
	};
	($open:literal, $name:expr, $close:literal) => {{
		const SPELLED: [u8; $crate::host::grammar::camel_len($open, $name, $close)] =
			$crate::host::grammar::camel($open, $name, $close);
		const TEXT: &str = match std::str::from_utf8(&SPELLED) {
			Ok(text) => text,
			Err(_) => panic!(),
		};
		TEXT
	}};
}

/// A type of the definition: how it reads off the text, its lists into the arena of `'a`, and
/// writes back.
pub(crate) trait Wire<'a>: Sized {
	fn read(r: &mut Reader<'a>) -> Result<Self, String>;

	/// What a field holds when its key is left out: none of most types, `None` of an option.
	fn absent() -> Option<Self> {
		None
	}

	fn write(&self, out: &mut String);

	/// The field holding the value leaves its key out.
	fn is_absent(&self) -> bool {
		false
	}
}

impl<'a> Wire<'a> for &'static str {
	fn read(r: &mut Reader<'a>) -> Result<Self, String> {
		r.str()
	}

	fn write(&self, out: &mut String) {
		crate::estree::write_json_string(out, self);
	}
}

/// Writes `"key":value` into the object being written, unless the value is absent.
pub(crate) fn entry<'a>(out: &mut String, key: &str, value: &impl Wire<'a>) {
	if value.is_absent() {
		return;
	}
	// an object's first key follows its brace, and no value ends in one
	if !out.ends_with('{') {
		out.push(',');
	}
	crate::estree::write_json_string(out, key);
	out.push(':');
	value.write(out);
}

impl<'a> Wire<'a> for bool {
	fn read(r: &mut Reader<'a>) -> Result<Self, String> {
		if r.eat_text("true") {
			Ok(true)
		} else if r.eat_text("false") {
			Ok(false)
		} else {
			Err(r.expected("a boolean"))
		}
	}

	fn write(&self, out: &mut String) {
		out.push_str(if *self { "true" } else { "false" });
	}
}

impl<'a, T: Wire<'a>> Wire<'a> for Option<T> {
	fn read(r: &mut Reader<'a>) -> Result<Self, String> {
		T::read(r).map(Some)
	}

	fn absent() -> Option<Self> {
		Some(None)
	}

	fn write(&self, out: &mut String) {
		if let Some(value) = self {
			value.write(out);
		}
	}

	fn is_absent(&self) -> bool {
		self.is_none()
	}
}

impl<'a, T: Wire<'a> + Copy> Wire<'a> for &'a [T] {
	fn read(r: &mut Reader<'a>) -> Result<Self, String> {
		let mut items = Items::new(r.arena);
		r.list(|r| {
			items.push(T::read(r)?);
			Ok(())
		})?;
		Ok(items.list())
	}

	fn write(&self, out: &mut String) {
		out.push('[');
		for (i, item) in self.iter().enumerate() {
			if i > 0 {
				out.push(',');
			}
			item.write(out);
		}
		out.push(']');
	}
}

/// A list of two; the second may be left out when it is absent, `[attribute]`.
impl<'a, A: Wire<'a>, B: Wire<'a>> Wire<'a> for (A, B) {
	fn read(r: &mut Reader<'a>) -> Result<Self, String> {
		r.expect(b'[')?;
		let a = A::read(r)?;
		if let Some(b) = B::absent()
			&& r.eat(b']')
		{
			return Ok((a, b));
		}
		r.expect(b',')?;
		let b = B::read(r)?;
		r.expect(b']')?;
		Ok((a, b))
	}

	fn write(&self, out: &mut String) {
		out.push('[');
		self.0.write(out);
		if !self.1.is_absent() {
			out.push(',');
			self.1.write(out);
		}
		out.push(']');
	}
}

/// An object of the definition: its keys in order, each holding a `T`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Record<'a, T>(pub &'a [(&'static str, T)]);

impl<'a, T: Wire<'a> + Copy> Wire<'a> for Record<'a, T> {
	fn read(r: &mut Reader<'a>) -> Result<Self, String> {
		let mut entries = Items::new(r.arena);
		r.object(&mut |r| {
			let key = r.str()?;
			r.expect(b':')?;
			entries.push((key, T::read(r)?));
			Ok(())
		})?;
		Ok(Record(entries.list()))
	}

	fn write(&self, out: &mut String) {
		out.push('{');
		for (key, value) in self.0 {
			entry(out, key, value);
		}
		out.push('}');
	}
}

/// A type of the definition, defined once: its Rust definition, its reader and its writer. Docs
/// come first on the type and on every field. A struct is an object of its fields, in any order;
/// a field that may be absent is an `Option`. A `copy enum` holds names only. An enum's variants
/// are tried in the order written, each told by the shape of its value: a name is its string,
/// `"inside"`, or the JSON it is given, `No = false`; a variant holding a value is an object of
/// one key, its name, `{"opt":[…]}`, or the bare value when it is given the shape, `string` or
/// `object`.
macro_rules! wire {
	(
		$(#[doc = $doc:literal])* $vis:vis struct $name:ident $(<$a:lifetime>)? {
			$($(#[doc = $fdoc:literal])* $fvis:vis $f:ident : $t:ty),* $(,)?
		}
	) => {
		$(#[doc = $doc])*
		#[derive(Clone, Copy, Debug)]
		$vis struct $name $(<$a>)? {
			$($(#[doc = $fdoc])* $fvis $f: $t,)*
		}

		impl<'a> $crate::host::grammar::Wire<'a> for $name $(<$a>)? {
			fn read(r: &mut $crate::host::grammar::Reader<'a>) -> Result<Self, String> {
				$(let mut $f = None::<$t>;)*
				// each key is matched where it stands: reading it as a string first, then
				// matching that, read the Svelte grammar 25% slower
				r.object(&mut |r| {
					$(if r.eat_text(spelled!("\"", stringify!($f), "\":")) {
						return r.once(&mut $f, spelled!(stringify!($f)), stringify!($name));
					})*
					r.skip_entry()
				})?;
				// one match over every slot, where a `?` per field would drop the others at each return
				$(let $f = $f.or_else(<$t as $crate::host::grammar::Wire>::absent);)*
				match ($($f,)*) {
					($(Some($f),)*) => Ok($name { $($f,)* }),
					($($f,)*) => Err(r.missing(stringify!($name), &[$((spelled!(stringify!($f)), $f.is_some()),)*])),
				}
			}

			fn write(&self, out: &mut String) {
				out.push('{');
				$($crate::host::grammar::entry(out, spelled!(stringify!($f)), &self.$f);)*
				out.push('}');
			}
		}
	};

	($(#[doc = $doc:literal])* $vis:vis enum $name:ident $(<$a:lifetime>)? { $($body:tt)* }) => {
		wire!(@variants $name [$(<$a>)?] [$(#[doc = $doc])* $vis derive(Clone, Copy, Debug)] (r out) [] [] [] $($body)*);
	};
	($(#[doc = $doc:literal])* $vis:vis copy enum $name:ident { $($body:tt)* }) => {
		wire!(@variants $name [] [$(#[doc = $doc])* $vis derive(Clone, Copy, Debug, PartialEq, Eq)] (r out) [] [] [] $($body)*);
	};

	// a name: its string, or the JSON given
	(@variants $name:ident $gen:tt $head:tt ($r:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($write:tt)*]
		$(#[doc = $vdoc:literal])* $variant:ident $(= $json:tt)? $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $gen $head ($r $out)
			[$($def)* $(#[doc = $vdoc])* $variant,]
			[$($read)* if $r.eat_text(wire!(@json $variant $($json)?)) { return Ok($name::$variant); }]
			[$($write)* $name::$variant => $out.push_str(wire!(@json $variant $($json)?)),]
			$($($rest)*)?);
	};

	// a variant holding one value: an object its name keys, or the bare value of the shape given
	(@variants $name:ident $gen:tt $head:tt ($r:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($write:tt)*]
		$(#[doc = $vdoc:literal])* $variant:ident ( $t:ty ) $(= $shape:ident)? $(, $($rest:tt)*)?
	) => {
		wire!(@variants $name $gen $head ($r $out)
			[$($def)* $(#[doc = $vdoc])* $variant($t),]
			[$($read)* wire!(@read $r $name $variant $t $(, $shape)?);]
			[$($write)* $name::$variant(value) => wire!(@write $out $variant value $(, $shape)?),]
			$($($rest)*)?);
	};

	(@variants $name:ident [$($gen:tt)*] [$(#[doc = $doc:literal])* $vis:vis derive($($derive:ident),*)] ($r:ident $out:ident) [$($def:tt)*] [$($read:tt)*] [$($write:tt)*]) => {
		$(#[doc = $doc])*
		#[derive($($derive),*)]
		$vis enum $name $($gen)* { $($def)* }

		impl<'a> $crate::host::grammar::Wire<'a> for $name $($gen)* {
			fn read($r: &mut $crate::host::grammar::Reader<'a>) -> Result<Self, String> {
				$($read)*
				Err($r.expected(concat!("a ", stringify!($name))))
			}

			fn write(&self, $out: &mut String) {
				match self { $($write)* }
			}
		}
	};

	(@json $variant:ident) => { spelled!("\"", stringify!($variant), "\"") };
	(@json $variant:ident $json:tt) => { stringify!($json) };

	(@read $r:ident $name:ident $variant:ident $t:ty) => {
		if $r.eat_text(spelled!("{\"", stringify!($variant), "\":")) {
			let value = <$t as $crate::host::grammar::Wire>::read($r)?;
			$r.expect(b'}')?;
			return Ok($name::$variant(value));
		}
	};
	(@read $r:ident $name:ident $variant:ident $t:ty, string) => {
		if $r.peek() == Some(b'"') {
			return <$t as $crate::host::grammar::Wire>::read($r).map($name::$variant);
		}
	};
	(@read $r:ident $name:ident $variant:ident $t:ty, object) => {
		if $r.peek() == Some(b'{') {
			return <$t as $crate::host::grammar::Wire>::read($r).map($name::$variant);
		}
	};
	(@write $out:ident $variant:ident $value:ident) => {{
		$out.push_str(spelled!("{\"", stringify!($variant), "\":"));
		$crate::host::grammar::Wire::write($value, $out);
		$out.push('}');
	}};
	(@write $out:ident $variant:ident $value:ident, $shape:ident) => {
		$crate::host::grammar::Wire::write($value, $out)
	};
}

/// A name a node type or a field has, by its number in `Grammar::names`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name(pub u32);

impl Name {
	/// No name: a node of the host's without a type, a block whose body is not filed.
	pub const NONE: Name = Name(u32::MAX);
}

/// The names the walker writes itself, spelled as they are written: the same numbers in every grammar.
#[allow(non_upper_case_globals)]
pub mod own {
	macro_rules! own {
		($($name:ident)*) => {
			#[allow(non_camel_case_types)]
			#[repr(u32)]
			enum Number {
				$($name),*
			}
			$(pub const $name: super::Name = super::Name(Number::$name as u32);)*
			pub const NAMES: &[&str] = &[$(stringify!($name)),*];
		};
	}
	own! {
		Fragment nodes Text raw data Comment Attribute name value attributes fragment expression modifiers
		Script context content StyleSheet children comments styles comment CSSComment position Atrule
		prelude block Rule SelectorList ComplexSelector RelativeSelector combinator selectors Combinator
		TypeSelector namespace IdSelector ClassSelector NestingSelector PseudoClassSelector
		PseudoElementSelector args AttributeSelector matcher flags Nth Percentage Block Declaration property
	}
}

/// A grammar's names as its definition is lowered: each numbered where it is first read.
struct Naming {
	names: Vec<&'static str>,
	numbers: crate::interner::FastMap<&'static str, u32>,
}

impl Naming {
	fn new() -> Naming {
		let names = own::NAMES.to_vec();
		let numbers = names.iter().enumerate().map(|(i, &name)| (name, i as u32)).collect();
		Naming { names, numbers }
	}

	fn name(&mut self, text: &'static str) -> Name {
		let next = self.names.len() as u32;
		let number = *self.numbers.entry(text).or_insert(next);
		if number == next {
			self.names.push(text);
		}
		Name(number)
	}

	fn text(&self, name: Name) -> &'static str {
		self.names[name.0 as usize]
	}
}

/// The shapes of a grammar's host nodes, numbered for good as the walker meets them: a node's
/// type, whether it has a span, and each field's name and kind of value. A front end builds the
/// nodes of one shape by one literal.
#[derive(Clone, Debug, Default)]
pub struct Shapes {
	/// Each type's shapes, by the type's number after the one of nodes without a type.
	// tried a table by hash with each type's last shape held in place: 0.3 ms slower a pass over Svelte's tests
	of: Vec<Vec<Shape>>,
	count: u32,
}

#[derive(Clone, Debug)]
struct Shape {
	id: u32,
	span: bool,
	/// Each field's name, then its kind of value in the low byte.
	fields: Vec<u64>,
}

impl Shapes {
	/// The number of the shape of a node of type `ty`, with a span or not, whose fields are named
	/// `keys` and hold `values`.
	pub(crate) fn of(&mut self, ty: Option<u32>, span: bool, keys: &[u32], values: &[HostValue]) -> u32 {
		let at = ty.map_or(0, |ty| ty as usize + 1);
		if self.of.len() <= at {
			self.of.resize_with(at + 1, Vec::new);
		}
		let fields = keys
			.iter()
			.zip(values)
			.map(|(&key, value)| (key as u64) << 8 | value.tag as u64);
		let shapes = &mut self.of[at];
		if let Some(shape) = shapes
			.iter()
			.find(|shape| shape.span == span && shape.fields.iter().copied().eq(fields.clone()))
		{
			return shape.id;
		}
		let id = self.count;
		self.count += 1;
		shapes.push(Shape {
			id,
			span,
			fields: fields.collect(),
		});
		id
	}
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
	/// The text up to the tag's end word, unread, for a host that reads its expressions later.
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
		field: Name,
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

fn collect_entries(items: &[Item], out: &mut Vec<(Name, bool)>) {
	for item in items {
		match item {
			Item::Entry { field, omit, .. } => out.push((*field, *omit)),
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

fn collect_bodies(form: &Form, out: &mut Vec<(Name, bool)>) {
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
	pub field: Name,
	/// The field is left out of blocks that never opened this body; otherwise it is null there.
	pub omit: bool,
	/// A branch that nests a new block of the same kind into the field, `{:else if}`: the field
	/// of the nested block that its own body fills, and the flag set on it.
	pub chain: Option<(Name, Name)>,
	pub declares: Vec<Declare>,
}

#[derive(Clone, Debug)]
pub struct Declare {
	pub field: Name,
	/// Declared in the scope around the block rather than inside it: a snippet's name.
	pub outside: bool,
}

#[derive(Clone, Debug)]
pub struct Form {
	pub items: Vec<Item>,
	pub body: Option<Body>,

	/// Every entry the form can read, and whether its field is left out when it was not.
	pub entries: Vec<(Name, bool)>,
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
	pub ty: Name,
	/// The field that takes the expression of a `this` attribute, which leaves the attributes,
	/// and whether text is accepted there as a string.
	pub this: Option<(Name, bool)>,
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
	Expression { field: Name, optional: bool, name: bool },
	/// The one pattern of the attribute value, `let:item={{ id }}`.
	Pattern { field: Name, optional: bool, name: bool },
	/// The attribute value as it is, text and expressions.
	Value(Name),
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
	pub ty: Name,
	pub kind: Option<Name>,
	pub raw: Option<Name>,
	pub value: DirectiveValue,
	pub flags: Vec<(Name, bool)>,
	pub unique: Unique,
	/// What the directive declares in the scope of its element: the fields of its form, or,
	/// with none named, its value.
	pub declares: Option<Vec<Name>>,
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

/// Which piece of a construct a marker is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
	Open,
	Branch(usize),
	Close,
}

/// A tag, or with a close a block.
#[derive(Clone, Debug)]
pub struct Construct {
	pub ty: Name,
	pub places: Vec<Place>,
	pub open: Piece,
	pub branches: Vec<Piece>,
	pub close: Option<Piece>,
	/// The boolean fields of the branches that reopen the block, each true when that branch did.
	pub chain_flags: Vec<Name>,

	/// Every entry the construct's forms can read, and every body they can open.
	pub entries: Vec<(Name, bool)>,
	pub bodies: Vec<(Name, bool)>,
}

impl Construct {
	pub fn piece(&self, which: Which) -> &Piece {
		match which {
			Which::Open => &self.open,
			Which::Branch(i) => &self.branches[i],
			Which::Close => self.close.as_ref().unwrap(),
		}
	}

	/// Its pieces, open first.
	pub fn pieces(&self) -> impl Iterator<Item = (Which, &Piece)> {
		std::iter::once((Which::Open, &self.open))
			.chain(
				self.branches
					.iter()
					.enumerate()
					.map(|(i, piece)| (Which::Branch(i), piece)),
			)
			.chain(self.close.iter().map(|piece| (Which::Close, piece)))
	}

	pub fn is_block(&self) -> bool {
		self.close.is_some()
	}

	pub fn stands(&self, place: Place) -> bool {
		self.places.contains(&place)
	}

	/// The one field of a tag that reads one thing, its entry.
	pub fn single(&self) -> Option<(Name, Entry)> {
		match self.open.form.items.as_slice() {
			[Item::Entry { field, entry, .. }, Item::Literal(_)] if self.close.is_none() => Some((*field, *entry)),
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
		field: Name,
		holds: RootField,
		omit: bool,
	},
	Scope(Vec<DocField>),
}

#[derive(Clone, Debug)]
pub struct DocumentRule {
	pub ty: Name,
	pub fields: Vec<DocField>,
}

/// A host language: its document, its content, and the JavaScript inside them.
#[derive(Clone, Debug)]
pub struct Grammar {
	pub name: &'static str,
	/// Every name of a type or field its documents' nodes have, by number, the walker's own first.
	pub names: Vec<&'static str>,
	pub shapes: std::cell::RefCell<Shapes>,
	pub document: DocumentRule,
	/// `{name}` among the attributes is `name={name}`: the marker and the closing word around it.
	pub shorthand: Option<(&'static str, &'static str)>,
	/// An element the browser would close when another opens is closed there.
	pub autoclose: bool,
	/// Whitespace at the end of the source is not part of the document.
	pub trim: bool,
	pub void: Vec<&'static str>,
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
	/// Every marker's first part, and the pieces it starts: what the walker matches once for them.
	pub firsts: Vec<First>,
}

/// The pieces whose marker starts with one part: those it is the whole marker of, and the others by
/// the first byte of their second part, each list in written order.
#[derive(Clone, Debug)]
pub struct First {
	pub part: &'static str,
	/// The part starts with a letter, so it starts a word.
	pub word: bool,
	pub whole: Vec<(usize, Which)>,
	pub next: Vec<(u8, Vec<(usize, Which)>)>,
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

/// A grammar as the builders make it, which is what crosses, lowered here into what the walker
/// reads.
pub mod definition {
	use super::{Arena, Reader, Record, Wire};

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
			List = [],
		}
	}

	wire! {
		/// One step of a form: a host word, fields, or a group.
		pub enum Item<'a> {
			Word(&'static str) = string,
			Opt(&'a [Item<'a>]),
			OneOf(&'a [&'a [Item<'a>]]),
			Scope(&'a [Item<'a>]),
			Fields(Record<'a, Source>) = object,
		}
	}

	wire! {
		/// A node type and the form its fields come from.
		pub struct Node<'a> {
			pub node: &'static str,
			pub form: Option<&'a [Item<'a>]>,
		}
	}

	wire! {
		pub copy enum Uniqueness {
			Kind,
			Attributes,
		}
	}

	wire! {
		pub struct Directive<'a> {
			pub node: &'static str,
			pub form: Option<&'a [Item<'a>]>,
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
		pub struct Element<'a> {
			pub node: &'static str,
			pub form: Option<&'a [Item<'a>]>,
			pub root: Option<bool>,
			pub once: Option<bool>,
			pub inside: Option<&'static str>,
			pub outside: Option<&'static str>,
			pub content: Option<Content>,
		}
	}

	wire! {
		pub struct Elements<'a> {
			pub rules: Option<Record<'a, Element<'a>>>,
			pub component: Option<Element<'a>>,
			pub other: Option<Element<'a>>,
		}
	}

	wire! {
		pub struct Script<'a> {
			pub element: &'static str,
			pub module: Option<&'a [(&'static str, Option<&'static str>)]>,
			pub typescript: Option<&'a [(&'static str, Option<&'static str>)]>,
		}
	}

	wire! {
		pub copy enum Raw {
			Raw,
		}
	}

	wire! {
		pub struct Directives<'a> {
			pub prefix: Option<&'static str>,
			pub arg: Option<&'static str>,
			pub modifier: Option<&'static str>,
			pub dynamic: Option<(&'static str, &'static str)>,
			pub unique: Option<Raw>,
			pub shorthands: Option<Record<'a, &'a [&'static str]>>,
			pub rules: Option<Record<'a, Directive<'a>>>,
			pub other: Option<Directive<'a>>,
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
		pub struct Piece<'a> {
			pub marker: &'a [&'static str],
			/// Whitespace must follow the marker.
			pub space: Option<bool>,
			pub form: &'a [Item<'a>],
		}
	}

	wire! {
		/// A branch of a block; `reopen` nests the block again into that field, with that flag true on it.
		pub struct Branch<'a> {
			pub marker: &'a [&'static str],
			pub space: Option<bool>,
			pub form: &'a [Item<'a>],
			pub reopen: Option<(&'static str, &'static str)>,
		}
	}

	wire! {
		/// A tag, or with `close` a block; the type of its node is the key it is written under.
		pub struct Construct<'a> {
			pub r#in: Option<&'a [Place]>,
			pub open: Piece<'a>,
			pub branches: Option<&'a [Branch<'a>]>,
			pub close: Option<Piece<'a>>,
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
		pub struct Definition<'a> {
			pub document: Node<'a>,
			pub attributes: Option<Attributes>,
			pub autoclose: Option<bool>,
			pub trim: Option<bool>,
			pub void: Option<&'a [&'static str]>,
			pub verbatim: Option<&'static str>,
			pub elements: Option<Elements<'a>>,
			pub script: Option<Script<'a>>,
			pub style: Option<&'static str>,
			pub directives: Option<Directives<'a>>,
			pub constructs: Option<Record<'a, Construct<'a>>>,
		}
	}

	wire! {
		/// What crosses: the host's name and its definition.
		pub struct Host<'a> {
			pub name: &'static str,
			pub definition: Definition<'a>,
		}
	}

	impl<'a> Host<'a> {
		/// Every name but an escaped one is a slice of `text`, which therefore lives for the
		/// process; every list is in `arena`.
		pub fn read(text: &'static str, arena: &'a Arena) -> Result<Host<'a>, String> {
			let mut reader = Reader::new(text, arena);
			let host = <Host as Wire>::read(&mut reader)?;
			reader.end()?;
			Ok(host)
		}

		/// The host as JSON, as the JavaScript side writes it.
		pub fn json(&self) -> String {
			let mut out = String::new();
			self.write(&mut out);
			out
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
fn sources<'a>(items: &[definition::Item<'a>]) -> Vec<(&'static str, &'a Source)> {
	items
		.iter()
		.filter_map(|item| match item {
			definition::Item::Fields(record) => Some(record.0.iter().map(|(field, source)| (*field, source))),
			_ => None,
		})
		.flatten()
		.collect()
}

fn reads(items: &[definition::Item], what: fn(&Source) -> bool) -> bool {
	items.iter().any(|item| match item {
		definition::Item::Fields(record) => record.0.iter().any(|(_, s)| what(s)),
		definition::Item::Opt(inner) | definition::Item::Scope(inner) => reads(inner, what),
		definition::Item::OneOf(list) => list.iter().any(|inner| reads(inner, what)),
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
				for inner in *list {
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

fn token(what: &str, s: &str) -> Result<(), String> {
	if s.is_empty() {
		return Err(format!("{what} is empty"));
	}
	Ok(())
}

fn teasels(owner: &str, what: impl std::fmt::Display) -> String {
	format!("{owner}: {what} is teasel's")
}

fn fixed(s: &Source) -> bool {
	matches!(s.from, "literal" | "directive")
}

fn form(
	ty: &str,
	items: &[definition::Item],
	site: At,
	bound: &mut Vec<Declare>,
	n: &mut Naming,
) -> Result<Form, String> {
	fields_in(ty, items)?;
	let (items, body) = sequence(ty, items, site, bound, n)?;
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
	n: &mut Naming,
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
					definition::Item::OneOf(list) => *list,
					definition::Item::Opt([]) => return Err(format!("{ty}: `opt` needs items")),
					definition::Item::Opt(inner) => match *inner {
						[definition::Item::OneOf(list)] => *list,
						_ => {
							one = [*inner];
							&one
						}
					},
					_ => unreachable!(),
				};
				let alternatives = alternatives(ty, list, site, bound, n)?;
				if list.iter().any(|items| reads(items, |s| s.from == "content")) {
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
			definition::Item::Scope(_) => return Err(format!("{ty}: a scope belongs to the document")),
			definition::Item::Fields(record) => {
				for (field, source) in record.0 {
					match source.from {
						"literal" | "directive" if site == At::Directive => continue,
						"content" => {
							if site != At::Block || at != items.len() - 1 {
								return Err(format!("{ty}: {field} is a body, which only ends a block's form"));
							}
							if let Some(first) = &body {
								return Err(format!("{ty}: {field} is a second body after {}", n.text(first.field)));
							}
							body = Some(Body {
								field: n.name(field),
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
					let (field, outside) = (n.name(field), source.bind == Bind::Outside);
					if source.bind != Bind::No && !bound.iter().any(|d| d.field == field && d.outside == outside) {
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
	list: &[&[definition::Item]],
	site: At,
	bound: &mut Vec<Declare>,
	n: &mut Naming,
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
			let bound = if reads(items, |s| s.from == "content") {
				&mut own
			} else {
				&mut *bound
			};
			let (items, body) = sequence(ty, items, site, bound, n)?;
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
		("content", _, _) => {
			return Err(format!("the document: its content is teasel's fragment, not {field}"));
		}
		(_, "style", _) => RootField::Style,
		(_, "comments", _) => RootField::Comments,
		(_, read, _) => return Err(format!("{field} on the document cannot hold {read}")),
	})
}

fn scoped(items: &[definition::Item], reads: &mut Vec<&'static str>, n: &mut Naming) -> Result<Vec<DocField>, String> {
	let mut out = Vec::new();
	for item in items {
		match item {
			definition::Item::Scope(inner) => out.push(DocField::Scope(scoped(inner, reads, n)?)),
			definition::Item::Fields(record) => {
				for (field, source) in record.0 {
					if *field == "fragment" {
						return Err(teasels("the document", "fragment"));
					}
					if source.from != "literal" {
						if reads.contains(&source.read) {
							return Err(format!("the document reads {} twice", source.read));
						}
						reads.push(source.read);
					}
					out.push(DocField::Field {
						field: n.name(field),
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

fn fragment_last(fields: &mut Vec<DocField>) {
	if let Some(DocField::Scope(inner)) = fields.last_mut() {
		return fragment_last(inner);
	}
	fields.push(DocField::Scope(vec![DocField::Field {
		field: own::fragment,
		holds: RootField::Fragment,
		omit: false,
	}]));
}

fn element(name: Match, rule: &definition::Element, n: &mut Naming) -> Result<ElementRule, String> {
	let this = match rule.form.unwrap_or_default() {
		[] => None,
		[definition::Item::Fields(Record(fields))]
			if fields.len() == 1
				&& fields[0].1.from == "element"
				&& matches!(fields[0].1.read, "this" | "this:text") =>
		{
			if matches!(fields[0].0, "name" | "attributes" | "fragment") {
				return Err(teasels(rule.node, fields[0].0));
			}
			Some((n.name(fields[0].0), fields[0].1.read == "this:text"))
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
		ty: n.name(rule.node),
		this,
		root: rule.root == Some(true),
		once: rule.once == Some(true),
		inside: rule.inside,
		outside: rule.outside,
		raw: rule.content == Some(definition::Content::Raw),
		rcdata: rule.content == Some(definition::Content::Rcdata),
	})
}

fn directive(name: Match, rule: &definition::Directive, n: &mut Naming) -> Result<DirectiveRule, String> {
	let items = rule.form.unwrap_or_default();
	let ty = rule.node;
	let grouped = items.iter().any(|item| match item {
		definition::Item::Opt(inner) | definition::Item::Scope(inner) => reads(inner, fixed),
		definition::Item::OneOf(list) => list.iter().any(|inner| reads(inner, fixed)),
		_ => false,
	});
	if grouped {
		return Err(format!(
			"{ty}: a flag, or what the name holds, stands outside groups, since every node of it has one"
		));
	}
	if let Some(field) = fields_in(ty, items)?
		.into_iter()
		.find(|field| matches!(*field, "name" | "modifiers"))
	{
		return Err(teasels(ty, field));
	}
	let (mut flags, mut kind, mut raw) = (Vec::new(), None, None);
	for (field, s) in sources(items) {
		match (s.from, s.read, s.literal) {
			("literal", _, Some(Literal::True)) => flags.push((n.name(field), true)),
			("literal", _, Some(Literal::False)) => flags.push((n.name(field), false)),
			("literal", ..) => return Err(format!("{ty}: {field} is a flag: true or false")),
			("directive", read @ ("name" | "raw"), _) => {
				let slot = if read == "name" { &mut kind } else { &mut raw };
				if slot.replace(n.name(field)).is_some() {
					return Err(format!("{ty} reads the directive's {read} twice"));
				}
			}
			("directive", read, _) => return Err(teasels(ty, format!("the directive's {read}"))),
			_ => {}
		}
	}
	let rest: Vec<&definition::Item> = items
		.iter()
		.filter(|item| match item {
			definition::Item::Fields(record) => record.0.iter().any(|(_, s)| !fixed(s)),
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
			definition::Item::Opt(inner) if inner.len() == 1 => Some(*inner),
			_ => None,
		};
		let mut single = match wrapped {
			Some(inner) => sources(inner),
			None => sources(std::slice::from_ref(*only)),
		};
		single.retain(|(_, s)| !fixed(s));
		if let [(field, s)] = single.as_slice()
			&& s.from == "value"
		{
			let field = *field;
			if s.optional {
				return Err(format!(
					"{ty}: {field} is a directive's value: `opt` makes it null when missing, it is never left out"
				));
			}
			let (optional, name_too) = (wrapped.is_some() || s.or_arg, s.or_arg);
			let field = n.name(field);
			let value = match s.read {
				"value" => DirectiveValue::Value(field),
				"pattern" => DirectiveValue::Pattern {
					field,
					optional,
					name: name_too,
				},
				_ => DirectiveValue::Expression {
					field,
					optional,
					name: name_too,
				},
			};
			return Ok(DirectiveRule {
				name,
				ty: n.name(ty),
				kind,
				raw,
				value,
				flags,
				unique,
				declares: (s.bind != Bind::No).then(Vec::new),
			});
		}
	}
	let mut bound = Vec::new();
	let items: Vec<definition::Item> = rest.into_iter().cloned().collect();
	let value = DirectiveValue::Form(form(ty, &items, At::Directive, &mut bound, n)?);
	Ok(DirectiveRule {
		name,
		ty: n.name(ty),
		kind,
		raw,
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
	n: &mut Naming,
) -> Result<Piece, String> {
	if marker.is_empty()
		|| marker
			.iter()
			.any(|part| part.is_empty() || part.contains(char::is_whitespace))
	{
		return Err(format!(
			"{ty}: a marker needs parts, none of them empty or holding whitespace"
		));
	}
	let form = form(ty, items, site, bound, n)?;
	let end = last_word(&form.items).ok_or_else(|| format!("{ty}: a marker's form ends its tag with a word"))?;
	Ok(Piece {
		display: keep(&display(marker, end)),
		marker: marker.to_vec(),
		space: space == Some(true),
		end,
		form,
	})
}

fn construct(ty: &'static str, rule: &definition::Construct, n: &mut Naming) -> Result<Construct, String> {
	let block = rule.close.is_some();
	let places = match rule.r#in {
		Some([]) => return Err(format!("{ty} stands nowhere")),
		Some(list) => list.to_vec(),
		None => vec![Place::Content],
	};
	let mut chain_flags = Vec::new();
	let mut branches = Vec::new();
	for branch in rule.branches.unwrap_or_default() {
		if !block {
			return Err(format!("{ty} has branches, so it closes"));
		}
		let mut piece = piece(
			ty,
			branch.marker,
			branch.space,
			branch.form,
			At::Block,
			&mut Vec::new(),
			n,
		)?;
		if let Some((field, flag)) = branch.reopen {
			let (field, flag) = (n.name(field), n.name(flag));
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
	let mut open = piece(
		ty,
		rule.open.marker,
		rule.open.space,
		rule.open.form,
		site,
		&mut bound,
		n,
	)?;
	let mut lists = Vec::new();
	for form in std::iter::once(&open.form).chain(branches.iter().map(|branch| &branch.form)) {
		collect_bodies(form, &mut lists);
	}
	if block && lists.is_empty() {
		if fields_in(ty, rule.open.form)?.contains(&"fragment") {
			return Err(teasels(ty, "fragment"));
		}
		open.form.body = Some(Body {
			field: own::fragment,
			omit: false,
			chain: None,
			declares: std::mem::take(&mut bound),
		});
	}
	if block && !closed(&open.form.items, open.form.body.as_ref()) {
		return Err(format!("{ty} has branches, so its open names its content too"));
	}
	if let ([(only, _)], true) = (&lists[..], block) {
		return Err(format!(
			"{ty}: its one list is teasel's fragment, not {}",
			n.text(*only)
		));
	}
	if let Some(unread) = bound.first() {
		return Err(format!(
			"{ty}: {} binds after the body, so nothing declares it",
			n.text(unread.field)
		));
	}
	let close = match &rule.close {
		Some(close) => {
			let piece = piece(ty, close.marker, close.space, close.form, At::Tag, &mut Vec::new(), n)?;
			if !piece.form.items.iter().all(|item| matches!(item, Item::Literal(_))) {
				return Err(format!("{ty}: a close reads words only"));
			}
			Some(piece)
		}
		None => None,
	};
	let construct = Construct {
		ty: n.name(ty),
		places,
		open,
		branches,
		close,
		chain_flags,
		entries: Vec::new(),
		bodies: Vec::new(),
	};
	if block && construct.places.iter().any(|place| *place != Place::Content) {
		return Err(format!("{ty} closes, so it stands in content only"));
	}
	if (construct.stands(Place::Value) || construct.stands(Place::Rcdata))
		&& !matches!(construct.single(), Some((_, Entry::Expression)))
	{
		return Err(format!(
			"{ty}: in a value or rcdata a tag reads one expression, then its end word"
		));
	}
	let mut seen = Vec::new();
	for (_, piece) in construct.pieces() {
		let marker = piece.marker.concat();
		if seen.contains(&marker) {
			return Err(format!("{ty}: two of its pieces share the marker {marker}"));
		}
		seen.push(marker);
	}
	Ok(construct)
}

fn lower(host: Host) -> Result<Grammar, String> {
	let d = host.definition;
	let n = &mut Naming::new();
	let mut elements = Vec::new();
	let e = d.elements.as_ref();
	for (name, rule) in e.and_then(|e| e.rules).iter().flat_map(|r| r.0) {
		elements.push(element(Match::Exact(name), rule, n)?);
	}
	if let Some(rule) = e.and_then(|e| e.component.as_ref()) {
		elements.push(element(Match::Component, rule, n)?);
	}
	if let Some(rule) = e.and_then(|e| e.other.as_ref()) {
		elements.push(element(Match::Any, rule, n)?);
	}
	for rule in &elements {
		if let Some(inside) = rule.inside
			&& !elements.iter().any(|other| other.name == Match::Exact(inside))
		{
			return Err(format!(
				"{}: inside names {inside}, which no element rule is",
				n.text(rule.ty)
			));
		}
	}
	let x = d.directives.as_ref();
	if let Some(x) = x {
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
		unique: x.unique.is_some(),
	});
	let shorthands = x
		.and_then(|x| x.shorthands)
		.map(|s| s.0)
		.unwrap_or_default()
		.iter()
		.map(|(token, list)| Shorthand {
			token,
			name: list.first().copied().unwrap_or(""),
			modifiers: list.get(1..).unwrap_or_default().to_vec(),
		})
		.collect();
	let mut directives = Vec::new();
	for (name, rule) in x.and_then(|x| x.rules).iter().flat_map(|r| r.0) {
		directives.push(directive(Match::Exact(name), rule, n)?);
	}
	if let Some(other) = x.and_then(|x| x.other.as_ref()) {
		directives.push(directive(Match::Any, other, n)?);
	}
	let mut constructs = Vec::new();
	for (name, rule) in d.constructs.iter().flat_map(|r| r.0) {
		constructs.push(construct(name, rule, n)?);
	}
	let shorthand = d.attributes.as_ref().and_then(|a| a.shorthand);
	if let Some((open, close)) = shorthand {
		token("the shorthand's marker", open)?;
		token("the shorthand's closing word", close)?;
	}
	let mut fields = scoped(d.document.form.unwrap_or_default(), &mut Vec::new(), n)?;
	fragment_last(&mut fields);
	let ty = n.name(d.document.node);
	Ok(Grammar {
		name: host.name,
		names: std::mem::take(&mut n.names),
		shapes: Default::default(),
		document: DocumentRule { ty, fields },
		shorthand,
		autoclose: d.autoclose == Some(true),
		trim: d.trim == Some(true),
		void: d.void.unwrap_or_default().to_vec(),
		verbatim: d.verbatim,
		elements,
		script: d.script.as_ref().map(|s| ScriptRule {
			name: s.element,
			module: s.module.unwrap_or_default().to_vec(),
			typescript: s.typescript.unwrap_or_default().to_vec(),
		}),
		style: d.style,
		directive_syntax,
		shorthands,
		directives,
		constructs,
		starts: [false; 256],
		firsts: Vec::new(),
	})
}

impl Form {
	fn finish(&mut self, names: &[&str]) -> Result<(), String> {
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
						names[body.field.0 as usize], names[declare.field.0 as usize]
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
			names: own::NAMES.to_vec(),
			shapes: Default::default(),
			document: DocumentRule {
				ty: Name::NONE,
				fields: vec![DocField::Field {
					field: own::fragment,
					holds: RootField::Fragment,
					omit: false,
				}],
			},
			shorthand: None,
			autoclose: false,
			trim: false,
			void: Vec::new(),
			verbatim: None,
			elements: Vec::new(),
			script: None,
			style: None,
			directive_syntax: None,
			shorthands: Vec::new(),
			directives: Vec::new(),
			constructs: Vec::new(),
			starts: [false; 256],
			firsts: Vec::new(),
		}
	}

	/// Reads a grammar off its JSON, kept for the process: the definition as the builders made it,
	/// lowered.
	pub fn read(text: &'static str) -> Result<Grammar, String> {
		let mut grammar = lower(Host::read(text, &Arena::default())?)?;
		grammar.finish()?;
		Ok(grammar)
	}

	fn finish(&mut self) -> Result<(), String> {
		if self.name.is_empty() {
			return Err("a grammar names its host".into());
		}
		if self.shorthand.is_some() && self.value_expression().is_none() {
			return Err("the attribute shorthand needs a tag in value that reads one expression".into());
		}
		let names = &self.names;
		for rule in &mut self.directives {
			if let DirectiveValue::Form(form) = &mut rule.value {
				form.finish(names)?;
			}
		}
		let mut starts = [false; 256];
		for construct in &mut self.constructs {
			construct.open.form.finish(names)?;
			let mut entries = construct.open.form.entries.clone();
			let mut bodies = Vec::new();
			collect_bodies(&construct.open.form, &mut bodies);
			for branch in &mut construct.branches {
				branch.form.finish(names)?;
				entries.extend_from_slice(&branch.form.entries);
				collect_bodies(&branch.form, &mut bodies);
			}
			if let Some(close) = &mut construct.close {
				close.form.finish(names)?;
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
		let mut firsts: Vec<First> = Vec::new();
		for (index, construct) in self.constructs.iter().enumerate() {
			for (which, piece) in construct.pieces() {
				let at = match firsts.iter().position(|first| first.part == piece.marker[0]) {
					Some(at) => at,
					None => {
						firsts.push(First {
							part: piece.marker[0],
							word: piece.marker[0].starts_with(crate::lexer::unicode::is_id_continue),
							whole: Vec::new(),
							next: Vec::new(),
						});
						firsts.len() - 1
					}
				};
				let first = &mut firsts[at];
				match piece.marker.get(1) {
					None => first.whole.push((index, which)),
					Some(next) => {
						let byte = next.as_bytes()[0];
						match first.next.iter_mut().find(|(known, _)| *known == byte) {
							Some((_, list)) => list.push((index, which)),
							None => first.next.push((byte, vec![(index, which)])),
						}
					}
				}
			}
		}
		self.firsts = firsts;
		let named = std::iter::once(self.document.ty)
			.chain(self.elements.iter().map(|rule| rule.ty))
			.chain(self.directives.iter().map(|rule| rule.ty))
			.chain(self.constructs.iter().map(|rule| rule.ty));
		for ty in named.map(|ty| self.text(ty)) {
			if matches!(ty, "Fragment" | "Text" | "Comment" | "Attribute" | "Script") {
				return Err(format!("a node type named {ty} is teasel's"));
			}
		}
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

	pub fn text(&self, name: Name) -> &'static str {
		self.names[name.0 as usize]
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
		let mut out: Vec<(Name, Vec<Name>)> = Vec::new();
		let mut add = |ty: Name, fields: &[Name]| {
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
		fn body(body: &Body, into: &mut Vec<Name>) {
			into.push(body.field);
			into.extend(body.chain.map(|(field, _)| field));
		}
		fn items(list: &[Item], into: &mut Vec<Name>) {
			for item in list {
				match item {
					Item::Literal(_) => {}
					// text and type parameters are answered as text
					Item::Entry { field, entry, .. } => {
						if !matches!(entry, Entry::Text | Entry::TypeParameters) {
							into.push(*field);
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
		fn form(form: &Form, into: &mut Vec<Name>) {
			items(&form.items, into);
			form.body.iter().for_each(|b| body(b, into));
		}
		fn document(fields: &[DocField], into: &mut Vec<Name>) {
			for field in fields {
				match field {
					// comments are not nodes: a walk does not follow them
					DocField::Field { field, holds, .. } => {
						if !matches!(holds, RootField::Null | RootField::Comments) {
							into.push(*field);
						}
					}
					DocField::Scope(inner) => document(inner, into),
				}
			}
		}
		let mut fields = Vec::new();
		document(&self.document.fields, &mut fields);
		add(self.document.ty, &fields);
		add(own::Fragment, &[own::nodes]);
		add(own::Text, &[]);
		add(own::Comment, &[]);
		for rule in &self.elements {
			fields.clear();
			fields.extend([own::attributes, own::fragment]);
			fields.extend(rule.this.map(|(field, _)| field));
			add(rule.ty, &fields);
		}
		add(own::Attribute, &[own::value]);
		if self.script.is_some() {
			add(own::Script, &[own::content, own::attributes]);
		}
		// a dynamic argument, `:[expression]`, is a node in the argument's field
		let dynamic = self
			.directive_syntax
			.as_ref()
			.and_then(|syntax| syntax.dynamic.and(Some(own::name)));
		for rule in &self.directives {
			fields.clear();
			fields.extend(dynamic);
			match &rule.value {
				DirectiveValue::Expression { field, .. }
				| DirectiveValue::Pattern { field, .. }
				| DirectiveValue::Value(field) => fields.push(*field),
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
		out.into_iter()
			.map(|(ty, fields)| {
				(
					self.text(ty),
					fields.into_iter().map(|field| self.text(field)).collect(),
				)
			})
			.collect()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// cargo test --release -p teasel --lib read_phases -- --ignored --nocapture
	#[test]
	#[ignore]
	fn read_phases() {
		let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/hosts/svelte/host.json"))
			.unwrap()
			.leak();
		let time = |label: &str, f: &dyn Fn()| {
			let t = std::time::Instant::now();
			for _ in 0..1000 {
				f();
			}
			println!("{label}: {:?}", t.elapsed() / 1000);
		};
		time("Host::read", &|| {
			std::hint::black_box(Host::read(text, &Arena::default()).unwrap());
		});
		let arena = Arena::default();
		let host = Host::read(text, &arena).unwrap();
		time("lower", &|| {
			std::hint::black_box(lower(host).unwrap());
		});
		let grammar = lower(host).unwrap();
		time("finish", &|| {
			let mut g = grammar.clone();
			g.finish().unwrap();
			std::hint::black_box(g);
		});
	}

	fn read<T: Wire<'static>>(text: &str) -> Result<T, String> {
		let mut reader = Reader::new(keep(text), Box::leak(Box::default()));
		let value = T::read(&mut reader)?;
		reader.end()?;
		Ok(value)
	}

	fn write<'a>(value: &impl Wire<'a>) -> String {
		let mut out = String::new();
		value.write(&mut out);
		out
	}

	#[test]
	fn json_round_trips() {
		let markers = r#"[["context","module"],["mo\"d\\ule"],["a\u0001","é"]]"#;
		let read_markers: &[(&str, Option<&str>)] = read(markers).unwrap();
		assert_eq!(
			read_markers,
			[("context", Some("module")), ("mo\"d\\ule", None), ("a\u{1}", Some("é"))]
		);
		assert_eq!(write(&read_markers), markers);
		assert_eq!(read::<&str>(r#""\n\/\u00e9\ud800""#).unwrap(), "\n/é\u{FFFD}");
		assert_eq!(read::<&str>(r#""\ud83d\ude00\ude00""#).unwrap(), "😀\u{FFFD}");
		// keys in any order, an absent option left out, the shape telling a variant
		let source = r#"{"orArg":true,"from":"js","read":"pattern","optional":false,"bind":"inside","literal":null}"#;
		let read_source: definition::Source = read(source).unwrap();
		assert!(read_source.or_arg && read_source.bind == definition::Bind::Inside);
		assert_eq!(read_source.literal, Some(definition::Literal::Null));
		assert_eq!(
			write(&read_source),
			r#"{"from":"js","read":"pattern","optional":false,"bind":"inside","orArg":true,"literal":null}"#
		);
		let plain = r#"{"from":"js","read":"expression","optional":false,"bind":false,"orArg":false}"#;
		let form =
			format!(r#"["{{",{{"opt":[{{"a":{plain}}}]}},{{"oneOf":[["x"],["y"]]}},{{"b":{plain},"c":{plain}}}]"#);
		let items: &[definition::Item] = read(&form).unwrap();
		assert!(matches!(
			items,
			[
				definition::Item::Word("{"),
				definition::Item::Opt(_),
				definition::Item::OneOf(_),
				definition::Item::Fields(Record(fields)),
			] if fields.len() == 2 && fields[1].0 == "c" && fields[1].1.literal.is_none()
		));
		assert_eq!(write(&items), form);
		let piece: definition::Piece = read(r#"{"form":[],"marker":["{"]}"#).unwrap();
		assert!(piece.space.is_none() && piece.marker == ["{"]);
	}

	#[test]
	fn json_errors_say_where() {
		let source = |text: &str| read::<definition::Source>(text).unwrap_err();
		let plain = r#"{"from":"js","read":"expression","optional":false,"bind":false,"orArg":false}"#;
		assert!(read::<definition::Source>(plain).is_ok());
		let missing = source(r#"{"from":"js","read":"expression","optional":false,"bind":false}"#);
		assert!(missing.contains("needs orArg"), "{missing}");
		// a key the struct lacks is stepped over, whatever it holds
		let renamed = source(&plain.replace("\"orArg\"", "\"or_arg\""));
		assert!(renamed.contains("needs orArg"), "{renamed}");
		let more = plain.replace(
			"\"orArg\":false",
			r#""note":{"a":["b",true,null,-1.5e3,{}]},"orArg":false"#,
		);
		assert!(read::<definition::Source>(&more).unwrap().bind == definition::Bind::No);
		let twice = source(&plain.replace("\"read\":\"expression\"", "\"from\":\"js\""));
		assert!(twice.contains("second from"), "{twice}");
		let spaced = source(&plain.replace(":", ": "));
		assert!(
			spaced.contains("a string expected at byte 8 of the grammar, which has no whitespace"),
			"{spaced}"
		);
		let number = source(&plain.replace("false,\"bind", "0,\"bind"));
		assert!(number.contains("a boolean expected at byte 44"), "{number}");
		let variant = source(&plain.replace("\"bind\":false", "\"bind\":\"around\""));
		assert!(variant.contains("a Bind expected at byte 57"), "{variant}");
		let cut = source(&plain[..plain.len() - 1]);
		assert!(cut.contains("ends early"), "{cut}");
		assert!(source("").contains("ends early") && source(r#"{"from":"j"#).contains("ends early"));
		assert!(read::<&[&str]>("[\"a\"]\n").is_ok());
		assert!(
			read::<&[&str]>(r#"["a"] x"#)
				.unwrap_err()
				.contains("text after the grammar at byte 6")
		);
		let nested = |n: usize| format!("{}\"x\"{}", r#"{"opt":["#.repeat(n), "]}".repeat(n));
		assert!(read::<definition::Item>(&nested(400)).is_ok());
		let deep = read::<definition::Item>(&nested(2000)).unwrap_err();
		assert!(deep.contains("nests deeper than"), "{deep}");
		let skipped = format!(r#"{{"note":{}{}, "from":"js"}}"#, "[".repeat(2000), "]".repeat(2000)).replace(", ", ",");
		assert!(
			read::<definition::Source>(&skipped)
				.unwrap_err()
				.contains("nests deeper than")
		);
		assert!(read::<&[&str]>("[\"a\u{1}\"]").is_err() && read::<&str>(r#""\x""#).is_err());
		assert!(read::<(&str, &str)>(r#"["a"]"#).is_err());
	}

	#[test]
	fn spells_names() {
		assert_eq!(spelled!("attribute_expressions"), "attributeExpressions");
		assert_eq!(spelled!("TypeParameters"), "typeParameters");
		assert_eq!(spelled!(stringify!(r#in)), "in");
		assert_eq!(spelled!("{\"", "OneOf", "\":"), r#"{"oneOf":"#);
	}
}
