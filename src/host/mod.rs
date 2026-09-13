mod css;
pub mod entities;
mod native;
pub mod plan;

use std::borrow::Cow;

use std::collections::BTreeMap;
use std::rc::Rc;

use self::plan::{
	Absence, AttributeMode, Base, Boundary, Construct, Form, Gap, Js, Mode, Path, Plan, Reader, Relation, SpanPolicy,
	Stop,
};
use crate::ast::{Ast, Host, HostBinding, HostParent, HostRegion, List, NodeId, NodeKind, Value};
use crate::error::{Code, SyntaxError};
use crate::interner::StrId;
use crate::lexer::unicode::{is_id_continue, is_id_start};
use crate::parser::{Entry, Extension, ForInit, Options, Parser, Result};

#[derive(Clone, Debug, PartialEq)]
enum Datum {
	Missing,
	Null,
	Bool(bool),
	Number(f64),
	Text(Rc<str>),
	Interned(StrId),
	Static(&'static str),
	Slice(u32, u32),
	Span(u32, u32, bool),
	Node(NodeId),
	Nodes(List),
	Array(Rc<Vec<Datum>>),
	Object(Rc<BTreeMap<Rc<str>, Datum>>),
	Facts(Rc<Vec<(&'static str, Datum)>>),
	Record(usize),
	Scopes(usize),
	Region(usize, usize),
	Incoming(usize),
}

impl Datum {
	fn yes(&self) -> bool {
		matches!(self, Self::Bool(true))
	}
	fn facts(fields: impl IntoIterator<Item = (&'static str, Datum)>) -> Self {
		Self::Facts(Rc::new(fields.into_iter().collect()))
	}
	fn object(fields: impl IntoIterator<Item = (impl Into<Rc<str>>, Datum)>) -> Self {
		Self::Object(Rc::new(fields.into_iter().map(|(k, v)| (k.into(), v)).collect()))
	}
	fn array(items: Vec<Datum>) -> Self {
		Self::Array(Rc::new(items))
	}
}

#[derive(Clone)]
struct Record {
	failure: Option<Box<SyntaxError>>,
	body_end: Option<u32>,
	children_end: Option<u32>,
	aborted: bool,
	parent: Option<usize>,
	rule: usize,
	ty: Rc<str>,
	slots: Vec<Datum>,
	event: Datum,
	owner: Option<usize>,
	ancestors: Vec<usize>,
	start: u32,
	node: Option<NodeId>,
}

#[derive(Clone)]
struct Element {
	record: usize,
	name: (u32, u32),
	empty: bool,
	attributes: Option<List>,
	content: Mode,
}

#[derive(Clone, Copy)]
struct Autoclosed {
	previous: (u32, u32),
	by: (u32, u32),
	depth: usize,
}

struct Checkpoint<M> {
	ast: crate::ast::Mark<M>,
	at: u32,
	limit: u32,
	records: usize,
	record: Record,
	elements: Vec<Element>,
	iteration: Vec<Vec<(Rc<str>, Datum)>>,
	autoclosed: Option<Autoclosed>,
}

pub fn parse(src: &str, plan: &Plan, options: Options) -> (Ast, std::result::Result<NodeId, Box<crate::SyntaxError>>) {
	parse_document::<()>(src, plan, options, None)
}

pub(crate) fn parse_document<E: Extension>(
	src: &str,
	plan: &Plan,
	options: Options,
	reused: Option<Ast<E::Data>>,
) -> (Ast<E::Data>, Result<NodeId>) {
	let cut = if plan.html.trim_end {
		src.trim_end_matches(is_space)
	} else {
		src
	};
	let mut w = Walker::<E> {
		src: cut,
		full: src.len() as u32,
		plan,
		options,
		ast: Some(reused.unwrap_or_else(|| Ast::sized(src.len()))),
		at: 0,
		limit: cut.len() as u32,
		records: Vec::new(),
		elements: Vec::new(),
		active: Vec::new(),
		iteration: Vec::new(),
		bindings: Vec::new(),
		region_slots: Vec::new(),
		resolving: Vec::new(),
		node_records: Default::default(),
		recovering_form: false,
		native_reads: 0,
		autoclosed: None,
	};
	let result = w.call(plan.document, Datum::Missing, None, "").and_then(|node| {
		if w.at != w.limit {
			return fail(w.at, w.at, Code::UnexpectedToken, None);
		}
		w.ast().nodes[node.index() as usize].end = w.full;
		w.regions()?;
		Ok(node)
	});
	let errors = &mut w.ast().errors;
	errors.sort_by_key(|error| error.pos);
	errors.dedup_by(|a, b| a.pos == b.pos && a.code == b.code);
	(w.ast.take().unwrap(), result)
}

struct Walker<'a, E: Extension> {
	src: &'a str,
	full: u32,
	plan: &'a Plan,
	options: Options,
	ast: Option<Ast<E::Data>>,
	at: u32,
	limit: u32,
	records: Vec<Record>,
	elements: Vec<Element>,
	active: Vec<usize>,
	iteration: Vec<Vec<(Rc<str>, Datum)>>,
	bindings: Vec<(Rc<str>, Datum)>,
	region_slots: Vec<Vec<Option<Vec<HostParent>>>>,
	resolving: Vec<(usize, usize)>,
	node_records: crate::ast::NodeMap<usize>,
	recovering_form: bool,
	native_reads: usize,
	autoclosed: Option<Autoclosed>,
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
	fn list(&mut self, nodes: &[NodeId]) -> List {
		self.ast().add_list_from(nodes.iter().copied().map(Some))
	}
	fn text_of<'b>(&'b self, value: &'b Datum) -> Option<&'b str> {
		match value {
			Datum::Text(s) => Some(s),
			Datum::Interned(id) => Some(self.tree().str(*id)),
			Datum::Static(s) => Some(s),
			Datum::Slice(a, b) => self.src.get(*a as usize..*b as usize),
			_ => None,
		}
	}
	fn count(&self, value: &Datum) -> usize {
		match value {
			Datum::Array(items) => items.len(),
			Datum::Nodes(list) => list.len as usize,
			_ => 0,
		}
	}
	fn item(&self, value: &Datum, index: usize) -> Datum {
		match value {
			Datum::Array(items) => items.get(index).cloned().unwrap_or(Datum::Missing),
			Datum::Nodes(list) => self
				.tree()
				.list(*list)
				.get(index)
				.map_or(Datum::Missing, |node| node.map_or(Datum::Null, Datum::Node)),
			_ => Datum::Missing,
		}
	}
	fn items(&self, value: &Datum) -> Vec<Datum> {
		(0..self.count(value)).map(|i| self.item(value, i)).collect()
	}

	fn slot(&self, record: usize, name: &str) -> Option<usize> {
		let rule = &self.plan.rules[self.records[record].rule];
		rule.fields.keys().position(|key| key.as_ref() == name).or_else(|| {
			rule.locals
				.iter()
				.position(|key| key.as_ref() == name)
				.map(|i| i + rule.fields.len())
		})
	}
	fn write(&mut self, record: usize, name: &str, value: Datum) {
		if value == Datum::Missing {
			return;
		}
		if let Some(index) = self.slot(record, name) {
			self.records[record].slots[index] = value;
		} else if let Some(slots) = self.iteration.last_mut()
			&& let Some((_, slot)) = slots.iter_mut().find(|(key, _)| key.as_ref() == name)
		{
			*slot = value;
		}
	}
	fn datum(&self, value: Value) -> Datum {
		match value {
			Value::Node(id) => Datum::Node(id),
			Value::Nodes(list) => Datum::Nodes(list),
			Value::Str(id) => Datum::Interned(id),
			Value::Slice(a, b) => Datum::Slice(a, b),
			Value::Strs(a, n) => Datum::array(
				self.tree().host_strings[a as usize..(a + n) as usize]
					.iter()
					.map(|id| Datum::Interned(*id))
					.collect(),
			),
			Value::Float(v) => Datum::Number(v),
			Value::Array(a, n) => Datum::array(
				self.tree().host_values[a as usize..(a + n) as usize]
					.iter()
					.map(|v| self.datum(*v))
					.collect(),
			),
			Value::Bool(v) => Datum::Bool(v),
			Value::Int(v) => Datum::Number(v as f64),
			Value::Null | Value::Comments => Datum::Null,
		}
	}
	fn property(&self, value: Datum, path: &Path) -> Datum {
		let Path::Name(key) = path else {
			let Path::Index(index) = path else { unreachable!() };
			return self.item(&value, *index);
		};
		match value {
			Datum::Object(fields) => fields.get(key.as_ref()).cloned().unwrap_or(Datum::Missing),
			Datum::Facts(fields) => fields
				.iter()
				.find(|(k, _)| *k == key.as_ref())
				.map_or(Datum::Missing, |(_, v)| v.clone()),
			Datum::Record(index) => {
				let rec = &self.records[index];
				match key.as_ref() {
					"type" => Datum::Text(rec.ty.clone()),
					"span" => Datum::Span(
						rec.start,
						rec.node.map_or(self.at, |id| self.tree().node(id).end),
						false,
					),
					"header" => self.property(rec.event.clone(), path),
					"scopes" => Datum::Scopes(index),
					_ => self.slot(index, key).map_or(Datum::Missing, |i| rec.slots[i].clone()),
				}
			}
			Datum::Scopes(index) => self.plan.rules[self.records[index].rule]
				.regions
				.iter()
				.position(|r| r.id.as_ref() == key.as_ref())
				.map_or(Datum::Missing, |r| Datum::Region(index, r)),
			Datum::Span(a, b, dynamic) => match key.as_ref() {
				"start" => Datum::Number(a as f64),
				"end" => Datum::Number(b as f64),
				"span" => Datum::Span(a, b, dynamic),
				"text" => Datum::Slice(a, b),
				"dynamic" => Datum::Bool(dynamic),
				_ => Datum::Missing,
			},
			Datum::Node(id) => {
				let node = self.tree().node(id);
				match key.as_ref() {
					"span" => return Datum::Span(node.start, node.end, false),
					"raw"
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
					"start" => return Datum::Number(node.start as f64),
					"end" => return Datum::Number(node.end as f64),
					"innerSource" if node.end > node.start + 1 => return Datum::Slice(node.start + 1, node.end - 1),
					"header" => {
						return self.node_records.get(id).map_or(Datum::Missing, |index| {
							self.property(self.records[*index].event.clone(), path)
						});
					}
					_ => {}
				}
				match node.kind {
					NodeKind::Host(index) => {
						let host = &self.tree().hosts[index as usize];
						if key.as_ref() == "type" {
							return Datum::Interned(host.ty);
						}
						self.tree().host_fields[host.fields.0 as usize..(host.fields.0 + host.fields.1) as usize]
							.iter()
							.find(|(k, _)| self.tree().str(*k) == key.as_ref())
							.map_or(Datum::Missing, |(_, v)| self.datum(*v))
					}
					NodeKind::Identifier { name } if key.as_ref() == "name" => Datum::Interned(name),
					_ => self.native_property(id, key),
				}
			}
			_ => Datum::Missing,
		}
	}
	fn constant(value: &plan::Json) -> Datum {
		match value {
			plan::Json::Null => Datum::Null,
			plan::Json::Bool(v) => Datum::Bool(*v),
			plan::Json::Number(v) => Datum::Number(v.parse().unwrap_or(0.0)),
			plan::Json::String(v) => Datum::Text(v.clone()),
			plan::Json::Array(v) if v.is_empty() => Datum::Nodes(List::EMPTY),
			plan::Json::Array(v) => Datum::array(v.iter().map(Self::constant).collect()),
			plan::Json::Object(v) => Datum::object(v.iter().map(|(k, v)| (k.as_ref(), Self::constant(v)))),
		}
	}
	fn equal(&self, a: &Datum, b: &Datum) -> bool {
		if matches!(a, Datum::Array(_) | Datum::Nodes(_)) && matches!(b, Datum::Array(_) | Datum::Nodes(_)) {
			return self.count(a) == self.count(b)
				&& (0..self.count(a)).all(|i| self.equal(&self.item(a, i), &self.item(b, i)));
		}
		match (self.text_of(a), self.text_of(b)) {
			(Some(a), Some(b)) => a == b,
			_ => a == b,
		}
	}
	fn eval(&mut self, expr: &plan::Value, record: usize) -> Result<Datum> {
		use plan::Value as V;
		Ok(match expr {
			V::Constant(value) => Self::constant(value),
			V::Get { base, path } => {
				let mut value = match base {
					Base::Value(value) => self.eval(value, record)?,
					Base::Name(name) => match name.as_ref() {
						"record" => Datum::Record(record),
						"event" => self.records[record].event.clone(),
						"locals" => Datum::Record(record),
						"iteration" => Datum::object(self.iteration.last().into_iter().flatten().cloned()),
						"owner" => self.records[record].owner.map_or(Datum::Missing, Datum::Record),
						"ancestors" => Datum::array(
							self.records[record]
								.ancestors
								.iter()
								.rev()
								.copied()
								.map(Datum::Record)
								.collect(),
						),
						"incoming" => Datum::Incoming(record),
						"scopes" => Datum::Scopes(record),
						_ => self
							.bindings
							.iter()
							.rev()
							.find(|(key, _)| key.as_ref() == name.as_ref())
							.map_or(Datum::Missing, |(_, v)| v.clone()),
					},
				};
				for part in path {
					value = self.property(value, part);
				}
				value
			}
			V::Compare {
				relation,
				left,
				right,
				set,
			} => {
				if let Some(set) = set {
					let needle = self.eval(&set.needle, record)?;
					return Ok(Datum::Bool(
						self.text_of(&needle).is_some_and(|s| set.strings.contains(s)),
					));
				}
				let left = self.eval(left, record)?;
				let yes = match relation {
					Relation::Present => left != Datum::Missing,
					Relation::Equal => {
						let right = self.eval(right.as_ref().unwrap(), record)?;
						self.equal(&left, &right)
					}
					Relation::Less => {
						let right = self.eval(right.as_ref().unwrap(), record)?;
						matches!((left,right),(Datum::Number(a),Datum::Number(b)) if a<b)
					}
				};
				Datum::Bool(yes)
			}
			V::Choose { condition, yes, no } => {
				let condition = self.eval(condition, record)?.yes();
				self.eval(if condition { yes } else { no }, record)?
			}
			V::FlatMap { .. } => {
				let mut out = Vec::new();
				self.eval_list(expr, record, &mut |_, value| {
					out.push(value);
					Ok(true)
				})?;
				Datum::array(out)
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
				Datum::array(items.iter().map(|v| self.eval(v, record)).collect::<Result<_>>()?)
			}
			V::Construct(Construct::Record {
				node_type,
				fields,
				span,
			}) => {
				let span = self.eval(span, record)?;
				let (start, end) = match span {
					Datum::Span(a, b, _) => (a, b),
					_ => (self.at, self.at),
				};
				let fields = fields
					.iter()
					.map(|(k, v)| Ok((k.as_ref(), self.eval(v, record)?)))
					.collect::<Result<Vec<_>>>()?;
				Datum::Node(self.make(
					node_type.as_deref().unwrap_or(""),
					start,
					end,
					&fields,
					span != Datum::Null,
				)?)
			}
		})
	}
	fn eval_list(
		&mut self,
		expr: &plan::Value,
		record: usize,
		emit: &mut dyn FnMut(&mut Self, Datum) -> Result<bool>,
	) -> Result<bool> {
		use plan::Value as V;
		match expr {
			V::FlatMap { list, binding, body } => self.eval_list(list, record, &mut |this, item| {
				this.bindings.push((binding.clone(), item));
				let result = this.eval_list(body, record, emit);
				this.bindings.pop();
				result
			}),
			V::Choose { condition, yes, no } => {
				let condition = self.eval(condition, record)?.yes();
				self.eval_list(if condition { yes } else { no }, record, emit)
			}
			V::Construct(Construct::Array(items)) => {
				for item in items {
					let item = self.eval(item, record)?;
					if !emit(self, item)? {
						return Ok(false);
					}
				}
				Ok(true)
			}
			_ => {
				let list = self.eval(expr, record)?;
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
		Ok(match value {
			Datum::Missing | Datum::Null | Datum::Scopes(_) | Datum::Region(..) | Datum::Incoming(_) => Value::Null,
			Datum::Bool(v) => Value::Bool(*v),
			Datum::Number(v) if *v >= 0.0 && *v <= u32::MAX as f64 && v.fract() == 0.0 => Value::Int(*v as u32),
			Datum::Number(v) => Value::Float(*v),
			Datum::Text(v) => Value::Str(self.intern(v)),
			Datum::Interned(id) => Value::Str(*id),
			Datum::Static(v) => Value::Str(self.intern(v)),
			Datum::Slice(a, b) => Value::Slice(*a, *b),
			Datum::Node(id) => Value::Node(*id),
			Datum::Nodes(list) => Value::Nodes(*list),
			Datum::Array(items) => {
				if !items.is_empty() && items.iter().all(|v| self.text_of(v).is_some()) {
					let start = self.tree().host_strings.len() as u32;
					for v in items.iter() {
						let text = self.text_of(v).unwrap().to_owned();
						let id = self.intern(&text);
						self.ast().host_strings.push(id);
					}
					Value::Strs(start, items.len() as u32)
				} else if !items
					.iter()
					.all(|v| matches!(v, Datum::Node(_) | Datum::Null | Datum::Missing))
				{
					let values = items.iter().map(|v| self.output(v)).collect::<Result<Vec<_>>>()?;
					let start = self.tree().host_values.len() as u32;
					self.ast().host_values.extend(values);
					Value::Array(start, items.len() as u32)
				} else {
					let nodes = items
						.iter()
						.map(|v| match v {
							Datum::Node(id) => Some(*id),
							_ => None,
						})
						.collect::<Vec<_>>();
					Value::Nodes(self.ast().add_list_from(nodes.into_iter()))
				}
			}
			Datum::Facts(fields) => Value::Node(self.make("", self.at, self.at, fields, false)?),
			Datum::Object(fields) => {
				let fields = fields.iter().map(|(k, v)| (k.as_ref(), v.clone())).collect::<Vec<_>>();
				Value::Node(self.make("", self.at, self.at, &fields, false)?)
			}
			Datum::Record(rec) => self.records[*rec].node.map_or(Value::Null, Value::Node),
			Datum::Span(a, b, _) => {
				let fields = [("start", Datum::Number(*a as f64)), ("end", Datum::Number(*b as f64))];
				Value::Node(self.make("", *a, *b, &fields, false)?)
			}
		})
	}
	fn make(&mut self, ty: &str, start: u32, end: u32, fields: &[(&str, Datum)], span: bool) -> Result<NodeId> {
		if ty.starts_with("js.") {
			let kind = self.native_construct(ty, start, end, fields)?;
			return Ok(self.ast().add(kind, start, end));
		}
		let mut values = Vec::with_capacity(fields.len());
		for (key, value) in fields {
			if *value != Datum::Missing {
				values.push((self.intern(key), self.output(value)?));
			}
		}
		let from = self.tree().host_fields.len() as u32;
		self.ast().host_fields.extend(values);
		let len = self.tree().host_fields.len() as u32 - from;
		let ty = self.intern(ty);
		let index = self.tree().hosts.len() as u32;
		self.ast().hosts.push(Host {
			ty,
			fields: (from, len),
			span,
		});
		Ok(self.ast().add(NodeKind::Host(index), start, end))
	}
	fn call(&mut self, rule: usize, event: Datum, ty: Option<&str>, follow: &str) -> Result<NodeId> {
		if self.active.len() > 256 {
			return fail(self.at, self.at, Code::TreeSize, None);
		}
		let schema = &self.plan.rules[rule];
		let record = self.records.len();
		self.records.push(Record {
			failure: None,
			body_end: None,
			children_end: None,
			aborted: false,
			parent: self.active.last().copied(),
			rule,
			ty: ty.unwrap_or(&schema.node_type).into(),
			slots: vec![Datum::Missing; schema.fields.len() + schema.locals.len()],
			event,
			owner: self
				.elements
				.iter()
				.rev()
				.find(|e| e.record != record)
				.map(|e| e.record),
			ancestors: self
				.elements
				.iter()
				.filter(|e| e.record != record)
				.map(|e| e.record)
				.collect(),
			start: self.at,
			node: None,
		});
		self.active.push(record);
		let result = if self.options.error_recovery && !self.recovering_form {
			let checkpoint = self.checkpoint(record);
			match self.strict(&schema.form, record, follow) {
				Ok(()) => Ok(()),
				Err(_) => {
					self.restore(record, checkpoint);
					self.recovering_form = true;
					let result = self.form(&schema.form, record, follow);
					self.recovering_form = false;
					result
				}
			}
		} else {
			self.form(&schema.form, record, follow)
		};
		self.active.pop();
		result.map_err(|error| {
			self.records[record]
				.failure
				.take()
				.filter(|failure| failure.pos > error.pos)
				.unwrap_or(error)
		})?;
		if self
			.elements
			.last()
			.is_some_and(|e| e.record == record && e.content == Mode::Raw)
		{
			let raw = self.property(self.records[record].event.clone(), &Path::Name("rawChildren".into()));
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
		for (i, absence) in schema.fields.values().enumerate() {
			if self.records[record].slots[i] == Datum::Missing && *absence == Absence::Null {
				self.records[record].slots[i] = Datum::Null;
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
		let fields = schema
			.fields
			.keys()
			.enumerate()
			.map(|(i, k)| (k.as_ref(), self.records[record].slots[i].clone()))
			.collect::<Vec<_>>();
		let node = self.make(
			ty.unwrap_or(&schema.node_type),
			start,
			end,
			&fields,
			schema.span != Some(SpanPolicy::None),
		)?;
		self.records[record].node = Some(node);
		Ok(node)
	}
	fn remember(&mut self, record: usize, error: Box<SyntaxError>) {
		if self.records[record]
			.failure
			.as_ref()
			.is_none_or(|prior| prior.pos < error.pos)
		{
			self.records[record].failure = Some(error);
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

	fn checkpoint(&self, record: usize) -> Checkpoint<<E::Data as crate::ast::Reuse>::Mark> {
		Checkpoint {
			ast: self.tree().mark(),
			at: self.at,
			limit: self.limit,
			records: self.records.len(),
			record: self.records[record].clone(),
			elements: self.elements.clone(),
			iteration: self.iteration.clone(),
			autoclosed: self.autoclosed,
		}
	}
	fn restore(&mut self, record: usize, checkpoint: Checkpoint<<E::Data as crate::ast::Reuse>::Mark>) {
		self.ast().truncate(checkpoint.ast);
		self.at = checkpoint.at;
		self.limit = checkpoint.limit;
		self.records.truncate(checkpoint.records);
		self.records[record] = checkpoint.record;
		self.elements = checkpoint.elements;
		self.iteration = checkpoint.iteration;
		self.autoclosed = checkpoint.autoclosed;
	}
	fn strict(&mut self, form: &Form, record: usize, follow: &str) -> Result<()> {
		let recover = std::mem::replace(&mut self.options.error_recovery, false);
		let result = self.form(form, record, follow);
		self.options.error_recovery = recover;
		result
	}

	fn form(&mut self, form: &Form, record: usize, follow: &str) -> Result<()> {
		if self.records[record].aborted && !matches!(form, Form::Seq(_) | Form::Emit { .. }) {
			return Ok(());
		}
		match form {
			Form::Seq(items) => {
				for item in items {
					let next = follow;
					if self.options.error_recovery
						&& self.records[record].body_end == Some(self.at)
						&& !self.records[record].aborted
						&& !matches!(item, Form::Emit { .. })
						&& (self.at == self.limit || self.matches("</") || self.structural_stop())
					{
						let checkpoint = self.checkpoint(record);
						let start = self.at;
						match self.strict(item, record, next) {
							Ok(()) => continue,
							Err(err) => {
								self.restore(record, checkpoint);
								let header_end =
									start + self.rest().find(self.plan.html.delimiters[1].as_ref()).unwrap_or(0) as u32;
								if (err.pos < header_end
									&& !self.rest().starts_with(&format!("{}:", self.plan.html.delimiters[0])))
									|| self.matches("</") || self.at == self.limit
								{
									self.records[record].aborted = true;
									let pos = self.records[record].start;
									let name = self.plan.rules[self.records[record].rule].name.to_ascii_lowercase();
									self.report(error(pos, pos + 1, Code::Unclosed, Some(&name)))?;
									continue;
								}
							}
						}
					}
					self.form(item, record, next)?;
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
				let next;
				let follow: &str = if local.is_empty() || !matches!(reader, Reader::Rule(_) | Reader::Javascript { .. })
				{
					follow
				} else if follow.is_empty() {
					local
				} else {
					next = format!("{local} {follow}");
					&next
				};
				let input = input.as_ref().map(|v| self.eval(v, record)).transpose()?;
				let value = self.read(reader, record, input, follow)?;
				if let Some(into) = into {
					self.write(record, into, value);
				}
			}
			Form::Choice {
				alternatives,
				disjoint,
				first,
			} => {
				if *disjoint {
					let rest = self.rest();
					let selected = first
						.iter()
						.position(|prefixes| {
							prefixes.iter().any(|p| {
								let rest = if p.tight {
									rest
								} else {
									rest.trim_start_matches(is_space)
								};
								rest.starts_with(&p.text)
									&& (!p.word || !rest[p.text.len()..].starts_with(is_id_continue))
							})
						})
						.or_else(|| {
							self.options.error_recovery.then(|| {
								first
									.iter()
									.position(|prefixes| {
										prefixes
											.iter()
											.any(|p| p.text.as_str() == self.plan.html.delimiters[1].as_ref())
									})
									.unwrap_or(0)
							})
						})
						.ok_or_else(|| {
							let pos =
								self.at + (self.rest().len() - self.rest().trim_start_matches(is_space).len()) as u32;
							let expected = first
								.iter()
								.flatten()
								.map(|p| p.text.as_str())
								.collect::<Vec<_>>()
								.join(" or ");
							error(pos, pos, Code::Expected, Some(&expected))
						})?;
					self.form(&alternatives[selected], record, follow)?;
				} else {
					let mut failure: Option<Box<crate::SyntaxError>> = None;
					let mut failed = 0;
					let mut failed_native = false;
					let mut matched = false;
					let start = self.at;
					for (i, alternative) in alternatives.iter().enumerate() {
						let checkpoint = self.checkpoint(record);
						let reads = self.native_reads;
						match self.strict(alternative, record, follow) {
							Ok(()) => {
								if self.options.error_recovery
									&& self.at == start && failed_native
									&& failure.as_ref().is_some_and(|e| e.pos > start)
								{
									self.restore(record, checkpoint);
									self.form(&alternatives[failed], record, follow)?;
								}
								matched = true;
								break;
							}
							Err(error) => {
								self.restore(record, checkpoint);
								if failure.as_ref().is_none_or(|prior| {
									prior.pos < error.pos
										|| self.options.error_recovery
											&& prior.pos == error.pos && (native_form(alternative, Js::Program)
											|| matches!(error.code, Code::ReservedWord | Code::UnexpectedKeyword)
												&& native_form(alternative, Js::Statement))
								}) {
									failure = Some(error);
									failed_native = self.native_reads > reads;
									failed = i;
								}
							}
						}
					}
					if matched && let Some(error) = failure.as_ref() {
						self.remember(record, error.clone());
					}
					if !matched {
						if self.options.error_recovery {
							self.form(&alternatives[failed], record, follow)?;
						} else {
							return Err(failure.unwrap());
						}
					}
				}
			}

			Form::Repeat {
				body,
				min,
				max,
				locals,
				yield_value,
				into,
			} => {
				let mut values = Vec::new();

				while max.is_none_or(|max| values.len() < max) {
					let checkpoint = self.checkpoint(record);
					let start = self.at;
					self.iteration
						.push(locals.iter().map(|key| (key.as_ref().into(), Datum::Missing)).collect());
					let result = self
						.strict(body, record, follow)
						.and_then(|()| self.eval(yield_value, record));
					self.iteration.pop();
					match result {
						Ok(value) => values.push(value),
						Err(error) => {
							self.restore(record, checkpoint);
							if values.len() < *min {
								if !self.options.error_recovery {
									return Err(error);
								}
								self.iteration
									.push(locals.iter().map(|key| (key.as_ref().into(), Datum::Missing)).collect());
								self.form(body, record, follow)?;
								values.push(self.eval(yield_value, record)?);
								self.iteration.pop();
								continue;
							}
							self.remember(record, error);
							break;
						}
					}
					if self.at == start && max.is_none() {
						return fail(start, start, Code::TreeSize, None);
					}
				}

				if values.len() < *min {
					return fail(self.at, self.at, Code::UnexpectedToken, None);
				}
				self.write(record, into, Datum::array(values));
			}
		}
		Ok(())
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
				Reader::Token { text, gap, word } => {
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
					if !self.eval(value, record)?.yes() {
						return fail(self.at, self.at, Code::UnexpectedToken, None);
					}
					Datum::Missing
				}
				Reader::Rule(rule) => {
					let child = self.records.len();
					let node = self.call(*rule, self.records[record].event.clone(), None, follow)?;
					if let Some(end) = self.records[child].children_end {
						self.records[record].body_end = Some(end);
					}
					Datum::Node(node)
				}
				Reader::Javascript { entry, boundary } => self.javascript(*entry, *boundary, follow)?,
				Reader::HtmlChildren { mode, stop } => {
					let nodes = self.children(*mode, stop)?;
					if matches!(stop, Stop::Prefixes(_)) {
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
	fn javascript(&mut self, entry: Js, boundary: Option<Boundary>, follow: &str) -> Result<Datum> {
		self.native_reads += 1;
		let stops = if entry == Js::Expression {
			follow
				.split_ascii_whitespace()
				.filter(|s| !matches!(*s, "(" | "[" | "." | "?." | "?"))
				.collect::<Vec<_>>()
				.join(" ")
		} else {
			follow.to_owned()
		};
		let mut parser = Parser::<E>::new(
			&self.src[..self.limit as usize],
			self.at,
			self.options,
			&stops,
			self.ast.take().unwrap(),
		);
		let result = parser
			.lexer
			.next_token_into(&mut parser.tok)
			.and_then(|()| match entry {
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
					let token_end = parser.tok.end;
					parser.parse_ident(false).map(|id| parser.list_of(&[id])).map_err(|e| {
						if e.code == Code::UnexpectedKeyword {
							error(
								e.pos,
								token_end,
								Code::ReservedWord,
								Some(&self.src[e.pos as usize..token_end as usize]),
							)
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
				let start = self.at;
				let at = error.pos;
				parser.record(Err(error)).unwrap();
				parser.skip_to_end();
				parser.prev_end = parser.prev_end.max(at);
				if entry == Js::Params {
					Ok(List::EMPTY)
				} else {
					let name = parser.intern("");
					let node =
						parser.add_with_end(NodeKind::Identifier { name }, start, parser.consumed_end().max(start));
					Ok(parser.list_of(&[node]))
				}
			}
			result => result,
		};
		let end = parser.consumed_end();
		self.ast = Some(parser.finish());
		let roots = result?;
		self.at = end;
		Ok(if entry == Js::Params {
			Datum::Nodes(roots)
		} else {
			Datum::Node(self.tree().nth(roots, 0).unwrap())
		})
	}
	fn text_chunk(&mut self, start: u32, end: u32, attribute: bool, raw: bool) -> Result<NodeId> {
		let decoded = if raw {
			Datum::Slice(start, end)
		} else {
			match decode(&self.src[start as usize..end as usize], attribute) {
				std::borrow::Cow::Borrowed(_) => Datum::Slice(start, end),
				std::borrow::Cow::Owned(s) => Datum::Text(s.into()),
			}
		};
		let event = Datum::facts([("raw", Datum::Slice(start, end)), ("decoded", decoded)]);
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
		let mut nodes = Vec::new();
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
				while self.at < self.limit && !name.is_some_and(|name| closing_tag(self.rest(), name).is_some()) {
					self.at += self.char().unwrap().len_utf8() as u32;
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
					let event = Datum::facts([("data", Datum::Slice(data, end - 3))]);
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
			while self.at < self.limit {
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

		Ok(self.list(&nodes))
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
		let mut lexer = crate::lexer::Lexer::with(&self.src[..self.limit as usize], Default::default());
		lexer.set_pos(self.at);
		lexer.set_stops(close);
		lexer.recover = self.options.error_recovery;
		lexer.at_sign = true;
		let mut templates = Vec::new();
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
		self.expect(close)?;
		Ok(())
	}
	fn scan_header(&mut self) -> Result<Datum> {
		let saved = self.at;
		let result = (|| {
			let mut attributes = Vec::new();
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
					attributes.push(Datum::facts([("kind", Datum::Static("expression"))]));
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
				let mut fields = vec![
					("kind", Datum::Static("ordinary")),
					("name", Datum::Slice(start, end)),
					("boolean", Datum::Bool(value.is_none())),
				];
				if let Some((a, b, _, _)) = value
					&& !self.src[a as usize..b as usize].contains(self.plan.html.delimiters[0].as_ref())
				{
					fields.push((
						"staticText",
						Datum::Text(decode(&self.src[a as usize..b as usize], true).as_ref().into()),
					));
				}
				attributes.push(Datum::facts(fields));
			}
			Ok(Datum::facts([("attributes", Datum::array(attributes))]))
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
		let facts = Datum::facts([
			("validHtmlName", Datum::Bool(valid_name(name))),
			("identifier", Datum::Bool(identifier(name))),
			(
				"uppercaseInitial",
				Datum::Bool(name.starts_with(|c: char| c.is_uppercase())),
			),
			(
				"dottedIdentifier",
				Datum::Bool(
					name.contains('.')
						&& (name.split('.').all(identifier)
							|| self.options.error_recovery
								&& name.ends_with('.') && name[..name.len() - 1].split('.').all(identifier)),
				),
			),
			(
				"namespace",
				name.split_once(':')
					.map_or(Datum::Missing, |(p, _)| Datum::Text(p.into())),
			),
		]);
		let header = self.scan_header()?;
		let event = Datum::facts([
			("name", Datum::Slice(span.0, span.1)),
			("nameFacts", facts),
			(
				"atDocument",
				Datum::Bool(self.active.len() == 1 || self.active.len() == 2 && self.elements.is_empty()),
			),
			("header", header),
		]);
		let dispatch = self.records.len();
		self.records.push(Record {
			failure: None,
			body_end: None,
			children_end: None,
			aborted: false,
			parent: None,
			rule: self.plan.document,
			ty: "".into(),
			slots: Vec::new(),
			event: event.clone(),
			owner: self.elements.last().map(|e| e.record),
			ancestors: self.elements.iter().map(|e| e.record).collect(),
			start,
			node: None,
		});
		let mut selected = None;
		for row in &self.plan.html.elements {
			if self.eval(&row.when, dispatch)?.yes() {
				selected = Some(row);
				break;
			}
		}
		self.records.pop();
		let row = selected.ok_or_else(|| error(span.0, span.1, Code::InvalidName, Some(name)))?;
		let record = self.records.len();
		self.elements.push(Element {
			record,
			name: span,
			empty: false,
			attributes: None,
			content: row.content.unwrap_or(Mode::Normal),
		});
		let result = self.call(row.rule, event, row.node_type.as_deref(), "");
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
		let mut nodes = Vec::new();
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
		let list = self.list(&nodes);
		if let Some(element) = self.elements.last_mut() {
			let name = &self.src[element.name.0 as usize..element.name.1 as usize];
			element.empty =
				empty || unclosed || name.starts_with('!') || self.plan.html.void.iter().any(|s| s.as_ref() == name);
			element.attributes = Some(list);
			if element.content == Mode::Raw {
				let start = self.at;
				let mut end = start;
				while end < self.limit && closing_tag(&self.src[end as usize..self.limit as usize], name).is_none() {
					end += self.src[end as usize..].chars().next().unwrap().len_utf8() as u32;
				}
				if end == self.limit && !self.options.error_recovery {
					return fail(end, end, Code::Unclosed, Some(name));
				}
				let Datum::Facts(event) = &mut self.records[record].event else {
					unreachable!()
				};
				Rc::make_mut(event).push(("rawChildren", Datum::Span(start, end, false)));
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
			let modifiers = Datum::array(
				after
					.split(syntax.modifier.as_ref())
					.filter(|s| !s.is_empty())
					.map(|s| Datum::Text(s.into()))
					.collect(),
			);
			let event = Datum::facts([
				("name", Datum::Text(key.into())),
				("rawName", Datum::Slice(a, b)),
				("argument", argument),
				("modifiers", modifiers),
				(
					"value",
					value.map_or(Datum::Missing, |(a, b, _, _)| Datum::Span(a, b, false)),
				),
				("quoted", Datum::Bool(value.is_some_and(|v| v.3))),
			]);
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
		let plain = &self.plan.html.plain_attribute;
		self.make(
			&plain.node_type,
			start,
			end,
			&[(&plain.name, Datum::Text(name.into())), (&plain.value, value)],
			true,
		)
	}
	fn parts(&mut self, interpolate: bool, quoted: bool) -> Result<Datum> {
		let mut nodes = Vec::new();
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
			Datum::Node(nodes[0])
		} else {
			Datum::Nodes(self.list(&nodes))
		})
	}
	fn attribute_parts(&mut self, record: usize) -> Result<Datum> {
		let value = self.property(self.records[record].event.clone(), &Path::Name("value".into()));
		let Datum::Span(a, b, _) = value else {
			return Ok(Datum::Bool(true));
		};
		let quoted = self
			.property(self.records[record].event.clone(), &Path::Name("quoted".into()))
			.yes();
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
		let value = self.javascript(entry, None, close)?;
		self.space();
		self.expect(close)?;
		Ok(value)
	}
	fn stylesheet(&mut self) -> Result<Datum> {
		let (children, comments) = css::read(self.src, &mut self.at, self.limit, self.ast.as_mut().unwrap(), None)?;
		Ok(Datum::facts([
			("children", Datum::Nodes(children)),
			("comments", Datum::Nodes(comments)),
		]))
	}
	fn roots(&self, value: &Datum, roots: &mut Vec<NodeId>) {
		match value {
			Datum::Node(id) => {
				if !roots.contains(id) {
					roots.push(*id);
				}
			}
			Datum::Nodes(list) => {
				for id in self.tree().list(*list).iter().flatten() {
					if !roots.contains(id) {
						roots.push(*id);
					}
				}
			}
			Datum::Array(items) => {
				for item in items.iter() {
					self.roots(item, roots);
				}
			}
			_ => {}
		}
	}
	fn region_target(&mut self, value: Datum) -> Result<HostParent> {
		match value {
			Datum::Null => Ok(HostParent::Root),
			Datum::Incoming(record) => Ok(self.records[record].node.map_or(HostParent::Root, HostParent::Incoming)),
			Datum::Region(record, region) => {
				let targets = self.region(record, region)?;
				if targets.len() != 1 {
					return fail(self.at, self.at, Code::Expected, Some("one parent region"));
				}
				Ok(targets[0])
			}
			_ => fail(self.at, self.at, Code::Expected, Some("a region")),
		}
	}
	fn region(&mut self, record: usize, index: usize) -> Result<Vec<HostParent>> {
		if let Some(targets) = &self.region_slots[record][index] {
			return Ok(targets.clone());
		}
		if self.resolving.contains(&(record, index)) {
			return fail(self.at, self.at, Code::Expected, Some("acyclic regions"));
		}
		self.resolving.push((record, index));
		let region = &self.plan.rules[self.records[record].rule].regions[index];
		let items = if let Some(each) = &region.each {
			let list = self.eval(&each.list, record)?;
			self.items(&list)
		} else {
			vec![Datum::Missing]
		};
		let mut targets = Vec::new();
		for item in items {
			if let Some(each) = &region.each {
				self.bindings.push((each.binding.clone(), item));
			}
			let parent = self.eval(&region.parent, record)?;
			let parent = self.region_target(parent)?;
			if region
				.when
				.as_ref()
				.map(|when| self.eval(when, record).map(|v| v.yes()))
				.transpose()?
				.unwrap_or(true)
			{
				let covers = self.eval(&region.covers, record)?;
				let mut roots = Vec::new();
				self.roots(&covers, &mut roots);
				let owner = self.records[record].node.unwrap();
				let node = roots
					.iter()
					.find(|id| match self.tree().node(**id).kind {
						NodeKind::Program { .. } => true,
						NodeKind::Host(i) => !self.tree().hosts[i as usize].span,
						_ => false,
					})
					.copied();
				let id = self.tree().host_regions.len() as u32;
				self.ast().host_regions.push(HostRegion {
					parent,
					kind: region.kind,
					owner,
					node,
				});
				self.ast().host_region_owners.entry(owner).push(id);
				for root in roots {
					self.ast().host_coverage.entry(root).push(id);
				}
				targets.push(HostParent::Region(id));
			} else {
				targets.push(parent);
			}
			if region.each.is_some() {
				self.bindings.pop();
			}
		}
		self.resolving.pop();
		self.region_slots[record][index] = Some(targets.clone());
		Ok(targets)
	}
	fn binding_leaves(&mut self, node: NodeId, binding: HostBinding) {
		match self.tree().node(node).kind {
			NodeKind::Identifier { .. } => self.ast().host_bindings.insert(node, binding),
			NodeKind::ArrayPattern { elements } | NodeKind::ObjectPattern { properties: elements } => {
				let nodes = self.tree().list(elements).iter().flatten().copied().collect::<Vec<_>>();
				for node in nodes {
					self.binding_leaves(node, binding);
				}
			}
			NodeKind::Property { value, .. } => self.binding_leaves(value, binding),
			NodeKind::AssignmentPattern { left, .. } => self.binding_leaves(left, binding),
			NodeKind::RestElement { argument } => self.binding_leaves(argument, binding),
			_ => {}
		}
	}
	fn regions(&mut self) -> Result<()> {
		for (i, record) in self.records.iter().enumerate() {
			if let Some(node) = record.node {
				self.node_records.insert(node, i);
			}
		}
		self.ast().host_plan = true;
		self.region_slots = self
			.records
			.iter()
			.map(|r| vec![None; self.plan.rules[r.rule].regions.len()])
			.collect();
		for record in 0..self.records.len() {
			let Some(node) = self.records[record].node else {
				continue;
			};
			let rule = &self.plan.rules[self.records[record].rule];
			let mut hidden = Vec::new();
			for value in &self.records[record].slots[rule.fields.len()..] {
				self.roots(value, &mut hidden);
			}
			hidden.retain(|id| !matches!(self.tree().node(*id).kind, NodeKind::Host(_)));
			if !hidden.is_empty() {
				self.ast().host_hidden.insert(node, hidden);
			}
			if let Some(parent) = self.records[record].parent.and_then(|r| self.records[r].node) {
				self.ast().host_occurrences.insert(node, parent);
			}
			for region in 0..self.plan.rules[self.records[record].rule].regions.len() {
				self.region(record, region)?;
			}
		}
		for record in 0..self.records.len() {
			if self.records[record].node.is_none() {
				continue;
			}
			for declaration in &self.plan.rules[self.records[record].rule].declares {
				let target = self.eval(&declaration.into, record)?;
				let target = self.region_target(target)?;
				let patterns = self.eval(&declaration.patterns, record)?;
				let mut roots = Vec::new();
				self.roots(&patterns, &mut roots);
				for pattern in roots {
					self.binding_leaves(
						pattern,
						HostBinding {
							target,
							kind: declaration.kind,
						},
					);
				}
			}
		}
		Ok(())
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

// code points a browser repairs when a reference names them
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

fn native_form(form: &Form, entry: Js) -> bool {
	match form {
		Form::Read {
			reader: Reader::Javascript { entry: read, .. },
			..
		} => *read == entry,
		Form::Seq(items) => items.iter().any(|form| native_form(form, entry)),
		_ => false,
	}
}
