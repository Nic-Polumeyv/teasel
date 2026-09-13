use std::collections::{BTreeMap, BTreeSet};

#[cold]
#[inline(never)]
fn map<T>(values: &mut dyn Iterator<Item = (Name, T)>) -> BTreeMap<Name, T> {
	let mut out = BTreeMap::new();
	for (key, value) in values {
		out.insert(key, value);
	}
	out
}
#[cold]
#[inline(never)]
fn try_map<T>(values: &mut dyn Iterator<Item = Result<(Name, T)>>) -> Result<BTreeMap<Name, T>> {
	let mut out = BTreeMap::new();
	for value in values {
		let (key, value) = value?;
		out.insert(key, value);
	}
	Ok(out)
}

#[path = "fold.rs"]
mod fold;

type Name = Box<str>;
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
	Null,
	Bool(bool),
	Number(Name),
	String(std::rc::Rc<str>),
	Array(Vec<Json>),
	Object(BTreeMap<Name, Json>),
}

#[derive(Clone, Debug)]
pub struct Plan {
	pub version: u32,
	pub document: usize,
	pub rules: Vec<Rule>,
	pub html: Html,
	pub(crate) stops: Vec<Name>,
	pub(super) program: super::program::Program,
}

#[derive(Clone, Debug)]
pub struct Html {
	pub delimiters: [Name; 2],
	pub attribute_interpolations: bool,
	pub attribute_comments: AttributeComments,
	pub autoclose: bool,
	pub trim_end: bool,
	pub void: Vec<Name>,
	pub text: usize,
	pub comment: usize,
	pub content: Vec<PrefixDispatch>,
	pub attribute: Vec<PrefixDispatch>,
	pub plain_attribute: PlainAttribute,
	pub elements: Vec<Dispatch>,
	pub directive_names: DirectiveNames,
	pub directives: Vec<NamedDispatch>,
}

#[derive(Clone, Debug)]
pub struct PrefixDispatch {
	pub prefix: Name,
	pub rule: usize,
}

#[derive(Clone, Debug)]
pub struct NamedDispatch {
	pub name: Name,
	pub rule: usize,
}

#[derive(Clone, Debug)]
pub struct PlainAttribute {
	pub node_type: Name,
	pub name: Name,
	pub value: Name,
	pub text: usize,
	pub expression: usize,
}

#[derive(Clone, Debug)]
pub struct DirectiveNames {
	pub prefix: Name,
	pub argument: Name,
	pub modifier: Name,
	pub require_argument: bool,
	pub dynamic: Option<[Name; 2]>,
	pub unknown: UnknownDirective,
}

#[derive(Clone, Debug)]
pub struct Dispatch {
	pub when: Value,
	pub rule: usize,
	pub node_type: Option<Name>,
	pub attributes: Option<AttributeMode>,
	pub content: Option<Mode>,
}

#[derive(Clone, Debug)]
pub struct Rule {
	pub name: Name,
	pub node_type: Name,
	pub fields: BTreeMap<Name, Absence>,
	pub locals: Vec<Name>,
	pub form: Form,
	pub regions: Vec<Region>,
	pub declares: Vec<Declare>,
	pub span: Option<SpanPolicy>,
	location: (usize, usize),
}

#[derive(Clone, Debug)]
pub enum Form {
	Seq(Vec<Form>),
	Choice {
		alternatives: Vec<Form>,
		disjoint: bool,
		first: Vec<Vec<Prefix>>,
	},
	Repeat {
		body: Box<Form>,
		min: usize,
		max: Option<usize>,
		locals: Vec<Name>,
		yield_value: Value,
		into: Name,
	},
	Read {
		follow: Name,
		reader: Reader,
		into: Option<Name>,
		input: Option<Value>,
	},
	Emit {
		into: Name,
		value: Value,
	},
}

#[derive(Clone, Debug)]
pub enum Reader {
	Token { text: Name, gap: Gap, word: bool },
	Space { min: usize },
	Test(Value),
	Rule(usize),
	Javascript { entry: Js, boundary: Option<Boundary> },
	HtmlSingle(Js),
	HtmlAttributes(AttributeMode),
	HtmlAttributeParts,
	HtmlChildren { mode: Mode, stop: Stop },
	CssStylesheet,
}

#[derive(Clone, Debug)]
pub enum Value {
	Constant(Json),
	Get {
		base: Base,
		path: Vec<Path>,
	},
	Compare {
		relation: Relation,
		left: Box<Value>,
		right: Option<Box<Value>>,
		set: Option<ConstantSet>,
	},
	Choose {
		condition: Box<Value>,
		yes: Box<Value>,
		no: Box<Value>,
	},
	FlatMap {
		list: Box<Value>,
		binding: std::rc::Rc<str>,
		body: Box<Value>,
	},
	Length(Box<Value>),
	At {
		list: Box<Value>,
		index: Box<Value>,
	},
	Construct(Construct),
}

#[derive(Clone, Debug)]
pub struct ConstantSet {
	pub needle: Box<Value>,
	pub strings: BTreeSet<std::rc::Rc<str>>,
}

#[derive(Clone, Debug)]
pub enum Construct {
	Array(Vec<Value>),
	Record {
		node_type: Option<Name>,
		fields: BTreeMap<Name, Value>,
		span: Box<Value>,
	},
}

#[derive(Clone, Debug)]
pub enum Base {
	Name(Name),
	Value(Box<Value>),
}

#[derive(Clone, Debug)]
pub enum Path {
	Name(Name),
	Index(usize),
}

#[derive(Clone, Debug)]
pub struct Region {
	pub id: Name,
	pub parent: Value,
	pub kind: RegionKind,
	pub covers: Value,
	pub when: Option<Value>,
	pub each: Option<Each>,
}

#[derive(Clone, Debug)]
pub struct Each {
	pub list: Value,
	pub binding: std::rc::Rc<str>,
}

#[derive(Clone, Debug)]
pub struct Declare {
	pub patterns: Value,
	pub into: Value,
	pub kind: DeclareKind,
}

#[derive(Clone, Debug)]
pub enum Stop {
	Prefixes(Vec<Name>),
	MatchingElement,
	DocumentEnd,
}

macro_rules! enums {
    ($($name:ident { $($variant:ident => $text:literal),+ $(,)? })+) => {$ (
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant),+ }
        impl $name {
            #[cold]
            fn read(node: &Node, context: &str) -> Result<Self> {
                match node.string(context)? {
                    $($text => Ok(Self::$variant),)+
                    text => Err(node.error(context, &format!("unknown {} {text:?}", stringify!($name)))),
                }
            }
        }
    )+};
}

enums! {
	Absence { Null => "null", Omit => "omit" }
	Gap { Space => "space*", None => "none" }
	Mode { Normal => "normal", Raw => "raw", Rcdata => "rcdata", Verbatim => "verbatim" }
	AttributeMode { Normal => "normal", Static => "static" }
	AttributeComments { Javascript => "javascript", None => "none" }
	UnknownDirective { PlainAttribute => "plain-attribute", WildcardRule => "wildcard-rule" }
	SpanPolicy { None => "none", ThroughNextTokenStart => "through-next-token-start" }
	Boundary { LastSharedWord => "last-shared-word" }
	RegionKind { Module => "module", Script => "script", Fragment => "fragment", Block => "block", Function => "function" }
	DeclareKind { Pattern => "pattern", Param => "param" }
	Relation { Equal => "equal", Less => "less", Present => "present" }
	Js {
		Expression => "expression", AssignmentExpression => "assignmentExpression", Pattern => "pattern",
		BindingIdentifier => "bindingIdentifier", IdentifierReference => "identifierReference", Params => "params",
		TypeParameters => "typeParameters", Statement => "statement", Program => "program"
	}
}

#[derive(Debug)]
struct Node {
	kind: NodeKind,
	location: (usize, usize),
	field: Option<Name>,
}

#[derive(Debug)]
enum NodeKind {
	Scalar(Json),
	Array(Vec<Node>),
	Object(Map<Node>),
}

impl Node {
	#[cold]
	fn error(&self, context: &str, reason: &str) -> String {
		let field = self
			.field
			.as_ref()
			.filter(|_| !context.contains(": field "))
			.map(|field| format!("field {field:?} "))
			.unwrap_or_default();
		format!("{}:{}: {context}: {field}{reason}", self.location.0, self.location.1)
	}

	#[cold]
	fn string(&self, context: &str) -> Result<&str> {
		if let NodeKind::Scalar(Json::String(s)) = &self.kind {
			Ok(s)
		} else {
			Err(self.error(context, "expected string"))
		}
	}

	#[cold]
	fn name(&self, context: &str) -> Result<Name> {
		self.string(context).map(Into::into)
	}

	#[cold]
	fn boolean(&self, context: &str) -> Result<bool> {
		if let NodeKind::Scalar(Json::Bool(b)) = self.kind {
			Ok(b)
		} else {
			Err(self.error(context, "expected boolean"))
		}
	}

	#[cold]
	fn integer(&self, context: &str) -> Result<usize> {
		if let NodeKind::Scalar(Json::Number(n)) = &self.kind
			&& let Ok(n) = n.parse()
		{
			return Ok(n);
		}
		Err(self.error(context, "expected nonnegative integer"))
	}

	#[cold]
	fn null(&self) -> bool {
		matches!(self.kind, NodeKind::Scalar(Json::Null))
	}

	#[cold]
	fn array(&self, context: &str) -> Result<&[Node]> {
		if let NodeKind::Array(a) = &self.kind {
			Ok(a)
		} else {
			Err(self.error(context, "expected array"))
		}
	}

	#[cold]
	fn names(&self, context: &str) -> Result<Vec<Name>> {
		self.array(context)?.iter().map(|n| n.name(context)).collect()
	}

	#[cold]
	fn pair(&self, context: &str) -> Result<[Name; 2]> {
		self.names(context)?
			.try_into()
			.map_err(|_| self.error(context, "expected two strings"))
	}

	#[cold]
	fn object(&self, context: &str) -> Result<&Map<Node>> {
		if let NodeKind::Object(o) = &self.kind {
			Ok(o)
		} else {
			Err(self.error(context, "expected object"))
		}
	}

	#[cold]
	fn properties(&self, context: &str, names: &[&str]) -> Result<()> {
		for key in self.object(context)?.keys() {
			if !names.contains(&key.as_ref()) {
				return Err(self.error(context, &format!("unknown field {key:?}")));
			}
		}
		Ok(())
	}

	#[cold]
	fn optional(&self, key: &str, context: &str) -> Result<Option<&Node>> {
		Ok(self.object(context)?.get(key))
	}

	#[cold]
	fn required(&self, key: &str, context: &str) -> Result<&Node> {
		self.optional(key, context)?
			.ok_or_else(|| self.error(context, &format!("missing field {key:?}")))
	}

	#[cold]
	fn json(&self) -> Json {
		match &self.kind {
			NodeKind::Scalar(j) => j.clone(),
			NodeKind::Array(a) => Json::Array(a.iter().map(Self::json).collect()),
			NodeKind::Object(o) => Json::Object(map(&mut o.iter().map(|(k, v)| (k.clone(), v.json())))),
		}
	}
}

struct JsonReader<'a> {
	text: &'a str,
	offset: usize,
	line: usize,
	column: usize,
	path: Vec<Name>,
}

impl<'a> JsonReader<'a> {
	#[cold]
	fn error(&self, reason: &str) -> String {
		format!(
			"{}:{}: {}: {reason}",
			self.line,
			self.column,
			if self.path.len() >= 2 && self.path[0].as_ref() == "rules" {
				format!("rule {}: field {:?}", self.path[1], self.path.last().unwrap())
			} else {
				format!("plan{}", self.path.iter().map(|s| format!(".{s}")).collect::<String>())
			}
		)
	}

	#[cold]
	fn peek(&self) -> Option<char> {
		self.text[self.offset..].chars().next()
	}

	#[cold]
	fn bump(&mut self) -> Option<char> {
		let c = self.peek()?;
		self.offset += c.len_utf8();
		if c == '\n' {
			self.line += 1;
			self.column = 1;
		} else {
			self.column += 1;
		}
		Some(c)
	}

	#[cold]
	fn whitespace(&mut self) {
		while matches!(self.peek(), Some(' ' | '\n' | '\r' | '\t')) {
			self.bump();
		}
	}

	#[cold]
	fn expect(&mut self, ch: char) -> Result<()> {
		if self.peek() == Some(ch) {
			self.bump();
			Ok(())
		} else {
			Err(self.error(&format!("expected {ch:?}")))
		}
	}

	#[cold]
	fn hex(&mut self) -> Result<u32> {
		let mut value = 0;
		for _ in 0..4 {
			let c = self
				.peek()
				.and_then(|c| c.to_digit(16))
				.ok_or_else(|| self.error("invalid unicode escape"))?;
			self.bump();
			value = value * 16 + c;
		}
		Ok(value)
	}

	#[cold]
	fn string(&mut self) -> Result<Name> {
		self.expect('"')?;
		let mut value = String::new();
		loop {
			match self.bump().ok_or_else(|| self.error("unterminated string"))? {
				'"' => return Ok(value.into()),
				'\\' => {
					let escaped = match self.bump().ok_or_else(|| self.error("unterminated escape"))? {
						'"' => '"',
						'\\' => '\\',
						'/' => '/',
						'b' => '\u{8}',
						'f' => '\u{c}',
						'n' => '\n',
						'r' => '\r',
						't' => '\t',
						'u' => {
							let mut code = self.hex()?;
							if (0xd800..=0xdbff).contains(&code) {
								self.expect('\\')?;
								self.expect('u')?;
								let low = self.hex()?;
								if !(0xdc00..=0xdfff).contains(&low) {
									return Err(self.error("invalid unicode surrogate pair"));
								}
								code = 0x10000 + ((code - 0xd800) << 10) + low - 0xdc00;
							}
							char::from_u32(code).ok_or_else(|| self.error("unpaired unicode surrogate"))?
						}
						_ => return Err(self.error("invalid string escape")),
					};
					value.push(escaped);
				}
				c if c < '\u{20}' => return Err(self.error("unescaped control character")),
				c => value.push(c),
			}
		}
	}

	#[cold]
	fn number(&mut self) -> Result<Json> {
		let start = self.offset;
		if self.peek() == Some('-') {
			self.bump();
		}
		match self.peek() {
			Some('0') => {
				self.bump();
			}
			Some('1'..='9') => {
				while matches!(self.peek(), Some('0'..='9')) {
					self.bump();
				}
			}
			_ => return Err(self.error("invalid number")),
		}
		if self.peek() == Some('.') {
			self.bump();
			self.digits()?;
		}
		if matches!(self.peek(), Some('e' | 'E')) {
			self.bump();
			if matches!(self.peek(), Some('+' | '-')) {
				self.bump();
			}
			self.digits()?;
		}
		Ok(Json::Number(self.text[start..self.offset].into()))
	}

	#[cold]
	fn digits(&mut self) -> Result<()> {
		if !matches!(self.peek(), Some('0'..='9')) {
			return Err(self.error("expected digit"));
		}
		while matches!(self.peek(), Some('0'..='9')) {
			self.bump();
		}
		Ok(())
	}

	#[cold]
	fn value(&mut self, depth: usize) -> Result<Node> {
		self.whitespace();
		if depth > 128 {
			return Err(self.error("JSON nesting exceeds 128"));
		}
		let location = (self.line, self.column);
		let field = self.path.last().cloned();
		let kind = match self.peek() {
			Some('"') => NodeKind::Scalar(Json::String(self.string()?.into())),
			Some('[') => {
				self.bump();
				self.whitespace();
				let mut items = Vec::new();
				if self.peek() != Some(']') {
					loop {
						items.push(self.value(depth + 1)?);
						self.whitespace();
						if self.peek() != Some(',') {
							break;
						}
						self.bump();
					}
				}
				self.expect(']')?;
				NodeKind::Array(items)
			}
			Some('{') => {
				self.bump();
				self.whitespace();
				let mut fields = Map::default();
				if self.peek() != Some('}') {
					loop {
						self.whitespace();
						let key = self.string()?;
						self.path.push(key.clone());
						if fields.contains_key(&key) {
							return Err(
								self.error(&format!("duplicate field {key:?}; absence policies must be consistent"))
							);
						}
						self.whitespace();
						self.expect(':')?;
						fields.insert(key, self.value(depth + 1)?);
						self.path.pop();
						self.whitespace();
						if self.peek() != Some(',') {
							break;
						}
						self.bump();
					}
				}
				self.expect('}')?;
				NodeKind::Object(fields)
			}
			Some('-' | '0'..='9') => NodeKind::Scalar(self.number()?),
			Some('t' | 'f' | 'n') => {
				let (word, value) = match self.peek().unwrap() {
					't' => ("true", Json::Bool(true)),
					'f' => ("false", Json::Bool(false)),
					_ => ("null", Json::Null),
				};
				for c in word.chars() {
					self.expect(c)?;
				}
				NodeKind::Scalar(value)
			}
			_ => return Err(self.error("expected JSON value")),
		};
		Ok(Node { kind, location, field })
	}
}

struct Decode<'a> {
	rules: &'a BTreeMap<Name, usize>,
	context: String,
}

impl Decode<'_> {
	#[cold]
	fn reference(&self, node: &Node) -> Result<usize> {
		let name = node.string(&self.context)?;
		self.rules
			.get(name)
			.copied()
			.ok_or_else(|| node.error(&self.context, &format!("unknown rule {name:?}")))
	}

	#[cold]
	fn value(&self, node: &Node) -> Result<Value> {
		let context = &self.context;
		let get = |key| node.required(key, context);
		let val = |key| self.value(get(key)?).map(Box::new);
		let op = get("op")?.string(context)?;
		let allowed: &[&str] = match op {
			"constant" => &["op", "value"],
			"get" => &["op", "base", "path"],
			"compare" => &["op", "relation", "left", "right"],
			"choose" => &["op", "condition", "yes", "no"],
			"flatMap" => &["op", "list", "as", "body"],
			"length" => &["op", "list"],
			"at" => &["op", "list", "index"],
			"construct" => match get("shape")?.string(context)? {
				"array" => &["op", "shape", "items"],
				"record" => &["op", "shape", "type", "fields", "span"],
				shape => return Err(node.error(context, &format!("unknown construct shape {shape:?}"))),
			},
			_ => return Err(node.error(context, &format!("unknown value operation {op:?}"))),
		};
		node.properties(context, allowed)?;
		Ok(match op {
			"constant" => Value::Constant(get("value")?.json()),
			"get" => Value::Get {
				base: if let Ok(name) = get("base")?.name(context) {
					Base::Name(name)
				} else {
					Base::Value(val("base")?)
				},
				path: get("path")?
					.array(context)?
					.iter()
					.map(|p| {
						if let Ok(name) = p.name(context) {
							Ok(Path::Name(name))
						} else {
							p.integer(context).map(Path::Index)
						}
					})
					.collect::<Result<_>>()?,
			},
			"compare" => {
				let relation = Relation::read(get("relation")?, context)?;
				let right = node
					.optional("right", context)?
					.map(|n| self.value(n).map(Box::new))
					.transpose()?;
				if (relation == Relation::Present) == right.is_some() {
					return Err(node.error(context, "compare field \"right\" is required only for equal/less"));
				}
				Value::Compare {
					relation,
					left: val("left")?,
					right,
					set: None,
				}
			}
			"choose" => Value::Choose {
				condition: val("condition")?,
				yes: val("yes")?,
				no: val("no")?,
			},
			"flatMap" => Value::FlatMap {
				list: val("list")?,
				binding: get("as")?.name(context)?.into(),
				body: val("body")?,
			},
			"length" => Value::Length(val("list")?),
			"at" => Value::At {
				list: val("list")?,
				index: val("index")?,
			},
			"construct" if get("shape")?.string(context)? == "array" => Value::Construct(Construct::Array(
				get("items")?
					.array(context)?
					.iter()
					.map(|n| self.value(n))
					.collect::<Result<_>>()?,
			)),
			"construct" => Value::Construct(Construct::Record {
				node_type: if get("type")?.null() {
					None
				} else {
					Some(get("type")?.name(context)?)
				},
				fields: try_map(
					&mut get("fields")?
						.object(context)?
						.iter()
						.map(|(k, v)| Ok((k.clone(), self.value(v)?))),
				)?,
				span: val("span")?,
			}),
			_ => unreachable!(),
		})
	}

	#[cold]
	fn reader(&self, node: &Node) -> Result<Reader> {
		let c = &self.context;
		let get = |key| node.required(key, c);
		let kind = get("kind")?.string(c)?;
		node.properties(
			c,
			match kind {
				"token" => &["kind", "text", "gap", "word"],
				"space" => &["kind", "min"],
				"test" => &["kind", "value"],
				"rule" => &["kind", "name"],
				"javascript" => &["kind", "entry", "boundary"],
				"html-single" => &["kind", "entry"],
				"html-attributes" => &["kind", "mode"],
				"html-attribute-parts" | "css-stylesheet" => &["kind"],
				"html-children" => &["kind", "mode", "stop"],
				_ => return Err(node.error(c, &format!("unknown reader {kind:?}"))),
			},
		)?;
		Ok(match kind {
			"token" => {
				let text = get("text")?.name(c)?;
				if text.is_empty() {
					return Err(node.error(c, "token text must not be empty"));
				}
				Reader::Token {
					text,
					gap: Gap::read(get("gap")?, c)?,
					word: get("word")?.boolean(c)?,
				}
			}
			"space" => Reader::Space {
				min: get("min")?.integer(c)?,
			},
			"test" => Reader::Test(self.value(get("value")?)?),
			"rule" => Reader::Rule(self.reference(get("name")?)?),
			"javascript" => Reader::Javascript {
				entry: Js::read(get("entry")?, c)?,
				boundary: node
					.optional("boundary", c)?
					.map(|n| Boundary::read(n, c))
					.transpose()?,
			},
			"html-single" => Reader::HtmlSingle(Js::read(get("entry")?, c)?),
			"html-attributes" => Reader::HtmlAttributes(AttributeMode::read(get("mode")?, c)?),
			"html-attribute-parts" => Reader::HtmlAttributeParts,
			"html-children" => {
				let stop = get("stop")?;
				stop.properties(c, &["prefixes", "matchingElement", "documentEnd"])?;
				if stop.object(c)?.len() != 1 {
					return Err(stop.error(c, "stop must have exactly one condition"));
				}
				let stop = if let Some(prefixes) = stop.optional("prefixes", c)? {
					let prefixes = prefixes.names(c)?;
					if prefixes.iter().any(|p| p.is_empty()) {
						return Err(node.error(c, "stop prefixes must not be empty"));
					}
					Stop::Prefixes(prefixes)
				} else if let Some(matching) = stop.optional("matchingElement", c)? {
					if !matching.boolean(c)? {
						return Err(matching.error(c, "matchingElement must be true"));
					}
					Stop::MatchingElement
				} else {
					if !stop.required("documentEnd", c)?.boolean(c)? {
						return Err(stop.error(c, "documentEnd must be true"));
					}
					Stop::DocumentEnd
				};
				Reader::HtmlChildren {
					mode: Mode::read(get("mode")?, c)?,
					stop,
				}
			}
			"css-stylesheet" => Reader::CssStylesheet,
			_ => unreachable!(),
		})
	}

	#[cold]
	fn form(&self, node: &Node) -> Result<Form> {
		let c = &self.context;
		let get = |key| node.required(key, c);
		let op = get("op")?.string(c)?;
		node.properties(
			c,
			match op {
				"seq" => &["op", "items"],
				"choice" => &["op", "alternatives"],
				"repeat" => &["op", "body", "min", "max", "locals", "yield", "into"],
				"read" => &["op", "reader", "into", "input"],
				"emit" => &["op", "into", "value"],
				_ => return Err(node.error(c, &format!("unknown form operation {op:?}"))),
			},
		)?;
		Ok(match op {
			"seq" => Form::Seq(
				get("items")?
					.array(c)?
					.iter()
					.map(|n| self.form(n))
					.collect::<Result<_>>()?,
			),
			"choice" => {
				let alternatives: Vec<_> = get("alternatives")?
					.array(c)?
					.iter()
					.map(|n| self.form(n))
					.collect::<Result<_>>()?;
				if alternatives.is_empty() {
					return Err(node.error(c, "choice requires an alternative"));
				}
				Form::Choice {
					alternatives,
					disjoint: false,
					first: Vec::new(),
				}
			}
			"repeat" => {
				let min = get("min")?.integer(c)?;
				let max = if get("max")?.null() {
					None
				} else {
					Some(get("max")?.integer(c)?)
				};
				if max.is_some_and(|max| min > max) {
					return Err(node.error(c, "repeat min exceeds max"));
				}
				Form::Repeat {
					body: Box::new(self.form(get("body")?)?),
					min,
					max,
					locals: get("locals")?.names(c)?,
					yield_value: self.value(get("yield")?)?,
					into: get("into")?.name(c)?,
				}
			}
			"read" => Form::Read {
				follow: "".into(),
				reader: self.reader(get("reader")?)?,
				into: node.optional("into", c)?.map(|n| n.name(c)).transpose()?,
				input: node.optional("input", c)?.map(|n| self.value(n)).transpose()?,
			},
			"emit" => Form::Emit {
				into: get("into")?.name(c)?,
				value: self.value(get("value")?)?,
			},
			_ => unreachable!(),
		})
	}

	#[cold]
	fn region(&self, node: &Node) -> Result<Region> {
		let c = &self.context;
		node.properties(c, &["id", "parent", "kind", "covers", "when", "each"])?;
		let get = |key| node.required(key, c);
		Ok(Region {
			id: get("id")?.name(c)?,
			parent: self.value(get("parent")?)?,
			kind: RegionKind::read(get("kind")?, c)?,
			covers: self.value(get("covers")?)?,
			when: node.optional("when", c)?.map(|n| self.value(n)).transpose()?,
			each: node
				.optional("each", c)?
				.map(|n| {
					n.properties(c, &["list", "as"])?;
					Ok::<_, String>(Each {
						list: self.value(n.required("list", c)?)?,
						binding: n.required("as", c)?.name(c)?.into(),
					})
				})
				.transpose()?,
		})
	}

	#[cold]
	fn rule(&self, name: &str, node: &Node) -> Result<Rule> {
		let c = &self.context;
		node.properties(c, &["type", "fields", "locals", "form", "regions", "declares", "span"])?;
		let get = |key| node.required(key, c);
		Ok(Rule {
			name: name.into(),
			node_type: get("type")?.name(c)?,
			fields: try_map(
				&mut get("fields")?
					.object(c)?
					.iter()
					.map(|(k, v)| Ok((k.clone(), Absence::read(v, &format!("{c}: field {k:?}"))?))),
			)?,
			locals: node
				.optional("locals", c)?
				.map(|n| n.names(c))
				.transpose()?
				.unwrap_or_default(),
			form: self.form(get("form")?)?,
			regions: node
				.optional("regions", c)?
				.map(|n| n.array(c)?.iter().map(|n| self.region(n)).collect::<Result<_>>())
				.transpose()?
				.unwrap_or_default(),
			declares: node
				.optional("declares", c)?
				.map(|n| {
					n.array(c)?
						.iter()
						.map(|n| {
							n.properties(c, &["patterns", "into", "kind"])?;
							Ok(Declare {
								patterns: self.value(n.required("patterns", c)?)?,
								into: self.value(n.required("into", c)?)?,
								kind: DeclareKind::read(n.required("kind", c)?, c)?,
							})
						})
						.collect::<Result<_>>()
				})
				.transpose()?
				.unwrap_or_default(),
			span: node.optional("span", c)?.map(|n| SpanPolicy::read(n, c)).transpose()?,
			location: node.location,
		})
	}

	#[cold]
	fn prefixes(&self, node: &Node) -> Result<Vec<PrefixDispatch>> {
		let c = &self.context;
		node.array(c)?
			.iter()
			.map(|n| {
				n.properties(c, &["prefix", "rule"])?;
				let prefix = n.required("prefix", c)?.name(c)?;
				if prefix.is_empty() {
					return Err(n.error(c, "dispatch prefix must not be empty"));
				}
				Ok(PrefixDispatch {
					prefix,
					rule: self.reference(n.required("rule", c)?)?,
				})
			})
			.collect()
	}

	#[cold]
	fn html(&self, node: &Node) -> Result<Html> {
		let c = &self.context;
		node.properties(
			c,
			&[
				"delimiters",
				"attributeInterpolations",
				"attributeComments",
				"autoclose",
				"trimEnd",
				"void",
				"text",
				"comment",
				"content",
				"attribute",
				"plainAttribute",
				"elements",
				"directiveNames",
				"directives",
			],
		)?;
		let get = |key| node.required(key, c);
		let plain = get("plainAttribute")?;
		plain.properties(c, &["type", "name", "value", "text", "expression"])?;
		let names = get("directiveNames")?;
		names.properties(
			c,
			&[
				"prefix",
				"argument",
				"modifier",
				"requireArgument",
				"dynamic",
				"unknown",
			],
		)?;
		let delimiters = get("delimiters")?.pair(c)?;
		if delimiters.iter().any(|s| s.is_empty()) {
			return Err(node.error(c, "delimiters must not be empty"));
		}
		Ok(Html {
			delimiters,
			attribute_interpolations: get("attributeInterpolations")?.boolean(c)?,
			attribute_comments: AttributeComments::read(get("attributeComments")?, c)?,
			autoclose: get("autoclose")?.boolean(c)?,
			trim_end: get("trimEnd")?.boolean(c)?,
			void: get("void")?.names(c)?,
			text: self.reference(get("text")?)?,
			comment: self.reference(get("comment")?)?,
			content: self.prefixes(get("content")?)?,
			attribute: self.prefixes(get("attribute")?)?,
			plain_attribute: PlainAttribute {
				node_type: plain.required("type", c)?.name(c)?,
				name: plain.required("name", c)?.name(c)?,
				value: plain.required("value", c)?.name(c)?,
				text: self.reference(plain.required("text", c)?)?,
				expression: self.reference(plain.required("expression", c)?)?,
			},
			elements: get("elements")?
				.array(c)?
				.iter()
				.map(|n| {
					n.properties(c, &["when", "rule", "type", "attributes", "content"])?;
					let attributes = n
						.optional("attributes", c)?
						.map(|n| AttributeMode::read(n, c))
						.transpose()?;
					if attributes == Some(AttributeMode::Normal) {
						return Err(n.error(c, "dispatch attributes must be static"));
					}
					Ok(Dispatch {
						when: self.value(n.required("when", c)?)?,
						rule: self.reference(n.required("rule", c)?)?,
						node_type: n.optional("type", c)?.map(|n| n.name(c)).transpose()?,
						attributes,
						content: n.optional("content", c)?.map(|n| Mode::read(n, c)).transpose()?,
					})
				})
				.collect::<Result<_>>()?,
			directive_names: DirectiveNames {
				prefix: names.required("prefix", c)?.name(c)?,
				argument: names.required("argument", c)?.name(c)?,
				modifier: names.required("modifier", c)?.name(c)?,
				require_argument: names.required("requireArgument", c)?.boolean(c)?,
				dynamic: if names.required("dynamic", c)?.null() {
					None
				} else {
					Some(names.required("dynamic", c)?.pair(c)?)
				},
				unknown: UnknownDirective::read(names.required("unknown", c)?, c)?,
			},
			directives: get("directives")?
				.array(c)?
				.iter()
				.map(|n| {
					n.properties(c, &["name", "rule"])?;
					Ok(NamedDispatch {
						name: n.required("name", c)?.name(c)?,
						rule: self.reference(n.required("rule", c)?)?,
					})
				})
				.collect::<Result<_>>()?,
		})
	}
}

impl Plan {
	#[cold]
	pub fn read(text: &str) -> Result<Self> {
		let mut reader = JsonReader {
			text,
			offset: 0,
			line: 1,
			column: 1,
			path: Vec::new(),
		};
		let node = reader.value(0)?;
		reader.whitespace();
		if reader.offset != text.len() {
			return Err(reader.error("trailing JSON input"));
		}
		node.properties("plan", &["version", "document", "rules", "html"])?;
		if node.required("version", "plan")?.integer("plan version")? != 1 {
			return Err(node.error("plan", "unsupported version"));
		}
		let nodes = node.required("rules", "plan")?.object("plan rules")?;
		let names: BTreeMap<_, _> = map(&mut nodes.keys().enumerate().map(|(i, n)| (n.clone(), i)));
		let decode = Decode {
			rules: &names,
			context: "plan".into(),
		};
		let mut plan = Self {
			version: 1,
			document: decode.reference(node.required("document", "plan")?)?,
			rules: nodes
				.iter()
				.map(|(name, node)| {
					Decode {
						rules: &names,
						context: format!("rule {name}"),
					}
					.rule(name, node)
				})
				.collect::<Result<_>>()?,
			html: decode.html(node.required("html", "plan")?)?,
			stops: Vec::new(),
			program: Default::default(),
		};
		plan.validate()?;
		fold::plan(&mut plan);
		compile(&mut plan);
		plan.program = super::program::Program::lower(&plan);
		Ok(plan)
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Type {
	Missing,
	Null,
	Bool,
	Number,
	String,
	Span,
	AttributeValue,
	Argument,
	Scope,
	Native(Js),
	Node(usize),
	List(Box<Type>),
	Record(Types),
	Union(Vec<Type>),
	Unknown,
}

impl Type {
	#[cold]
	fn union(types: impl IntoIterator<Item = Self>) -> Self {
		let mut unique = Vec::new();
		for ty in types {
			let members = if let Self::Union(members) = ty {
				members
			} else {
				vec![ty]
			};
			for member in members {
				if member != Self::Missing && !unique.contains(&member) {
					unique.push(member);
				}
			}
		}
		match unique.len() {
			0 => Self::Missing,
			1 => unique.pop().unwrap(),
			_ => Self::Union(unique),
		}
	}

	#[cold]
	fn list(item: Self) -> Self {
		Self::List(Box::new(item))
	}

	#[cold]
	fn record(fields: &[(&str, Self)]) -> Self {
		Self::Record(Types::read(
			&mut fields.iter().map(|(name, ty)| ((*name).into(), ty.clone())),
		))
	}

	#[cold]
	fn members(&self) -> &[Self] {
		if let Self::Union(members) = self {
			members
		} else {
			std::slice::from_ref(self)
		}
	}

	#[cold]
	fn all(&self, predicate: impl Fn(&Self) -> bool) -> bool {
		self.members().iter().all(|t| *t == Self::Missing || predicate(t))
	}

	#[cold]
	fn item(&self) -> Result<Self> {
		let mut items = Vec::new();
		for ty in self.members() {
			match ty {
				Self::List(item) => items.push((**item).clone()),
				Self::Missing => {}
				_ => return Err("expected list".into()),
			}
		}
		Ok(Self::union(items))
	}

	#[cold]
	fn patterns(&self) -> bool {
		self.members().iter().all(|ty| match ty {
			Self::Missing | Self::Null => true,
			Self::Native(Js::Pattern | Js::BindingIdentifier | Js::Params) => true,
			Self::List(item) => item.patterns(),
			_ => false,
		})
	}

	#[cold]
	fn roots(&self) -> bool {
		self.members().iter().all(|ty| match ty {
			Self::Missing | Self::Null | Self::Native(_) | Self::Node(_) => true,
			Self::List(item) => item.roots(),
			Self::Record(_) => true,
			_ => false,
		})
	}

	#[cold]
	fn constant(value: &Json) -> Self {
		match value {
			Json::Null => Self::Null,
			Json::Bool(_) => Self::Bool,
			Json::Number(_) => Self::Number,
			Json::String(_) => Self::String,
			Json::Array(a) => Self::list(Self::union(a.iter().map(Self::constant))),
			Json::Object(o) => Self::Record(Types::read(&mut o.iter().map(|(k, v)| (k.clone(), Self::constant(v))))),
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Channel {
	Document,
	Content,
	Attribute,
	Directive,
	Element,
	Text,
	Comment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Context {
	channel: Channel,
	bounded: bool,
}

#[derive(Clone, Default, PartialEq, Eq)]
struct Captures {
	fields: Types,
	locals: Types,
}

#[derive(Clone)]
struct Environment {
	captures: Captures,
	iteration: Option<Types>,
	bindings: Types,
	context: Option<Context>,
	successful: bool,
}

struct Validator<'a> {
	plan: &'a Plan,
	types: &'a [Captures],
	rule: usize,
	strict: bool,
}

impl Validator<'_> {
	#[cold]
	fn error(&self, field: &str, reason: &str) -> String {
		let rule = &self.plan.rules[self.rule];
		format!(
			"{}:{}: rule {}: field {field:?} {reason}",
			rule.location.0, rule.location.1, rule.name
		)
	}

	#[cold]
	fn check(&self, valid: bool, field: &str, reason: &str) -> Result<()> {
		if !self.strict || valid {
			Ok(())
		} else {
			Err(self.error(field, reason))
		}
	}

	#[cold]
	fn elements(&self) -> Type {
		Type::union(self.plan.html.elements.iter().map(|d| Type::Node(d.rule)))
	}

	#[cold]
	fn nodes(&self) -> Type {
		Type::union(
			[Type::Node(self.plan.html.text), Type::Node(self.plan.html.comment)]
				.into_iter()
				.chain(self.plan.html.content.iter().map(|d| Type::Node(d.rule)))
				.chain(self.plan.html.elements.iter().map(|d| Type::Node(d.rule))),
		)
	}

	#[cold]
	fn attributes(&self) -> Type {
		let plain = &self.plan.html.plain_attribute;
		let ordinary = Type::Record(Types::from([
			("type".into(), Type::String),
			("span".into(), Type::Span),
			(plain.name.clone(), Type::String),
			(plain.value.clone(), self.attribute_parts()),
		]));
		Type::list(Type::union(
			[ordinary]
				.into_iter()
				.chain(self.plan.html.attribute.iter().map(|d| Type::Node(d.rule)))
				.chain(self.plan.html.directives.iter().map(|d| Type::Node(d.rule))),
		))
	}

	#[cold]
	fn attribute_parts(&self) -> Type {
		Type::union([
			Type::Bool,
			Type::list(Type::union([
				Type::Node(self.plan.html.plain_attribute.text),
				Type::Node(self.plan.html.plain_attribute.expression),
			])),
		])
	}

	#[cold]
	fn header() -> Type {
		Type::record(&[(
			"attributes",
			Type::list(Type::record(&[
				("kind", Type::String),
				("name", Type::String),
				("boolean", Type::Bool),
				("staticText", Type::String),
				("value", Type::Span),
				("source", Type::Span),
			])),
		)])
	}

	#[cold]
	fn event(context: Option<Context>) -> Type {
		let mut fields = Types::default();
		let channel = context.map(|c| c.channel);
		if channel.is_none() || channel == Some(Channel::Text) {
			fields.extend([("decoded".into(), Type::String), ("raw".into(), Type::String)]);
		}
		if channel.is_none() || channel == Some(Channel::Comment) {
			fields.insert("data".into(), Type::String);
		}
		if channel.is_none() || channel == Some(Channel::Element) {
			fields.extend([
				("name".into(), Type::String),
				("atDocument".into(), Type::Bool),
				("header".into(), Self::header()),
				("rawChildren".into(), Type::Span),
				(
					"nameFacts".into(),
					Type::record(&[
						("validHtmlName", Type::Bool),
						("identifier", Type::Bool),
						("uppercaseInitial", Type::Bool),
						("dottedIdentifier", Type::Bool),
						("namespace", Type::String),
					]),
				),
			]);
		}
		if channel.is_none() || matches!(channel, Some(Channel::Directive | Channel::Attribute)) {
			fields.extend([
				("name".into(), Type::String),
				("rawName".into(), Type::String),
				("argument".into(), Type::Argument),
				("modifiers".into(), Type::list(Type::String)),
				("value".into(), Type::AttributeValue),
			]);
		}
		Type::Record(fields)
	}

	#[cold]
	fn scopes(&self, rule: usize) -> Type {
		Type::Record(Types::read(
			&mut self.plan.rules[rule]
				.regions
				.iter()
				.map(|r| (r.id.clone(), Type::Scope)),
		))
	}

	#[cold]
	fn property(&self, base: &Type, part: &Path) -> Result<Type> {
		let mut values = Vec::new();
		let mut absent = false;
		for ty in base.members() {
			let value = match (ty, part) {
				(Type::Missing | Type::Null, _) => continue,
				(Type::Unknown, _) => Some(Type::Unknown),
				(Type::List(item), Path::Index(_)) => Some((**item).clone()),
				(Type::Record(fields), Path::Name(name)) => fields.get(name).cloned(),
				(Type::Node(rule), Path::Name(name)) => match name.as_ref() {
					"type" => Some(Type::String),
					"span" => Some(Type::Span),
					"header" => Some(Self::header()),
					"scopes" => Some(self.scopes(*rule)),
					_ => self.types[*rule].fields.get(name).cloned(),
				},
				(Type::Span | Type::AttributeValue | Type::Argument, Path::Name(name)) => match name.as_ref() {
					"start" | "end" => Some(Type::Number),
					"text" => Some(Type::String),
					"span" => Some(Type::Span),
					"dynamic" if *ty == Type::Argument => Some(Type::Bool),
					_ => None,
				},
				(Type::Native(entry), Path::Name(name)) => match name.as_ref() {
					"type" => Some(Type::String),
					"span" => Some(Type::Span),
					"start" | "end" => Some(Type::Number),
					"name" if matches!(entry, Js::BindingIdentifier | Js::IdentifierReference) => Some(Type::String),
					"innerSource" if *entry == Js::TypeParameters => Some(Type::String),
					"body" if *entry == Js::Program => Some(Type::list(Type::Native(Js::Statement))),
					"sourceType" if *entry == Js::Program => Some(Type::String),
					_ if matches!(
						entry,
						Js::BindingIdentifier | Js::IdentifierReference | Js::TypeParameters | Js::Program
					) =>
					{
						None
					}
					_ => Some(Type::Unknown),
				},
				_ => None,
			};
			if let Some(value) = value {
				values.push(value);
			} else {
				absent = true;
			}
		}
		if values.is_empty() && absent {
			let key = match part {
				Path::Name(n) => n.to_string(),
				Path::Index(i) => i.to_string(),
			};
			return Err(format!("get path selects unknown field {key:?} or indexes a non-list"));
		}
		Ok(Type::union(values))
	}

	#[cold]
	fn value(&self, value: &Value, env: &Environment, field: &str) -> Result<Type> {
		let result = self.value_inner(value, env, field);
		if !self.strict {
			return Ok(result.unwrap_or(Type::Missing));
		}
		result.map_err(|reason| {
			if reason.starts_with(&format!("{}:", self.plan.rules[self.rule].location.0)) {
				reason
			} else {
				self.error(field, &reason)
			}
		})
	}

	#[cold]
	fn value_inner(&self, value: &Value, env: &Environment, field: &str) -> Result<Type> {
		let eval = |v| self.value(v, env, field);
		Ok(match value {
			Value::Constant(value) => Type::constant(value),
			Value::Get { base, path } => {
				let mut ty = match base {
					Base::Value(value) => eval(value)?,
					Base::Name(name) => match name.as_ref() {
						"record" => {
							let mut fields = env.captures.fields.clone();
							if !fields.contains_key("type") {
								fields.insert("type".into(), Type::String);
							}
							if !fields.contains_key("span") {
								fields.insert("span".into(), Type::Span);
							}
							if !fields.contains_key("header") {
								fields.insert("header".into(), Self::header());
							}
							if !fields.contains_key("scopes") {
								fields.insert("scopes".into(), self.scopes(self.rule));
							}
							Type::Record(fields)
						}
						"locals" => Type::Record(env.captures.locals.clone()),
						"iteration" => Type::Record(env.iteration.clone().ok_or("unknown local context iteration")?),
						"event" => Self::event(env.context),
						"owner" => self.elements(),
						"ancestors" => Type::list(self.elements()),
						"incoming" => Type::Scope,
						"scopes" => self.scopes(self.rule),
						_ => env
							.bindings
							.get(name)
							.cloned()
							.ok_or_else(|| format!("unknown local reference {name:?}"))?,
					},
				};
				for part in path {
					ty = self.property(&ty, part)?;
				}
				ty
			}
			Value::Compare {
				relation, left, right, ..
			} => {
				let left = eval(left)?;
				let right = right.as_ref().map(|r| eval(r)).transpose()?;
				if *relation == Relation::Less {
					self.check(
						left.all(|t| *t == Type::Number) && right.as_ref().unwrap().all(|t| *t == Type::Number),
						field,
						"less requires numbers",
					)?;
				}
				Type::Bool
			}
			Value::Choose { condition, yes, no } => {
				let condition = eval(condition)?;
				self.check(
					condition.all(|t| *t == Type::Bool),
					field,
					"choose condition must be boolean",
				)?;
				Type::union([eval(yes)?, eval(no)?])
			}
			Value::FlatMap { list, binding, body } => {
				let list = eval(list)?.item().map_err(|_| "flatMap requires a list")?;
				let mut nested = env.clone();
				self.binding(&mut nested, binding, list, field)?;
				Type::list(
					self.value(body, &nested, field)?
						.item()
						.map_err(|_| "flatMap body must be a list")?,
				)
			}
			Value::Length(list) => {
				eval(list)?.item().map_err(|_| "length requires a list")?;
				Type::Number
			}
			Value::At { list, index } => {
				let item = eval(list)?.item().map_err(|_| "at requires a list")?;
				self.check(
					eval(index)?.all(|t| *t == Type::Number),
					field,
					"at index must be a number",
				)?;
				item
			}
			Value::Construct(Construct::Array(items)) => {
				Type::list(Type::union(items.iter().map(eval).collect::<Result<Vec<_>>>()?))
			}
			Value::Construct(Construct::Record {
				node_type,
				fields,
				span,
			}) => {
				self.check(
					eval(span)?.all(|t| matches!(t, Type::Span | Type::Null)),
					field,
					"construct span must be a span or null",
				)?;
				let mut fields: Types = Types::try_read(&mut fields.iter().map(|(k, v)| Ok((k.clone(), eval(v)?))))?;
				if node_type.is_some() {
					fields.insert("type".into(), Type::String);
				}
				fields.insert("span".into(), Type::Span);
				Type::Record(fields)
			}
		})
	}

	#[cold]
	fn binding(&self, env: &mut Environment, name: &str, ty: Type, field: &str) -> Result<()> {
		self.check(
			!matches!(
				name,
				"record" | "locals" | "iteration" | "event" | "owner" | "ancestors" | "incoming" | "scopes"
			) && !name.is_empty(),
			field,
			"invalid local binding name",
		)?;
		env.bindings.insert(name.into(), ty);
		Ok(())
	}

	#[cold]
	fn read_type(&self, reader: &Reader) -> Type {
		match reader {
			Reader::Token { .. } | Reader::Space { .. } | Reader::Test(_) => Type::Missing,
			Reader::Rule(rule) => Type::Node(*rule),
			Reader::Javascript { entry: Js::Params, .. } | Reader::HtmlSingle(Js::Params) => {
				Type::list(Type::Native(Js::Pattern))
			}
			Reader::Javascript { entry, .. } | Reader::HtmlSingle(entry) => Type::Native(*entry),
			Reader::HtmlAttributes(_) => self.attributes(),
			Reader::HtmlAttributeParts => self.attribute_parts(),
			Reader::HtmlChildren { .. } => Type::list(self.nodes()),
			Reader::CssStylesheet => Type::record(&[
				(
					"children",
					Type::list(Type::record(&[("type", Type::String), ("span", Type::Span)])),
				),
				(
					"comments",
					Type::list(Type::record(&[("type", Type::String), ("span", Type::Span)])),
				),
			]),
		}
	}

	#[cold]
	fn reader(&self, reader: &Reader, input: &Option<Value>, env: &Environment, field: &str) -> Result<Type> {
		let input = input.as_ref().map(|v| self.value(v, env, field)).transpose()?;
		let context = env.context;
		let channel = context.map(|c| c.channel);
		let streaming = context.is_none_or(|c| c.bounded || matches!(c.channel, Channel::Content | Channel::Attribute));
		match reader {
			Reader::Javascript { entry, boundary } => {
				if let Some(input) = &input {
					self.check(
						input.all(|t| {
							matches!(t, Type::Span | Type::Argument)
								|| (!self.plan.html.attribute_interpolations && *t == Type::AttributeValue)
						}),
						field,
						"javascript input must be a source span; use html-single for an interpolated attribute value",
					)?;
				} else {
					self.check(
						streaming,
						field,
						"javascript reader needs bounded input on this channel",
					)?;
				}
				self.check(
					boundary.is_none() || (input.is_none() && *entry == Js::Expression),
					field,
					"last-shared-word requires an unbounded expression read",
				)?;
			}
			Reader::HtmlSingle(_) => {
				self.check(
					input.as_ref().is_some_and(|t| t.all(|t| *t == Type::AttributeValue)),
					field,
					"html-single input must be an attribute value",
				)?;
				self.check(
					channel.is_none_or(|c| matches!(c, Channel::Directive | Channel::Attribute)),
					field,
					"html-single reader is not valid on this channel",
				)?;
			}
			Reader::CssStylesheet => {
				self.check(
					input.as_ref().is_some_and(|t| t.all(|t| *t == Type::Span)),
					field,
					"css-stylesheet input must be a source span",
				)?;
			}
			Reader::Rule(_) => {
				self.check(
					input.as_ref().is_none_or(|t| {
						t.all(|t| {
							matches!(t, Type::Span | Type::Argument)
								|| (!self.plan.html.attribute_interpolations && *t == Type::AttributeValue)
						})
					}),
					field,
					"rule input must be a source span",
				)?;
			}
			Reader::Test(value) => {
				self.check(input.is_none(), field, "test reader does not accept input")?;
				self.check(
					self.value(value, env, field)?.all(|t| *t == Type::Bool),
					field,
					"test value must be boolean",
				)?;
			}
			Reader::Token { .. } | Reader::Space { .. } => {
				self.check(input.is_none(), field, "token/space reader does not accept input")?;
				self.check(
					streaming,
					field,
					"token/space reader needs a content, attribute, or bounded cursor",
				)?;
			}
			Reader::HtmlAttributes(_) => {
				self.check(
					input.is_none()
						&& channel.is_none_or(|c| c == Channel::Element)
						&& context.is_none_or(|c| !c.bounded),
					field,
					"html-attributes reader requires an element channel and no input",
				)?;
			}
			Reader::HtmlAttributeParts => {
				self.check(
					input.is_none() && channel.is_none_or(|c| matches!(c, Channel::Attribute | Channel::Directive)),
					field,
					"html-attribute-parts reader requires an attribute channel and no input",
				)?;
			}
			Reader::HtmlChildren { stop, .. } => {
				let valid = channel.is_none_or(|c| match stop {
					Stop::MatchingElement => c == Channel::Element,
					Stop::DocumentEnd => c == Channel::Document,
					Stop::Prefixes(_) => matches!(c, Channel::Content | Channel::Element | Channel::Document),
				});
				self.check(
					input.is_none() && valid && context.is_none_or(|c| !c.bounded),
					field,
					"html-children stop/input does not match its channel",
				)?;
			}
		}
		Ok(self.read_type(reader))
	}

	#[cold]
	fn write(&self, env: &mut Environment, written: &mut Set<Name>, into: &str, ty: Type) -> Result<()> {
		let slots = if let Some(iteration) = &mut env.iteration {
			iteration
		} else if env.captures.fields.contains_key(into) {
			&mut env.captures.fields
		} else {
			&mut env.captures.locals
		};
		self.check(slots.contains_key(into), into, "is emitted but not declared")?;
		if ty == Type::Missing {
			return Ok(());
		}
		self.check(
			!env.successful || !written.contains(into),
			into,
			"is written twice on one successful path (scalar overwrite)",
		)?;
		written.insert(into.into());
		slots.insert(into.into(), ty);
		Ok(())
	}

	#[cold]
	fn form(&self, form: &Form, env: &mut Environment, written: &mut Set<Name>) -> Result<()> {
		match form {
			Form::Seq(items) => {
				for form in items {
					self.form(form, env, written)?;
				}
			}
			Form::Choice { alternatives, .. } => {
				let mut branches = Vec::new();
				let mut writes = Set::new();
				for alternative in alternatives {
					let mut branch = env.clone();
					branch.successful &= can_succeed(alternative);
					let mut branch_writes = written.clone();
					self.form(alternative, &mut branch, &mut branch_writes)?;
					if can_succeed(alternative) {
						branches.push(branch);
						writes.extend(branch_writes);
					}
				}
				for (name, ty) in &mut env.captures.fields {
					*ty = Type::union(branches.iter().map(|b| b.captures.fields[name].clone()));
				}
				for (name, ty) in &mut env.captures.locals {
					*ty = Type::union(branches.iter().map(|b| b.captures.locals[name].clone()));
				}
				if let Some(iteration) = &mut env.iteration {
					for (name, ty) in iteration {
						*ty = Type::union(branches.iter().map(|b| b.iteration.as_ref().unwrap()[name].clone()));
					}
				}
				*written = writes;
			}
			Form::Repeat {
				body,
				locals,
				yield_value,
				into,
				..
			} => {
				let mut nested = env.clone();
				nested.successful &= can_succeed(body);
				let slots: Types = Types::read(&mut locals.iter().map(|n| (n.clone(), Type::Missing)));
				self.check(slots.len() == locals.len(), into, "repeat has duplicate locals")?;
				nested.iteration = Some(slots);
				self.form(body, &mut nested, &mut Set::new())?;
				let item = self.value(yield_value, &nested, "yield")?;
				self.write(env, written, into, Type::list(item))?;
			}
			Form::Read {
				reader, input, into, ..
			} => {
				let ty = self.reader(reader, input, env, into.as_deref().unwrap_or("reader"))?;
				if let Some(into) = into {
					self.write(env, written, into, ty)?;
				}
			}
			Form::Emit { into, value } => {
				let ty = self.value(value, env, into)?;
				self.write(env, written, into, ty)?;
			}
		}
		Ok(())
	}

	#[cold]
	fn environment(&self, context: Option<Context>) -> Environment {
		let rule = &self.plan.rules[self.rule];
		Environment {
			captures: Captures {
				fields: Types::read(&mut rule.fields.keys().map(|n| (n.clone(), Type::Missing))),
				locals: Types::read(&mut rule.locals.iter().map(|n| (n.clone(), Type::Missing))),
			},
			iteration: None,
			bindings: Types::default(),
			context,
			successful: can_succeed(&rule.form),
		}
	}

	#[cold]
	fn validate(&self, context: Option<Context>) -> Result<Captures> {
		let rule = &self.plan.rules[self.rule];
		let mut env = self.environment(context);
		self.check(
			env.captures.locals.len() == rule.locals.len(),
			"locals",
			"contains duplicate local names",
		)?;
		for name in &rule.locals {
			self.check(!rule.fields.contains_key(name), name, "is both a field and a local")?;
		}
		let regions: Set<_> = rule.regions.iter().map(|r| &r.id).collect();
		self.check(
			regions.len() == rule.regions.len(),
			"regions",
			"contains duplicate region names",
		)?;
		self.form(&rule.form, &mut env, &mut Set::new())?;
		for (name, absence) in &rule.fields {
			if *absence == Absence::Null && env.captures.fields[name] == Type::Missing {
				env.captures.fields.insert(name.clone(), Type::Null);
			}
		}
		for region in &rule.regions {
			let mut nested = env.clone();
			if let Some(each) = &region.each {
				let ty = self.value(&each.list, &nested, "each.list")?;
				let item = match ty.item() {
					Ok(item) => item,
					Err(_) if !self.strict => Type::Missing,
					Err(_) => return Err(self.error("each.list", "requires a list")),
				};
				self.binding(&mut nested, &each.binding, item, "each.as")?;
			}
			let parent = self.value(&region.parent, &nested, &format!("regions.{}.parent", region.id))?;
			self.check(
				parent.all(|t| matches!(t, Type::Scope | Type::Null)),
				"parent",
				"must select a region or null",
			)?;
			let covers = self.value(&region.covers, &nested, &format!("regions.{}.covers", region.id))?;
			self.check(covers.roots(), "covers", "must select node roots")?;
			if let Some(when) = &region.when {
				self.check(
					self.value(when, &nested, "when")?.all(|t| *t == Type::Bool),
					"when",
					"must be boolean",
				)?;
			}
		}
		for declare in &rule.declares {
			self.check(
				self.value(&declare.patterns, &env, "declares.patterns")?.patterns(),
				"declares.patterns",
				"must select pattern, bindingIdentifier, or params reads",
			)?;
			self.check(
				self.value(&declare.into, &env, "declares.into")?
					.all(|t| *t == Type::Scope),
				"declares.into",
				"must select a region",
			)?;
		}
		Ok(env.captures)
	}
}

#[cold]
fn can_succeed(form: &Form) -> bool {
	match form {
		Form::Read {
			reader: Reader::Test(Value::Constant(Json::Bool(false))),
			..
		} => false,
		Form::Seq(items) => items.iter().all(can_succeed),
		Form::Choice { alternatives, .. } => alternatives.iter().any(can_succeed),
		Form::Repeat { body, min, .. } => *min == 0 || can_succeed(body),
		_ => true,
	}
}

#[cold]
fn nullable(form: &Form, rules: &[bool]) -> bool {
	if !can_succeed(form) {
		return false;
	}
	match form {
		Form::Seq(items) => items.iter().all(|f| nullable(f, rules)),
		Form::Choice { alternatives, .. } => alternatives.iter().any(|f| nullable(f, rules)),
		Form::Repeat { body, min, max, .. } => *min == 0 || *max == Some(0) || nullable(body, rules),
		Form::Emit { .. } => true,
		Form::Read { reader, input, .. } => {
			input.is_some()
				|| match reader {
					Reader::Token { .. } => false,
					Reader::Space { min } => *min == 0,
					Reader::Rule(rule) => rules[*rule],
					Reader::Javascript { entry, .. } => *entry == Js::Program,
					Reader::Test(_)
					| Reader::HtmlAttributes(_)
					| Reader::HtmlAttributeParts
					| Reader::HtmlChildren { .. }
					| Reader::HtmlSingle(_)
					| Reader::CssStylesheet => true,
				}
		}
	}
}

#[cold]
fn calls(form: &Form, visit: &mut impl FnMut(usize, bool)) {
	match form {
		Form::Seq(items)
		| Form::Choice {
			alternatives: items, ..
		} => {
			for f in items {
				calls(f, visit);
			}
		}
		Form::Repeat { body, .. } => calls(body, visit),
		Form::Read {
			reader: Reader::Rule(rule),
			input,
			..
		} => visit(*rule, input.is_some()),
		_ => {}
	}
}

#[cold]
fn leading_calls(form: &Form, rules: &[bool], content: &[PrefixDispatch], edges: &mut Set<usize>) {
	match form {
		Form::Seq(items) => {
			for f in items {
				leading_calls(f, rules, content, edges);
				if !nullable(f, rules) || !can_succeed(f) {
					break;
				}
			}
		}
		Form::Choice { alternatives, .. } => {
			// an alternative that cannot succeed still runs its leading calls before it fails
			for f in alternatives {
				leading_calls(f, rules, content, edges);
			}
		}
		Form::Repeat { body, max, .. } if *max != Some(0) => leading_calls(body, rules, content, edges),
		Form::Read {
			reader: Reader::Rule(rule),
			..
		} => {
			edges.insert(*rule);
		}
		Form::Read {
			reader: Reader::HtmlChildren {
				mode: Mode::Normal,
				stop,
			},
			input: None,
			..
		} if !matches!(stop, Stop::MatchingElement) => {
			for row in content {
				let stopped = match stop {
					Stop::Prefixes(prefixes) => prefixes.iter().any(|prefix| {
						prefix == &row.prefix
							|| (row.prefix.starts_with(prefix.as_ref())
								&& !prefix.ends_with(|c: char| c.is_alphanumeric() || c == '_'))
					}),
					_ => false,
				};
				if !stopped {
					edges.insert(row.rule);
				}
			}
		}
		_ => {}
	}
}

#[cold]
fn cycle(edges: &[Set<usize>]) -> Option<usize> {
	#[cold]
	fn visit(node: usize, edges: &[Set<usize>], states: &mut [u8]) -> Option<usize> {
		if states[node] == 1 {
			return Some(node);
		}
		if states[node] == 2 {
			return None;
		}
		states[node] = 1;
		for &next in &edges[node] {
			if let Some(node) = visit(next, edges, states) {
				return Some(node);
			}
		}
		states[node] = 2;
		None
	}
	let mut states = vec![0; edges.len()];
	(0..edges.len()).find_map(|node| visit(node, edges, &mut states))
}

#[cold]
fn reaches(edges: &[Set<usize>], start: usize, target: usize) -> bool {
	let mut pending = vec![start];
	let mut seen = Set::new();
	while let Some(node) = pending.pop() {
		if node == target {
			return true;
		}
		if seen.insert(node) {
			pending.extend(&edges[node]);
		}
	}
	false
}

#[cold]
fn repeat_progress(form: &Form, rules: &[bool]) -> Result<()> {
	match form {
		Form::Seq(items)
		| Form::Choice {
			alternatives: items, ..
		} => {
			for f in items {
				repeat_progress(f, rules)?;
			}
		}
		Form::Repeat { body, max, .. } => {
			if max.is_none() && nullable(body, rules) {
				return Err("repeat can loop without consuming input".into());
			}
			repeat_progress(body, rules)?;
		}
		_ => {}
	}
	Ok(())
}

#[cold]
fn value_children(value: &Value, visit: &mut impl FnMut(&Value)) {
	match value {
		Value::Constant(_) => {}
		Value::Get {
			base: Base::Value(value),
			..
		} => visit(value),
		Value::Get { .. } => {}
		Value::Compare { left, right, .. } => {
			visit(left);
			if let Some(right) = right {
				visit(right);
			}
		}
		Value::Choose { condition, yes, no } => {
			visit(condition);
			visit(yes);
			visit(no);
		}
		Value::FlatMap { list, body, .. } => {
			visit(list);
			visit(body);
		}
		Value::Length(list) => visit(list),
		Value::At { list, index } => {
			visit(list);
			visit(index);
		}
		Value::Construct(Construct::Array(items)) => {
			for item in items {
				visit(item);
			}
		}
		Value::Construct(Construct::Record { fields, span, .. }) => {
			for field in fields.values() {
				visit(field);
			}
			visit(span);
		}
	}
}

#[cold]
fn get_path<'a>(value: &'a Value, path: &mut Vec<&'a Path>) -> Option<&'a str> {
	let Value::Get { base, path: parts } = value else {
		return None;
	};
	let name = match base {
		Base::Name(name) => Some(name.as_ref()),
		Base::Value(value) => get_path(value, path),
	};
	path.extend(parts);
	name
}

#[cold]
fn emit_values<'a>(form: &'a Form, name: &str, values: &mut Vec<&'a Value>) {
	match form {
		Form::Seq(items)
		| Form::Choice {
			alternatives: items, ..
		} => {
			for f in items {
				emit_values(f, name, values);
			}
		}
		Form::Emit { into, value } if into.as_ref() == name => values.push(value),
		_ => {}
	}
}

#[cold]
fn parent_dependencies(value: &Value, rule: &Rule, seen: &mut Set<Name>, dependencies: &mut Set<usize>) {
	let mut path = Vec::new();
	if let Some(base) = get_path(value, &mut path) {
		let parts: Vec<_> = path
			.iter()
			.filter_map(|p| {
				if let Path::Name(name) = p {
					Some(name.as_ref())
				} else {
					None
				}
			})
			.collect();
		let region = match (base, parts.as_slice()) {
			("scopes", [name, ..]) | ("record", ["scopes", name, ..]) => Some(*name),
			_ => None,
		};
		if let Some(name) = region
			&& let Some(index) = rule.regions.iter().position(|r| r.id.as_ref() == name)
		{
			dependencies.insert(index);
		}
		if matches!(base, "record" | "locals")
			&& let Some(name) = parts.first()
			&& seen.insert((*name).into())
		{
			let mut values = Vec::new();
			emit_values(&rule.form, name, &mut values);
			for value in values {
				parent_dependencies(value, rule, seen, dependencies);
			}
		}
	}
	value_children(value, &mut |child| parent_dependencies(child, rule, seen, dependencies));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix {
	pub(crate) text: String,
	pub(crate) word: bool,
	complete: bool,
	pub(crate) tight: bool,
}

#[cold]
fn prefixes(form: &Form, rules: &[Rule], active: &mut Set<usize>, budget: usize) -> Vec<Prefix> {
	let empty = || {
		vec![Prefix {
			text: String::new(),
			word: false,
			complete: true,
			tight: true,
		}]
	};
	let unknown = || {
		vec![Prefix {
			text: String::new(),
			word: false,
			complete: false,
			tight: true,
		}]
	};
	if budget == 0 {
		return unknown();
	}
	match form {
		Form::Emit { .. }
		| Form::Read {
			reader: Reader::Test(_),
			..
		} => empty(),
		Form::Read { input: Some(_), .. } => empty(),
		Form::Read {
			reader: Reader::Token { text, word, gap },
			..
		} => vec![Prefix {
			text: text.to_string(),
			word: *word,
			complete: true,
			tight: *gap == Gap::None,
		}],
		Form::Read {
			reader: Reader::Space { .. },
			..
		} => unknown(),
		Form::Read {
			reader: Reader::Rule(rule),
			..
		} => {
			if !active.insert(*rule) {
				return unknown();
			}
			let result = prefixes(&rules[*rule].form, rules, active, budget - 1);
			active.remove(rule);
			result
		}
		Form::Read { .. } => unknown(),
		Form::Choice { alternatives, .. } => {
			let mut result = Vec::new();
			for alternative in alternatives {
				result.extend(prefixes(alternative, rules, active, budget - 1));
				if result.len() > 64 {
					return unknown();
				}
			}
			result
		}
		Form::Seq(items) => {
			let mut result = empty();
			for item in items {
				if result.iter().all(|p| !p.complete) {
					break;
				}
				let next = prefixes(item, rules, active, budget - 1);
				let mut combined = Vec::new();
				for prefix in result {
					if !prefix.complete {
						combined.push(prefix);
						continue;
					}
					for suffix in &next {
						// Ignoring a token gap would falsely prove overlapping alternatives disjoint.
						let separated =
							prefix.word && suffix.text.starts_with(|c: char| c.is_alphanumeric() || c == '_');
						if separated
							|| (!prefix.text.is_empty() && !suffix.tight)
							|| prefix.text.len() + suffix.text.len() > 128
						{
							combined.push(Prefix {
								complete: false,
								..prefix.clone()
							});
						} else {
							combined.push(Prefix {
								text: format!("{}{}", prefix.text, suffix.text),
								word: if suffix.text.is_empty() {
									prefix.word
								} else {
									suffix.word
								},
								complete: suffix.complete,
								tight: if prefix.text.is_empty() {
									prefix.tight && suffix.tight
								} else {
									prefix.tight
								},
							});
						}
					}
					if combined.len() > 64 {
						return unknown();
					}
				}
				result = combined;
			}
			result
		}
		Form::Repeat { body, min, max, .. } => {
			if *max == Some(0) {
				return empty();
			}
			let mut result = prefixes(body, rules, active, budget - 1);
			for p in &mut result {
				p.complete = false;
			}
			if *min == 0 {
				result.extend(empty());
			}
			result
		}
	}
}

#[cold]
fn distinct(left: &Prefix, right: &Prefix) -> bool {
	let left_text = left
		.text
		.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
	let right_text = right
		.text
		.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
	let common = left_text.len().min(right_text.len());
	if left_text.as_bytes()[..common] != right_text.as_bytes()[..common] {
		return true;
	}
	let (short, long) = if left_text.len() < right_text.len() {
		(left, right_text)
	} else {
		(right, left_text)
	};
	short.word && long[common..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
}

#[cold]
fn mark_choices(form: &mut Form, rules: &[Rule]) {
	match form {
		Form::Choice {
			alternatives,
			disjoint,
			first: compiled,
		} => {
			let first: Vec<_> = alternatives
				.iter()
				.map(|f| prefixes(f, rules, &mut Set::new(), 32))
				.collect();
			*disjoint = first.iter().enumerate().all(|(i, a)| {
				first[i + 1..]
					.iter()
					.all(|b| a.iter().all(|a| b.iter().all(|b| distinct(a, b))))
			});
			*compiled = first;
			for f in alternatives {
				mark_choices(f, rules);
			}
		}
		Form::Seq(items) => {
			for f in items {
				mark_choices(f, rules);
			}
		}
		Form::Repeat { body, .. } => mark_choices(body, rules),
		_ => {}
	}
}

impl Plan {
	#[cold]
	fn contexts(&self) -> Vec<Set<Context>> {
		let mut contexts = vec![Set::new(); self.rules.len()];
		let mut seed = |rule: usize, channel| {
			contexts[rule].insert(Context {
				channel,
				bounded: false,
			});
		};
		seed(self.document, Channel::Document);
		seed(self.html.text, Channel::Text);
		seed(self.html.comment, Channel::Comment);
		seed(self.html.plain_attribute.text, Channel::Text);
		seed(self.html.plain_attribute.expression, Channel::Attribute);
		for row in &self.html.content {
			seed(row.rule, Channel::Content);
		}
		for row in &self.html.attribute {
			seed(row.rule, Channel::Attribute);
		}
		for row in &self.html.elements {
			seed(row.rule, Channel::Element);
		}
		for row in &self.html.directives {
			seed(row.rule, Channel::Directive);
		}
		loop {
			let mut added = false;
			for (i, rule) in self.rules.iter().enumerate() {
				let callers = contexts[i].clone();
				calls(&rule.form, &mut |callee, bounded| {
					for context in &callers {
						added |= contexts[callee].insert(Context {
							bounded: bounded || context.bounded,
							..*context
						});
					}
				});
			}
			if !added {
				return contexts;
			}
		}
	}

	#[cold]
	fn validate(&mut self) -> Result<()> {
		let mut nullable_rules = vec![false; self.rules.len()];
		loop {
			let next: Vec<_> = self.rules.iter().map(|r| nullable(&r.form, &nullable_rules)).collect();
			if next == nullable_rules {
				break;
			}
			nullable_rules = next;
		}
		let mut edges = vec![Set::new(); self.rules.len()];
		for (i, rule) in self.rules.iter().enumerate() {
			leading_calls(&rule.form, &nullable_rules, &self.html.content, &mut edges[i]);
		}
		if let Some(i) = cycle(&edges) {
			return Err(format!(
				"{}:{}: rule {}: field \"form\" has nullable recursion that can loop without consuming input",
				self.rules[i].location.0, self.rules[i].location.1, self.rules[i].name
			));
		}
		let mut all_edges = vec![Set::new(); self.rules.len()];
		for (i, rule) in self.rules.iter().enumerate() {
			calls(&rule.form, &mut |j, _| {
				all_edges[i].insert(j);
			});
		}
		for (i, rule) in self.rules.iter().enumerate() {
			let error = |field: &str, reason: &str| {
				format!(
					"{}:{}: rule {}: field {field:?} {reason}",
					rule.location.0, rule.location.1, rule.name
				)
			};
			repeat_progress(&rule.form, &nullable_rules).map_err(|e| error("form", &e))?;
			let mut resets = false;
			calls(&rule.form, &mut |callee, bounded| {
				resets |= bounded && reaches(&all_edges, callee, i);
			});
			if resets {
				return Err(error(
					"input",
					"recursive bounded read can loop without consuming input",
				));
			}
			let mut parents = vec![Set::new(); rule.regions.len()];
			for (i, region) in rule.regions.iter().enumerate() {
				parent_dependencies(&region.parent, rule, &mut Set::new(), &mut parents[i]);
				if let Some(each) = &region.each {
					parent_dependencies(&each.list, rule, &mut Set::new(), &mut parents[i]);
				}
			}
			if let Some(i) = cycle(&parents) {
				return Err(error(
					&format!("regions.{}.parent", rule.regions[i].id),
					"region parents form a cycle",
				));
			}
		}
		let mut types = vec![Captures::default(); self.rules.len()];
		for (i, rule) in self.rules.iter().enumerate() {
			types[i].fields = Types::read(&mut rule.fields.keys().map(|n| (n.clone(), Type::Missing)));
			types[i].locals = Types::read(&mut rule.locals.iter().map(|n| (n.clone(), Type::Missing)));
		}
		let mut converged = false;
		for _ in 0..self.rules.len() * 4 + 16 {
			let next: Vec<_> = (0..self.rules.len())
				.map(|rule| {
					Validator {
						plan: self,
						types: &types,
						rule,
						strict: false,
					}
					.validate(None)
				})
				.collect::<Result<_>>()?;
			if next == types {
				converged = true;
				break;
			}
			types = next;
		}
		if !converged {
			let rule = &self.rules[self.document];
			return Err(format!(
				"{}:{}: rule {}: field shapes do not converge",
				rule.location.0, rule.location.1, rule.name
			));
		}
		let contexts = self.contexts();
		for (rule, contexts) in contexts.iter().enumerate() {
			let validator = Validator {
				plan: self,
				types: &types,
				rule,
				strict: true,
			};
			validator.validate(None)?;
			for context in contexts {
				validator.validate(Some(*context))?;
			}
		}
		let validator = Validator {
			plan: self,
			types: &types,
			rule: self.document,
			strict: true,
		};
		let mut env = validator.environment(Some(Context {
			channel: Channel::Element,
			bounded: false,
		}));
		env.captures = Captures::default();
		for row in &self.html.elements {
			let when = validator.value(&row.when, &env, "html.elements.when")?;
			validator.check(when.all(|t| *t == Type::Bool), "html.elements.when", "must be boolean")?;
		}
		for row in self.html.content.iter().chain(&self.html.attribute) {
			if nullable_rules[row.rule] {
				let rule = &self.rules[row.rule];
				return Err(format!(
					"{}:{}: rule {}: field \"form\" dispatch can succeed without consuming input",
					rule.location.0, rule.location.1, rule.name
				));
			}
		}
		let rules = self.rules.clone();
		for rule in &mut self.rules {
			mark_choices(&mut rule.form, &rules);
		}
		Ok(())
	}
}

#[cold]
fn follow(rules: &[Rule], forms: &[Form], out: &mut Vec<String>, depth: usize) {
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
				follow(rules, std::slice::from_ref(&rules[*rule].form), out, depth + 1);
				break;
			}
			Form::Seq(items) => {
				follow(rules, items, out, depth + 1);
				if !items.is_empty() {
					break;
				}
			}
			Form::Choice { alternatives, .. } => {
				for f in alternatives {
					follow(rules, std::slice::from_ref(f), out, depth + 1);
				}
			}
			Form::Repeat { body, .. } => follow(rules, std::slice::from_ref(body), out, depth + 1),
			Form::Emit { .. }
			| Form::Read {
				reader: Reader::Space { .. } | Reader::Test(_),
				..
			} => {}
			_ => break,
		}
	}
}
#[cold]
fn compile(plan: &mut Plan) {
	#[cold]
	fn form(f: &mut Form, rules: &[Rule], suffix: &str, stops: &mut Vec<Name>) {
		match f {
			Form::Seq(items) => {
				let suffixes = (0..items.len())
					.map(|i| {
						let mut next = Vec::new();
						follow(rules, &items[i + 1..], &mut next, 0);
						if !suffix.is_empty() {
							next.push(suffix.to_owned());
						}
						next.join(" ")
					})
					.collect::<Vec<_>>();
				for (item, next) in items.iter_mut().zip(suffixes) {
					form(item, rules, &next, stops);
				}
			}
			Form::Choice { alternatives, .. } => {
				for item in alternatives {
					form(item, rules, suffix, stops);
				}
			}
			Form::Repeat { body, .. } => form(body, rules, suffix, stops),
			Form::Read { reader, follow, .. } => {
				*follow = suffix.into();
				if let Reader::HtmlChildren {
					stop: Stop::Prefixes(prefixes),
					..
				} = reader
				{
					stops.extend(prefixes.iter().cloned());
				}
			}
			Form::Emit { .. } => {}
		}
	}
	let rules = plan.rules.clone();
	for rule in &mut plan.rules {
		form(&mut rule.form, &rules, "", &mut plan.stops);
	}
	plan.stops.sort();
	plan.stops.dedup();
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Map<T>(Vec<(Name, T)>);
type Types = Map<Type>;
impl<T> Map<T> {
	#[cold]
	fn read(values: &mut dyn Iterator<Item = (Name, T)>) -> Self {
		let mut out = Self::default();
		for (name, ty) in values {
			out.insert(name, ty);
		}
		out
	}
	#[cold]
	fn try_read(values: &mut dyn Iterator<Item = Result<(Name, T)>>) -> Result<Self> {
		let mut out = Self::default();
		for value in values {
			let (name, ty) = value?;
			out.insert(name, ty);
		}
		Ok(out)
	}
	#[cold]
	fn find(&self, name: &str) -> std::result::Result<usize, usize> {
		self.0.binary_search_by(|(key, _)| key.as_ref().cmp(name))
	}
	#[cold]
	fn get(&self, name: &str) -> Option<&T> {
		self.find(name).ok().map(|i| &self.0[i].1)
	}
	#[cold]
	fn contains_key(&self, name: &str) -> bool {
		self.find(name).is_ok()
	}
	#[cold]
	fn insert(&mut self, name: Name, ty: T) -> Option<T> {
		match self.find(&name) {
			Ok(i) => Some(std::mem::replace(&mut self.0[i].1, ty)),
			Err(i) => {
				self.0.insert(i, (name, ty));
				None
			}
		}
	}
	#[cold]
	fn len(&self) -> usize {
		self.0.len()
	}
	#[cold]
	fn keys(&self) -> impl Iterator<Item = &Name> {
		self.0.iter().map(|(name, _)| name)
	}
	#[cold]
	fn extend(&mut self, values: impl IntoIterator<Item = (Name, T)>) {
		for (name, ty) in values {
			self.insert(name, ty);
		}
	}
}
impl<T, const N: usize> From<[(Name, T); N]> for Map<T> {
	#[cold]
	fn from(values: [(Name, T); N]) -> Self {
		Self::read(&mut values.into_iter())
	}
}
impl<T> std::ops::Index<&Name> for Map<T> {
	type Output = T;
	#[cold]
	fn index(&self, name: &Name) -> &T {
		self.get(name).unwrap()
	}
}
impl<'a, T> IntoIterator for &'a mut Map<T> {
	type Item = (&'a Name, &'a mut T);
	type IntoIter = std::iter::Map<std::slice::IterMut<'a, (Name, T)>, fn(&'a mut (Name, T)) -> Self::Item>;
	#[cold]
	fn into_iter(self) -> Self::IntoIter {
		self.0.iter_mut().map(|(name, ty)| (&*name, ty))
	}
}

impl<T> Default for Map<T> {
	fn default() -> Self {
		Self(Vec::new())
	}
}
impl<T> Map<T> {
	#[cold]
	fn iter(&self) -> impl Iterator<Item = (&Name, &T)> {
		self.0.iter().map(|(name, value)| (name, value))
	}
}

#[derive(Clone)]
struct Set<T>(Vec<T>);
impl<T: Ord> Set<T> {
	#[cold]
	fn new() -> Self {
		Self(Vec::new())
	}
	#[cold]
	fn insert(&mut self, value: T) -> bool {
		match self.0.binary_search(&value) {
			Ok(_) => false,
			Err(i) => {
				self.0.insert(i, value);
				true
			}
		}
	}
	#[cold]
	fn contains<Q: Ord + ?Sized>(&self, value: &Q) -> bool
	where
		T: std::borrow::Borrow<Q>,
	{
		self.0.binary_search_by(|v| v.borrow().cmp(value)).is_ok()
	}
	#[cold]
	fn remove<Q: Ord + ?Sized>(&mut self, value: &Q) -> bool
	where
		T: std::borrow::Borrow<Q>,
	{
		match self.0.binary_search_by(|v| v.borrow().cmp(value)) {
			Ok(i) => {
				self.0.remove(i);
				true
			}
			Err(_) => false,
		}
	}
	#[cold]
	fn len(&self) -> usize {
		self.0.len()
	}
	#[cold]
	fn iter(&self) -> std::slice::Iter<'_, T> {
		self.0.iter()
	}
	#[cold]
	fn extend(&mut self, values: impl IntoIterator<Item = T>) {
		for value in values {
			self.insert(value);
		}
	}
}
impl<T: Ord> FromIterator<T> for Set<T> {
	#[cold]
	fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
		let mut set = Self::new();
		set.extend(values);
		set
	}
}
impl<T> IntoIterator for Set<T> {
	type Item = T;
	type IntoIter = std::vec::IntoIter<T>;
	#[cold]
	fn into_iter(self) -> Self::IntoIter {
		self.0.into_iter()
	}
}
impl<'a, T> IntoIterator for &'a Set<T> {
	type Item = &'a T;
	type IntoIter = std::slice::Iter<'a, T>;
	#[cold]
	fn into_iter(self) -> Self::IntoIter {
		self.0.iter()
	}
}
