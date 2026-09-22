mod css;
pub mod entities;
pub mod grammar;
mod native;
pub mod plan;
mod program;

pub use self::plan::Plan;
use std::borrow::Cow;

use self::plan::{Absence, AttributeMode, Boundary, Gap, Js, Mode, Relation, SpanPolicy, Stop};
use self::program::{
	Base, Code as ExprCode, Construct, Expr, Form, Key, Path, Property, Reader, Repeat, Slot, Symbol, Token,
};
use crate::ast::{Ast, Host, List, NodeId, NodeKind, Run, Value};
use crate::error::{Code, SyntaxError};
use crate::interner::StrId;
use crate::lexer::unicode::{is_id_continue, is_id_start};
use crate::parser::{Entry, Extension, ForInit, Options, Parser, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Datum {
	Missing,
	Null,
	Bool(bool),
	Number(f64),
	Text(StrId),
	Interned(StrId),
	Slice(u32, u32),
	Span(u32, u32, bool),
	Node(NodeId),
	Nodes(List),
	Array(u32, u32),
	Constants(u32, u32),
	Strings(u32, u32),
	Object(usize),
	Event(usize),
	Header(Run),
	Attribute(usize),
	NameFacts(usize),
	Record(usize),
	Scopes(usize),
	Region(u32, u32),
	Incoming(usize),
	Ancestors(usize),
	Iteration(usize),
	Regex(StrId, StrId),
	Template(StrId, Option<StrId>),
	Stylesheet(usize),
}

const _: () = assert!(std::mem::size_of::<Datum>() <= 16);

impl Datum {
	fn yes(&self) -> bool {
		matches!(self, Self::Bool(true))
	}
}

#[derive(Clone, Copy, Debug)]
enum Event {
	Stylesheet(List, List),
	Text {
		raw: Datum,
		decoded: Datum,
	},
	Comment(Datum),
	Element {
		name: (u32, u32),
		id: Option<StrId>,
		header: Option<Run>,
		after_name: u32,
		facts: u8,
		at_document: bool,
		raw: Datum,
	},
	Directive {
		name: (u32, u32),
		raw: (u32, u32),
		argument: Datum,
		modifiers: Run,
		value: Datum,
		quoted: bool,
	},
}

#[derive(Clone, Copy, Debug)]
struct HeaderAttribute {
	name: Datum,
	boolean: bool,
	text: Datum,
	expression: bool,
}

#[derive(Clone, Copy, Debug)]
struct Record {
	failure: Option<usize>,
	body_end: Option<u32>,
	children_end: Option<u32>,
	aborted: bool,
	rule: usize,
	ty: StrId,
	slots: usize,
	event: Datum,
	owner: Option<usize>,
	start: u32,
	node: Option<NodeId>,
}

#[derive(Clone, Copy, Debug)]
struct Element {
	record: usize,
	name: (u32, u32),
	empty: bool,
	attributes: Option<List>,
	content: Mode,
}

#[derive(Clone, Copy, Debug)]
struct Autoclosed {
	previous: (u32, u32),
	by: (u32, u32),
	depth: usize,
}

#[derive(Clone, Debug)]
enum Rejection {
	Expected(u32, u32, StrId),
	Space(u32, u32),
	Error(Box<SyntaxError>),
}
impl From<Box<SyntaxError>> for Rejection {
	fn from(error: Box<SyntaxError>) -> Self {
		Self::Error(error)
	}
}
impl Rejection {
	fn pos(&self) -> u32 {
		match self {
			Self::Expected(pos, ..) | Self::Space(pos, ..) => *pos,
			Self::Error(error) => error.pos,
		}
	}
	fn boxed(self, plan: &Plan) -> Box<SyntaxError> {
		match self {
			Self::Error(error) => error,
			Self::Expected(pos, end, id) => error(
				pos,
				end,
				Code::Expected,
				Some(&plan.program.strings[id.index() as usize]),
			),
			Self::Space(pos, end) => error(pos, end, Code::Expected, Some("whitespace")),
		}
	}
}

#[derive(Default)]
pub(crate) struct Spare {
	records: Vec<Record>,
	slots: Vec<Datum>,
	elements: Vec<Element>,
	active: Vec<usize>,
	iteration: Vec<Run>,
	iteration_slots: Vec<Datum>,
	bindings: Vec<Datum>,
	events: Vec<Event>,
	attributes: Vec<HeaderAttribute>,
	values: Vec<Datum>,
	saved: Vec<Datum>,
	nodes: Vec<Vec<NodeId>>,
	arrays: Vec<Vec<Datum>>,
	follows: Vec<String>,
	failures: Vec<Rejection>,
	node_records: Vec<usize>,
	ids: Vec<u32>,
	scan_strings: crate::interner::Interner,
	scan_stops: Vec<(u32, u32, bool)>,
	scan_templates: Vec<u32>,
	scan_regexp: crate::lexer::regexp::Scratch,
}

impl std::fmt::Debug for Spare {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Spare").finish_non_exhaustive()
	}
}

pub fn parse(src: &str, plan: &Plan, options: Options) -> (Ast, std::result::Result<NodeId, Box<crate::SyntaxError>>) {
	let (ast, root) = parse_document::<()>(src, plan, options, None, true);
	(*ast, root)
}

pub(crate) fn parse_document<E: Extension>(
	src: &str,
	plan: &Plan,
	options: Options,
	reused: Option<Box<Ast<E::Data>>>,
	regions: bool,
) -> (Box<Ast<E::Data>>, Result<NodeId>) {
	let cut = if plan.html.trim_end {
		src.trim_end_matches(is_space)
	} else {
		src
	};
	let mut ast = reused.unwrap_or_else(|| Box::new(Ast::sized(src.len())));
	let mut spare = ast.host_spare.take().unwrap_or_default();
	spare.ids.clear();
	spare.ids.resize(plan.program.strings.len(), u32::MAX);
	spare.records.clear();
	spare.slots.clear();
	spare.elements.clear();
	spare.active.clear();
	spare.iteration.clear();
	spare.iteration_slots.clear();
	spare.bindings.clear();
	spare.events.clear();
	spare.scan_strings.clear();
	spare.attributes.clear();
	spare.values.clear();
	spare.saved.clear();
	spare.failures.clear();
	spare.node_records.clear();
	let mut w = Walker::<E> {
		src: cut,
		full: src.len() as u32,
		plan,
		options,
		ast: Some(ast),
		at: 0,
		limit: cut.len() as u32,
		spare,
		native_reads: 0,
		autoclosed: None,
	};
	let result = w.call(plan.document, Datum::Missing, None, "").and_then(|node| {
		if w.at != w.limit {
			return fail(w.at, w.at, Code::UnexpectedToken, None);
		}
		w.ast().nodes[node.index() as usize].end = w.full;
		let _ = regions;
		Ok(node)
	});

	let errors = &mut w.ast().errors;
	errors.sort_by_key(|error| error.pos);
	errors.dedup_by(|a, b| a.pos == b.pos && a.code == b.code);
	let mut ast = w.ast.take().unwrap();
	let spare = w.spare;
	ast.host_spare = Some(spare);
	(ast, result)
}

struct Walker<'a, E: Extension> {
	src: &'a str,
	full: u32,
	plan: &'a Plan,
	options: Options,
	at: u32,
	limit: u32,
	spare: Box<Spare>,
	native_reads: usize,
	autoclosed: Option<Autoclosed>,
	ast: Option<Box<Ast<E::Data>>>,
}

impl<E: Extension> std::ops::Deref for Walker<'_, E> {
	type Target = Spare;
	fn deref(&self) -> &Spare {
		&self.spare
	}
}
impl<E: Extension> std::ops::DerefMut for Walker<'_, E> {
	fn deref_mut(&mut self) -> &mut Spare {
		&mut self.spare
	}
}

impl<'a, E: Extension> Walker<'a, E> {
	fn ast(&mut self) -> &mut Ast<E::Data> {
		self.ast.as_mut().unwrap()
	}
	fn tree(&self) -> &Ast<E::Data> {
		self.ast.as_ref().unwrap()
	}
	fn rest(&self) -> &'a str {
		&self.src[self.at as usize..self.limit as usize]
	}
	fn skip_text(&mut self) {
		let stops = &self.plan.program.text_stops;
		let bytes = self.src.as_bytes();
		while self.at < self.limit {
			let b = bytes[self.at as usize];
			if stops[b as usize / 64] & (1 << (b % 64)) != 0 {
				break;
			}
			self.at += 1;
		}
	}
	fn skip_raw(&mut self, name: &str) {
		let bytes = self.src.as_bytes();
		while self.at < self.limit {
			if bytes[self.at as usize] == b'<' && closing_tag(self.rest(), name).is_some() {
				return;
			}
			self.at += 1;
		}
	}
	fn char(&self) -> Option<char> {
		self.rest().chars().next()
	}
	fn matches(&self, text: &str) -> bool {
		self.rest().starts_with(text)
	}
	fn eat(&mut self, text: &str) -> bool {
		if !self.matches(text) {
			return false;
		}
		self.at += text.len() as u32;
		true
	}
	fn expect(&mut self, text: &str) -> Result<()> {
		if self.eat(text) {
			Ok(())
		} else {
			self.report(error(self.at, self.at, Code::Expected, Some(text)))
		}
	}
	fn report(&mut self, error: Box<crate::SyntaxError>) -> Result<()> {
		if self.options.error_recovery {
			self.ast().errors.push(*error);
			Ok(())
		} else {
			Err(error)
		}
	}
	fn space(&mut self) {
		while let Some(c) = self.char().filter(|c| is_space(*c)) {
			self.at += c.len_utf8() as u32;
		}
	}
	fn intern(&mut self, text: &str) -> StrId {
		self.ast().strings.intern(text)
	}
	fn tree_id(&mut self, id: StrId) -> StrId {
		let i = id.index() as usize;
		let known = self.ids[i];
		if known != u32::MAX {
			return StrId::at(known);
		}
		let plan = self.plan;
		let tree = self.ast().strings.intern(&plan.program.strings[i]);
		self.spare.ids[i] = tree.index();
		tree
	}
	fn list(&mut self, nodes: &[NodeId]) -> List {
		self.ast().add_list_from(nodes.iter().copied().map(Some))
	}
	fn event(&mut self, event: Event) -> Datum {
		let id = self.events.len();
		self.events.push(event);
		Datum::Event(id)
	}
	fn array(&mut self, values: Vec<Datum>) -> Datum {
		let start = self.values.len() as u32;
		self.values.extend_from_slice(&values);
		let count = values.len() as u32;
		self.recycle_values(values);
		Datum::Array(start, count)
	}
	fn take_values(&mut self) -> Vec<Datum> {
		self.arrays.pop().unwrap_or_default()
	}
	fn recycle_values(&mut self, mut values: Vec<Datum>) {
		values.clear();
		self.arrays.push(values);
	}
	fn take_nodes(&mut self) -> Vec<NodeId> {
		self.nodes.pop().unwrap_or_default()
	}
	fn finish_nodes(&mut self, mut nodes: Vec<NodeId>) -> List {
		let list = self.list(&nodes);
		nodes.clear();
		self.nodes.push(nodes);
		list
	}
	fn decoded(&mut self, start: u32, end: u32, attribute: bool) -> Datum {
		match decode(&self.src[start as usize..end as usize], attribute) {
			Cow::Borrowed(_) => Datum::Slice(start, end),
			Cow::Owned(s) => Datum::Interned(self.intern(&s)),
		}
	}
	fn text_of<'b>(&'b self, value: &'b Datum) -> Option<&'b str> {
		match value {
			Datum::Text(id) => Some(&self.plan.program.strings[id.index() as usize]),
			Datum::Interned(id) => Some(self.tree().str(*id)),
			Datum::Slice(a, b) => self.src.get(*a as usize..*b as usize),
			_ => None,
		}
	}
	fn count(&self, value: &Datum) -> usize {
		match value {
			Datum::Array(_, n) | Datum::Constants(_, n) | Datum::Strings(_, n) => *n as usize,
			Datum::Nodes(list) => list.len as usize,
			Datum::Header(run) => run.len as usize,
			Datum::Ancestors(record) => {
				let mut owner = self.records[*record].owner;
				let mut n = 0;
				while let Some(i) = owner {
					n += 1;
					owner = self.records[i].owner;
				}
				n
			}
			_ => 0,
		}
	}
	fn item(&self, value: &Datum, index: usize) -> Datum {
		if index >= self.count(value) {
			return Datum::Missing;
		}
		match *value {
			Datum::Array(a, _) => self.values[a as usize + index],
			Datum::Constants(a, _) => self.plan.program.constants[a as usize + index],
			Datum::Strings(a, _) => Datum::Interned(self.tree().host_strings[a as usize + index]),

			Datum::Nodes(list) => self.tree().list(list)[index].map_or(Datum::Null, Datum::Node),
			Datum::Header(run) => Datum::Attribute(run.start as usize + index),
			Datum::Ancestors(record) => {
				let mut owner = self.records[record].owner;
				for _ in 0..index {
					owner = owner.and_then(|i| self.records[i].owner);
				}
				owner.map_or(Datum::Missing, Datum::Record)
			}
			_ => Datum::Missing,
		}
	}
	fn write(&mut self, record: usize, slot: &Slot, value: Datum) {
		if value == Datum::Missing {
			return;
		}
		match *slot {
			Slot::Record(i) => {
				let i = self.records[record].slots + i as usize;
				self.slots[i] = value;
			}
			Slot::Iteration(i) => {
				if let Some(run) = self.iteration.last().copied() {
					self.iteration_slots[run.start as usize + i as usize] = value;
				}
			}
			Slot::Missing => {}
		}
	}
	fn datum(&self, value: Value) -> Datum {
		match value {
			Value::Node(id) => Datum::Node(id),
			Value::Nodes(list) => Datum::Nodes(list),
			Value::Str(id) => Datum::Interned(id),
			Value::Slice(a, b) => Datum::Slice(a, b),
			Value::Strs(a, n) => Datum::Strings(a, n),
			Value::Bool(v) => Datum::Bool(v),
			Value::Int(v) => Datum::Number(v as f64),
			Value::Null | Value::Comments => Datum::Null,
		}
	}
	fn property(&mut self, value: Datum, path: &Path) -> Datum {
		let Path::Name(prop) = path else {
			let Path::Index(i) = path else { unreachable!() };
			return self.item(&value, *i);
		};
		let key = prop.key;
		match value {
			Datum::Object(i) => self.plan.program.objects[i]
				.iter()
				.find(|(k, _)| k.name == prop.name)
				.map_or(Datum::Missing, |(_, v)| *v),
			Datum::Record(i) => {
				let rec = self.records[i];
				match key {
					Key::Type => Datum::Text(rec.ty),
					Key::Span => Datum::Span(
						rec.start,
						rec.node.map_or(self.at, |id| self.tree().node(id).end),
						false,
					),
					Key::Header => self.property(rec.event, path),
					Key::Scopes => Datum::Scopes(i),
					_ => self.plan.program.rules[rec.rule].lookup[prop.name.index() as usize]
						.map_or(Datum::Missing, |n| self.slots[rec.slots + n]),
				}
			}
			Datum::Scopes(i) => self.plan.program.rules[self.records[i].rule].scopes[prop.name.index() as usize]
				.map_or(Datum::Missing, |r| Datum::Region(i as u32, r as u32)),
			Datum::Span(a, b, dynamic) => match key {
				Key::Start => Datum::Number(a as f64),
				Key::End => Datum::Number(b as f64),
				Key::Span => value,
				Key::Text => Datum::Slice(a, b),
				Key::Dynamic => Datum::Bool(dynamic),
				_ => Datum::Missing,
			},
			Datum::Event(i) => self.event_value(i, key),
			Datum::Header(run) => {
				if key == Key::Attributes {
					Datum::Header(run)
				} else {
					Datum::Missing
				}
			}
			Datum::Attribute(i) => {
				let a = self.attributes[i];
				match key {
					Key::Kind => Datum::Text(if a.expression {
						Symbol::WordExpression.id()
					} else {
						Symbol::WordOrdinary.id()
					}),
					Key::Name if !a.expression => a.name,
					Key::Boolean if !a.expression => Datum::Bool(a.boolean),
					Key::StaticText if !a.expression => a.text,
					_ => Datum::Missing,
				}
			}
			Datum::NameFacts(i) => {
				let Event::Element { name, facts, .. } = self.events[i] else {
					unreachable!()
				};
				match key {
					Key::ValidHtmlName => Datum::Bool(facts & 1 != 0),
					Key::Identifier => Datum::Bool(facts & 2 != 0),
					Key::UppercaseInitial => Datum::Bool(facts & 4 != 0),
					Key::DottedIdentifier => Datum::Bool(facts & 8 != 0),
					Key::Namespace => self.src[name.0 as usize..name.1 as usize]
						.find(':')
						.map_or(Datum::Missing, |n| Datum::Slice(name.0, name.0 + n as u32)),
					_ => Datum::Missing,
				}
			}
			Datum::Regex(pattern, flags) => match key {
				Key::Pattern => Datum::Interned(pattern),
				Key::Flags => Datum::Interned(flags),
				_ => Datum::Missing,
			},
			Datum::Template(raw, cooked) => match key {
				Key::Raw => Datum::Interned(raw),
				Key::Cooked => cooked.map_or(Datum::Null, Datum::Interned),
				_ => Datum::Missing,
			},
			Datum::Stylesheet(i) => self.event_value(i, key),
			Datum::Node(id) => {
				let node = self.tree().node(id);
				match key {
					Key::Span => return Datum::Span(node.start, node.end, false),
					Key::Raw
						if matches!(
							node.kind,
							NodeKind::NumberLiteral { .. }
								| NodeKind::StringLiteral { .. }
								| NodeKind::BooleanLiteral { .. }
								| NodeKind::NullLiteral | NodeKind::BigIntLiteral
								| NodeKind::RegExpLiteral { .. }
						) =>
					{
						return Datum::Slice(node.start, node.end);
					}
					Key::Start => return Datum::Number(node.start as f64),
					Key::End => return Datum::Number(node.end as f64),
					Key::InnerSource if node.end > node.start + 1 => return Datum::Slice(node.start + 1, node.end - 1),
					Key::Header => {
						return self
							.node_records
							.get(id.index() as usize)
							.copied()
							.filter(|i| *i != usize::MAX)
							.map_or(Datum::Missing, |i| self.property(self.records[i].event, path));
					}
					_ => {}
				}
				if let NodeKind::Host(i) = node.kind {
					let host = &self.tree().hosts[i as usize];
					if key == Key::Type {
						return self.plan_text(host.ty);
					}
					self.tree().host_fields[host.fields.0 as usize..(host.fields.0 + host.fields.1) as usize]
						.iter()
						.find(|(k, _)| std::ptr::eq(*k, self.plan.program.names[prop.name.index() as usize]))
						.map_or(Datum::Missing, |(_, v)| self.datum(*v))
				} else {
					self.native_property(id, key)
				}
			}
			_ => Datum::Missing,
		}
	}
	fn equal(&self, a: &Datum, b: &Datum) -> bool {
		if let (Datum::Object(a), Datum::Object(b)) = (a, b) {
			let a = &self.plan.program.objects[*a];
			let b = &self.plan.program.objects[*b];
			return a.len() == b.len()
				&& a.iter()
					.zip(b)
					.all(|((ka, va), (kb, vb))| ka.name == kb.name && self.equal(va, vb));
		}
		match (a, b) {
			(Datum::Text(a), Datum::Text(b)) | (Datum::Interned(a), Datum::Interned(b)) => return a == b,
			(Datum::Text(p), Datum::Interned(t)) | (Datum::Interned(t), Datum::Text(p)) => {
				return self.plan.program.strings[p.index() as usize].as_ref() == self.tree().str(*t);
			}
			_ => {}
		}
		if matches!(
			a,
			Datum::Array(..) | Datum::Constants(..) | Datum::Strings(..) | Datum::Nodes(_)
		) && matches!(
			b,
			Datum::Array(..) | Datum::Constants(..) | Datum::Strings(..) | Datum::Nodes(_)
		) {
			return self.count(a) == self.count(b)
				&& (0..self.count(a)).all(|i| self.equal(&self.item(a, i), &self.item(b, i)));
		}
		match (self.text_of(a), self.text_of(b)) {
			(Some(a), Some(b)) => a == b,
			_ => a == b,
		}
	}
	#[inline]
	fn eval(&mut self, code: &ExprCode, record: usize) -> Result<Datum> {
		Ok(match &self.plan.program.exprs[code.index()] {
			Expr::Constant(value) => *value,
			Expr::Slot(i) => self.slots[self.records[record].slots + *i as usize],
			Expr::Iteration(i) => self
				.iteration
				.last()
				.map_or(Datum::Missing, |r| self.iteration_slots[r.start as usize + *i as usize]),
			Expr::Event(key) => self.event_field(record, *key),
			Expr::RecordType => Datum::Text(self.records[record].ty),
			Expr::Get {
				base: Base::Binding(i),
				path,
			} if path.len == 0 => self.bindings[self.bindings.len() - 1 - *i as usize],
			_ => return self.eval_complex(code, record),
		})
	}
	fn test(&mut self, code: &ExprCode, record: usize) -> Result<bool> {
		Ok(match &self.plan.program.exprs[code.index()] {
			Expr::Constant(value) => value.yes(),
			Expr::NameEq(id) => {
				if let Datum::Event(i) = self.records[record].event {
					match self.events[i] {
						Event::Element { id: actual, .. } => actual == Some(*id),
						_ => {
							let name = self.event_field(record, Key::Name);
							self.equal(&name, &Datum::Text(*id))
						}
					}
				} else {
					false
				}
			}
			Expr::Member { needle, strings } => {
				let needle = match &self.plan.program.exprs[needle.index()] {
					Expr::Get { base, path } if path.len == 1 => self.get1(*base, path.start, record)?,
					_ => self.eval(needle, record)?,
				};
				let strings = &self.plan.program.sets[strings.indices()];
				if let Datum::Text(id) = needle {
					strings.binary_search_by_key(&id.index(), |s| s.index()).is_ok()
				} else {
					self.text_of(&needle).is_some_and(|text| {
						strings
							.iter()
							.any(|id| self.plan.program.strings[id.index() as usize].as_ref() == text)
					})
				}
			}
			Expr::Compare { relation, left, right } => {
				let left = self.eval(left, record)?;
				match relation {
					Relation::Present => left != Datum::Missing,
					Relation::Equal => {
						let right = self.eval(right.as_ref().unwrap(), record)?;
						self.equal(&left, &right)
					}
					Relation::Less => {
						let right = self.eval(right.as_ref().unwrap(), record)?;
						matches!((left,right),(Datum::Number(a),Datum::Number(b)) if a<b)
					}
				}
			}
			Expr::Not(value) => !self.test(value, record)?,
			Expr::And(left, right) => self.test(left, record)? && self.test(right, record)?,
			Expr::Or(left, right) => self.test(left, record)? || self.test(right, record)?,
			Expr::Choose { condition, yes, no } => {
				let condition = self.test(condition, record)?;
				return self.test(if condition { yes } else { no }, record);
			}
			Expr::Exists(list) => {
				let mut found = false;
				self.eval_list(list, record, &mut |_, _| {
					found = true;
					Ok(false)
				})?;
				found
			}
			_ => self.eval(code, record)?.yes(),
		})
	}
	fn base_value(&mut self, base: Base, record: usize) -> Result<Datum> {
		Ok(match base {
			Base::Value(v) => self.eval(&v, record)?,
			Base::Record => Datum::Record(record),
			Base::Event => self.records[record].event,
			Base::Owner => self.records[record].owner.map_or(Datum::Missing, Datum::Record),
			Base::Ancestors => Datum::Ancestors(record),
			Base::Incoming => Datum::Incoming(record),
			Base::Scopes => Datum::Scopes(record),
			Base::Iteration => self
				.iteration
				.len()
				.checked_sub(1)
				.map_or(Datum::Missing, Datum::Iteration),
			Base::Slot(Slot::Record(i)) => self.slots[self.records[record].slots + i as usize],
			Base::Slot(Slot::Iteration(i)) => self
				.iteration
				.last()
				.map_or(Datum::Missing, |r| self.iteration_slots[r.start as usize + i as usize]),
			Base::Region(i) => Datum::Region(record as u32, i),
			Base::Binding(i) => self.bindings[self.bindings.len() - 1 - i as usize],
			Base::Missing | Base::Slot(Slot::Missing) => Datum::Missing,
		})
	}
	fn type_in(&mut self, id: NodeId, strings: &[StrId]) -> bool {
		let ty = match self.tree().node(id).kind {
			NodeKind::Host(i) => self.plan_text(self.tree().hosts[i as usize].ty),
			_ => self.native_property(id, Key::Type),
		};
		match ty {
			Datum::Text(id) => strings.binary_search_by_key(&id.index(), |s| s.index()).is_ok(),
			other => self.text_of(&other).is_some_and(|text| {
				strings
					.iter()
					.any(|id| self.plan.program.strings[id.index() as usize].as_ref() == text)
			}),
		}
	}
	fn get1(&mut self, base: Base, path: u32, record: usize) -> Result<Datum> {
		let value = self.base_value(base, record)?;
		let part = &self.plan.program.paths[path as usize];
		if let (Path::Name(prop), Datum::Node(id)) = (part, value)
			&& prop.key == Key::Type
			&& let NodeKind::Host(i) = self.tree().node(id).kind
		{
			return Ok(self.plan_text(self.tree().hosts[i as usize].ty));
		}
		Ok(self.property(value, part))
	}
	fn eval_complex(&mut self, code: &ExprCode, record: usize) -> Result<Datum> {
		use program::Expr as V;
		Ok(match &self.plan.program.exprs[code.index()] {
			V::Slot(i) => self.slots[self.records[record].slots + *i as usize],
			V::Iteration(i) => self
				.iteration
				.last()
				.map_or(Datum::Missing, |r| self.iteration_slots[r.start as usize + *i as usize]),
			V::Event(key) => self.event_field(record, *key),
			V::RecordType => Datum::Text(self.records[record].ty),
			V::Not(_) | V::And(..) | V::Or(..) | V::NameEq(_) | V::Exists(_) | V::Member { .. } | V::Compare { .. } => {
				Datum::Bool(self.test(code, record)?)
			}
			V::Constant(value) => *value,
			V::Get { base, path } => {
				let mut value = self.base_value(*base, record)?;
				for part in &self.plan.program.paths[path.indices()] {
					value = self.property(value, part);
				}
				value
			}
			V::Choose { condition, yes, no } => {
				let condition = self.test(condition, record)?;
				self.eval(if condition { yes } else { no }, record)?
			}
			V::FlatMap { .. } | V::Filter { .. } | V::TypeFilter { .. } | V::Concat(_) => {
				let mut out = self.take_values();
				self.eval_list(code, record, &mut |_, value| {
					out.push(value);
					Ok(true)
				})?;
				self.array(out)
			}
			V::Length(list) => {
				let mut count = 0;
				self.eval_list(list, record, &mut |_, _| {
					count += 1;
					Ok(true)
				})?;
				Datum::Number(count as f64)
			}
			V::At { list, index } => {
				let index = self.eval(index, record)?;
				let mut out = Datum::Missing;
				if let Datum::Number(index) = index
					&& index >= 0.0 && index.fract() == 0.0
				{
					let mut n = 0;
					self.eval_list(list, record, &mut |_, value| {
						if n == index as usize {
							out = value;
							return Ok(false);
						}
						n += 1;
						Ok(true)
					})?;
				}
				out
			}
			V::Construct(Construct::Array(items)) => {
				let mut values = self.take_values();
				for item in &self.plan.program.args[items.indices()] {
					values.push(self.eval(item, record)?);
				}
				self.array(values)
			}
			V::Construct(Construct::Record {
				node_type,
				fields,
				span,
			}) => {
				let fields = &self.plan.program.fields[fields.indices()];
				let span = self.eval(span, record)?;
				let (start, end) = match span {
					Datum::Span(a, b, _) => (a, b),
					_ => (self.at, self.at),
				};
				let mut values = self.take_values();
				for (_, expr) in fields {
					values.push(self.eval(expr, record)?);
				}
				let ty = &self.plan.program.strings[node_type.index() as usize];
				let node = if ty.starts_with("js.") {
					let output: Vec<_> = fields
						.iter()
						.zip(&values)
						.map(|((name, _), value)| (self.plan.program.properties[name.index() as usize], *value))
						.collect();
					let kind = self.native_construct(ty, start, end, &output)?;
					self.ast().add(kind, start, end)
				} else {
					let len = values.iter().filter(|v| **v != Datum::Missing).count();
					let from = self.tree().host_fields.len();
					self.ast().host_fields.resize(from + len, ("", Value::Null));
					let mut n = from;
					for ((name, _), value) in fields.iter().zip(&values) {
						if *value == Datum::Missing {
							continue;
						}
						let key = self.plan.program.names[name.index() as usize];
						let value = self.output(value)?;
						self.ast().host_fields[n] = (key, value);
						n += 1;
					}
					self.host_id(*node_type, start, end, from, len, span != Datum::Null)
				};
				self.recycle_values(values);
				Datum::Node(node)
			}
		})
	}
	fn eval_list(
		&mut self,
		code: &ExprCode,
		record: usize,
		emit: &mut dyn FnMut(&mut Self, Datum) -> Result<bool>,
	) -> Result<bool> {
		use program::Expr as V;
		match &self.plan.program.exprs[code.index()] {
			V::Concat(items) => {
				for item in &self.plan.program.args[items.indices()] {
					if !self.eval_list(item, record, emit)? {
						return Ok(false);
					}
				}
				Ok(true)
			}

			V::TypeFilter { list, strings, negate } => self.eval_list(list, record, &mut |this, item| {
				let keep = match item {
					Datum::Node(id) => {
						let strings = &this.plan.program.sets[strings.indices()];
						this.type_in(id, strings) != *negate
					}
					_ => false,
				};
				if keep { emit(this, item) } else { Ok(true) }
			}),
			V::Filter { list, predicate } => self.eval_list(list, record, &mut |this, item| {
				this.bindings.push(item);
				let yes = this.test(predicate, record);
				this.bindings.pop();
				if yes? { emit(this, item) } else { Ok(true) }
			}),

			V::FlatMap { list, body } => self.eval_list(list, record, &mut |this, item| {
				this.bindings.push(item);
				let result = this.eval_list(body, record, emit);
				this.bindings.pop();
				result
			}),
			V::Choose { condition, yes, no } => {
				let condition = self.test(condition, record)?;
				self.eval_list(if condition { yes } else { no }, record, emit)
			}
			V::Construct(Construct::Array(items)) => {
				for item in &self.plan.program.args[items.indices()] {
					let item = self.eval(item, record)?;
					if !emit(self, item)? {
						return Ok(false);
					}
				}
				Ok(true)
			}
			_ => {
				let list = self.eval(code, record)?;
				if let Datum::Nodes(nodes) = list {
					for i in 0..nodes.len {
						let item = self.tree().nth(nodes, i).map_or(Datum::Null, Datum::Node);
						if !emit(self, item)? {
							return Ok(false);
						}
					}
					return Ok(true);
				}
				for i in 0..self.count(&list) {
					let item = self.item(&list, i);
					if !emit(self, item)? {
						return Ok(false);
					}
				}
				Ok(true)
			}
		}
	}
	fn output(&mut self, value: &Datum) -> Result<Value> {
		Ok(match *value {
			Datum::Missing | Datum::Null | Datum::Scopes(_) | Datum::Region(..) | Datum::Incoming(_) => Value::Null,
			Datum::Bool(v) => Value::Bool(v),
			Datum::Number(v) if v >= 0.0 && v <= u32::MAX as f64 && v.fract() == 0.0 => Value::Int(v as u32),
			Datum::Number(_) => return fail(self.at, self.at, Code::Expected, Some("a representable host number")),
			Datum::Text(id) => Value::Str(self.tree_id(id)),
			Datum::Interned(id) => Value::Str(id),
			Datum::Slice(a, b) => Value::Slice(a, b),
			Datum::Node(id) => Value::Node(id),
			Datum::Nodes(list) => Value::Nodes(list),
			Datum::Strings(a, n) => Value::Strs(a, n),

			Datum::Array(..) | Datum::Constants(..) => {
				let count = self.count(value);
				if count > 0 && (0..count).all(|i| self.text_of(&self.item(value, i)).is_some()) {
					let start = self.tree().host_strings.len() as u32;
					for i in 0..count {
						let item = self.item(value, i);
						let id = self.string(item);
						self.ast().host_strings.push(id);
					}
					Value::Strs(start, count as u32)
				} else if (0..count)
					.all(|i| matches!(self.item(value, i), Datum::Node(_) | Datum::Null | Datum::Missing))
				{
					let start = self.tree().lists.len() as u32;
					for i in 0..count {
						let node = match self.item(value, i) {
							Datum::Node(id) => Some(id),
							_ => None,
						};
						self.ast().lists.push(node);
					}
					Value::Nodes(List {
						start,
						len: count as u32,
					})
				} else {
					return fail(self.at, self.at, Code::Expected, Some("a representable host array"));
				}
			}
			Datum::Object(i) => {
				let fields = &self.plan.program.objects[i];
				let start = self.tree().host_fields.len();
				self.ast().host_fields.resize(start + fields.len(), ("", Value::Null));
				for (n, (prop, value)) in fields.iter().enumerate() {
					let key = self.plan.program.names[prop.name.index() as usize];
					let value = self.output(value)?;
					self.ast().host_fields[start + n] = (key, value);
				}
				Value::Node(self.host_id(Symbol::Empty.id(), self.at, self.at, start, fields.len(), false))
			}
			Datum::Record(rec) => self.records[rec].node.map_or(Value::Null, Value::Node),
			Datum::Span(a, b, _) => {
				let keys = self.plan.program.keys;
				Value::Node(self.make(
					Symbol::Empty.id(),
					a,
					b,
					&[
						(keys.start, Datum::Number(a as f64)),
						(keys.end, Datum::Number(b as f64)),
					],
					false,
				)?)
			}
			Datum::Regex(pattern, flags) => {
				let keys = self.plan.program.keys;
				Value::Node(self.make(
					Symbol::Empty.id(),
					self.at,
					self.at,
					&[
						(keys.pattern, Datum::Interned(pattern)),
						(keys.flags, Datum::Interned(flags)),
					],
					false,
				)?)
			}
			Datum::Template(raw, cooked) => {
				let keys = self.plan.program.keys;
				Value::Node(self.make(
					Symbol::Empty.id(),
					self.at,
					self.at,
					&[
						(keys.raw, Datum::Interned(raw)),
						(keys.cooked, cooked.map_or(Datum::Null, Datum::Interned)),
					],
					false,
				)?)
			}
			Datum::Stylesheet(i) => {
				let Event::Stylesheet(children, comments) = self.events[i] else {
					unreachable!()
				};
				let keys = self.plan.program.keys;
				Value::Node(self.make(
					Symbol::Empty.id(),
					self.at,
					self.at,
					&[
						(keys.children, Datum::Nodes(children)),
						(keys.comments, Datum::Nodes(comments)),
					],
					false,
				)?)
			}
			_ => Value::Null,
		})
	}
	fn string(&mut self, value: Datum) -> StrId {
		match value {
			Datum::Interned(id) => id,
			Datum::Text(id) => self.tree_id(id),
			Datum::Slice(a, b) => self.intern(&self.src[a as usize..b as usize]),
			_ => self.intern(""),
		}
	}
	fn host_id(&mut self, ty: StrId, start: u32, end: u32, from: usize, len: usize, span: bool) -> NodeId {
		let index = self.tree().hosts.len() as u32;
		let ty = self.plan.program.names[ty.index() as usize];
		self.ast().hosts.push(Host {
			ty,
			scope: None,
			fields: (from as u32, len as u32),
			span,
		});
		self.ast().add(NodeKind::Host(index), start, end)
	}
	fn make(&mut self, ty: StrId, start: u32, end: u32, fields: &[(StrId, Datum)], span: bool) -> Result<NodeId> {
		let from = self.tree().host_fields.len();
		for (key, value) in fields {
			if *value == Datum::Missing {
				continue;
			}
			let value = self.output(value)?;
			let key = self.plan.program.names[key.index() as usize];
			self.ast().host_fields.push((key, value));
		}
		let len = self.tree().host_fields.len() - from;
		Ok(self.host_id(ty, start, end, from, len, span))
	}
	fn call(&mut self, rule: usize, event: Datum, ty: Option<StrId>, follow: &str) -> Result<NodeId> {
		self.call_form(rule, event, ty, follow)
			.map_err(|error| error.boxed(self.plan))
	}
	fn call_form(
		&mut self,
		rule: usize,
		event: Datum,
		ty: Option<StrId>,
		follow: &str,
	) -> std::result::Result<NodeId, Rejection> {
		if self.active.len() >= 64 {
			return fail(self.at, self.at, Code::NestingDepth, None).map_err(Into::into);
		}
		let schema = &self.plan.program.rules[rule];
		let record = self.records.len();
		let slots = self.slots.len();
		if let Some(leaf) = &schema.leaf {
			return Ok(self.leaf(rule, record, event, ty, leaf));
		}
		self.slots.resize(slots + schema.slots, Datum::Missing);
		let owner = self
			.elements
			.iter()
			.rev()
			.find(|e| e.record != record)
			.map(|e| e.record);
		let ty_id = ty.unwrap_or(schema.ty);
		let start = self.at;
		self.records.push(Record {
			failure: None,
			body_end: None,
			children_end: None,
			aborted: false,
			rule,
			ty: ty_id,
			slots,
			event,
			owner,
			start,
			node: None,
		});
		self.active.push(record);
		let result = self.form(
			if self.options.error_recovery {
				&schema.form
			} else {
				&schema.strict
			},
			record,
			follow,
		);
		self.active.pop();
		result.map_err(|error| {
			self.records[record]
				.failure
				.take()
				.filter(|i| self.failures[*i].pos() > error.pos())
				.map_or(error, |i| self.failures[i].clone())
		})?;
		if self
			.elements
			.last()
			.is_some_and(|e| e.record == record && e.content == Mode::Raw)
		{
			let raw = self.event_field(record, Key::RawChildren);
			if let Datum::Span(_, end, _) = raw
				&& self.at <= end
			{
				self.at = end;
				let element = self.elements.last().unwrap();
				let name = &self.src[element.name.0 as usize..element.name.1 as usize];
				if let Some(len) = closing_tag(self.rest(), name) {
					self.at += len as u32;
				} else {
					self.report(error(self.at, self.at, Code::Unclosed, Some(name)))?;
				}
			}
		}
		let start = self.records[record].start;
		let end = if schema.span == Some(SpanPolicy::ThroughNextTokenStart) {
			let at = self.at;
			self.space();
			let end = self.at;
			self.at = at;
			end
		} else {
			self.at
		};
		let ty = &self.plan.program.strings[ty_id.index() as usize];
		let span = schema.span != Some(SpanPolicy::None);
		let node = if ty.starts_with("js.") {
			for (i, (_, absence)) in schema.fields.iter().enumerate() {
				if self.slots[slots + i] == Datum::Missing && *absence == Absence::Null {
					self.slots[slots + i] = Datum::Null;
				}
			}
			let fields: Vec<_> = schema
				.fields
				.iter()
				.enumerate()
				.map(|(i, (k, _))| (self.plan.program.properties[k.index() as usize], self.slots[slots + i]))
				.collect();
			let kind = self.native_construct(ty, start, end, &fields)?;
			self.ast().add(kind, start, end)
		} else {
			let from = self.tree().host_fields.len();
			for (i, (key, absence)) in schema.fields.iter().enumerate() {
				let value = match self.slots[slots + i] {
					Datum::Missing if *absence == Absence::Null => Value::Null,
					Datum::Missing => continue,
					Datum::Node(id) => Value::Node(id),
					Datum::Nodes(list) => Value::Nodes(list),
					Datum::Null => Value::Null,
					Datum::Slice(a, b) => Value::Slice(a, b),
					Datum::Bool(v) => Value::Bool(v),
					value => self.output(&value)?,
				};
				let key = self.plan.program.names[key.index() as usize];
				self.ast().host_fields.push((key, value));
			}
			let len = self.tree().host_fields.len() - from;
			self.host_id(ty_id, start, end, from, len, span)
		};
		self.records[record].node = Some(node);
		Ok(node)
	}
	fn leaf(&mut self, rule: usize, record: usize, event: Datum, ty: Option<StrId>, leaf: &[(u32, Key)]) -> NodeId {
		let schema = &self.plan.program.rules[rule];
		let ty_id = ty.unwrap_or(schema.ty);
		let start = self.at;
		let slots = self.slots.len();
		let owner = self.elements.last().map(|e| e.record);
		self.records.push(Record {
			failure: None,
			body_end: None,
			children_end: None,
			aborted: false,
			rule,
			ty: ty_id,
			slots,
			event,
			owner,
			start,
			node: None,
		});
		let from = self.tree().host_fields.len();
		for (i, (key, absence)) in schema.fields.iter().enumerate() {
			let value =
				leaf.iter()
					.find(|(slot, _)| *slot as usize == i)
					.map_or(Datum::Missing, |(_, key)| match event {
						Datum::Event(e) => self.event_value(e, *key),
						_ => Datum::Missing,
					});
			let value = match value {
				Datum::Missing if *absence == Absence::Null => Value::Null,
				Datum::Missing => continue,
				Datum::Node(id) => Value::Node(id),
				Datum::Nodes(list) => Value::Nodes(list),
				Datum::Slice(a, b) => Value::Slice(a, b),
				Datum::Text(id) => Value::Str(self.tree_id(id)),
				Datum::Interned(id) => Value::Str(id),
				Datum::Bool(v) => Value::Bool(v),
				Datum::Strings(a, n) => Value::Strs(a, n),
				value => self.output(&value).unwrap_or(Value::Null),
			};
			let key = self.plan.program.names[key.index() as usize];
			self.ast().host_fields.push((key, value));
		}
		let len = self.tree().host_fields.len() - from;
		let span = schema.span != Some(SpanPolicy::None);
		let node = self.host_id(ty_id, start, self.at, from, len, span);
		self.records[record].node = Some(node);
		node
	}
	fn leading_mismatch(&self, form: &Form) -> Option<Rejection> {
		self.leading(form, &mut self.at.clone(), 0).err()
	}
	fn leading(&self, form: &Form, at: &mut u32, depth: usize) -> std::result::Result<bool, Rejection> {
		if depth >= 256 {
			return Ok(false);
		}
		match form {
			Form::Seq(items) => {
				for item in items {
					if !self.leading(item, at, depth + 1)? {
						return Ok(false);
					}
				}
			}
			Form::Emit { .. } => {}
			Form::Tokens(tokens) => {
				for token in tokens {
					match token {
						Token::Text { text, gap, word, .. } => self.token_here(at, *text, *text, *gap, *word)?,
						Token::Space { min, .. } => self.space_here(at, *min)?,
					}
				}
			}
			Form::Read {
				reader, input: None, ..
			} => match &self.plan.program.readers[*reader as usize] {
				Reader::Token {
					text,
					expected,
					gap,
					word,
				} => self.token_here(at, *text, *expected, *gap, *word)?,
				Reader::Space { min } => self.space_here(at, *min)?,
				Reader::Rule(rule) => return self.leading(&self.plan.program.rules[*rule].strict, at, depth + 1),
				_ => return Ok(false),
			},
			Form::Repeat(repeat) if repeat.min > 0 => return self.leading(&repeat.body, at, depth + 1),
			_ => return Ok(false),
		}
		Ok(true)
	}
	fn space_here(&self, at: &mut u32, min: usize) -> std::result::Result<(), Rejection> {
		let start = *at;
		let rest = &self.src[start as usize..self.limit as usize];
		*at = start + (rest.len() - rest.trim_start_matches(is_space).len()) as u32;
		if (*at - start) < min as u32 {
			return Err(Rejection::Space(start, start));
		}
		Ok(())
	}
	fn token_here(
		&self,
		at: &mut u32,
		text: StrId,
		expected: StrId,
		gap: Gap,
		word: bool,
	) -> std::result::Result<(), Rejection> {
		let text = &self.plan.program.strings[text.index() as usize];
		if gap == Gap::Space {
			let rest = &self.src[*at as usize..self.limit as usize];
			*at += (rest.len() - rest.trim_start_matches(is_space).len()) as u32;
		}
		let rest = &self.src[*at as usize..self.limit as usize];
		if !rest.starts_with(text.as_ref()) {
			return Err(Rejection::Expected(*at, *at, expected));
		}
		let end = *at + text.len() as u32;
		if word && rest[text.len()..].starts_with(is_id_continue) {
			return Err(Rejection::Space(end, end));
		}
		*at = end;
		Ok(())
	}
	fn remember(&mut self, record: usize, error: Rejection) {
		if self.records[record]
			.failure
			.is_none_or(|i| self.failures[i].pos() < error.pos())
		{
			let i = self.failures.len();
			self.failures.push(error);
			self.records[record].failure = Some(i);
		}
	}
	#[inline]
	fn event_field(&mut self, record: usize, key: Key) -> Datum {
		if let Datum::Event(i) = self.records[record].event {
			self.event_value(i, key)
		} else {
			Datum::Missing
		}
	}
	fn event_value(&mut self, i: usize, key: Key) -> Datum {
		match self.events[i] {
			Event::Stylesheet(children, comments) => match key {
				Key::Children => Datum::Nodes(children),
				Key::Comments => Datum::Nodes(comments),
				_ => Datum::Missing,
			},
			Event::Text { raw, decoded } => match key {
				Key::Raw => raw,
				Key::Decoded => decoded,
				_ => Datum::Missing,
			},
			Event::Comment(data) => {
				if key == Key::Data {
					data
				} else {
					Datum::Missing
				}
			}
			Event::Element {
				name,
				header,
				after_name,
				at_document,
				raw,
				..
			} => match key {
				Key::Name => Datum::Slice(name.0, name.1),
				Key::NameFacts => Datum::NameFacts(i),
				Key::AtDocument => Datum::Bool(at_document),
				Key::Header => match header {
					Some(run) => Datum::Header(run),
					None => {
						let (at, limit) = (self.at, self.limit);
						self.at = after_name;
						let run = self.scan_header().unwrap_or(Run { start: 0, len: 0 });
						(self.at, self.limit) = (at, limit);
						if let Event::Element { header, .. } = &mut self.events[i] {
							*header = Some(run);
						}
						Datum::Header(run)
					}
				},
				Key::RawChildren => raw,
				_ => Datum::Missing,
			},
			Event::Directive {
				name,
				raw,
				argument,
				modifiers,
				value,
				quoted,
			} => match key {
				Key::Name => Datum::Slice(name.0, name.1),
				Key::RawName => Datum::Slice(raw.0, raw.1),
				Key::Argument => argument,
				Key::Modifiers => Datum::Strings(modifiers.start, modifiers.len),
				Key::Value => value,
				Key::Quoted => Datum::Bool(quoted),
				_ => Datum::Missing,
			},
		}
	}
	fn structural_stop(&self) -> bool {
		let tail = self
			.rest()
			.strip_prefix(self.plan.html.delimiters[0].as_ref())
			.unwrap_or("");
		if tail.starts_with("/*") || tail.starts_with("//") {
			return false;
		}
		self.plan.stops.iter().any(|prefix| self.matches(prefix))
	}

	fn strict(&mut self, form: &Form, record: usize, follow: &str) -> std::result::Result<(), Rejection> {
		let recover = std::mem::replace(&mut self.options.error_recovery, false);
		let result = self.form(form, record, follow);
		self.options.error_recovery = recover;
		result
	}

	fn form(&mut self, form: &Form, record: usize, follow: &str) -> std::result::Result<(), Rejection> {
		if self.records[record].aborted && !matches!(form, Form::Seq(_) | Form::Emit { .. }) {
			return Ok(());
		}
		match form {
			Form::Tokens(tokens) => {
				for token in tokens {
					self.token(token, record)?;
				}
			}
			Form::Seq(items) => {
				for item in items {
					self.form(item, record, follow)?;
				}
			}
			Form::Emit { into, value } => {
				let value = self.eval(value, record)?;
				self.write(record, into, value);
			}
			Form::Read {
				reader,
				into,
				input,
				follow: local,
			} => {
				self.read_form(
					&self.plan.program.readers[*reader as usize],
					record,
					into,
					input,
					&self.plan.program.follows[*local as usize],
					follow,
				)?;
			}

			Form::Choice(choice) => {
				if choice.disjoint {
					let selected = choice
						.first
						.iter()
						.position(|prefixes| {
							prefixes.iter().any(|prefix| {
								let rest = if prefix.tight {
									self.rest()
								} else {
									self.rest().trim_start_matches(is_space)
								};
								rest.starts_with(&prefix.text)
									&& (!prefix.word || !rest[prefix.text.len()..].starts_with(is_id_continue))
							})
						})
						.ok_or(Rejection::Expected(self.at, self.at, choice.expected))?;
					return self.form(&choice.alternatives[selected], record, follow);
				}

				let mut failure = None;
				let mut selected = None;
				for alternative in &choice.alternatives {
					if let Some(error) = self.leading_mismatch(alternative) {
						if failure
							.as_ref()
							.is_none_or(|prior: &Rejection| prior.pos() < error.pos())
						{
							failure = Some(error);
						}
					} else {
						selected = Some(alternative);
						break;
					}
				}
				if let Some(selected) = selected {
					self.form(selected, record, follow)?;
				} else {
					return Err(failure.unwrap());
				}
			}

			Form::Repeat(repeat) => {
				let Repeat {
					body,
					min,
					max,
					locals,
					yield_value,
					into,
				} = repeat.as_ref();
				let mut values = self.take_values();

				while max.is_none_or(|max| values.len() < max) {
					if self.leading_mismatch(body).is_some() {
						break;
					}
					let start = self.at;
					self.push_iteration(*locals);
					let result = self
						.strict(body, record, follow)
						.and_then(|()| self.eval(yield_value, record).map_err(Into::into));
					self.pop_iteration();
					match result {
						Ok(value) => {
							values.push(value);
						}
						Err(error) => {
							self.at = start;
							if values.len() < *min {
								if !self.options.error_recovery {
									return Err(error);
								}
								self.push_iteration(*locals);
								self.form(body, record, follow)?;
								values.push(self.eval(yield_value, record)?);
								self.pop_iteration();
								continue;
							}
							self.remember(record, error);
							break;
						}
					}
					if self.at == start && max.is_none() {
						return fail(start, start, Code::TreeSize, None).map_err(Into::into);
					}
				}

				if values.len() < *min {
					return fail(self.at, self.at, Code::UnexpectedToken, None).map_err(Into::into);
				}
				let value = self.array(values);
				self.write(record, into, value);
			}
		}
		Ok(())
	}

	fn token(&mut self, token: &Token, record: usize) -> std::result::Result<(), Rejection> {
		let (start, into) = match token {
			Token::Text { text, gap, word, into } => {
				let id = *text;
				let text = &self.plan.program.strings[id.index() as usize];
				if *gap == Gap::Space {
					self.space();
				}
				let start = self.at;
				if !self.matches(text) {
					return Err(Rejection::Expected(start, start, id));
				}
				let end = start + text.len() as u32;
				if *word && self.rest()[text.len()..].starts_with(is_id_continue) {
					return Err(Rejection::Space(end, end));
				}
				self.at = end;
				(start, into)
			}
			Token::Space { min, into } => {
				let start = self.at;
				self.space();
				if self.at - start < *min as u32 {
					return Err(Rejection::Space(start, start));
				}
				(start, into)
			}
		};
		if let Some(into) = into {
			self.write(record, into, Datum::Span(start, self.at, false));
		}
		Ok(())
	}
	fn read_form(
		&mut self,
		reader: &Reader,
		record: usize,
		into: &Option<Slot>,
		input: &Option<ExprCode>,
		local: &str,
		follow: &str,
	) -> std::result::Result<(), Rejection> {
		if local.is_empty() || !matches!(reader, Reader::Rule(_) | Reader::Javascript { .. }) {
			return self.read_value(reader, record, into, input, follow);
		}
		if follow.is_empty() {
			return self.read_value(reader, record, into, input, local);
		}
		let mut joined = self.follows.pop().unwrap_or_default();
		joined.push_str(local);
		joined.push(' ');
		joined.push_str(follow);
		let result = self.read_value(reader, record, into, input, &joined);
		joined.clear();
		self.follows.push(joined);
		result
	}
	fn read_value(
		&mut self,
		reader: &Reader,
		record: usize,
		into: &Option<Slot>,
		input: &Option<ExprCode>,
		follow: &str,
	) -> std::result::Result<(), Rejection> {
		if input.is_none() && !self.options.error_recovery {
			match reader {
				Reader::Token {
					text,
					expected,
					gap,
					word,
				} => {
					let text = &self.plan.program.strings[text.index() as usize];
					if *gap == Gap::Space {
						self.space();
					}
					if *word && self.matches(text) && self.rest()[text.len()..].starts_with(is_id_continue) {
						let end = self.at + text.len() as u32;
						return Err(Rejection::Space(end, end));
					}
					let start = self.at;
					if !self.eat(text) {
						return Err(Rejection::Expected(self.at, self.at, *expected));
					}
					if let Some(into) = into {
						self.write(record, into, Datum::Span(start, self.at, false));
					}
					return Ok(());
				}
				Reader::Space { min } => {
					let start = self.at;
					self.space();
					if self.at - start < *min as u32 {
						return Err(Rejection::Space(start, start));
					}
					if let Some(into) = into {
						self.write(record, into, Datum::Span(start, self.at, false));
					}
					return Ok(());
				}
				_ => {}
			}
		}
		let input = input.as_ref().map(|v| self.eval(v, record)).transpose()?;
		let value = if input.is_none()
			&& let Reader::Rule(rule) = reader
		{
			let child = self.records.len();
			let event = match self.records[record].event {
				Datum::Event(i) => self.event(self.events[i]),
				other => other,
			};
			let node = self.call_form(*rule, event, None, follow)?;
			if let Some(end) = self.records[child].children_end {
				self.records[record].body_end = Some(end);
			}
			Datum::Node(node)
		} else if input.is_none()
			&& let Reader::Javascript { entry, boundary } = reader
		{
			self.javascript(*entry, *boundary, follow)?
		} else {
			self.read(reader, record, input, follow)?
		};
		if let Some(into) = into {
			self.write(record, into, value);
		}
		Ok(())
	}
	fn push_iteration(&mut self, len: usize) {
		let start = self.iteration_slots.len();
		self.iteration_slots.resize(start + len, Datum::Missing);
		self.iteration.push(Run {
			start: start as u32,
			len: len as u32,
		});
	}
	fn pop_iteration(&mut self) {
		let run = self.iteration.pop().unwrap();
		self.iteration_slots.truncate(run.start as usize);
	}
	fn read(&mut self, reader: &Reader, record: usize, input: Option<Datum>, follow: &str) -> Result<Datum> {
		let saved = (self.at, self.limit);
		if let Some(input) = &input {
			let Datum::Span(a, b, _) = input else {
				return fail(self.at, self.at, Code::Expected, Some("an input span"));
			};
			self.at = *a;
			self.limit = *b;
		}
		let result = (|| {
			Ok(match reader {
				Reader::Token { text, gap, word, .. } => {
					let text = &self.plan.program.strings[text.index() as usize];
					if *gap == Gap::Space {
						self.space();
					}
					if *word && self.matches(text) && self.rest()[text.len()..].starts_with(is_id_continue) {
						let end = self.at + text.len() as u32;
						return fail(end, end, Code::Expected, Some("whitespace"));
					}
					let start = self.at;
					self.expect(text)?;
					Datum::Span(start, self.at, false)
				}
				Reader::Space { min } => {
					let start = self.at;
					self.space();
					if (self.at - start) < *min as u32 {
						self.report(error(start, start, Code::Expected, Some("whitespace")))?;
					}
					Datum::Span(start, self.at, false)
				}
				Reader::Test(value) => {
					if !self.test(value, record)? {
						return fail(self.at, self.at, Code::UnexpectedToken, None);
					}
					Datum::Missing
				}
				Reader::Rule(rule) => {
					let child = self.records.len();
					let node = self.call(*rule, self.records[record].event, None, follow)?;
					if let Some(end) = self.records[child].children_end {
						self.records[record].body_end = Some(end);
					}
					Datum::Node(node)
				}
				Reader::Javascript { entry, boundary } => self
					.javascript(*entry, *boundary, follow)
					.map_err(|error| error.boxed(self.plan))?,
				Reader::HtmlChildren { mode, stop } => {
					let nodes = self.children(*mode, stop)?;
					if matches!(stop.as_ref(), Stop::Prefixes(_)) {
						self.records[record].children_end = Some(self.at);
					}
					Datum::Nodes(nodes)
				}
				Reader::HtmlAttributes(mode) => Datum::Nodes(self.attributes(*mode, record)?),
				Reader::HtmlAttributeParts => self.attribute_parts(record)?,
				Reader::HtmlSingle(entry) => self.single(*entry)?,
				Reader::CssStylesheet => self.stylesheet()?,
			})
		})();
		let result = if result.is_ok() && input.is_some() {
			self.space();
			if self.at != self.limit {
				if self.options.error_recovery {
					self.report(error(self.at, self.at, Code::UnexpectedToken, None))?;
					self.at = self.limit;
					result
				} else {
					fail(self.at, self.at, Code::UnexpectedToken, None)
				}
			} else {
				result
			}
		} else {
			result
		};
		if input.is_some() {
			(self.at, self.limit) = saved;
		}
		result
	}
	fn javascript(
		&mut self,
		entry: Js,
		boundary: Option<Boundary>,
		follow: &str,
	) -> std::result::Result<Datum, Rejection> {
		let excluded = |s: &str| matches!(s, "(" | "[" | "." | "?." | "?");
		if entry != Js::Expression || !follow.split_ascii_whitespace().any(excluded) {
			return self.native_read(entry, boundary, follow);
		}
		let mut stops = self.follows.pop().unwrap_or_default();
		for stop in follow.split_ascii_whitespace().filter(|s| !excluded(s)) {
			if !stops.is_empty() {
				stops.push(' ');
			}
			stops.push_str(stop);
		}
		let result = self.native_read(entry, boundary, &stops);
		stops.clear();
		self.follows.push(stops);
		result
	}
	fn native_read(
		&mut self,
		entry: Js,
		boundary: Option<Boundary>,
		follow: &str,
	) -> std::result::Result<Datum, Rejection> {
		self.native_reads += 1;
		let at = self.at;
		let src = &self.src[..self.limit as usize];
		let mut options = self.options;
		options.allow_undeclared_exports = true;
		let mut parser = Parser::<E>::new(src, at, options, follow, self.ast.take().unwrap());
		let result = parser.start().and_then(|()| match entry {
			Js::Program => parser.parse_program().map(|id| parser.list_of(&[id])),
			Js::AssignmentExpression => {
				parser.enter_scope(crate::parser::scope::SCOPE_TOP);
				parser
					.parse_maybe_assign(ForInit::No, &mut None)
					.map(|id| parser.list_of(&[id]))
			}
			Js::BindingIdentifier | Js::IdentifierReference => {
				if !matches!(
					parser.tok.kind,
					crate::lexer::token::TokenKind::Ident(_) | crate::lexer::token::TokenKind::Keyword(_)
				) {
					return fail(
						parser.tok.start,
						parser.tok.start,
						Code::Expected,
						Some("an identifier"),
					);
				}
				let end = parser.tok.end;
				parser.parse_ident(false).map(|id| parser.list_of(&[id])).map_err(|e| {
					if e.code == Code::UnexpectedKeyword {
						error(e.pos, end, Code::ReservedWord, Some(&src[e.pos as usize..end as usize]))
					} else {
						e
					}
				})
			}
			_ => parser.read_entry_boundary(
				match entry {
					Js::Expression => Entry::Expression,
					Js::Pattern => Entry::Pattern,
					Js::Params => Entry::Params,
					Js::TypeParameters => Entry::TypeParameters,
					Js::Statement => Entry::Statement,
					_ => unreachable!(),
				},
				boundary == Some(Boundary::LastSharedWord),
			),
		});
		let result = match result {
			Err(error) if parser.recovering() => {
				let pos = error.pos;
				parser.record(Err(error)).unwrap();
				parser.skip_to_end();
				parser.prev_end = parser.prev_end.max(pos);
				if entry == Js::Params {
					Ok(List::EMPTY)
				} else {
					let name = parser.intern("");
					let node = parser.add_with_end(NodeKind::Identifier { name }, at, parser.consumed_end().max(at));
					Ok(parser.list_of(&[node]))
				}
			}
			result => result,
		};
		let end = parser.consumed_end();
		self.ast = Some(parser.finish());
		result
			.map(|roots| {
				self.at = end;
				if entry == Js::Params {
					Datum::Nodes(roots)
				} else {
					Datum::Node(self.tree().nth(roots, 0).unwrap())
				}
			})
			.map_err(Into::into)
	}
	fn text_chunk(&mut self, start: u32, end: u32, attribute: bool, raw: bool) -> Result<NodeId> {
		let raw_value = Datum::Slice(start, end);
		let decoded = if raw {
			raw_value
		} else {
			self.decoded(start, end, attribute)
		};
		let event = self.event(Event::Text {
			raw: raw_value,
			decoded,
		});
		let at = self.at;
		self.at = start;
		let node = self.call(self.plan.html.text, event, None, "");
		self.at = at;
		if let Ok(id) = node {
			self.ast().nodes[id.index() as usize].end = end;
		}
		node
	}
	fn children(&mut self, mode: Mode, stop: &Stop) -> Result<List> {
		let element = self.elements.last().cloned();
		if matches!(stop, Stop::MatchingElement) && element.as_ref().is_some_and(|e| e.empty) {
			return Ok(List::EMPTY);
		}
		let mode = if self.elements.iter().any(|e| e.content == Mode::Verbatim) {
			Mode::Verbatim
		} else {
			mode
		};
		let name = element
			.as_ref()
			.map(|e| &self.src[e.name.0 as usize..e.name.1 as usize]);
		let mut nodes = self.take_nodes();
		let mut closed = false;
		while self.at < self.limit {
			if let Stop::Prefixes(prefixes) = stop
				&& prefixes.iter().any(|p| self.matches(p))
				&& self.structural_stop()
			{
				break;
			}
			if self.matches("</")
				&& (matches!(mode, Mode::Normal | Mode::Verbatim)
					|| name.is_some_and(|name| closing_tag(self.rest(), name).is_some()))
			{
				if let Some(name) = name
					&& let Some(len) = closing_tag(self.rest(), name)
				{
					if matches!(stop, Stop::MatchingElement) {
						if nodes.is_empty() && mode == Mode::Raw {
							nodes.push(self.text_chunk(self.at, self.at, false, true)?);
						}
						self.at += len as u32;
						closed = true;
						break;
					}
					break;
				}
				if matches!(stop, Stop::MatchingElement) && (self.plan.html.autoclose || self.options.error_recovery) {
					closed = true;
					break;
				}
				let (_, end) = self.peek_name(self.at + 2, false);
				let close = &self.src[(self.at + 2) as usize..end as usize];
				if self.plan.html.void.iter().any(|name| name.as_ref() == close) {
					return fail(
						self.at,
						self.at + 1,
						Code::Placement,
						Some("A closing tag of a void element"),
					);
				}
				let close = if let Some(Autoclosed { previous, by, depth }) = self.autoclosed
					&& depth == self.elements.len()
					&& self.src[previous.0 as usize..previous.1 as usize] == *close
				{
					format!("{close}, closed by {}", &self.src[by.0 as usize..by.1 as usize])
				} else {
					close.to_owned()
				};
				return fail(self.at, self.at + 1, Code::UnexpectedClose, Some(&close));
			}
			if mode != Mode::Verbatim && self.structural_stop() {
				if self.options.error_recovery && matches!(stop, Stop::MatchingElement) {
					closed = true;
					break;
				}
				let start = self.at;
				let open = self.plan.html.delimiters[0].len();
				let tail = &self.rest()[open..];
				let closing = tail.starts_with('/');
				let end = tail.find(self.plan.html.delimiters[1].as_ref()).unwrap_or(tail.len());
				let name = tail.get(1..end).unwrap_or("");
				self.report(error(
					start,
					start + 1,
					if closing {
						Code::UnexpectedClose
					} else {
						Code::Placement
					},
					Some(if closing { name } else { "A branch outside its block" }),
				))?;
				self.at += (open + end) as u32;
				self.eat(&self.plan.html.delimiters[1]);
				continue;
			}
			if mode == Mode::Raw {
				let start = self.at;
				match name {
					Some(name) => self.skip_raw(name),
					None => self.at = self.limit,
				}
				nodes.push(self.text_chunk(start, self.at, false, true)?);
				continue;
			}
			if mode != Mode::Rcdata
				&& self.matches("<")
				&& self
					.rest()
					.as_bytes()
					.get(1)
					.is_none_or(|b| b.is_ascii_alphabetic() || *b == b'!')
			{
				if self.matches("<!--") {
					let start = self.at;
					self.at += 4;
					let data = self.at;
					let len = self
						.rest()
						.find("-->")
						.ok_or_else(|| error(self.limit, self.limit, Code::Expected, Some("-->")))?;
					let end = self.at + len as u32 + 3;
					let event = self.event(Event::Comment(Datum::Slice(data, end - 3)));
					self.at = start;
					let node = self.call(self.plan.html.comment, event, None, "")?;
					self.ast().nodes[node.index() as usize].end = end;
					self.at = end;
					nodes.push(node);
					continue;
				}
				let next = self.peek_name(self.at + 1, false);
				if let Some(name) = name
					&& self.plan.html.autoclose
					&& closes(name, &self.src[next.0 as usize..next.1 as usize])
				{
					self.autoclosed = Some(Autoclosed {
						previous: element.as_ref().unwrap().name,
						by: next,
						depth: self.elements.len() - 1,
					});
					closed = true;
					break;
				}
				nodes.push(self.element()?);
				continue;
			}
			if mode != Mode::Verbatim
				&& (self.matches(&self.plan.html.delimiters[0])
					|| self.plan.html.content.iter().any(|p| self.matches(&p.prefix)))
			{
				let row = self
					.plan
					.html
					.content
					.iter()
					.find(|p| self.matches(&p.prefix))
					.ok_or_else(|| error(self.at, self.at, Code::UnexpectedToken, None))?;
				nodes.push(self.call(row.rule, Datum::Missing, None, "")?);
				continue;
			}
			let start = self.at;
			self.at += self.char().unwrap().len_utf8() as u32;
			loop {
				self.skip_text();
				if self.at >= self.limit {
					break;
				}
				if mode != Mode::Verbatim
					&& (self.matches(&self.plan.html.delimiters[0])
						|| self.plan.html.content.iter().any(|p| self.matches(&p.prefix)))
				{
					break;
				}
				if (self.matches("</")
					&& (mode != Mode::Rcdata || name.is_some_and(|name| closing_tag(self.rest(), name).is_some())))
					|| (mode != Mode::Rcdata
						&& self.matches("<")
						&& self
							.rest()
							.as_bytes()
							.get(1)
							.is_none_or(|b| b.is_ascii_alphabetic() || *b == b'!'))
				{
					break;
				}
				self.at += self.char().unwrap().len_utf8() as u32;
			}
			nodes.push(self.text_chunk(start, self.at, false, false)?);
		}
		if self.at == self.limit && matches!(stop, Stop::MatchingElement) && !closed {
			let start = element.as_ref().map_or(self.at, |e| e.name.0 - 1);
			self.report(error(start, start + 1, Code::Unclosed, name))?;
		}

		Ok(self.finish_nodes(nodes))
	}
	fn peek_name(&self, start: u32, attribute: bool) -> (u32, u32) {
		let mut end = start;
		let mut brackets = 0;
		for c in self.src[start as usize..self.limit as usize].chars() {
			if attribute && c == '[' {
				brackets += 1;
			}
			if brackets == 0 && (is_space(c) || c == '/' || c == '>' || (attribute && matches!(c, '"' | '\'' | '='))) {
				break;
			}
			if c == ']' && brackets > 0 {
				brackets -= 1;
			}
			end += c.len_utf8() as u32;
		}
		(start, end)
	}
	fn value_span(&mut self, interpolate: bool) -> Result<(u32, u32, u32, bool)> {
		let start = self.at;
		let quote = self.char().filter(|c| matches!(c, '"' | '\''));
		if quote.is_some() {
			self.at += 1;
		}
		let content = self.at;
		while self.at < self.limit {
			if let Some(q) = quote {
				if self.char() == Some(q) {
					let end = self.at;
					self.at += 1;
					return Ok((content, end, self.at, true));
				}
			} else if self.char().is_some_and(|c| is_space(c) || c == '>') || self.matches("/>") && self.at > content {
				break;
			}
			if interpolate && self.matches(&self.plan.html.delimiters[0]) {
				self.interpolation_span()?;
			} else {
				self.at += self.char().unwrap().len_utf8() as u32;
			}
		}
		if quote.is_some() || self.at == content {
			self.report(error(start, start, Code::Expected, Some("an attribute value")))?;
		}
		Ok((content, self.at, self.at, quote.is_some()))
	}
	fn interpolation_span(&mut self) -> Result<()> {
		self.at += self.plan.html.delimiters[0].len() as u32;
		let close = &self.plan.html.delimiters[1];
		use crate::lexer::token::TokenKind;
		let strings = std::mem::take(&mut self.scan_strings);
		let mut lexer = crate::lexer::Lexer::with(&self.src[..self.limit as usize], strings);
		lexer.stop_ranges = std::mem::take(&mut self.scan_stops);
		lexer.regexp = std::mem::take(&mut self.scan_regexp);
		lexer.set_pos(self.at);
		lexer.set_stops(close);
		lexer.recover = self.options.error_recovery;
		lexer.at_sign = true;
		let mut templates = std::mem::take(&mut self.scan_templates);
		templates.clear();
		let mut operand = false;
		loop {
			let mut token = lexer.next_token()?;
			if self.src[token.start as usize..].starts_with("/>") {
				self.at = token.start;
				break;
			}
			if token.stop || token.kind == TokenKind::Eof {
				self.at = token.start;
				break;
			}
			if matches!(token.kind, TokenKind::Slash | TokenKind::SlashEq) && !operand {
				token = lexer.read_regex(token)?;
			}
			if token.kind == TokenKind::Backquote
				|| token.kind == TokenKind::BraceR && templates.last() == Some(&lexer.depth)
			{
				if token.kind == TokenKind::BraceR {
					templates.pop();
				}
				let depth = lexer.depth;
				token = lexer.read_template()?;
				if matches!(token.kind, TokenKind::Template { tail: false, .. }) {
					templates.push(depth);
					operand = false;
					continue;
				}
			}
			operand = token.ends_operand();
		}
		self.scan_strings = lexer.strings;
		self.scan_stops = lexer.stop_ranges;
		self.scan_templates = templates;
		self.scan_regexp = lexer.regexp;
		self.expect(close)?;
		Ok(())
	}
	fn scan_header(&mut self) -> Result<Run> {
		let saved = self.at;
		let result = (|| {
			let from = self.attributes.len() as u32;
			loop {
				self.space();
				if self.at >= self.limit
					|| self.matches(">")
					|| self.matches("/>")
					|| (self.options.error_recovery
						&& (self.matches("<")
							|| (self.matches(&self.plan.html.delimiters[0])
								&& (self.structural_stop()
									|| self.plan.html.content.iter().any(|row| {
										row.prefix.len() > self.plan.html.delimiters[0].len()
											&& self.matches(&row.prefix)
									}))))) {
					break;
				}
				if self.plan.html.attribute_comments == plan::AttributeComments::Javascript
					&& (self.matches("//") || self.matches("/*"))
				{
					if self.eat("//") {
						self.at += self.rest().find('\n').unwrap_or(self.rest().len()) as u32;
					} else {
						self.at += 2;
						self.at += self.rest().find("*/").map_or(self.rest().len(), |n| n + 2) as u32;
					}
					continue;
				}
				if self.plan.html.attribute.iter().any(|row| self.matches(&row.prefix)) {
					self.interpolation_span()?;
					self.attributes.push(HeaderAttribute {
						name: Datum::Missing,
						boolean: false,
						text: Datum::Missing,
						expression: true,
					});
					continue;
				}
				let (start, end) = self.peek_name(self.at, true);
				if start == end {
					break;
				}
				self.at = end;
				self.space();
				if self.char().is_some_and(|c| matches!(c, '\'' | '"')) {
					return fail(self.at, self.at, Code::Expected, Some("="));
				}
				let value = if self.eat("=") {
					self.space();
					Some(self.value_span(self.plan.html.attribute_interpolations)?)
				} else {
					None
				};
				let text = if let Some((a, b, _, _)) = value
					&& !self.src[a as usize..b as usize].contains(self.plan.html.delimiters[0].as_ref())
				{
					self.decoded(a, b, true)
				} else {
					Datum::Missing
				};
				self.attributes.push(HeaderAttribute {
					name: Datum::Slice(start, end),
					boolean: value.is_none(),
					text,
					expression: false,
				});
			}
			Ok(Run {
				start: from,
				len: self.attributes.len() as u32 - from,
			})
		})();
		self.at = saved;
		result
	}
	fn element(&mut self) -> Result<NodeId> {
		let start = self.at;
		self.at += 1;
		let span = self.peek_name(self.at, false);
		self.at = span.1;
		if span.0 == span.1 || self.at == self.limit && !self.options.error_recovery {
			return fail(self.at, self.at, Code::UnexpectedEof, None);
		}
		let name = &self.src[span.0 as usize..span.1 as usize];
		let identifier = |s: &str| {
			let mut chars = s.chars();
			chars.next().is_some_and(is_id_start) && chars.all(is_id_continue)
		};
		let facts = u8::from(valid_name(name))
			| (u8::from(identifier(name)) << 1)
			| (u8::from(name.starts_with(|c: char| c.is_uppercase())) << 2)
			| (u8::from(
				name.contains('.')
					&& (name.split('.').all(identifier)
						|| self.options.error_recovery
							&& name.ends_with('.')
							&& name[..name.len() - 1].split('.').all(identifier)),
			) << 3);
		let at_document = self.active.len() == 1 || self.active.len() == 2 && self.elements.is_empty();
		let id = self.plan.program.interner.find(name);
		let event = self.event(Event::Element {
			name: span,
			id,
			header: None,
			after_name: self.at,
			facts,
			at_document,
			raw: Datum::Missing,
		});
		let dispatch = self.records.len();
		let owner = self.elements.last().map(|e| e.record);
		let rule = self.plan.document;
		let slots = self.slots.len();
		self.spare
			.slots
			.resize(slots + self.plan.program.rules[rule].slots, Datum::Missing);
		self.records.push(Record {
			failure: None,
			body_end: None,
			children_end: None,
			aborted: false,
			rule,
			ty: StrId::at(0),
			slots,
			event,
			owner,
			start,
			node: None,
		});
		let mut selected = None;
		let range = self
			.plan
			.program
			.dispatch_names
			.get(id.map_or(usize::MAX, |id| id.index() as usize))
			.copied()
			.unwrap_or(self.plan.program.dispatch_other);
		for (i, code) in &self.plan.program.dispatch_rows[range.indices()] {
			if self.test(code, dispatch)? {
				selected = Some((*i as usize, &self.plan.html.elements[*i as usize]));
				break;
			}
		}
		self.records.pop();
		self.slots.truncate(slots);
		let (selected, row) = selected.ok_or_else(|| error(span.0, span.1, Code::InvalidName, Some(name)))?;
		let record = self.records.len();
		self.elements.push(Element {
			record,
			name: span,
			empty: false,
			attributes: None,
			content: row.content.unwrap_or(Mode::Normal),
		});
		let result = self.call(
			self.plan.program.dispatch_rules[selected],
			event,
			self.plan.program.types[selected],
			"",
		);
		self.elements.pop();
		if let Ok(node) = result {
			self.ast().nodes[node.index() as usize].start = start;
		}
		result
	}
	fn attributes(&mut self, mode: AttributeMode, record: usize) -> Result<List> {
		let mode = if self.elements.iter().any(|e| e.content == Mode::Verbatim) {
			AttributeMode::Static
		} else {
			mode
		};
		if let Some(list) = self.elements.last().and_then(|e| e.attributes) {
			return Ok(list);
		}
		let mut nodes = self.take_nodes();
		loop {
			self.space();
			if self.at >= self.limit
				|| self.matches(">")
				|| self.matches("/>")
				|| (self.options.error_recovery
					&& (self.matches("<")
						|| (self.matches(&self.plan.html.delimiters[0])
							&& (self.structural_stop()
								|| self.plan.html.content.iter().any(|row| {
									row.prefix.len() > self.plan.html.delimiters[0].len() && self.matches(&row.prefix)
								}))))) {
				break;
			}
			if self.plan.html.attribute_comments == plan::AttributeComments::Javascript && self.attribute_comment() {
				continue;
			}
			if mode == AttributeMode::Normal
				&& let Some(row) = self.plan.html.attribute.iter().find(|p| self.matches(&p.prefix))
			{
				nodes.push(self.call(row.rule, Datum::Missing, None, "")?);
				continue;
			}
			nodes.push(self.attribute(mode)?);
		}
		if self.at == self.limit && !self.options.error_recovery {
			return fail(self.at, self.at, Code::UnexpectedEof, None);
		}
		let empty = self.eat("/");
		let unclosed = !self.matches(">");
		self.expect(">")?;
		let list = self.finish_nodes(nodes);
		if let Some(element) = self.spare.elements.last_mut() {
			let name = &self.src[element.name.0 as usize..element.name.1 as usize];
			element.empty =
				empty || unclosed || name.starts_with('!') || self.plan.html.void.iter().any(|s| s.as_ref() == name);
			element.attributes = Some(list);
			if element.content == Mode::Raw {
				let start = self.at;
				self.skip_raw(name);
				let end = self.at;
				self.at = start;
				if end == self.limit && !self.options.error_recovery {
					return fail(end, end, Code::Unclosed, Some(name));
				}
				let Datum::Event(i) = self.records[record].event else {
					unreachable!()
				};
				let Event::Element { raw, .. } = &mut self.events[i] else {
					unreachable!()
				};
				*raw = Datum::Span(start, end, false);
			}
		}
		Ok(list)
	}
	fn attribute_comment(&mut self) -> bool {
		use crate::ast::{Comment, CommentKind};
		let start = self.at;
		let kind = if self.eat("//") {
			self.at += self.rest().find('\n').unwrap_or(self.rest().len()) as u32;
			CommentKind::Line
		} else if self.eat("/*") {
			match self.rest().find("*/") {
				Some(n) => {
					self.at += n as u32 + 2;
					CommentKind::Block
				}
				None => {
					self.at = self.limit;
					CommentKind::Unclosed
				}
			}
		} else {
			return false;
		};
		let end = self.at;
		self.ast().comments.push(Comment { kind, start, end });
		true
	}
	fn attribute(&mut self, mode: AttributeMode) -> Result<NodeId> {
		let start = self.at;
		let (a, b) = self.peek_name(start, true);
		if a == b {
			return fail(start, start, Code::UnexpectedToken, None);
		}
		self.at = b;
		let name = &self.src[a as usize..b as usize];
		self.space();
		if self.char().is_some_and(|c| matches!(c, '\'' | '"')) {
			return fail(self.at, self.at, Code::Expected, Some("="));
		}
		let value = if self.eat("=") {
			self.space();
			Some(self.value_span(mode == AttributeMode::Normal && self.plan.html.attribute_interpolations)?)
		} else {
			None
		};
		let end = value.map_or(b, |v| v.2);
		let syntax = &self.plan.html.directive_names;
		let selected = if mode == AttributeMode::Static {
			None
		} else {
			let shorthand = self.plan.html.directives.iter().find(|row| {
				!row.name.chars().next().is_some_and(is_id_start)
					&& row.name.as_ref() != "*"
					&& name.starts_with(row.name.as_ref())
			});
			if let Some(row) = shorthand {
				Some((
					row,
					row.name.as_ref(),
					&name[row.name.len()..],
					a + row.name.len() as u32,
				))
			} else if let Some(tail) = name.strip_prefix(syntax.prefix.as_ref()) {
				let n = tail
					.find([
						syntax.argument.chars().next().unwrap_or('\0'),
						syntax.modifier.chars().next().unwrap_or('\0'),
					])
					.unwrap_or(tail.len());
				let key = &tail[..n];
				let row = self
					.plan
					.html
					.directives
					.iter()
					.find(|r| r.name.as_ref() == key)
					.or_else(|| {
						(syntax.unknown == plan::UnknownDirective::WildcardRule)
							.then(|| self.plan.html.directives.iter().find(|r| r.name.as_ref() == "*"))
							.flatten()
					});
				row.filter(|_| !syntax.prefix.is_empty() || tail[n..].starts_with(syntax.argument.as_ref()))
					.map(|row| {
						let rest = tail[n..].strip_prefix(syntax.argument.as_ref()).unwrap_or(&tail[n..]);
						(row, key, rest, b - rest.len() as u32)
					})
			} else {
				None
			}
		};
		if let Some((row, key, rest, offset)) = selected {
			let (argument, after) = if let Some([open, close]) = &syntax.dynamic
				&& let Some(tail) = rest.strip_prefix(open.as_ref())
			{
				let n = tail
					.find(close.as_ref())
					.ok_or_else(|| error(a, b, Code::Expected, Some(close)))?;
				(
					Datum::Span(offset + open.len() as u32, offset + (open.len() + n) as u32, true),
					&tail[n + close.len()..],
				)
			} else {
				let n = rest.find(syntax.modifier.as_ref()).unwrap_or(rest.len());
				(
					if n == 0 {
						Datum::Missing
					} else {
						Datum::Span(offset, offset + n as u32, false)
					},
					&rest[n..],
				)
			};
			if syntax.require_argument && argument == Datum::Missing {
				return fail(a, b, Code::Expected, Some("a directive name"));
			}
			let modifiers_start = self.tree().host_strings.len() as u32;
			for modifier in after.split(syntax.modifier.as_ref()).filter(|s| !s.is_empty()) {
				let id = self.intern(modifier);
				self.ast().host_strings.push(id);
			}
			let modifiers = Run {
				start: modifiers_start,
				len: self.tree().host_strings.len() as u32 - modifiers_start,
			};
			let key_start = if key.as_ptr() as usize >= self.src.as_ptr() as usize
				&& (key.as_ptr() as usize) < self.src.as_ptr() as usize + self.src.len()
			{
				key.as_ptr() as usize - self.src.as_ptr() as usize
			} else {
				a as usize
			};
			let event = self.event(Event::Directive {
				name: (key_start as u32, (key_start + key.len()) as u32),
				raw: (a, b),
				argument,
				modifiers,
				value: value.map_or(Datum::Missing, |(a, b, _, _)| Datum::Span(a, b, false)),
				quoted: value.is_some_and(|v| v.3),
			});
			let at = self.at;
			self.at = start;
			let node = self.call(row.rule, event, None, "");
			self.at = at;
			if let Ok(id) = node {
				self.ast().nodes[id.index() as usize].end = end;
			}
			return node;
		}
		let value = if let Some((a, b, _, quoted)) = value {
			let saved = (self.at, self.limit);
			self.at = a;
			self.limit = b;
			let result = self.parts(
				mode == AttributeMode::Normal && self.plan.html.attribute_interpolations,
				quoted,
			);
			(self.at, self.limit) = saved;
			result?
		} else {
			Datum::Bool(true)
		};
		let keys = self.plan.program.keys;
		self.make(
			keys.attribute,
			start,
			end,
			&[(keys.name, Datum::Slice(a, b)), (keys.value, value)],
			true,
		)
	}
	fn parts(&mut self, interpolate: bool, quoted: bool) -> Result<Datum> {
		let mut nodes = self.take_nodes();
		let mut text_only = true;
		if self.at == self.limit {
			nodes.push(self.text_chunk(self.at, self.at, true, false)?);
		}
		while self.at < self.limit {
			if interpolate && self.matches(&self.plan.html.delimiters[0]) {
				nodes.push(self.call(self.plan.html.plain_attribute.expression, Datum::Missing, None, "")?);
				text_only = false;
			} else {
				let start = self.at;
				while self.at < self.limit && !(interpolate && self.matches(&self.plan.html.delimiters[0])) {
					self.at += self.char().unwrap().len_utf8() as u32;
				}
				nodes.push(self.text_chunk(start, self.at, true, false)?);
			}
		}
		Ok(if !quoted && !text_only && nodes.len() == 1 {
			let node = nodes[0];
			nodes.clear();
			self.nodes.push(nodes);
			Datum::Node(node)
		} else {
			Datum::Nodes(self.finish_nodes(nodes))
		})
	}
	fn attribute_parts(&mut self, record: usize) -> Result<Datum> {
		let value = self.event_field(record, Key::Value);
		let Datum::Span(a, b, _) = value else {
			return Ok(Datum::Bool(true));
		};
		let quoted = self.event_field(record, Key::Quoted).yes();
		let saved = (self.at, self.limit);
		self.at = a;
		self.limit = b;
		let value = self.parts(self.plan.html.attribute_interpolations, quoted);
		(self.at, self.limit) = saved;
		value
	}
	fn single(&mut self, entry: Js) -> Result<Datum> {
		let open = &self.plan.html.delimiters[0];
		if !self.matches(open) {
			return fail(self.at, self.at, Code::Expected, Some("an expression, not text"));
		}
		let close = &self.plan.html.delimiters[1];
		self.expect(open)?;
		let value = self
			.javascript(entry, None, close)
			.map_err(|error| error.boxed(self.plan))?;
		self.space();
		self.expect(close)?;
		Ok(value)
	}
	fn stylesheet(&mut self) -> Result<Datum> {
		let (children, comments) = self.style_sheet()?;
		let i = self.events.len();
		self.events.push(Event::Stylesheet(children, comments));
		Ok(Datum::Stylesheet(i))
	}
	fn len(&self) -> u32 {
		self.limit
	}
	fn byte(&self) -> Option<u8> {
		self.rest().as_bytes().first().copied()
	}
	fn plan_text(&self, text: &str) -> Datum {
		self.plan
			.program
			.interner
			.find(text)
			.map_or(Datum::Missing, Datum::Text)
	}
	fn host(&mut self, ty: &'static str, start: u32, end: u32, fields: &[(&'static str, Value)], span: bool) -> NodeId {
		let ast = self.ast();
		let from = ast.host_fields.len() as u32;
		ast.host_fields.extend_from_slice(fields);
		let index = ast.hosts.len() as u32;
		ast.hosts.push(Host {
			ty,
			fields: (from, fields.len() as u32),
			span,
			scope: None,
		});
		ast.add(NodeKind::Host(index), start, end)
	}
}

fn closes(current: &str, next: &str) -> bool {
	match current {
		"li" => next == "li",
		"dt" | "dd" => matches!(next, "dt" | "dd"),
		"p" => {
			matches!(
				next,
				"address"
					| "article" | "aside"
					| "blockquote" | "div"
					| "dl" | "fieldset"
					| "footer" | "form"
					| "h1" | "h2" | "h3"
					| "h4" | "h5" | "h6"
					| "header" | "hgroup"
					| "hr" | "main" | "menu"
					| "nav" | "ol" | "p"
					| "pre" | "section"
					| "table" | "ul"
			)
		}
		"rt" | "rp" => matches!(next, "rt" | "rp"),
		"optgroup" => next == "optgroup",
		"option" => matches!(next, "option" | "optgroup"),
		"thead" | "tbody" => matches!(next, "tbody" | "tfoot"),
		"tfoot" => next == "tbody",
		"tr" => matches!(next, "tr" | "tbody"),
		"td" | "th" => matches!(next, "td" | "th" | "tr"),
		_ => false,
	}
}

fn is_space(c: char) -> bool {
	matches!(
		c,
		' ' | '\t'..='\r'
			| '\u{a0}' | '\u{1680}'
			| '\u{2000}'..='\u{200a}'
			| '\u{2028}' | '\u{2029}'
			| '\u{202f}' | '\u{205f}'
			| '\u{3000}' | '\u{feff}'
	)
}

fn valid_name(name: &str) -> bool {
	let mut chars = name.chars();
	let Some(first) = chars.next() else { return false };
	if first == '!' {
		return name.len() > 1 && chars.all(|c| c.is_ascii_alphabetic());
	}
	if !first.is_ascii_alphabetic() {
		return false;
	}
	if let Some((prefix, rest)) = name.split_once(':') {
		return prefix.chars().all(|c| c.is_ascii_alphanumeric())
			&& rest.starts_with(|c: char| c.is_ascii_alphabetic())
			&& rest.ends_with(|c: char| c.is_ascii_alphanumeric())
			&& rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
	}
	let mut seen_dash = false;
	for c in chars {
		if c == '-' {
			seen_dash = true;
		} else if !c.is_ascii_alphanumeric()
			&& !(seen_dash && (c == '.' || c == '_' || c == '\u{b7}' || c as u32 >= 0xc0))
		{
			return false;
		}
	}
	true
}

const WINDOWS_1252: [u32; 32] = [
	8364, 129, 8218, 402, 8222, 8230, 8224, 8225, 710, 8240, 352, 8249, 338, 141, 381, 143, 144, 8216, 8217, 8220,
	8221, 8226, 8211, 8212, 732, 8482, 353, 8250, 339, 157, 382, 376,
];

fn valid_code(code: u32, attribute: bool) -> u32 {
	if code == 10 && !attribute {
		return 32;
	}
	if code < 128 {
		return code;
	}
	if code <= 159 {
		return WINDOWS_1252[code as usize - 128];
	}
	if (55296..=57343).contains(&code) {
		return 0;
	}
	if code <= 0x2ffff || (0xe0000..=0xe007f).contains(&code) || (0xe0100..=0xe01ef).contains(&code) {
		return code;
	}
	0
}

fn decode(raw: &str, attribute: bool) -> Cow<'_, str> {
	if !raw.contains('&') {
		return Cow::Borrowed(raw);
	}
	let mut out = String::with_capacity(raw.len());
	let bytes = raw.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] != b'&' {
			let start = i;
			while i < bytes.len() && bytes[i] != b'&' {
				i += 1;
			}
			out.push_str(&raw[start..i]);
			continue;
		}
		let rest = &raw[i + 1..];
		let mut code = None;
		let mut mark = None;
		let mut consumed = 0;
		if let Some(number) = rest.strip_prefix('#') {
			let (digits, hex) = match number.strip_prefix(['x', 'X']) {
				Some(hex) => (hex, true),
				None => (number, false),
			};
			let len = digits
				.bytes()
				.take_while(|b| if hex { b.is_ascii_hexdigit() } else { b.is_ascii_digit() })
				.count();
			if len > 0 {
				code = u32::from_str_radix(&digits[..len], if hex { 16 } else { 10 }).ok();
				consumed = 1 + usize::from(hex) + len;
				if rest.as_bytes().get(consumed) == Some(&b';') {
					consumed += 1;
				}
			}
		} else {
			let len = rest
				.bytes()
				.take(entities::LONGEST)
				.take_while(|b| b.is_ascii_alphanumeric())
				.count();
			let with = rest.as_bytes().get(len) == Some(&b';');
			let bare = (1..=len.min(entities::LONGEST_BARE)).rev();
			for try_len in with.then_some(len + 1).into_iter().chain(bare) {
				let Some((found, found_mark)) = entities::lookup(&rest[..try_len]) else {
					continue;
				};
				let after = rest.as_bytes().get(try_len);
				if try_len <= len
					&& attribute && after.is_some_and(|b| *b == b'=' || b.is_ascii_alphanumeric() || *b == b'_')
				{
					continue;
				}
				code = Some(found);
				mark = found_mark;
				consumed = try_len;
				break;
			}
		}
		match code.filter(|&c| c != 0) {
			Some(code) => {
				out.push(char::from_u32(valid_code(code, attribute)).unwrap_or('\0'));
				out.extend(mark);
				i += 1 + consumed;
			}
			None => {
				out.push('&');
				i += 1;
			}
		}
	}
	Cow::Owned(out)
}

fn error(pos: u32, end: u32, code: Code, arg: Option<&str>) -> Box<SyntaxError> {
	let message: Cow<'static, str> = match arg {
		Some(arg) => code.with(arg).into(),
		None => code.message().into(),
	};
	Box::new(SyntaxError::with(pos, code, message).to(end))
}

fn fail<T>(pos: u32, end: u32, code: Code, arg: Option<&str>) -> Result<T> {
	Err(error(pos, end, code, arg))
}

fn closing_tag(rest: &str, name: &str) -> Option<usize> {
	let head = 2 + name.len();
	let start = rest.get(..head)?;
	if !start.starts_with("</") || !start[2..].eq_ignore_ascii_case(name) {
		return None;
	}
	let tail = &rest[head..];
	if tail.starts_with('>') {
		return Some(head + 1);
	}
	if !tail.starts_with(is_space) {
		return None;
	}
	tail.find('>').map(|i| head + i + 1)
}

/// Whether a root element's static attributes select TypeScript.
pub fn typescript(src: &str, plan: &Plan) -> bool {
	if plan.html.typescript.is_empty() {
		return false;
	}
	let mut at = 0;
	let mut elements: Vec<&str> = Vec::new();
	while let Some(offset) = src[at..].find('<') {
		at += offset;
		let rest = &src[at..];
		if let Some(comment) = rest.strip_prefix("<!--") {
			at += comment.find("-->").map_or(rest.len(), |end| end + 7);
			continue;
		}
		let closing = rest.starts_with("</");
		let start = at + if closing { 2 } else { 1 };
		let name_end = src[start..]
			.find(|c: char| is_space(c) || matches!(c, '/' | '>'))
			.map_or(src.len(), |n| start + n);
		let name = &src[start..name_end];
		if name.is_empty() {
			at += 1;
			continue;
		}
		let end = name_end + tag_end(&src[name_end..]);
		at = (end + 1).min(src.len());
		if closing {
			if let Some(i) = elements.iter().rposition(|open| *open == name) {
				elements.truncate(i);
			}
			continue;
		}
		if plan.html.autoclose {
			while elements.last().is_some_and(|open| closes(open, name)) {
				elements.pop();
			}
		}
		let mut attributes = src[name_end..end].trim_start_matches(is_space);
		while !attributes.is_empty() {
			attributes = attributes.trim_start_matches(is_space);
			if attributes.is_empty() {
				break;
			}
			let len = attributes
				.find(|c: char| is_space(c) || c == '=' || c == '/')
				.unwrap_or(attributes.len());
			let attribute = &attributes[..len];
			attributes = attributes[len..].trim_start_matches(is_space);
			let mut value = None;
			if let Some(after) = attributes.strip_prefix('=') {
				let after = after.trim_start_matches(is_space);
				let (text, len) = match after.chars().next() {
					Some(q @ ('"' | '\'')) => {
						let close = after[1..].find(q).unwrap_or(after.len() - 1);
						(&after[1..1 + close], (close + 2).min(after.len()))
					}
					_ => {
						let len = after.find(is_space).unwrap_or(after.len());
						(&after[..len], len)
					}
				};
				value = Some(text);
				attributes = after[len..].trim_start_matches(is_space);
			} else if attribute.is_empty() {
				attributes = &attributes[1..];
			}
			if elements.is_empty()
				&& plan.html.typescript.iter().any(|row| {
					row.element.as_ref() == name
						&& row.attribute.as_ref() == attribute
						&& row.value.as_deref().is_none_or(|wanted| value == Some(wanted))
				}) {
				return true;
			}
		}
		if name.starts_with('!')
			|| src[name_end..end].trim_end().ends_with('/')
			|| plan.html.void.iter().any(|v| v.as_ref() == name)
		{
			continue;
		}
		if matches!(name, "script" | "style" | "textarea" | "title") {
			while let Some(offset) = src[at..].find("</") {
				at += offset;
				if let Some(len) = closing_tag(&src[at..], name) {
					at += len;
					break;
				}
				at += 2;
			}
		} else {
			elements.push(name);
		}
	}
	false
}

fn tag_end(after: &str) -> usize {
	let mut quote = None;
	for (i, c) in after.char_indices() {
		match quote {
			Some(q) if c == q => quote = None,
			Some(_) => {}
			None if c == '"' || c == '\'' => quote = Some(c),
			None if c == '>' => return i,
			None => {}
		}
	}
	after.len()
}
