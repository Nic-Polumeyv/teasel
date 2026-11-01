//! Serializes an `Ast` to ESTree as JSON text: what `cargo test` and `scripts/check.ts` read.

use crate::Options;
use crate::ast::{Ast, List, NodeId, NodeKind, Value};
use crate::engine;
use crate::error::Code;
use crate::host::{self, Grammar};
use crate::interner::StrId;
use crate::json::{Json, write_json_string};
use crate::layout::Ty;
use crate::parser::Entry;
use crate::positions::{Cursor, Positions};
use crate::recipe::{Op, Slot};
use crate::scopes::Role;

/// One entry at a byte offset, as JSON.
pub fn parse(source: &str, entry: Entry, offset: u32, options: Options) -> String {
	if !source.is_char_boundary(offset as usize) {
		return error_json(&format!("offset {offset} is not a character boundary"), offset);
	}
	written(source, entry, offset, options, None)
}

/// A whole document of a host language by its grammar, as JSON; see `host::parse_document`.
pub fn parse_document(source: &str, grammar: &str, options: Options) -> String {
	match engine::grammar(grammar) {
		Ok(grammar) => written(source, Entry::Program, 0, options, Some(grammar)),
		Err(message) => error_json(&message, 0),
	}
}

fn written(source: &str, entry: Entry, offset: u32, options: Options, grammar: Option<&Grammar>) -> String {
	let positions = Positions::of(source, options);
	engine::read(
		source,
		options,
		entry,
		offset,
		"",
		grammar,
		|ast, roots, end, options, _| {
			let names = grammar.map_or(host::grammar::own::NAMES, |grammar| &grammar.names);
			Writer::new(ast, source, &positions, names).answer(entry, roots, end, options)
		},
	)
}

/// The error answer for a parse that never ran, what is wrong as the one argument.
fn error_json(reason: &str, pos: u32) -> String {
	use std::fmt::Write;
	let mut out = format!("{{\"error\":{{\"code\":{},\"args\":[", Code::InvalidRequest as u8);
	write_json_string(&mut out, reason);
	write!(out, "],\"pos\":{pos},\"end\":{pos}}}}}").unwrap();
	out
}

pub struct Writer<'a> {
	ast: &'a Ast,
	source: &'a str,
	names: &'a [&'static str],
	pub(crate) json: Json,
	positions: &'a Positions,
	cursor: Cursor,
	pub(crate) options: Options,
	/// The parse failed: `node` is null and `errors` holds the one error, nothing else is listed.
	pub(crate) failed: bool,
	/// The node being written names something bound by another node: no scope facts on it.
	name_only: bool,
	/// What erasure left in place, in emission order.
	kept: Vec<(&'static str, NodeId)>,
	/// Nodes erasure skipped whose facts the next node written takes over.
	adopted: Vec<NodeId>,
}

impl<'a> Writer<'a> {
	/// `names` are the grammar's, which the hosts' rows number.
	pub(crate) fn new(ast: &'a Ast, source: &'a str, positions: &'a Positions, names: &'a [&'static str]) -> Self {
		Self {
			ast,
			source,
			names,
			json: Json::default(),
			positions,
			cursor: Cursor::default(),
			options: Options::default(),
			failed: false,
			name_only: false,
			kept: Vec::new(),
			adopted: Vec::new(),
		}
	}

	/// An answer as JSON: the roots as `node`, one node or the patterns of a parameter list, null
	/// when the parse failed, then `end` and what the options add.
	pub(crate) fn answer(mut self, entry: Entry, roots: Option<List>, end: u32, options: Options) -> String {
		self.options = options;
		self.failed = roots.is_none();
		self.json.object();
		match roots {
			None => {
				self.key("node");
				self.json.null();
			}
			Some(roots) if entry == Entry::Params => self.list("node", roots),
			Some(roots) => self.field("node", self.ast.list(roots)[0].unwrap()),
		}
		self.key("end");
		let end = self.positions.offset(&mut self.cursor, end);
		self.json.int(end);
		self.trailers();
		self.json.end();
		self.json.finish()
	}

	/// Records a node erasure had to leave in place.
	pub(crate) fn keep(&mut self, ty: &'static str, id: NodeId) {
		self.kept.push((ty, id));
	}

	/// What erasure left in place, as `typescript`, in source order.
	fn all_kept(&mut self) {
		self.key("typescript");
		self.json.list();
		let mut kept = std::mem::take(&mut self.kept);
		// a node comes before what it contains
		kept.sort_by_key(|&(_, id)| {
			let node = self.ast.node(id);
			(node.start, std::cmp::Reverse(node.end))
		});
		for &(ty, id) in &kept {
			self.json.begin(ty);
			let node = self.ast.node(id);
			self.span(node.start, node.end);
			self.json.end();
		}
		self.json.end();
	}

	pub(crate) fn begin(&mut self, ty: &'static str, id: NodeId) {
		let node = self.ast.node(id);
		self.json.begin(ty);
		if matches!(self.ast.nodes[id.index() as usize].kind, NodeKind::Host(_)) {
			self.host_span(node.start, node.end);
		} else {
			self.span(node.start, node.end);
		}
		self.scope_facts(id);
		if self.ast.is_parenthesized(id) {
			self.bool("parenthesized", true);
		}
		if self.options.typescript() {
			self.ast.typescript.write_extras(self, id);
		}
		if let Some(&attached) = self.ast.attached.get(id) {
			self.comments("leadingComments", attached.leading);
			self.comments("trailingComments", attached.trailing);
			self.comments("innerComments", attached.inner);
		}
	}

	/// The scope a node opens and what an identifier is: the binding it declares, or the one it
	/// refers to (null for a global) and whether it writes to it or mutates its value.
	fn scope_facts(&mut self, id: NodeId) {
		let Some(scopes) = &self.ast.scopes else { return };
		if self.name_only {
			return;
		}
		if let Some(scope) = scopes.of_node.get(id) {
			self.key("scope");
			self.json.int(scope);
		}
		match scopes.of_identifier.get(id) {
			Some(Role::Declares(binding)) => {
				self.key("declares");
				self.json.int(binding);
			}
			Some(Role::Reference(reference)) => {
				self.key("reference");
				self.json.int(reference);
			}
			None => {}
		}
		let adopted = std::mem::take(&mut self.adopted);
		for node in adopted.iter().copied().chain([id]) {
			if let Some(root) = scopes.root_of.get(node) {
				self.key("root");
				self.json.int(root);
			}
			let bindings = scopes.declared_by.get(node);
			if !bindings.is_empty() {
				self.key("defines");
				self.json.ints(bindings);
			}
			let writes = scopes.writes_of.get(node);
			if !writes.is_empty() {
				self.key("writes");
				self.json.ints(writes);
			}
		}
	}

	/// The facts of a node erasure leaves out go on the next node written, the one standing in
	/// for it.
	pub(crate) fn adopt(&mut self, id: NodeId) {
		self.adopted.push(id);
	}

	/// The scope, binding and reference tables: what the `scope`, `declares`, `reference` and
	/// `writes` numbers index.
	fn all_scopes(&mut self) {
		let Some(scopes) = &self.ast.scopes else { return };
		self.rows("scopes", &scopes.scopes, 0);
		self.rows("bindings", &scopes.bindings, 1);
		self.rows("references", &scopes.references, 2);
		if !self.ast.host_rows.is_empty() {
			self.rows("roots", &scopes.roots, 3);
		}
	}

	/// A table, each row spelled by its recipe of `recipe::RECIPES`.
	fn rows<T>(&mut self, key: &'static str, rows: &[T], table: usize) {
		static RESOLVED: std::sync::OnceLock<Vec<&'static [Op<Slot>]>> = std::sync::OnceLock::new();
		let ops = RESOLVED.get_or_init(|| {
			crate::recipe::RECIPES
				.iter()
				.zip(crate::recipe::ROWS)
				.map(|((name, ops), (_, _, fields))| crate::recipe::resolve_ops(ops, fields, name))
				.collect()
		})[table];
		self.key(key);
		self.json.list();
		for row in rows {
			self.json.object();
			self.run(NodeId::at(0), ops, row as *const T as *const u8);
			self.json.end();
		}
		self.json.end();
	}

	fn comments(&mut self, key: &'static str, comments: crate::ast::Run) {
		if !comments.is_empty() {
			self.key(key);
			self.comment_list(comments.indices());
		}
	}

	fn comment_list(&mut self, comments: impl IntoIterator<Item = u32>) {
		self.json.list();
		for index in comments {
			let comment = self.ast.comments[index as usize];
			self.json.begin(if comment.is_block() { "Block" } else { "Line" });
			self.key("value");
			let range = comment.text_range();
			self.slice(range.start as u32, range.end as u32);
			self.span(comment.start, comment.end);
			self.json.end();
		}
		self.json.end();
	}

	/// The recovered errors as the thrown one would be: code, args, `pos`, `end`, `line`, `column`.
	fn errors(&mut self) {
		let places = self.positions.of_errors(self.source, &self.ast.errors);
		self.key("errors");
		self.json.list();
		for (error, [pos, end, line, column]) in self.ast.errors.iter().zip(places) {
			self.json.object();
			self.key("code");
			self.json.int(error.code as u32);
			self.key("args");
			self.json.list();
			for &arg in error.args.iter().flatten() {
				self.str_of(arg);
			}
			self.json.end();
			self.key("pos");
			self.json.int(pos);
			self.key("end");
			self.json.int(end);
			self.key("line");
			self.json.int(line);
			self.key("column");
			self.json.int(column);
			self.json.end();
		}
		self.json.end();
	}

	/// What the options add after a root: every comment, the errors recovered from, what erasure
	/// kept, the scopes; a failed parse lists its error alone.
	fn trailers(&mut self) {
		if self.failed {
			return self.errors();
		}
		if self.options.has(Options::COMMENTS) {
			self.key("comments");
			self.comment_list(0..self.ast.comments.len() as u32);
		}
		if self.options.has(Options::ERROR_RECOVERY) {
			self.errors();
		}
		if self.options.has(Options::ERASE) {
			self.all_kept();
		}
		if self.options.has(Options::SCOPES) {
			self.all_scopes();
		}
	}

	pub(crate) fn span(&mut self, start: u32, end: u32) {
		self.located(start, end, false);
	}

	fn host_span(&mut self, start: u32, end: u32) {
		self.located(start, end, true);
	}

	fn located(&mut self, start: u32, end: u32, host: bool) {
		let (span, loc) = self.positions.span(&mut self.cursor, start, end, host);
		self.json.key("start");
		self.json.int(span.start);
		self.json.key("end");
		self.json.int(span.end);
		if let Some(loc) = loc {
			self.json.key("loc");
			self.json.object();
			for (key, at) in [("start", loc.start), ("end", loc.end)] {
				self.json.key(key);
				self.json.object();
				self.json.key("line");
				self.json.int(at.line);
				self.json.key("column");
				self.json.int(at.column);
				self.json.end();
			}
			self.json.end();
		}
	}

	/// The source between two byte offsets, as a string value.
	fn slice(&mut self, start: u32, end: u32) {
		self.json.text(&self.source[start as usize..end as usize]);
	}

	pub(crate) fn end(&mut self) {
		self.json.end();
	}

	pub(crate) fn key(&mut self, key: &'static str) {
		self.json.key(key);
	}

	pub(crate) fn field(&mut self, key: &'static str, id: NodeId) {
		self.key(key);
		self.node(id);
	}

	/// A specifier's other name, which is the same node as the binding one in `import { a }`
	/// and `export { a }`, and then only names: the binding facts stay on the binding one.
	fn other_name(&mut self, key: &'static str, id: NodeId, binding: NodeId) {
		let was = self.name_only;
		self.name_only = id == binding;
		self.field(key, id);
		self.name_only = was;
	}

	pub(crate) fn kind(&self, id: NodeId) -> NodeKind {
		self.ast.node(id).kind
	}

	/// The key only when there is a node; an unset property is left out.
	pub(crate) fn opt_key(&mut self, key: &'static str, id: Option<NodeId>) {
		if let Some(id) = id {
			self.field(key, id);
		}
	}

	pub(crate) fn opt(&mut self, key: &'static str, id: Option<NodeId>) {
		self.key(key);
		match id {
			Some(id) => self.node(id),
			None => self.json.null(),
		}
	}

	pub(crate) fn list(&mut self, key: &'static str, list: List) {
		self.key(key);
		self.items(list);
	}

	fn items(&mut self, list: List) {
		self.json.list();
		let ast = self.ast;
		for item in ast.list(list) {
			if self.options.has(Options::ERASE) && item.is_some_and(|id| ast.typescript.erased(ast, id)) {
				continue;
			}
			match item {
				Some(id) => self.node(*id),
				None => self.json.null(),
			}
		}
		self.json.end();
	}

	/// A parameter list; erasing drops TypeScript's `this` parameter.
	pub(crate) fn params(&mut self, key: &'static str, list: List) {
		if self.options.has(Options::ERASE)
			&& let Some(&Some(first)) = self.ast.list(list).first()
			&& let NodeKind::Identifier { name } = self.kind(first)
			&& self.ast.str(name) == "this"
		{
			let rest = List {
				start: list.start + 1,
				len: list.len - 1,
			};
			return self.list(key, rest);
		}
		self.list(key, list)
	}

	pub(crate) fn bool(&mut self, key: &'static str, value: bool) {
		self.key(key);
		self.json.bool(value);
	}

	pub(crate) fn text(&mut self, key: &'static str, value: &str) {
		self.key(key);
		self.json.text(value);
	}

	pub(crate) fn interned(&mut self, key: &'static str, id: StrId) {
		self.key(key);
		self.str_of(id);
	}

	fn str_of(&mut self, id: StrId) {
		self.json.marked(self.ast.str(id), self.ast.strings.marks_of(id));
	}

	pub(crate) fn raw(&mut self, id: NodeId) {
		let node = self.ast.node(id);
		self.key("raw");
		self.slice(node.start, node.end);
	}

	/// The recipes of the JavaScript kinds, resolved once.
	fn js_recipes() -> &'static [&'static [Op<Slot>]] {
		static RESOLVED: std::sync::OnceLock<&'static [&'static [Op<Slot>]]> = std::sync::OnceLock::new();
		RESOLVED.get_or_init(|| crate::recipe::resolve(crate::recipe::JS, crate::ast::node_layout::VARIANTS))
	}

	/// Follows a kind's recipe over the record at `base`, a `repr(C, u32)` enum's payload; the
	/// caller ends the node, unless a child stood in for it: false then.
	pub(crate) fn run(&mut self, id: NodeId, ops: &[Op<Slot>], base: *const u8) -> bool {
		for op in ops {
			match *op {
				Op::Keep(name) => {
					if self.options.has(Options::ERASE) {
						self.keep(name, id);
					}
				}
				Op::KeepIf(name, slot) => {
					if self.options.has(Options::ERASE) && get::<u8>(base, slot) == 1 {
						self.keep(name, id);
					}
				}
				Op::Through(slot) => {
					if self.options.has(Options::ERASE) {
						self.adopt(id);
						self.node(get(base, slot));
						return false;
					}
				}
				Op::EnumOr(key, slot, other) => {
					let Ty::OptEnum(names) = slot.ty else { unreachable!() };
					self.text(key, names.get(get::<u8>(base, slot) as usize).copied().unwrap_or(other));
				}
				Op::Type(name) => self.begin(name, id),
				Op::TypeOf(slot) => {
					let Ty::Enum(names) = slot.ty else { unreachable!() };
					self.begin(names[get::<u8>(base, slot) as usize], id);
				}
				Op::Node(key, slot) => self.field(key, get(base, slot)),
				Op::Int(key, slot) => {
					self.key(key);
					self.json.int(get(base, slot));
				}
				Op::Pair(key, slot) => {
					self.key(key);
					self.json.ints(&get::<[u32; 2]>(base, slot));
				}
				Op::Opt(key, slot) => match slot.ty {
					Ty::OptU32 => {
						self.key(key);
						match get::<Option<u32>>(base, slot) {
							Some(value) => self.json.int(value),
							None => self.json.null(),
						}
					}
					Ty::OptStr => {
						self.key(key);
						match get::<Option<StrId>>(base, slot) {
							Some(string) => self.str_of(string),
							None => self.json.null(),
						}
					}
					_ => self.opt(key, get(base, slot)),
				},
				// JSON spells undefined by leaving the key out
				Op::Maybe(key, slot) => match slot.ty {
					Ty::OptList => self.list(key, get::<Option<List>>(base, slot).unwrap_or(List::EMPTY)),
					// a missing enum is a byte past the names, wherever the compiler put it
					Ty::OptEnum(names) => {
						if let Some(&name) = names.get(get::<u8>(base, slot) as usize) {
							self.text(key, name);
						}
					}
					_ => self.opt_key(key, get(base, slot)),
				},
				Op::List(key, slot) => self.list(key, get(base, slot)),
				Op::OptListKey(key, slot) => {
					if let Some(list) = get::<Option<List>>(base, slot) {
						self.list(key, list);
					}
				}
				Op::Params(key, slot) => self.params(key, get(base, slot)),
				Op::Bool(key, slot) => self.bool(key, get(base, slot)),
				Op::Str(key, slot) => self.interned(key, get(base, slot)),
				Op::OptStrKey(key, slot) => {
					if let Some(string) = get::<Option<StrId>>(base, slot) {
						self.interned(key, string);
					}
				}
				Op::Enum(key, slot) => {
					let Ty::Enum(names) = slot.ty else { unreachable!() };
					self.text(key, names[get::<u8>(base, slot) as usize]);
				}
				Op::Modifier(key, slot) => {
					let Ty::OptEnum(names) = slot.ty else { unreachable!() };
					if let Some(&name) = names.get(get::<u8>(base, slot) as usize) {
						if name == "true" {
							self.bool(key, true);
						} else {
							self.text(key, name);
						}
					}
				}
				Op::BoolNames(key, slot, yes, no) => self.text(key, if get::<bool>(base, slot) { yes } else { no }),
				Op::Float(key, slot) => {
					let value = self.ast.numbers[get::<u32>(base, slot) as usize];
					self.key(key);
					if value.is_finite() {
						self.json.float(value);
					} else {
						self.json.null();
					}
				}
				Op::Raw => self.raw(id),
				Op::BigInt => {
					let node = self.ast.node(id);
					let raw = &self.source[node.start as usize..node.end as usize - 1];
					let bigint = bigint_decimal(raw);
					self.text("bigint", &bigint);
				}
				Op::ConstBool(key, value) => self.bool(key, value),
				Op::Null(key) | Op::RegExp(key, ..) | Op::BigIntValue(key) => {
					self.key(key);
					self.json.null();
				}
				Op::EmptyList(key) => self.list(key, List::EMPTY),
				Op::Object(key, inner) => {
					self.key(key);
					self.json.object();
					self.run(id, inner, base);
					self.json.end();
				}
				Op::OtherName(key, name, binding) => self.other_name(key, get(base, name), get(base, binding)),
			}
		}
		true
	}

	pub(crate) fn node(&mut self, id: NodeId) {
		use NodeKind::*;
		let kind = &self.ast.nodes[id.index() as usize].kind;
		match *kind {
			// TypeScript closes its own node, since erasing may put another in its place
			Extension(index) => return self.ast.typescript.write_node(self, id, index),
			Host(index) => {
				let host = self.ast.host_rows[index as usize];
				match host.ty.map(|ty| self.names[ty as usize]) {
					None => {
						// an object of the host's without a type, positions and all
						let node = self.ast.node(id);
						self.json.object();
						self.host_span(node.start, node.end);
					}
					Some(ty) if host.span => self.begin(ty, id),
					Some(ty) => {
						self.json.begin(ty);
						self.scope_facts(id);
					}
				}
				for i in host.fields.indices() {
					self.json.key(self.names[self.ast.host_keys[i as usize] as usize]);
					match self.ast.host_vals[i as usize].value() {
						Value::Node(child) => self.node(child),
						Value::Nodes(children) => self.items(children),
						Value::Str(string) => self.str_of(string),
						Value::Slice(start, end) => self.slice(start, end),
						Value::Strs(start, len) => {
							self.json.list();
							for i in start as usize..(start + len) as usize {
								self.str_of(self.ast.host_strings[i]);
							}
							self.json.end();
						}
						Value::Bool(value) => self.json.bool(value),
						Value::Int(value) => self.json.int(value),
						Value::Null => self.json.null(),
						Value::Comments => self.comment_list(0..self.ast.comments.len() as u32),
					}
				}
			}
			_ => {
				let base = kind as *const NodeKind as *const u8;
				self.run(id, Self::js_recipes()[tag(base)], base);
			}
		}
		self.end();
	}
}

/// The tag of a `repr(C, u32)` enum at `base`: its first word.
pub(crate) fn tag(base: *const u8) -> usize {
	unsafe { base.cast::<u32>().read() as usize }
}

/// The field at `slot` of the record at `base`, as the type the recipe reads it by.
fn get<T: Copy>(base: *const u8, slot: Slot) -> T {
	unsafe { base.add(slot.at).cast::<T>().read_unaligned() }
}

/// The decimal digits of a BigInt literal's text, without the `n`.
fn bigint_decimal(raw: &str) -> String {
	let (radix, digits) = match raw.get(..2) {
		Some("0x" | "0X") => (16, &raw[2..]),
		Some("0o" | "0O") => (8, &raw[2..]),
		Some("0b" | "0B") => (2, &raw[2..]),
		_ => return raw.replace('_', ""),
	};
	let mut limbs: Vec<u32> = vec![0];
	for digit in digits.bytes().filter(|b| *b != b'_') {
		let mut carry = (digit as char).to_digit(radix).unwrap() as u64;
		for limb in limbs.iter_mut() {
			let v = *limb as u64 * radix as u64 + carry;
			*limb = (v % 1_000_000_000) as u32;
			carry = v / 1_000_000_000;
		}
		if carry > 0 {
			limbs.push(carry as u32);
		}
	}
	use std::fmt::Write;
	let mut out = String::with_capacity(limbs.len() * 9);
	write!(out, "{}", limbs.last().unwrap()).unwrap();
	for limb in limbs.iter().rev().skip(1) {
		write!(out, "{limb:09}").unwrap();
	}
	out
}
