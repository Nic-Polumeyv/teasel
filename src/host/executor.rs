use std::collections::BTreeMap;
use std::rc::Rc;

use super::plan::{
	self, Absence, AttributeMode, Base, Boundary, Construct, Form, Gap, Js, Mode, Path, Plan, Reader, Relation,
	SpanPolicy, Stop,
};
use super::{closing_tag, decode, error, fail, is_space, valid_name};
use crate::ast::{Ast, Host, List, NodeId, NodeKind, Value, VariableKind};
use crate::error::Code;
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
	Slice(u32, u32),
	Span(u32, u32, bool),
	Node(NodeId),
	Nodes(List),
	Array(Rc<Vec<Datum>>),
	Object(Rc<BTreeMap<Rc<str>, Datum>>),
	Record(usize),
}

impl Datum {
	fn yes(&self) -> bool {
		matches!(self, Self::Bool(true))
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

pub fn parse(src: &str, plan: &Plan, options: Options) -> (Ast, std::result::Result<NodeId, Box<crate::SyntaxError>>) {
	execute::<()>(src, plan, options, None)
}

pub(crate) fn execute<E: Extension>(
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
	};
	let result = w.call(plan.document, Datum::Missing, None).and_then(|node| {
		if w.at != w.limit {
			return fail(w.at, w.at, Code::UnexpectedToken, None);
		}
		w.ast().nodes[node.index() as usize].end = w.full;
		Ok(node)
	});
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
			fail(self.at, self.at, Code::Expected, Some(text))
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
			Datum::Slice(a, b) => self.src.get(*a as usize..*b as usize),
			_ => None,
		}
	}
	fn items(&self, value: &Datum) -> Vec<Datum> {
		match value {
			Datum::Array(items) => items.as_ref().clone(),
			Datum::Nodes(list) => self
				.tree()
				.list(*list)
				.iter()
				.map(|id| id.map_or(Datum::Null, Datum::Node))
				.collect(),
			_ => Vec::new(),
		}
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
			Value::Str(id) => Datum::Text(self.tree().str(id).into()),
			Value::Slice(a, b) => Datum::Slice(a, b),
			Value::Strs(a, n) => Datum::array(
				self.tree().host_strings[a as usize..(a + n) as usize]
					.iter()
					.map(|id| Datum::Text(self.tree().str(*id).into()))
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
			return self.items(&value).get(*index).cloned().unwrap_or(Datum::Missing);
		};
		match value {
			Datum::Object(fields) => fields.get(key.as_ref()).cloned().unwrap_or(Datum::Missing),
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
					_ => self.slot(index, key).map_or(Datum::Missing, |i| rec.slots[i].clone()),
				}
			}
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
					"start" => return Datum::Number(node.start as f64),
					"end" => return Datum::Number(node.end as f64),
					"innerSource" if node.end > node.start + 1 => return Datum::Slice(node.start + 1, node.end - 1),
					"header" => {
						return self
							.records
							.iter()
							.find(|r| r.node == Some(id))
							.map_or(Datum::Missing, |r| self.property(r.event.clone(), path));
					}
					_ => {}
				}
				match node.kind {
					NodeKind::Host(index) => {
						let host = &self.tree().hosts[index as usize];
						if key.as_ref() == "type" {
							return Datum::Text(self.tree().str(host.ty).into());
						}
						self.tree().host_fields[host.fields.0 as usize..(host.fields.0 + host.fields.1) as usize]
							.iter()
							.find(|(k, _)| self.tree().str(*k) == key.as_ref())
							.map_or(Datum::Missing, |(_, v)| self.datum(*v))
					}
					NodeKind::Identifier { name } if key.as_ref() == "name" => {
						Datum::Text(self.tree().str(name).into())
					}
					NodeKind::Program { body, .. } if key.as_ref() == "body" => Datum::Nodes(body),
					_ => Datum::Missing,
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
			plan::Json::String(v) => Datum::Text(v.as_ref().into()),
			plan::Json::Array(v) => Datum::array(v.iter().map(Self::constant).collect()),
			plan::Json::Object(v) => Datum::object(v.iter().map(|(k, v)| (k.as_ref(), Self::constant(v)))),
		}
	}
	fn equal(&self, a: &Datum, b: &Datum) -> bool {
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
						"incoming" | "scopes" => Datum::Missing,
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
			V::FlatMap { list, binding, body } => {
				let list = self.eval(list, record)?;
				let mut out = Vec::new();
				for item in self.items(&list) {
					self.bindings.push((binding.as_ref().into(), item));
					let value = self.eval(body, record);
					self.bindings.pop();
					out.extend(self.items(&value?));
				}
				Datum::array(out)
			}
			V::Length(list) => {
				let list = self.eval(list, record)?;
				Datum::Number(self.items(&list).len() as f64)
			}
			V::At { list, index } => {
				let list = self.eval(list, record)?;
				match self.eval(index, record)? {
					Datum::Number(index) if index >= 0.0 && index.fract() == 0.0 => {
						self.items(&list).get(index as usize).cloned().unwrap_or(Datum::Missing)
					}
					_ => Datum::Missing,
				}
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
	fn output(&mut self, value: &Datum) -> Result<Value> {
		Ok(match value {
			Datum::Missing | Datum::Null => Value::Null,
			Datum::Bool(v) => Value::Bool(*v),
			Datum::Number(v) if *v >= 0.0 && *v <= u32::MAX as f64 && v.fract() == 0.0 => Value::Int(*v as u32),
			Datum::Number(v) => Value::Float(*v),
			Datum::Text(v) => Value::Str(self.intern(v)),
			Datum::Slice(a, b) => Value::Slice(*a, *b),
			Datum::Node(id) => Value::Node(*id),
			Datum::Nodes(list) => Value::Nodes(*list),
			Datum::Array(items) => {
				if !items.is_empty() && items.iter().all(|v| matches!(v, Datum::Text(_) | Datum::Slice(..))) {
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
		let node = |name: &str| {
			fields
				.iter()
				.find(|(k, _)| *k == name)
				.and_then(|(_, v)| if let Datum::Node(id) = v { Some(*id) } else { None })
		};
		let kind = match ty {
			"js.VariableDeclarator" => Some(NodeKind::VariableDeclarator {
				id: node("id").ok_or_else(|| error(start, end, Code::Expected, Some("a pattern")))?,
				init: node("init"),
			}),
			"js.VariableDeclaration" => {
				let declarations = fields
					.iter()
					.find(|(k, _)| *k == "declarations")
					.map(|(_, v)| v.clone())
					.unwrap_or(Datum::Missing);
				let Value::Nodes(declarations) = self.output(&declarations)? else {
					return fail(start, end, Code::Expected, Some("declarations"));
				};
				let kind = fields
					.iter()
					.find(|(k, _)| *k == "kind")
					.and_then(|(_, v)| self.text_of(v));
				Some(NodeKind::VariableDeclaration {
					declarations,
					kind: match kind {
						Some("var") => VariableKind::Var,
						Some("let") => VariableKind::Let,
						_ => VariableKind::Const,
					},
				})
			}
			"js.Identifier" => {
				let name = fields
					.iter()
					.find(|(k, _)| *k == "name")
					.and_then(|(_, v)| self.text_of(v))
					.unwrap_or("")
					.to_owned();
				Some(NodeKind::Identifier {
					name: self.intern(&name),
				})
			}
			_ if ty.starts_with("js.") => return fail(start, end, Code::Expected, Some("a native constructor")),
			_ => None,
		};
		if let Some(kind) = kind {
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
			scope: None,
		});
		Ok(self.ast().add(NodeKind::Host(index), start, end))
	}
	fn call(&mut self, rule: usize, event: Datum, ty: Option<&str>) -> Result<NodeId> {
		if self.active.len() > 256 {
			return fail(self.at, self.at, Code::TreeSize, None);
		}
		let schema = &self.plan.rules[rule];
		let record = self.records.len();
		self.records.push(Record {
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
		let result = self.form(&schema.form, record, "");
		self.active.pop();
		result?;
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
				let len = closing_tag(self.rest(), name)
					.ok_or_else(|| error(self.at, self.at, Code::Unclosed, Some(name)))?;
				self.at += len as u32;
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
	fn form(&mut self, form: &Form, record: usize, follow: &str) -> Result<()> {
		match form {
			Form::Seq(items) => {
				for (i, item) in items.iter().enumerate() {
					let mut stops = Vec::new();
					self.follow(&items[i + 1..], &mut stops, 0);
					let next = if stops.is_empty() {
						follow.to_owned()
					} else {
						format!("{} {}", stops.join(" "), follow)
					};
					self.form(item, record, &next)?;
				}
			}
			Form::Emit { into, value } => {
				let value = self.eval(value, record)?;
				self.write(record, into, value);
			}
			Form::Read { reader, into, input } => {
				let input = input.as_ref().map(|v| self.eval(v, record)).transpose()?;
				let value = self.read(reader, record, input, follow)?;
				if let Some(into) = into {
					self.write(record, into, value);
				}
			}
			Form::Choice { alternatives, .. } => {
				let choice = alternatives
					.iter()
					.find(|form| self.first(form, self.at, 0) != Some(false))
					.ok_or_else(|| error(self.at, self.at, Code::UnexpectedToken, None))?;
				self.form(choice, record, follow)?;
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
				while max.is_none_or(|max| values.len() < max) && self.first(body, self.at, 0) != Some(false) {
					if self.at >= self.limit && values.len() >= *min {
						break;
					}
					let start = self.at;
					self.iteration
						.push(locals.iter().map(|key| (key.as_ref().into(), Datum::Missing)).collect());
					let result = self
						.form(body, record, follow)
						.and_then(|()| self.eval(yield_value, record));
					self.iteration.pop();
					values.push(result?);
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
	fn follow(&self, forms: &[Form], out: &mut Vec<String>, depth: usize) {
		if depth > 32 {
			return;
		}
		for form in forms {
			match form {
				Form::Read {
					reader: Reader::Token { text, .. },
					..
				} => {
					out.push(text.to_string());
					break;
				}
				Form::Read {
					reader: Reader::Rule(rule),
					..
				} => {
					self.follow(std::slice::from_ref(&self.plan.rules[*rule].form), out, depth + 1);
					break;
				}
				Form::Seq(items) => {
					self.follow(items, out, depth + 1);
					if !items.is_empty() {
						break;
					}
				}
				Form::Choice { alternatives, .. } => {
					for f in alternatives {
						self.follow(std::slice::from_ref(f), out, depth + 1);
					}
				}
				Form::Repeat { body, .. } => self.follow(std::slice::from_ref(body), out, depth + 1),
				Form::Emit { .. }
				| Form::Read {
					reader: Reader::Space { .. } | Reader::Test(_),
					..
				} => {}
				_ => break,
			}
		}
	}
	fn first(&self, form: &Form, at: u32, depth: usize) -> Option<bool> {
		if depth > 32 {
			return None;
		}
		let rest = &self.src[at as usize..self.limit as usize];
		match form {
			Form::Read {
				reader: Reader::Token { text, gap, word },
				..
			} => {
				let rest = if *gap == Gap::Space {
					rest.trim_start_matches(is_space)
				} else {
					rest
				};
				Some(rest.starts_with(text.as_ref()) && (!*word || !rest[text.len()..].starts_with(is_id_continue)))
			}
			Form::Read {
				reader: Reader::Rule(rule),
				..
			} => self.first(&self.plan.rules[*rule].form, at, depth + 1),
			Form::Read {
				reader: Reader::Space { min },
				..
			} if *min > 0 => Some(rest.starts_with(is_space)),
			Form::Seq(items) => items.iter().find_map(|f| self.first(f, at, depth + 1)),
			_ => None,
		}
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
						return fail(self.at, self.at, Code::Expected, Some(text));
					}
					let start = self.at;
					self.expect(text)?;
					Datum::Span(start, self.at, false)
				}
				Reader::Space { min } => {
					let start = self.at;
					self.space();
					if (self.at - start) < *min as u32 {
						return fail(start, start, Code::Expected, Some("whitespace"));
					}
					Datum::Span(start, self.at, false)
				}
				Reader::Test(value) => {
					if !self.eval(value, record)?.yes() {
						return fail(self.at, self.at, Code::UnexpectedToken, None);
					}
					Datum::Missing
				}
				Reader::Rule(rule) => Datum::Node(self.call(*rule, self.records[record].event.clone(), None)?),
				Reader::Javascript { entry, boundary } => self.javascript(*entry, *boundary, follow)?,
				Reader::HtmlChildren { mode, stop } => Datum::Nodes(self.children(*mode, stop)?),
				Reader::HtmlAttributes(mode) => Datum::Nodes(self.attributes(*mode, record)?),
				Reader::HtmlAttributeParts => self.attribute_parts(record)?,
				Reader::HtmlSingle(entry) => self.single(*entry)?,
				Reader::CssStylesheet => self.stylesheet()?,
			})
		})();
		let result = if result.is_ok() && input.is_some() {
			self.space();
			if self.at != self.limit {
				fail(self.at, self.at, Code::UnexpectedToken, None)
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
		let mut parser = Parser::<E>::new(
			&self.src[..self.limit as usize],
			self.at,
			self.options,
			follow,
			self.ast.take().unwrap(),
		);
		let result = parser.start().and_then(|()| match entry {
			Js::Program => parser.parse_program().map(|id| parser.list_of(&[id])),
			Js::AssignmentExpression => parser
				.parse_maybe_assign(ForInit::No, &mut None)
				.map(|id| parser.list_of(&[id])),
			Js::BindingIdentifier | Js::IdentifierReference => {
				parser.parse_ident(false).map(|id| parser.list_of(&[id]))
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
		let event = Datum::object([("raw", Datum::Slice(start, end)), ("decoded", decoded)]);
		let at = self.at;
		self.at = start;
		let node = self.call(self.plan.html.text, event, None);
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
		while self.at < self.limit {
			if let Stop::Prefixes(prefixes) = stop
				&& prefixes.iter().any(|p| self.matches(p))
			{
				break;
			}
			if self.matches("</") {
				if let Some(name) = name
					&& let Some(len) = closing_tag(self.rest(), name)
				{
					if matches!(stop, Stop::MatchingElement) {
						self.at += len as u32;
						break;
					}
					return fail(self.at, self.at + 1, Code::Unclosed, Some("block"));
				}
				if matches!(stop, Stop::MatchingElement) && self.plan.html.autoclose {
					break;
				}
				return fail(
					self.at,
					self.at + 1,
					Code::UnexpectedClose,
					Some(name.unwrap_or("element")),
				);
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
					let event = Datum::object([("data", Datum::Slice(data, end - 3))]);
					self.at = start;
					let node = self.call(self.plan.html.comment, event, None)?;
					self.ast().nodes[node.index() as usize].end = end;
					self.at = end;
					nodes.push(node);
					continue;
				}
				let next = self.peek_name(self.at + 1, false);
				if let Some(name) = name
					&& self.plan.html.autoclose
					&& super::closes(name, &self.src[next.0 as usize..next.1 as usize])
				{
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
				nodes.push(self.call(row.rule, Datum::Missing, None)?);
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
				if self.matches("</")
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
		if self.at == self.limit
			&& matches!(stop, Stop::MatchingElement)
			&& !self.src[..self.at as usize].ends_with('>')
		{
			return fail(self.limit, self.limit, Code::Unclosed, name);
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
			} else if self.char().is_some_and(|c| is_space(c) || c == '>') || self.matches("/>") {
				break;
			}
			if interpolate && self.matches(&self.plan.html.delimiters[0]) {
				self.at += self.plan.html.delimiters[0].len() as u32;
				let close = &self.plan.html.delimiters[1];
				let mut lexer = crate::lexer::Lexer::with(&self.src[..self.limit as usize], Default::default());
				lexer.set_pos(self.at);
				lexer.set_stops(close);
				loop {
					let token = lexer.next_token()?;
					if token.kind == crate::lexer::token::TokenKind::Eof {
						self.at = token.start;
						break;
					}
				}
				self.expect(close)?;
			} else {
				self.at += self.char().unwrap().len_utf8() as u32;
			}
		}
		if quote.is_some() || self.at == content {
			return fail(start, start, Code::Expected, Some("an attribute value"));
		}
		Ok((content, self.at, self.at, false))
	}
	fn scan_header(&mut self) -> Result<Datum> {
		let saved = self.at;
		let result = (|| {
			let mut attributes = Vec::new();
			loop {
				self.space();
				if self.at >= self.limit || self.matches(">") || self.matches("/>") {
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
				let (start, end) = self.peek_name(self.at, true);
				if start == end {
					break;
				}
				self.at = end;
				self.space();
				let value = if self.eat("=") {
					self.space();
					Some(self.value_span(self.plan.html.attribute_interpolations)?)
				} else {
					None
				};
				let mut fields = vec![
					("kind", Datum::Text("ordinary".into())),
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
				attributes.push(Datum::object(fields));
			}
			Ok(Datum::object([("attributes", Datum::array(attributes))]))
		})();
		self.at = saved;
		result
	}
	fn element(&mut self) -> Result<NodeId> {
		let start = self.at;
		self.at += 1;
		let span = self.peek_name(self.at, false);
		self.at = span.1;
		if span.0 == span.1 {
			return fail(self.at, self.at, Code::UnexpectedEof, None);
		}
		let name = &self.src[span.0 as usize..span.1 as usize];
		let identifier = |s: &str| {
			let mut chars = s.chars();
			chars.next().is_some_and(is_id_start) && chars.all(is_id_continue)
		};
		let facts = Datum::object([
			("validHtmlName", Datum::Bool(valid_name(name))),
			("identifier", Datum::Bool(identifier(name))),
			(
				"uppercaseInitial",
				Datum::Bool(name.starts_with(|c: char| c.is_uppercase())),
			),
			(
				"dottedIdentifier",
				Datum::Bool(name.contains('.') && name.split('.').all(identifier)),
			),
			(
				"namespace",
				name.split_once(':')
					.map_or(Datum::Missing, |(p, _)| Datum::Text(p.into())),
			),
		]);
		let header = self.scan_header()?;
		let event = Datum::object([
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
		let result = self.call(row.rule, event, row.node_type.as_deref());
		self.elements.pop();
		if let Ok(node) = result {
			self.ast().nodes[node.index() as usize].start = start;
		}
		result
	}
	fn attributes(&mut self, mode: AttributeMode, record: usize) -> Result<List> {
		if let Some(list) = self.elements.last().and_then(|e| e.attributes) {
			return Ok(list);
		}
		let mut nodes = Vec::new();
		loop {
			self.space();
			if self.at >= self.limit || self.matches(">") || self.matches("/>") {
				break;
			}
			if self.plan.html.attribute_comments == plan::AttributeComments::Javascript && self.attribute_comment() {
				continue;
			}
			if mode == AttributeMode::Normal
				&& let Some(row) = self.plan.html.attribute.iter().find(|p| self.matches(&p.prefix))
			{
				nodes.push(self.call(row.rule, Datum::Missing, None)?);
				continue;
			}
			nodes.push(self.attribute(mode)?);
		}
		let empty = self.eat("/");
		self.expect(">")?;
		let list = self.list(&nodes);
		if let Some(element) = self.elements.last_mut() {
			let name = &self.src[element.name.0 as usize..element.name.1 as usize];
			element.empty = empty || self.plan.html.void.iter().any(|s| s.as_ref() == name);
			element.attributes = Some(list);
			if element.content == Mode::Raw {
				let start = self.at;
				let mut end = start;
				while end < self.limit && closing_tag(&self.src[end as usize..self.limit as usize], name).is_none() {
					end += self.src[end as usize..].chars().next().unwrap().len_utf8() as u32;
				}
				let Datum::Object(event) = &mut self.records[record].event else {
					unreachable!()
				};
				Rc::make_mut(event).insert("rawChildren".into(), Datum::Span(start, end, false));
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
			let event = Datum::object([
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
			let node = self.call(row.rule, event, None);
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
				nodes.push(self.call(self.plan.html.plain_attribute.expression, Datum::Missing, None)?);
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
		let close = &self.plan.html.delimiters[1];
		self.expect(open)?;
		let value = self.javascript(entry, None, close)?;
		self.space();
		self.expect(close)?;
		Ok(value)
	}
	fn stylesheet(&mut self) -> Result<Datum> {
		let (children, comments) =
			super::css::read(self.src, &mut self.at, self.limit, self.ast.as_mut().unwrap(), None)?;
		Ok(Datum::object([
			("children", Datum::Nodes(children)),
			("comments", Datum::Nodes(comments)),
		]))
	}
}
