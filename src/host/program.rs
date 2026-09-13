use super::{Datum, plan};
use crate::interner::{Interner, StrId};
use plan::{Absence, AttributeMode, Boundary, Gap, Js, Mode, Relation, SpanPolicy, Stop};

#[derive(Clone, Debug, Default)]
struct Builder {
	pub strings: Vec<Box<str>>,
	pub constants: Vec<Datum>,
	pub objects: Vec<Vec<(Property, Datum)>>,
	pub rules: Vec<RuleTree>,
	pub dispatch: Vec<ExprTree>,
	pub types: Vec<Option<StrId>>,
	pub dispatch_rules: Vec<usize>,
	pub checkpoint_depth: usize,
}

#[derive(Clone, Debug)]
struct RuleTree {
	pub source: usize,
	pub ty: StrId,
	pub fields: Vec<(StrId, Absence)>,
	pub slots: usize,
	pub lookup: Vec<Option<usize>>,
	pub scopes: Vec<Option<usize>>,
	pub form: FormTree,
	pub strict: FormTree,
	pub regions: Vec<Region>,
	pub declares: Vec<Declare>,
	pub span: Option<SpanPolicy>,
}

#[derive(Clone, Debug)]
struct Region {
	pub parent: ExprTree,
	pub kind: plan::RegionKind,
	pub covers: ExprTree,
	pub when: Option<ExprTree>,
	pub each: Option<ExprTree>,
}

#[derive(Clone, Debug)]
struct Declare {
	pub patterns: ExprTree,
	pub into: ExprTree,
	pub kind: plan::DeclareKind,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Slot {
	Record(u32),
	Iteration(u32),
	Missing,
}

#[derive(Clone, Debug)]
enum FormTree {
	Seq(Vec<FormTree>),
	Choice {
		alternatives: Vec<FormTree>,
		disjoint: bool,
		first: Vec<Vec<plan::Prefix>>,
		expected: StrId,
	},
	Repeat {
		body: Box<FormTree>,
		min: usize,
		max: Option<usize>,
		locals: usize,
		yield_value: ExprTree,
		into: Slot,
	},
	Read {
		follow: Box<str>,
		reader: ReaderTree,
		into: Option<Slot>,
		input: Option<ExprTree>,
	},
	Emit {
		into: Slot,
		value: ExprTree,
	},
}

#[derive(Clone, Debug)]
enum ReaderTree {
	Token { expected: StrId, gap: Gap, word: bool },
	Space { min: usize },
	Test(ExprTree),
	Rule(usize),
	Javascript { entry: Js, boundary: Option<Boundary> },
	HtmlSingle(Js),
	HtmlAttributes(AttributeMode),
	HtmlAttributeParts,
	HtmlChildren { mode: Mode, stop: Stop },
	CssStylesheet,
}

#[derive(Clone, Debug)]
enum ExprTree {
	Slot(u32),
	Iteration(u32),
	Event(Key),
	RecordType,
	NameEq(StrId),
	Filter {
		list: Box<ExprTree>,
		predicate: Box<ExprTree>,
	},
	Exists(Box<ExprTree>),
	Concat(Vec<ExprTree>),
	Constant(Datum),
	Get {
		base: BaseTree,
		path: Vec<Path>,
	},
	Compare {
		relation: Relation,
		left: Box<ExprTree>,
		right: Option<Box<ExprTree>>,
	},
	Member {
		needle: Box<ExprTree>,
		strings: Vec<StrId>,
	},
	Choose {
		condition: Box<ExprTree>,
		yes: Box<ExprTree>,
		no: Box<ExprTree>,
	},
	FlatMap {
		list: Box<ExprTree>,
		body: Box<ExprTree>,
	},
	Length(Box<ExprTree>),
	At {
		list: Box<ExprTree>,
		index: Box<ExprTree>,
	},
	Construct(ConstructTree),
}

fn refers_binding(tree: &ExprTree) -> bool {
	use ExprTree as T;
	match tree {
		T::Slot(_) | T::Iteration(_) | T::Event(_) | T::RecordType | T::NameEq(_) | T::Constant(_) => false,
		T::Filter { list, predicate } => refers_binding(list) || refers_binding(predicate),
		T::Exists(inner) | T::Length(inner) => refers_binding(inner),
		T::Concat(items) => items.iter().any(refers_binding),
		T::Get { base, .. } => match base {
			BaseTree::Binding(_) => true,
			BaseTree::Value(inner) => refers_binding(inner),
			_ => false,
		},
		T::Compare { left, right, .. } => refers_binding(left) || right.as_deref().is_some_and(refers_binding),
		T::Member { needle, .. } => refers_binding(needle),
		T::Choose { condition, yes, no } => refers_binding(condition) || refers_binding(yes) || refers_binding(no),
		T::FlatMap { list, body } => refers_binding(list) || refers_binding(body),
		T::At { list, index } => refers_binding(list) || refers_binding(index),
		T::Construct(ConstructTree::Array(items)) => items.iter().any(refers_binding),
		T::Construct(ConstructTree::Record { fields, span, .. }) => {
			fields.iter().any(|(_, v)| refers_binding(v)) || refers_binding(span)
		}
	}
}

#[derive(Clone, Debug)]
enum ConstructTree {
	Array(Vec<ExprTree>),
	Record {
		node_type: StrId,
		fields: Vec<(StrId, ExprTree)>,
		span: Box<ExprTree>,
	},
}

#[derive(Clone, Debug)]
enum BaseTree {
	Value(Box<ExprTree>),
	Record,
	Event,
	Owner,
	Ancestors,
	Incoming,
	Scopes,
	Iteration,
	Slot(Slot),
	Region(usize),
	Binding(usize),
	Missing,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Path {
	Name(Property),
	Index(usize),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Property {
	pub key: Key,
	pub name: StrId,
}

impl Builder {
	#[cold]
	fn name(&mut self, name: &str) -> StrId {
		if let Some(i) = self.strings.iter().position(|s| s.as_ref() == name) {
			return StrId(i as u32);
		}
		let id = StrId(self.strings.len() as u32);
		self.strings.push(name.into());
		id
	}
	#[cold]
	fn property(&mut self, name: &str) -> Property {
		Property {
			key: Key::read(name),
			name: self.name(name),
		}
	}
	#[cold]
	fn constant(&mut self, value: &plan::Json) -> Datum {
		match value {
			plan::Json::Null => Datum::Null,
			plan::Json::Bool(v) => Datum::Bool(*v),
			plan::Json::Number(v) => Datum::Number(v.parse().unwrap_or(0.0)),
			plan::Json::String(v) => Datum::Text(self.name(v)),
			plan::Json::Array(items) => {
				let values: Vec<_> = items.iter().map(|v| self.constant(v)).collect();
				let start = self.constants.len() as u32;
				self.constants.extend(values);
				Datum::Constants(start, items.len() as u32)
			}
			plan::Json::Object(fields) => {
				let values = fields
					.iter()
					.map(|(k, v)| (self.property(k), self.constant(v)))
					.collect();
				let id = self.objects.len();
				self.objects.push(values);
				Datum::Object(id)
			}
		}
	}
	#[cold]
	pub(super) fn lower(plan: &plan::Plan) -> Self {
		let mut p = Self {
			strings: STATIC.iter().map(|(_, s)| (*s).into()).collect(),
			..Self::default()
		};
		for rule in &plan.rules {
			let mut c = Compiler {
				program: &mut p,
				rule,
				iteration: &[],
				bindings: Vec::new(),
			};
			let ty = c.program.name(&rule.node_type);
			let fields = rule.fields.iter().map(|(k, v)| (c.program.name(k), *v)).collect();
			let form = c.form(&rule.form);
			let regions = rule
				.regions
				.iter()
				.map(|r| {
					let each = r.each.as_ref().map(|e| c.expr(&e.list));
					if let Some(e) = &r.each {
						c.bindings.push(&e.binding);
					}
					let region = Region {
						each,
						parent: c.expr(&r.parent),
						kind: r.kind,
						covers: c.expr(&r.covers),
						when: r.when.as_ref().map(|w| c.expr(w)),
					};
					if r.each.is_some() {
						c.bindings.pop();
					}
					region
				})
				.collect();
			let declares = rule
				.declares
				.iter()
				.map(|d| Declare {
					patterns: c.expr(&d.patterns),
					into: c.expr(&d.into),
					kind: d.kind,
				})
				.collect();
			p.rules.push(RuleTree {
				source: p.rules.len(),
				ty,
				fields,
				slots: rule.fields.len() + rule.locals.len(),
				lookup: Vec::new(),
				scopes: Vec::new(),
				strict: form.clone(),
				form,
				regions,
				declares,
				span: rule.span,
			});
		}
		for row in &plan.html.elements {
			let mut c = Compiler {
				program: &mut p,
				rule: &plan.rules[plan.document],
				iteration: &[],
				bindings: Vec::new(),
			};
			let expr = c.expr(&row.when);
			p.dispatch.push(expr);
			let ty = row.node_type.as_ref().map(|ty| p.name(ty));
			p.types.push(ty);
		}
		for (i, rule) in plan.rules.iter().enumerate() {
			p.rules[i].lookup = p
				.strings
				.iter()
				.map(|name| rule.fields.keys().chain(rule.locals.iter()).position(|n| n == name))
				.collect();
			p.rules[i].scopes = p
				.strings
				.iter()
				.map(|name| rule.regions.iter().position(|r| r.id == *name))
				.collect();
		}
		for (i, row) in plan.html.elements.iter().enumerate() {
			let rule = if let Some(ty) = p.types[i] {
				let mut rule = p.rules[row.rule].clone();
				rule.ty = ty;
				let i = p.rules.len();
				p.rules.push(rule);
				i
			} else {
				row.rule
			};
			p.dispatch_rules.push(rule);
		}
		for rule in &mut p.rules {
			rule.specialize();
		}
		p.checkpoint_depth = 258 * p.rules.iter().map(|r| r.form.depth()).max().unwrap_or(1);
		p
	}
}

struct Compiler<'a, 'p> {
	program: &'p mut Builder,
	rule: &'a plan::Rule,
	iteration: &'a [Box<str>],
	bindings: Vec<&'a str>,
}

impl<'a> Compiler<'a, '_> {
	#[cold]
	fn slot(&self, name: &str) -> Slot {
		if let Some(i) = self
			.rule
			.fields
			.keys()
			.chain(self.rule.locals.iter())
			.position(|n| n.as_ref() == name)
		{
			Slot::Record(i as u32)
		} else if let Some(i) = self.iteration.iter().position(|n| n.as_ref() == name) {
			Slot::Iteration(i as u32)
		} else {
			Slot::Missing
		}
	}
	#[cold]
	fn expr(&mut self, expr: &'a plan::Value) -> ExprTree {
		use plan::Value as V;
		match expr {
			V::Constant(v) => ExprTree::Constant(self.program.constant(v)),
			V::Get { base, path } => {
				let mut rest = path.as_slice();
				let base = match base {
					plan::Base::Value(value) => {
						let value = self.expr(value);
						if let ExprTree::Get { base, path } = value {
							let mut path = path;
							path.extend(rest.iter().map(|p| match p {
								plan::Path::Name(n) => Path::Name(self.program.property(n)),
								plan::Path::Index(i) => Path::Index(*i),
							}));
							if let (BaseTree::Record, Some(Path::Name(p))) = (&base, path.first())
								&& !matches!(p.key, Key::Type | Key::Span | Key::Header | Key::Scopes)
							{
								let slot = self.slot(&self.program.strings[p.name.0 as usize]);
								path.remove(0);
								return if path.is_empty()
									&& let Slot::Record(i) = slot
								{
									ExprTree::Slot(i)
								} else {
									ExprTree::Get {
										base: BaseTree::Slot(slot),
										path,
									}
								};
							}
							return ExprTree::Get { base, path };
						}
						BaseTree::Value(Box::new(value))
					}
					plan::Base::Name(name) => {
						let first = match rest.first() {
							Some(plan::Path::Name(n)) => Some(n.as_ref()),
							_ => None,
						};
						match (name.as_ref(), first) {
							("record" | "locals", Some(n)) if !matches!(n, "type" | "span" | "header" | "scopes") => {
								rest = &rest[1..];
								BaseTree::Slot(self.slot(n))
							}
							("iteration", Some(n)) => {
								rest = &rest[1..];
								BaseTree::Slot(
									self.iteration
										.iter()
										.position(|k| k.as_ref() == n)
										.map_or(Slot::Missing, |i| Slot::Iteration(i as u32)),
								)
							}
							("scopes", Some(n)) => {
								rest = &rest[1..];
								self.rule
									.regions
									.iter()
									.position(|r| r.id.as_ref() == n)
									.map_or(BaseTree::Missing, BaseTree::Region)
							}
							("record" | "locals", _) => BaseTree::Record,
							("event", _) => BaseTree::Event,
							("owner", _) => BaseTree::Owner,
							("ancestors", _) => BaseTree::Ancestors,
							("incoming", _) => BaseTree::Incoming,
							("scopes", _) => BaseTree::Scopes,
							("iteration", _) => BaseTree::Iteration,
							(n, _) => self
								.bindings
								.iter()
								.rev()
								.position(|k| *k == n)
								.map_or(BaseTree::Missing, BaseTree::Binding),
						}
					}
				};
				let path: Vec<_> = rest
					.iter()
					.map(|p| match p {
						plan::Path::Name(n) => Path::Name(self.program.property(n)),
						plan::Path::Index(i) => Path::Index(*i),
					})
					.collect();
				match (&base, path.as_slice()) {
					(BaseTree::Slot(Slot::Record(i)), []) => ExprTree::Slot(*i),
					(BaseTree::Slot(Slot::Iteration(i)), []) => ExprTree::Iteration(*i),
					(BaseTree::Event, [Path::Name(p)]) => ExprTree::Event(p.key),
					(BaseTree::Record, [Path::Name(p)]) if p.key == Key::Type => ExprTree::RecordType,
					_ => ExprTree::Get { base, path },
				}
			}
			V::Compare {
				relation,
				left,
				right,
				set,
			} => {
				if let Some(set) = set {
					let needle = Box::new(self.expr(&set.needle));
					let mut strings: Vec<_> = set.strings.iter().map(|s| self.program.name(s)).collect();
					strings.sort_unstable_by_key(|id| id.0);
					ExprTree::Member { needle, strings }
				} else {
					let left = self.expr(left);
					let right = right.as_ref().map(|v| self.expr(v));
					if *relation == Relation::Equal {
						if let (ExprTree::Event(Key::Name), Some(ExprTree::Constant(Datum::Text(id)))) = (&left, &right)
						{
							return ExprTree::NameEq(*id);
						}
						if let (ExprTree::Constant(Datum::Text(id)), Some(ExprTree::Event(Key::Name))) = (&left, &right)
						{
							return ExprTree::NameEq(*id);
						}
					}
					if *relation == Relation::Less
						&& matches!(left,ExprTree::Constant(Datum::Number(v)) if v == 0.0)
						&& let Some(ExprTree::Length(list)) = right
					{
						return ExprTree::Exists(list);
					}
					ExprTree::Compare {
						relation: *relation,
						left: Box::new(left),
						right: right.map(Box::new),
					}
				}
			}
			V::Choose { condition, yes, no } => ExprTree::Choose {
				condition: Box::new(self.expr(condition)),
				yes: Box::new(self.expr(yes)),
				no: Box::new(self.expr(no)),
			},
			V::FlatMap { list, binding, body } => {
				let list = Box::new(self.expr(list));
				self.bindings.push(binding);
				let body = Box::new(self.expr(body));
				self.bindings.pop();
				let empty = |v: &ExprTree| {
					matches!(v, ExprTree::Constant(Datum::Constants(_, 0)))
						|| matches!(v, ExprTree::Construct(ConstructTree::Array(items)) if items.is_empty())
				};
				let item = |v: &ExprTree| matches!(v, ExprTree::Construct(ConstructTree::Array(items)) if matches!(items.as_slice(), [ExprTree::Get { base: BaseTree::Binding(0), path }] if path.is_empty()));
				if let ExprTree::Choose { condition, yes, no } = body.as_ref()
					&& empty(no) && item(yes)
				{
					return ExprTree::Filter {
						list,
						predicate: condition.clone(),
					};
				}
				// a map of each item to itself is the list
				if item(&body) {
					return *list;
				}
				if matches!(body.as_ref(),ExprTree::Get {base:BaseTree::Binding(0),path} if path.is_empty())
					&& let ExprTree::Construct(ConstructTree::Array(items)) = *list
				{
					return ExprTree::Concat(items);
				}
				ExprTree::FlatMap { list, body }
			}
			V::Length(v) => ExprTree::Length(Box::new(self.expr(v))),
			V::At { list, index } => ExprTree::At {
				list: Box::new(self.expr(list)),
				index: Box::new(self.expr(index)),
			},
			V::Construct(plan::Construct::Array(items)) => {
				ExprTree::Construct(ConstructTree::Array(items.iter().map(|v| self.expr(v)).collect()))
			}
			V::Construct(plan::Construct::Record {
				node_type,
				fields,
				span,
			}) => ExprTree::Construct(ConstructTree::Record {
				node_type: self.program.name(node_type.as_deref().unwrap_or("")),
				fields: fields
					.iter()
					.map(|(k, v)| (self.program.name(k), self.expr(v)))
					.collect(),
				span: Box::new(self.expr(span)),
			}),
		}
	}
	#[cold]
	fn form(&mut self, form: &'a plan::Form) -> FormTree {
		use plan::Form as F;
		match form {
			F::Seq(items) => FormTree::Seq(items.iter().map(|f| self.form(f)).collect()),
			F::Choice {
				alternatives,
				disjoint,
				first,
			} => FormTree::Choice {
				alternatives: alternatives.iter().map(|f| self.form(f)).collect(),
				disjoint: *disjoint,
				first: first.clone(),
				expected: self.program.name(
					&first
						.iter()
						.flatten()
						.map(|p| p.text.as_str())
						.collect::<Vec<_>>()
						.join(" or "),
				),
			},
			F::Repeat {
				body,
				min,
				max,
				locals,
				yield_value,
				into,
			} => {
				let into = self.slot(into);
				let saved = std::mem::replace(&mut self.iteration, locals);
				let body = Box::new(self.form(body));
				let yield_value = self.expr(yield_value);
				self.iteration = saved;
				FormTree::Repeat {
					body,
					min: *min,
					max: *max,
					locals: locals.len(),
					yield_value,
					into,
				}
			}
			F::Read {
				follow,
				reader,
				into,
				input,
			} => FormTree::Read {
				follow: follow.clone(),
				reader: match reader {
					plan::Reader::Token { text, gap, word } => ReaderTree::Token {
						expected: self.program.name(text),
						gap: *gap,
						word: *word,
					},
					plan::Reader::Space { min } => ReaderTree::Space { min: *min },
					plan::Reader::Test(v) => ReaderTree::Test(self.expr(v)),
					plan::Reader::Rule(r) => ReaderTree::Rule(*r),
					plan::Reader::Javascript { entry, boundary } => ReaderTree::Javascript {
						entry: *entry,
						boundary: *boundary,
					},
					plan::Reader::HtmlSingle(e) => ReaderTree::HtmlSingle(*e),
					plan::Reader::HtmlAttributes(m) => ReaderTree::HtmlAttributes(*m),
					plan::Reader::HtmlAttributeParts => ReaderTree::HtmlAttributeParts,
					plan::Reader::HtmlChildren { mode, stop } => ReaderTree::HtmlChildren {
						mode: *mode,
						stop: stop.clone(),
					},
					plan::Reader::CssStylesheet => ReaderTree::CssStylesheet,
				},
				into: into.as_ref().map(|n| self.slot(n)),
				input: input.as_ref().map(|v| self.expr(v)),
			},
			F::Emit { into, value } => FormTree::Emit {
				into: self.slot(into),
				value: self.expr(value),
			},
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// a property named key is a key of the event, and the variant says so
#[allow(clippy::enum_variant_names)]
pub(super) enum Key {
	Other,
	IsAwait,
	Alternate,
	Argument,
	Arguments,
	Async,
	AtDocument,
	Attributes,
	Bigint,
	Block,
	Body,
	Boolean,
	Callee,
	Cases,
	Children,
	Comments,
	Computed,
	Consequent,
	Cooked,
	Data,
	Declaration,
	Declarations,
	Decoded,
	Delegate,
	Directive,
	Discriminant,
	DottedIdentifier,
	Dynamic,
	Elements,
	End,
	Exported,
	Expression,
	Expressions,
	Finalizer,
	Flags,
	Generator,
	Handler,
	Header,
	Id,
	Identifier,
	Imported,
	Init,
	InnerSource,
	Key,
	Kind,
	Label,
	Left,
	Local,
	Meta,
	Method,
	Modifiers,
	Name,
	NameFacts,
	Namespace,
	Object,
	Operator,
	Optional,
	Options,
	Param,
	Params,
	Pattern,
	Prefix,
	Properties,
	Property,
	Quasi,
	Quasis,
	Quoted,
	Raw,
	RawChildren,
	RawName,
	Regex,
	Right,
	Scopes,
	Shorthand,
	Source,
	SourceType,
	Span,
	Specifiers,
	Start,
	Static,
	StaticText,
	SuperClass,
	Tag,
	Tail,
	Test,
	Text,
	Type,
	Update,
	UppercaseInitial,
	ValidHtmlName,
	Value,
}
impl Key {
	#[cold]
	fn read(name: &str) -> Self {
		match name {
			"is_await" => Self::IsAwait,
			"alternate" => Self::Alternate,
			"argument" => Self::Argument,
			"arguments" => Self::Arguments,
			"async" => Self::Async,
			"atDocument" => Self::AtDocument,
			"attributes" => Self::Attributes,
			"bigint" => Self::Bigint,
			"block" => Self::Block,
			"body" => Self::Body,
			"boolean" => Self::Boolean,
			"callee" => Self::Callee,
			"cases" => Self::Cases,
			"children" => Self::Children,
			"comments" => Self::Comments,
			"computed" => Self::Computed,
			"consequent" => Self::Consequent,
			"cooked" => Self::Cooked,
			"data" => Self::Data,
			"declaration" => Self::Declaration,
			"declarations" => Self::Declarations,
			"decoded" => Self::Decoded,
			"delegate" => Self::Delegate,
			"directive" => Self::Directive,
			"discriminant" => Self::Discriminant,
			"dottedIdentifier" => Self::DottedIdentifier,
			"dynamic" => Self::Dynamic,
			"elements" => Self::Elements,
			"end" => Self::End,
			"exported" => Self::Exported,
			"expression" => Self::Expression,
			"expressions" => Self::Expressions,
			"finalizer" => Self::Finalizer,
			"flags" => Self::Flags,
			"generator" => Self::Generator,
			"handler" => Self::Handler,
			"header" => Self::Header,
			"id" => Self::Id,
			"identifier" => Self::Identifier,
			"imported" => Self::Imported,
			"init" => Self::Init,
			"innerSource" => Self::InnerSource,
			"key" => Self::Key,
			"kind" => Self::Kind,
			"label" => Self::Label,
			"left" => Self::Left,
			"local" => Self::Local,
			"meta" => Self::Meta,
			"method" => Self::Method,
			"modifiers" => Self::Modifiers,
			"name" => Self::Name,
			"nameFacts" => Self::NameFacts,
			"namespace" => Self::Namespace,
			"object" => Self::Object,
			"operator" => Self::Operator,
			"optional" => Self::Optional,
			"options" => Self::Options,
			"param" => Self::Param,
			"params" => Self::Params,
			"pattern" => Self::Pattern,
			"prefix" => Self::Prefix,
			"properties" => Self::Properties,
			"property" => Self::Property,
			"quasi" => Self::Quasi,
			"quasis" => Self::Quasis,
			"quoted" => Self::Quoted,
			"raw" => Self::Raw,
			"rawChildren" => Self::RawChildren,
			"rawName" => Self::RawName,
			"regex" => Self::Regex,
			"right" => Self::Right,
			"scopes" => Self::Scopes,
			"shorthand" => Self::Shorthand,
			"source" => Self::Source,
			"sourceType" => Self::SourceType,
			"span" => Self::Span,
			"specifiers" => Self::Specifiers,
			"start" => Self::Start,
			"static" => Self::Static,
			"staticText" => Self::StaticText,
			"superClass" => Self::SuperClass,
			"tag" => Self::Tag,
			"tail" => Self::Tail,
			"test" => Self::Test,
			"text" => Self::Text,
			"type" => Self::Type,
			"update" => Self::Update,
			"uppercaseInitial" => Self::UppercaseInitial,
			"validHtmlName" => Self::ValidHtmlName,
			"value" => Self::Value,
			_ => Self::Other,
		}
	}
}

impl RuleTree {
	#[cold]
	fn specialize(&mut self) {
		self.form.specialize(self.ty);
		self.strict = self.form.clone().flatten();
		for region in &mut self.regions {
			region.parent.specialize(self.ty);
			region.covers.specialize(self.ty);
			if let Some(expr) = &mut region.when {
				expr.specialize(self.ty);
			}
			if let Some(expr) = &mut region.each {
				expr.specialize(self.ty);
			}
		}
		for declaration in &mut self.declares {
			declaration.patterns.specialize(self.ty);
			declaration.into.specialize(self.ty);
		}
	}
}
impl FormTree {
	#[cold]
	fn specialize(&mut self, ty: StrId) {
		match self {
			Self::Seq(items)
			| Self::Choice {
				alternatives: items, ..
			} => {
				for item in items {
					item.specialize(ty);
				}
			}
			Self::Repeat { body, yield_value, .. } => {
				body.specialize(ty);
				yield_value.specialize(ty);
			}
			Self::Read { reader, input, .. } => {
				if let Some(input) = input {
					input.specialize(ty);
				}
				if let ReaderTree::Test(expr) = reader {
					expr.specialize(ty);
				}
			}
			Self::Emit { value, .. } => value.specialize(ty),
		}
	}
}
impl ExprTree {
	#[cold]
	fn specialize(&mut self, ty: StrId) {
		match self {
			Self::RecordType => {
				*self = Self::Constant(Datum::Text(ty));
			}
			Self::Get { base, path } => {
				if let BaseTree::Value(value) = base {
					value.specialize(ty);
				}
				if matches!(base, BaseTree::Record) && matches!(path.as_slice(),[Path::Name(p)] if p.key == Key::Type) {
					*self = Self::Constant(Datum::Text(ty));
				}
			}
			Self::Member { needle, strings } => {
				needle.specialize(ty);
				if let Self::Constant(Datum::Text(id)) = needle.as_ref() {
					*self = Self::Constant(Datum::Bool(strings.binary_search_by_key(&id.0, |id| id.0).is_ok()));
				}
			}
			Self::Choose { condition, yes, no } => {
				condition.specialize(ty);
				yes.specialize(ty);
				no.specialize(ty);
				if let Self::Constant(value) = condition.as_ref() {
					let branch = if value.yes() { yes } else { no };
					*self = std::mem::replace(branch, Box::new(Self::Constant(Datum::Missing)))
						.as_ref()
						.clone();
				}
			}
			Self::Filter { list, predicate } => {
				list.specialize(ty);
				predicate.specialize(ty);
				if let Self::Constant(value) = predicate.as_ref() {
					*self = if value.yes() {
						list.as_ref().clone()
					} else {
						Self::Constant(Datum::Nodes(crate::ast::List::EMPTY))
					};
				}
			}
			Self::FlatMap { list, body } => {
				list.specialize(ty);
				body.specialize(ty);
			}
			Self::Length(list) | Self::Exists(list) => list.specialize(ty),
			Self::At { list, index } => {
				list.specialize(ty);
				index.specialize(ty);
			}
			Self::Compare { left, right, .. } => {
				left.specialize(ty);
				if let Some(right) = right {
					right.specialize(ty);
				}
			}
			Self::Construct(ConstructTree::Array(items)) | Self::Concat(items) => {
				for item in items {
					item.specialize(ty);
				}
			}
			Self::Construct(ConstructTree::Record { fields, span, .. }) => {
				for (_, value) in fields {
					value.specialize(ty);
				}
				span.specialize(ty);
			}
			_ => {}
		}
	}
}

impl FormTree {
	#[cold]
	fn flatten(self) -> Self {
		match self {
			Self::Seq(items) => {
				let mut out = Vec::new();
				for item in items {
					match item.flatten() {
						Self::Seq(items) => out.extend(items),
						item => out.push(item),
					}
				}
				if out.len() == 1 {
					out.pop().unwrap()
				} else {
					Self::Seq(out)
				}
			}
			Self::Choice {
				alternatives,
				disjoint,
				first,
				expected,
			} => Self::Choice {
				alternatives: alternatives.into_iter().map(Self::flatten).collect(),
				disjoint,
				first,
				expected,
			},
			Self::Repeat {
				body,
				min,
				max,
				locals,
				yield_value,
				into,
			} => Self::Repeat {
				body: Box::new(body.flatten()),
				min,
				max,
				locals,
				yield_value,
				into,
			},
			form => form,
		}
	}
}

#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum Symbol {
	Empty,
	TypeArrayExpression,
	TypeArrayPattern,
	TypeArrowFunctionExpression,
	TypeAssignmentExpression,
	TypeAssignmentPattern,
	TypeAwaitExpression,
	TypeBinaryExpression,
	TypeBlockStatement,
	TypeBreakStatement,
	TypeCallExpression,
	TypeCatchClause,
	TypeChainExpression,
	TypeClassBody,
	TypeClassDeclaration,
	TypeClassExpression,
	TypeConditionalExpression,
	TypeContinueStatement,
	TypeDebuggerStatement,
	TypeDoWhileStatement,
	TypeEmptyStatement,
	TypeExportAllDeclaration,
	TypeExportDefaultDeclaration,
	TypeExportNamedDeclaration,
	TypeExportSpecifier,
	TypeExpressionStatement,
	TypeForInStatement,
	TypeForOfStatement,
	TypeForStatement,
	TypeFunctionDeclaration,
	TypeFunctionExpression,
	TypeIdentifier,
	TypeIfStatement,
	TypeImportAttribute,
	TypeImportDeclaration,
	TypeImportDefaultSpecifier,
	TypeImportExpression,
	TypeImportNamespaceSpecifier,
	TypeImportSpecifier,
	TypeLabeledStatement,
	TypeLiteral,
	TypeLogicalExpression,
	TypeMemberExpression,
	TypeMetaProperty,
	TypeMethodDefinition,
	TypeNewExpression,
	TypeObjectExpression,
	TypeObjectPattern,
	TypePrivateIdentifier,
	TypeProgram,
	TypeProperty,
	TypePropertyDefinition,
	TypeRestElement,
	TypeReturnStatement,
	TypeSequenceExpression,
	TypeSpreadElement,
	TypeStaticBlock,
	TypeSuper,
	TypeSwitchCase,
	TypeSwitchStatement,
	TypeTaggedTemplateExpression,
	TypeTemplateElement,
	TypeTemplateLiteral,
	TypeThisExpression,
	TypeThrowStatement,
	TypeTryStatement,
	TypeUnaryExpression,
	TypeUpdateExpression,
	TypeVariableDeclaration,
	TypeVariableDeclarator,
	TypeWhileStatement,
	TypeWithStatement,
	TypeYieldExpression,
	WordAwaitUsing,
	WordConst,
	WordExpression,
	WordGet,
	WordInit,
	WordLet,
	WordModule,
	WordOrdinary,
	WordScript,
	WordSet,
	WordUsing,
	WordVar,
	WordConstructor,
	WordMethod,
	WordRaw,
	WordCooked,
	WordPattern,
	WordFlags,
}
impl Symbol {
	pub(super) const fn id(self) -> StrId {
		StrId(self as u32)
	}
}
const STATIC: &[(Symbol, &str)] = &[
	(Symbol::Empty, ""),
	(Symbol::TypeArrayExpression, "ArrayExpression"),
	(Symbol::TypeArrayPattern, "ArrayPattern"),
	(Symbol::TypeArrowFunctionExpression, "ArrowFunctionExpression"),
	(Symbol::TypeAssignmentExpression, "AssignmentExpression"),
	(Symbol::TypeAssignmentPattern, "AssignmentPattern"),
	(Symbol::TypeAwaitExpression, "AwaitExpression"),
	(Symbol::TypeBinaryExpression, "BinaryExpression"),
	(Symbol::TypeBlockStatement, "BlockStatement"),
	(Symbol::TypeBreakStatement, "BreakStatement"),
	(Symbol::TypeCallExpression, "CallExpression"),
	(Symbol::TypeCatchClause, "CatchClause"),
	(Symbol::TypeChainExpression, "ChainExpression"),
	(Symbol::TypeClassBody, "ClassBody"),
	(Symbol::TypeClassDeclaration, "ClassDeclaration"),
	(Symbol::TypeClassExpression, "ClassExpression"),
	(Symbol::TypeConditionalExpression, "ConditionalExpression"),
	(Symbol::TypeContinueStatement, "ContinueStatement"),
	(Symbol::TypeDebuggerStatement, "DebuggerStatement"),
	(Symbol::TypeDoWhileStatement, "DoWhileStatement"),
	(Symbol::TypeEmptyStatement, "EmptyStatement"),
	(Symbol::TypeExportAllDeclaration, "ExportAllDeclaration"),
	(Symbol::TypeExportDefaultDeclaration, "ExportDefaultDeclaration"),
	(Symbol::TypeExportNamedDeclaration, "ExportNamedDeclaration"),
	(Symbol::TypeExportSpecifier, "ExportSpecifier"),
	(Symbol::TypeExpressionStatement, "ExpressionStatement"),
	(Symbol::TypeForInStatement, "ForInStatement"),
	(Symbol::TypeForOfStatement, "ForOfStatement"),
	(Symbol::TypeForStatement, "ForStatement"),
	(Symbol::TypeFunctionDeclaration, "FunctionDeclaration"),
	(Symbol::TypeFunctionExpression, "FunctionExpression"),
	(Symbol::TypeIdentifier, "Identifier"),
	(Symbol::TypeIfStatement, "IfStatement"),
	(Symbol::TypeImportAttribute, "ImportAttribute"),
	(Symbol::TypeImportDeclaration, "ImportDeclaration"),
	(Symbol::TypeImportDefaultSpecifier, "ImportDefaultSpecifier"),
	(Symbol::TypeImportExpression, "ImportExpression"),
	(Symbol::TypeImportNamespaceSpecifier, "ImportNamespaceSpecifier"),
	(Symbol::TypeImportSpecifier, "ImportSpecifier"),
	(Symbol::TypeLabeledStatement, "LabeledStatement"),
	(Symbol::TypeLiteral, "Literal"),
	(Symbol::TypeLogicalExpression, "LogicalExpression"),
	(Symbol::TypeMemberExpression, "MemberExpression"),
	(Symbol::TypeMetaProperty, "MetaProperty"),
	(Symbol::TypeMethodDefinition, "MethodDefinition"),
	(Symbol::TypeNewExpression, "NewExpression"),
	(Symbol::TypeObjectExpression, "ObjectExpression"),
	(Symbol::TypeObjectPattern, "ObjectPattern"),
	(Symbol::TypePrivateIdentifier, "PrivateIdentifier"),
	(Symbol::TypeProgram, "Builder"),
	(Symbol::TypeProperty, "Property"),
	(Symbol::TypePropertyDefinition, "PropertyDefinition"),
	(Symbol::TypeRestElement, "RestElement"),
	(Symbol::TypeReturnStatement, "ReturnStatement"),
	(Symbol::TypeSequenceExpression, "SequenceExpression"),
	(Symbol::TypeSpreadElement, "SpreadElement"),
	(Symbol::TypeStaticBlock, "StaticBlock"),
	(Symbol::TypeSuper, "Super"),
	(Symbol::TypeSwitchCase, "SwitchCase"),
	(Symbol::TypeSwitchStatement, "SwitchStatement"),
	(Symbol::TypeTaggedTemplateExpression, "TaggedTemplateExpression"),
	(Symbol::TypeTemplateElement, "TemplateElement"),
	(Symbol::TypeTemplateLiteral, "TemplateLiteral"),
	(Symbol::TypeThisExpression, "ThisExpression"),
	(Symbol::TypeThrowStatement, "ThrowStatement"),
	(Symbol::TypeTryStatement, "TryStatement"),
	(Symbol::TypeUnaryExpression, "UnaryExpression"),
	(Symbol::TypeUpdateExpression, "UpdateExpression"),
	(Symbol::TypeVariableDeclaration, "VariableDeclaration"),
	(Symbol::TypeVariableDeclarator, "VariableDeclarator"),
	(Symbol::TypeWhileStatement, "WhileStatement"),
	(Symbol::TypeWithStatement, "WithStatement"),
	(Symbol::TypeYieldExpression, "YieldExpression"),
	(Symbol::WordAwaitUsing, "await using"),
	(Symbol::WordConst, "const"),
	(Symbol::WordExpression, "expression"),
	(Symbol::WordGet, "get"),
	(Symbol::WordInit, "init"),
	(Symbol::WordLet, "let"),
	(Symbol::WordModule, "module"),
	(Symbol::WordOrdinary, "ordinary"),
	(Symbol::WordScript, "script"),
	(Symbol::WordSet, "set"),
	(Symbol::WordUsing, "using"),
	(Symbol::WordVar, "var"),
	(Symbol::WordConstructor, "constructor"),
	(Symbol::WordMethod, "method"),
	(Symbol::WordRaw, "raw"),
	(Symbol::WordCooked, "cooked"),
	(Symbol::WordPattern, "pattern"),
	(Symbol::WordFlags, "flags"),
];

impl FormTree {
	#[cold]
	fn depth(&self) -> usize {
		1 + match self {
			Self::Seq(items)
			| Self::Choice {
				alternatives: items, ..
			} => items.iter().map(Self::depth).max().unwrap_or(0),
			Self::Repeat { body, .. } => body.depth(),
			_ => 0,
		}
	}
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Code(std::num::NonZeroU32);
impl Code {
	pub(super) fn index(self) -> usize {
		self.0.get() as usize - 1
	}
}
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Range {
	pub start: u32,
	pub len: u32,
}
impl Range {
	pub(super) fn indices(self) -> std::ops::Range<usize> {
		self.start as usize..(self.start + self.len) as usize
	}
}

#[derive(Clone, Debug, Default)]
pub(super) struct Program {
	pub strings: Vec<Box<str>>,
	/// The strings by text; a tree interns one of them the first time a node carries it.
	pub interner: Interner,
	/// The strings the walker's own nodes carry.
	pub keys: Keys,
	pub css: super::css::Names,
	/// The bytes a text run stops at to look closer: a tag, a delimiter, or a content prefix.
	pub text_stops: [u64; 4],
	pub constants: Vec<Datum>,
	pub objects: Vec<Vec<(Property, Datum)>>,
	pub rules: Vec<Rule>,
	pub dispatch: Vec<Code>,
	pub types: Vec<Option<StrId>>,
	pub dispatch_rules: Vec<usize>,
	pub checkpoint_depth: usize,
	pub exprs: Vec<Expr>,
	pub args: Vec<Code>,
	pub paths: Vec<Path>,
	pub sets: Vec<StrId>,
	pub fields: Vec<(StrId, Code)>,
	pub readers: Vec<Reader>,
	pub follows: Vec<Box<str>>,
	pub dispatch_names: Vec<Range>,
	pub dispatch_other: Range,
	pub dispatch_rows: Vec<(u32, Code)>,
}
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Keys {
	pub start: StrId,
	pub end: StrId,
	pub pattern: StrId,
	pub flags: StrId,
	pub raw: StrId,
	pub cooked: StrId,
	pub children: StrId,
	pub comments: StrId,
	pub attribute: StrId,
	pub name: StrId,
	pub value: StrId,
}
#[derive(Clone, Debug)]
pub(super) struct Rule {
	pub source: usize,
	pub ty: StrId,
	pub fields: Vec<(StrId, Absence)>,
	pub slots: usize,
	pub lookup: Vec<Option<usize>>,
	pub scopes: Vec<Option<usize>>,
	pub form: Form,
	pub strict: Form,
	pub regions: Vec<RegionCode>,
	pub declares: Vec<DeclareCode>,
	pub span: Option<SpanPolicy>,
	/// Field index and event key for a rule whose form only copies event facts; run without a form.
	pub leaf: Option<Box<[(u32, Key)]>>,
}
#[derive(Clone, Debug)]
pub(super) struct RegionCode {
	pub parent: Code,
	pub kind: plan::RegionKind,
	pub covers: Code,
	/// The slots `covers` names when it is a plain list of fields: read them, no evaluation.
	pub slots: Option<Box<[u32]>>,
	pub when: Option<Code>,
	/// `when` reads the iterated item, so it is tested per item; otherwise once, before `each` runs.
	pub when_item: bool,
	pub each: Option<Code>,
}
#[derive(Clone, Debug)]
pub(super) struct DeclareCode {
	pub patterns: Code,
	pub into: Code,
	pub kind: plan::DeclareKind,
}
#[derive(Clone, Debug)]
pub(super) enum Form {
	Seq(Box<[Form]>),
	Choice(Box<Choice>),
	Repeat(Box<Repeat>),
	Tokens(Box<[Token]>),
	Read {
		follow: u32,
		reader: u32,
		into: Option<Slot>,
		input: Option<Code>,
	},
	Emit {
		into: Slot,
		value: Code,
	},
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Token {
	Text {
		text: StrId,
		gap: Gap,
		word: bool,
		into: Option<Slot>,
	},
	Space {
		min: usize,
		into: Option<Slot>,
	},
}
#[derive(Clone, Debug)]
pub(super) struct Choice {
	pub alternatives: Vec<Form>,
	pub disjoint: bool,
	pub first: Vec<Vec<plan::Prefix>>,
	pub expected: StrId,
}
#[derive(Clone, Debug)]
pub(super) struct Repeat {
	pub body: Form,
	pub min: usize,
	pub max: Option<usize>,
	pub locals: usize,
	pub yield_value: Code,
	pub into: Slot,
}
#[derive(Clone, Debug)]
pub(super) enum Reader {
	Token {
		text: StrId,
		expected: StrId,
		gap: Gap,
		word: bool,
	},
	Space {
		min: usize,
	},
	Test(Code),
	Rule(usize),
	Javascript {
		entry: Js,
		boundary: Option<Boundary>,
	},
	HtmlSingle(Js),
	HtmlAttributes(AttributeMode),
	HtmlAttributeParts,
	HtmlChildren {
		mode: Mode,
		stop: Box<Stop>,
	},
	CssStylesheet,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Expr {
	Constant(Datum),
	Slot(u32),
	Iteration(u32),
	Event(Key),
	RecordType,
	NameEq(StrId),
	Get {
		base: Base,
		path: Range,
	},
	Member {
		needle: Code,
		strings: Range,
	},
	Compare {
		relation: Relation,
		left: Code,
		right: Option<Code>,
	},
	Not(Code),
	And(Code, Code),
	Or(Code, Code),
	Choose {
		condition: Code,
		yes: Code,
		no: Code,
	},
	Filter {
		list: Code,
		predicate: Code,
	},
	/// `filter(list, isType(item, strings))`, or its negation: the item's type decides, no binding.
	TypeFilter {
		list: Code,
		strings: Range,
		negate: bool,
	},
	FlatMap {
		list: Code,
		body: Code,
	},
	Length(Code),
	Exists(Code),
	At {
		list: Code,
		index: Code,
	},
	Construct(Construct),
	Concat(Range),
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Construct {
	Array(Range),
	Record {
		node_type: StrId,
		fields: Range,
		span: Code,
	},
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Base {
	Value(Code),
	Record,
	Event,
	Owner,
	Ancestors,
	Incoming,
	Scopes,
	Iteration,
	Slot(Slot),
	Region(u32),
	Binding(u32),
	Missing,
}
impl Program {
	#[cold]
	pub(super) fn lower(plan: &plan::Plan) -> Self {
		let b = Builder::lower(plan);
		let mut p = Self {
			strings: b.strings,
			constants: b.constants,
			objects: b.objects,
			types: b.types,
			dispatch_rules: b.dispatch_rules,
			checkpoint_depth: b.checkpoint_depth,
			..Self::default()
		};
		for rule in b.rules {
			let form = p.form(rule.form);
			let mut strict = p.form(rule.strict);
			strict.fuse(&p);
			let regions: Vec<RegionCode> = rule
				.regions
				.into_iter()
				.map(|r| {
					let when_item = r.when.as_ref().is_some_and(refers_binding);
					let covers = p.expr(r.covers);
					let slots = p.plain_slots(covers).map(Vec::into_boxed_slice);
					RegionCode {
						parent: p.expr(r.parent),
						kind: r.kind,
						covers,
						slots,
						when_item,
						when: r
							.when
							.map(|v| p.expr(v))
							.filter(|code| !matches!(p.exprs[code.index()], Expr::Constant(Datum::Bool(true)))),
						each: r.each.map(|v| p.expr(v)),
					}
				})
				.collect();
			let declares: Vec<DeclareCode> = rule
				.declares
				.into_iter()
				.map(|d| DeclareCode {
					patterns: p.expr(d.patterns),
					into: p.expr(d.into),
					kind: d.kind,
				})
				.collect();
			let leaf = if regions.is_empty()
				&& declares.is_empty()
				&& rule.slots == rule.fields.len()
				&& !p.strings[rule.ty.0 as usize].starts_with("js.")
			{
				let items: &[Form] = match &form {
					Form::Seq(items) => items,
					Form::Emit { .. } => std::slice::from_ref(&form),
					_ => &[],
				};
				let copies: Option<Vec<(u32, Key)>> = items
					.iter()
					.map(|item| match item {
						Form::Emit {
							into: Slot::Record(i),
							value,
						} => match p.exprs[value.index()] {
							Expr::Event(key) => Some((*i, key)),
							_ => None,
						},
						_ => None,
					})
					.collect();
				copies.filter(|c| !c.is_empty()).map(Vec::into_boxed_slice)
			} else {
				None
			};
			p.rules.push(Rule {
				source: rule.source,
				ty: rule.ty,
				fields: rule.fields,
				slots: rule.slots,
				lookup: rule.lookup,
				scopes: rule.scopes,
				form,
				strict,
				regions,
				declares,
				span: rule.span,
				leaf,
			});
		}
		for dispatch in b.dispatch {
			let code = p.expr(dispatch);
			p.dispatch.push(code);
		}
		for id in 0..p.strings.len() {
			let range = p.dispatch_for(Some(StrId(id as u32)));
			p.dispatch_names.push(range);
		}
		p.dispatch_other = p.dispatch_for(None);
		let mut name = |text: &str| match p.strings.iter().position(|s| s.as_ref() == text) {
			Some(i) => StrId(i as u32),
			None => {
				p.strings.push(text.into());
				StrId(p.strings.len() as u32 - 1)
			}
		};
		p.keys = Keys {
			start: name("start"),
			end: name("end"),
			pattern: name("pattern"),
			flags: name("flags"),
			raw: name("raw"),
			cooked: name("cooked"),
			children: name("children"),
			comments: name("comments"),
			attribute: name(&plan.html.plain_attribute.node_type),
			name: name(&plan.html.plain_attribute.name),
			value: name(&plan.html.plain_attribute.value),
		};
		p.css = super::css::Names::new(&mut name);
		let mut stop = |text: &str| {
			if let Some(&b) = text.as_bytes().first() {
				p.text_stops[b as usize / 64] |= 1 << (b % 64);
			}
		};
		stop("<");
		stop(&plan.html.delimiters[0]);
		for row in &plan.html.content {
			stop(&row.prefix);
		}
		p.interner = Interner::sized(p.strings.len() * 16);
		for text in &p.strings {
			p.interner.intern(text);
		}
		p
	}
	/// The slots a covers expression names when it is only fields, singly or in lists.
	#[cold]
	fn plain_slots(&self, code: Code) -> Option<Vec<u32>> {
		match &self.exprs[code.index()] {
			Expr::Slot(i) => Some(vec![*i]),
			Expr::Construct(Construct::Array(items)) | Expr::Concat(items) => self.args[items.indices()]
				.iter()
				.map(|code| self.plain_slots(*code))
				.collect::<Option<Vec<Vec<u32>>>>()
				.map(|lists| lists.concat()),
			_ => None,
		}
	}
	#[cold]
	fn args(&mut self, items: Vec<ExprTree>) -> Range {
		let items: Vec<_> = items.into_iter().map(|e| self.expr(e)).collect();
		let range = Range {
			start: self.args.len() as u32,
			len: items.len() as u32,
		};
		self.args.extend(items);
		range
	}
	#[cold]
	fn expr(&mut self, expr: ExprTree) -> Code {
		let expr = match expr {
			ExprTree::Constant(v) => Expr::Constant(v),
			ExprTree::Slot(i) => Expr::Slot(i),
			ExprTree::Iteration(i) => Expr::Iteration(i),
			ExprTree::Event(k) => Expr::Event(k),
			ExprTree::RecordType => Expr::RecordType,
			ExprTree::NameEq(s) => Expr::NameEq(s),
			ExprTree::Get { base, path } => {
				let base = match base {
					BaseTree::Value(v) => Base::Value(self.expr(*v)),
					BaseTree::Record => Base::Record,
					BaseTree::Event => Base::Event,
					BaseTree::Owner => Base::Owner,
					BaseTree::Ancestors => Base::Ancestors,
					BaseTree::Incoming => Base::Incoming,
					BaseTree::Scopes => Base::Scopes,
					BaseTree::Iteration => Base::Iteration,
					BaseTree::Slot(s) => Base::Slot(s),
					BaseTree::Region(i) => Base::Region(i as u32),
					BaseTree::Binding(i) => Base::Binding(i as u32),
					BaseTree::Missing => Base::Missing,
				};
				let range = Range {
					start: self.paths.len() as u32,
					len: path.len() as u32,
				};
				self.paths.extend(path);
				Expr::Get { base, path: range }
			}
			ExprTree::Member { needle, strings } => {
				let needle = self.expr(*needle);
				let range = Range {
					start: self.sets.len() as u32,
					len: strings.len() as u32,
				};
				self.sets.extend(strings);
				Expr::Member { needle, strings: range }
			}
			ExprTree::Compare { relation, left, right } => Expr::Compare {
				relation,
				left: self.expr(*left),
				right: right.map(|v| self.expr(*v)),
			},
			ExprTree::Choose { condition, yes, no } => {
				let condition = self.expr(*condition);
				match (*yes, *no) {
					(ExprTree::Constant(Datum::Bool(false)), ExprTree::Constant(Datum::Bool(true))) => {
						Expr::Not(condition)
					}
					(ExprTree::Constant(Datum::Bool(true)), no) if no.boolean() => Expr::Or(condition, self.expr(no)),
					(yes, ExprTree::Constant(Datum::Bool(false))) if yes.boolean() => {
						Expr::And(condition, self.expr(yes))
					}
					(yes, no) => Expr::Choose {
						condition,
						yes: self.expr(yes),
						no: self.expr(no),
					},
				}
			}
			ExprTree::Filter { list, predicate } => {
				let (negate, test) = match predicate.as_ref() {
					ExprTree::Choose { condition, yes, no }
						if matches!(yes.as_ref(), ExprTree::Constant(Datum::Bool(false)))
							&& matches!(no.as_ref(), ExprTree::Constant(Datum::Bool(true))) =>
					{
						(true, condition.as_ref())
					}
					other => (false, other),
				};
				if let ExprTree::Member { needle, strings } = test
					&& let ExprTree::Get {
						base: BaseTree::Binding(0),
						path,
					} = needle.as_ref()
					&& let [Path::Name(prop)] = path.as_slice()
					&& prop.key == Key::Type
				{
					let range = Range {
						start: self.sets.len() as u32,
						len: strings.len() as u32,
					};
					self.sets.extend(strings.iter().copied());
					let list = self.expr(*list);
					Expr::TypeFilter {
						list,
						strings: range,
						negate,
					}
				} else {
					Expr::Filter {
						list: self.expr(*list),
						predicate: self.expr(*predicate),
					}
				}
			}
			ExprTree::FlatMap { list, body } => Expr::FlatMap {
				list: self.expr(*list),
				body: self.expr(*body),
			},
			ExprTree::Length(v) => Expr::Length(self.expr(*v)),
			ExprTree::Exists(v) => Expr::Exists(self.expr(*v)),
			ExprTree::At { list, index } => Expr::At {
				list: self.expr(*list),
				index: self.expr(*index),
			},
			ExprTree::Concat(items) => Expr::Concat(self.args(items)),
			ExprTree::Construct(ConstructTree::Array(items)) => Expr::Construct(Construct::Array(self.args(items))),
			ExprTree::Construct(ConstructTree::Record {
				node_type,
				fields,
				span,
			}) => {
				let span = self.expr(*span);
				let fields: Vec<_> = fields.into_iter().map(|(name, v)| (name, self.expr(v))).collect();
				let range = Range {
					start: self.fields.len() as u32,
					len: fields.len() as u32,
				};
				self.fields.extend(fields);
				Expr::Construct(Construct::Record {
					node_type,
					fields: range,
					span,
				})
			}
		};
		let code = Code(std::num::NonZeroU32::new(self.exprs.len() as u32 + 1).unwrap());
		self.exprs.push(expr);
		code
	}
	#[cold]
	fn form(&mut self, form: FormTree) -> Form {
		match form {
			FormTree::Seq(items) => Form::Seq(items.into_iter().map(|f| self.form(f)).collect()),
			FormTree::Choice {
				alternatives,
				disjoint,
				first,
				expected,
			} => Form::Choice(Box::new(Choice {
				alternatives: alternatives.into_iter().map(|f| self.form(f)).collect(),
				disjoint,
				first,
				expected,
			})),
			FormTree::Repeat {
				body,
				min,
				max,
				locals,
				yield_value,
				into,
			} => Form::Repeat(Box::new(Repeat {
				body: self.form(*body),
				min,
				max,
				locals,
				yield_value: self.expr(yield_value),
				into,
			})),
			FormTree::Emit { into, value } => Form::Emit {
				into,
				value: self.expr(value),
			},
			FormTree::Read {
				follow,
				reader,
				into,
				input,
			} => {
				let reader = match reader {
					ReaderTree::Token {
						expected, gap, word, ..
					} => Reader::Token {
						text: expected,
						expected,
						gap,
						word,
					},
					ReaderTree::Space { min } => Reader::Space { min },
					ReaderTree::Test(v) => Reader::Test(self.expr(v)),
					ReaderTree::Rule(i) => Reader::Rule(i),
					ReaderTree::Javascript { entry, boundary } => Reader::Javascript { entry, boundary },
					ReaderTree::HtmlSingle(e) => Reader::HtmlSingle(e),
					ReaderTree::HtmlAttributes(m) => Reader::HtmlAttributes(m),
					ReaderTree::HtmlAttributeParts => Reader::HtmlAttributeParts,
					ReaderTree::HtmlChildren { mode, stop } => Reader::HtmlChildren {
						mode,
						stop: Box::new(stop),
					},
					ReaderTree::CssStylesheet => Reader::CssStylesheet,
				};
				let index = self.readers.len() as u32;
				self.readers.push(reader);
				let at = self.follows.len() as u32;
				self.follows.push(follow);
				Form::Read {
					follow: at,
					reader: index,
					into,
					input: input.map(|e| self.expr(e)),
				}
			}
		}
	}
}

impl Form {
	#[cold]
	fn fuse(&mut self, program: &Program) {
		match self {
			Self::Read {
				reader,
				into,
				input: None,
				..
			} => {
				let token = match program.readers[*reader as usize] {
					Reader::Token { text, gap, word, .. } => Token::Text {
						text,
						gap,
						word,
						into: *into,
					},
					Reader::Space { min } => Token::Space { min, into: *into },
					_ => return,
				};
				*self = Self::Tokens(Box::new([token]));
			}
			Self::Seq(items) => {
				let mut out = Vec::new();
				let mut tokens = Vec::new();
				for mut form in std::mem::take(items).into_vec() {
					form.fuse(program);
					if let Self::Tokens(run) = form {
						tokens.extend(run.into_vec());
					} else {
						if !tokens.is_empty() {
							out.push(Self::Tokens(std::mem::take(&mut tokens).into_boxed_slice()));
						}
						out.push(form);
					}
				}
				if !tokens.is_empty() {
					out.push(Self::Tokens(tokens.into_boxed_slice()));
				}
				*self = if out.len() == 1 {
					out.pop().unwrap()
				} else {
					Self::Seq(out.into_boxed_slice())
				};
			}
			Self::Choice(choice) => {
				for form in &mut choice.alternatives {
					form.fuse(program);
				}
			}
			Self::Repeat(repeat) => repeat.body.fuse(program),
			_ => {}
		}
	}
}

impl ExprTree {
	#[cold]
	fn boolean(&self) -> bool {
		match self {
			Self::Constant(Datum::Bool(_))
			| Self::NameEq(_)
			| Self::Member { .. }
			| Self::Compare { .. }
			| Self::Exists(_) => true,
			Self::Choose { yes, no, .. } => yes.boolean() && no.boolean(),
			_ => false,
		}
	}
}

impl Program {
	#[cold]
	fn dispatch_for(&mut self, name: Option<StrId>) -> Range {
		let start = self.dispatch_rows.len() as u32;
		for i in 0..self.dispatch.len() {
			let code = self.name_test(self.dispatch[i], name);
			if matches!(self.exprs[code.index()],Expr::Constant(value) if !value.yes()) {
				continue;
			}
			self.dispatch_rows.push((i as u32, code));
			if matches!(self.exprs[code.index()], Expr::Constant(Datum::Bool(true))) {
				break;
			}
		}
		Range {
			start,
			len: self.dispatch_rows.len() as u32 - start,
		}
	}
	#[cold]
	fn name_test(&mut self, code: Code, name: Option<StrId>) -> Code {
		let expr = match self.exprs[code.index()] {
			Expr::NameEq(id) => Expr::Constant(Datum::Bool(name == Some(id))),
			Expr::Member { needle, strings } if matches!(self.exprs[needle.index()], Expr::Event(Key::Name)) => {
				Expr::Constant(Datum::Bool(
					name.is_some_and(|id| self.sets[strings.indices()].contains(&id)),
				))
			}
			Expr::Not(value) => {
				let value = self.name_test(value, name);
				match self.exprs[value.index()] {
					Expr::Constant(v) => Expr::Constant(Datum::Bool(!v.yes())),
					_ => Expr::Not(value),
				}
			}
			Expr::And(left, right) => {
				let left = self.name_test(left, name);
				match self.exprs[left.index()] {
					Expr::Constant(v) if !v.yes() => Expr::Constant(Datum::Bool(false)),
					Expr::Constant(_) => return self.name_test(right, name),
					_ => {
						let right = self.name_test(right, name);
						match self.exprs[right.index()] {
							Expr::Constant(Datum::Bool(true)) => return left,
							_ => Expr::And(left, right),
						}
					}
				}
			}
			Expr::Or(left, right) => {
				let left = self.name_test(left, name);
				match self.exprs[left.index()] {
					Expr::Constant(v) if v.yes() => Expr::Constant(Datum::Bool(true)),
					Expr::Constant(_) => return self.name_test(right, name),
					_ => {
						let right = self.name_test(right, name);
						match self.exprs[right.index()] {
							Expr::Constant(Datum::Bool(false)) => return left,
							_ => Expr::Or(left, right),
						}
					}
				}
			}
			Expr::Choose { condition, yes, no } => {
				let condition = self.name_test(condition, name);
				match self.exprs[condition.index()] {
					Expr::Constant(value) => return self.name_test(if value.yes() { yes } else { no }, name),
					_ => Expr::Choose {
						condition,
						yes: self.name_test(yes, name),
						no: self.name_test(no, name),
					},
				}
			}
			_ => return code,
		};
		let code = Code(std::num::NonZeroU32::new(self.exprs.len() as u32 + 1).unwrap());
		self.exprs.push(expr);
		code
	}
}
