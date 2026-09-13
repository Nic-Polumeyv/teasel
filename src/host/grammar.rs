//! A host grammar: what a template language puts around the JavaScript it embeds, read once
//! from the text a host hands over. Every form is a sequence of the host's own words and
//! punctuators around JavaScript entries; what may follow an entry is what ends it.

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
	/// The text up to the closing delimiter, unread, for a host that reads its expressions later.
	Text,
}

/// One step of a form.
#[derive(Clone, Debug)]
pub enum Item {
	Literal(&'static str),
	/// `field=entry`; `field?=entry` leaves the field out when the entry was not read, where the
	/// plain form gives it null.
	Entry {
		field: &'static str,
		entry: Entry,
		omit: bool,
		stops: Stops,
	},
	/// `[ a | b ]`: at most one alternative, tried in order; `{ a | b }`: exactly one.
	Group {
		alternatives: Vec<Alternative>,
		required: bool,
		/// The literals that may follow the group.
		after: &'static [&'static str],
	},
}

/// The literals that may follow an entry, as the parser takes them: all of them, and for an
/// expression without the tokens that continue one, which are JavaScript's before the host's.
#[derive(Clone, Copy, Debug)]
pub struct Stops {
	pub list: &'static [&'static str],
	pub joined: &'static str,
	pub expression: &'static str,
}

impl Stops {
	const NONE: Stops = Stops {
		list: &[],
		joined: "",
		expression: "",
	};

	fn of(list: Vec<&'static str>) -> Stops {
		let joined = keep(&list.join(" "));
		let expression: Vec<&str> = list
			.iter()
			.copied()
			.filter(|s| !matches!(*s, "(" | "[" | "." | "?." | "`"))
			.collect();
		Stops {
			joined,
			expression: keep(&expression.join(" ")),
			list: Vec::leak(list),
		}
	}
}

/// Gives every entry and group of `items` what may follow it, `follow` following them all.
fn resolve(items: &mut [Item], follow: &[&'static str]) {
	for i in 0..items.len() {
		let (head, rest) = items.split_at_mut(i + 1);
		match head.last_mut().unwrap() {
			Item::Literal(_) => {}
			Item::Entry { stops, .. } => *stops = Stops::of(first_literals(rest, follow)),
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
	let mut out = Vec::new();
	for item in items {
		match item {
			Item::Literal(literal) => {
				out.push(*literal);
				return out;
			}
			Item::Entry { .. } => return out,
			Item::Group {
				alternatives, required, ..
			} => {
				for alternative in alternatives {
					out.extend(first_literals(&alternative.items, &[]));
				}
				if *required {
					return out;
				}
			}
		}
	}
	out.extend_from_slice(follow);
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

#[derive(Clone, Debug)]
pub struct Alternative {
	pub items: Vec<Item>,
	/// The body the block opens when this alternative was read, `[ then value=pattern -> then ]`.
	pub body: Option<Body>,
}

/// What a block's body is: the field that holds it, and what the body's scope declares.
#[derive(Clone, Debug)]
pub struct Body {
	pub field: &'static str,
	/// The field is left out of blocks that never opened this body; otherwise it is null there.
	pub omit: bool,
	/// A branch that nests a new block of the same kind into the field, `{:else if}`: the field
	/// of the nested block that its own body fills.
	pub chain: Option<&'static str>,
	pub declares: Vec<Declare>,
}

#[derive(Clone, Debug)]
pub struct Declare {
	pub field: &'static str,
	/// Declared in the scope around the block rather than inside it: a snippet's name.
	pub outside: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Form {
	pub items: Vec<Item>,
	pub body: Option<Body>,
	/// Every entry the form can read, and whether its field is left out when it was not.
	pub entries: Vec<(&'static str, bool)>,
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
	pub name_field: Option<&'static str>,
	pub arg_field: Option<&'static str>,
	pub modifiers_field: Option<&'static str>,
	pub raw_field: Option<&'static str>,
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
	/// The one expression of the attribute value, `on:click={handler}`.
	Expression {
		optional: bool,
		/// Without a value, the directive's own argument is the expression: `bind:value`.
		name: bool,
	},
	/// The one pattern of the attribute value, `let:item={{ id }}`.
	Pattern { optional: bool, name: bool },
	/// The attribute value as it is, text and expressions.
	Value,
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
	pub ty: &'static str,
	pub value: DirectiveValue,
	pub flags: Vec<(&'static str, bool)>,
	pub unique: Unique,
	/// What the directive declares in the scope of its element: the fields of its form, or,
	/// with none named, its value.
	pub declares: Option<Vec<&'static str>>,
}

#[derive(Clone, Debug)]
pub struct BlockRule {
	pub name: &'static str,
	pub ty: &'static str,
	pub open: Form,
	pub branches: Vec<BranchRule>,
	/// The boolean field that says the block was opened by a chained branch.
	pub chain_flag: Option<&'static str>,
	/// Every entry the block's forms can read, and every body they can open.
	pub entries: Vec<(&'static str, bool)>,
	pub bodies: Vec<(&'static str, bool)>,
}

#[derive(Clone, Debug)]
pub struct BranchRule {
	pub words: Vec<&'static str>,
	pub form: Form,
}

#[derive(Clone, Debug)]
pub struct TagRule {
	pub name: &'static str,
	pub ty: &'static str,
	pub form: Form,
	/// The tag stands among an element's attributes rather than in content.
	pub attribute: bool,
}

/// The characters after the opening delimiter that make a tag a block, a branch, a close or a
/// special tag: `{#if}`, `{:else}`, `{/if}`, `{@html}`.
#[derive(Clone, Debug)]
pub struct Sigils {
	pub open: &'static str,
	pub branch: &'static str,
	pub close: &'static str,
	pub tag: &'static str,
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

/// A field of the document's root, or a scope around fields: `{ instance?=script { fragment=fragment } }`.
#[derive(Clone, Debug)]
pub enum DocField {
	/// A field, what it holds, and whether it is left out rather than null when there is nothing.
	Field(&'static str, RootField, bool),
	Scope(Vec<DocField>),
}

#[derive(Clone, Debug)]
pub struct DocumentRule {
	pub ty: &'static str,
	pub fields: Vec<DocField>,
}

/// The fields of every element node.
#[derive(Clone, Debug)]
pub struct ElementFields {
	pub name: &'static str,
	pub attributes: &'static str,
	pub children: &'static str,
}

/// The type and fields of every text node: the text as read, and as written.
#[derive(Clone, Debug)]
pub struct TextRule {
	pub ty: &'static str,
	pub data: &'static str,
	pub raw: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct CommentRule {
	pub ty: &'static str,
	pub data: &'static str,
}

#[derive(Debug)]
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

// the writer's constants are keyed by address, so a grammar's names live for the process
fn keep(s: &str) -> &'static str {
	Box::leak(s.to_owned().into_boxed_str())
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

/// The fields of a document line from `at`, a `{` opening a scope of them up to its `}`.
fn doc_fields(tokens: &[String], at: &mut usize) -> Result<Vec<DocField>, String> {
	let mut fields = Vec::new();
	while *at < tokens.len() {
		let token = tokens[*at].as_str();
		match token {
			"{" => {
				*at += 1;
				let inner = doc_fields(tokens, at)?;
				if tokens.get(*at).map(String::as_str) != Some("}") {
					return Err("a scope of the document is not closed".into());
				}
				*at += 1;
				fields.push(DocField::Scope(inner));
			}
			"}" => break,
			_ => {
				let (field, holds) = token
					.split_once('=')
					.ok_or_else(|| format!("unexpected {token} on the document"))?;
				let (field, omit) = match field.strip_suffix('?') {
					Some(field) => (field, true),
					None => (field, false),
				};
				let holds = match holds {
					"fragment" => RootField::Fragment,
					"script" => RootField::Script { module: false },
					"script:module" => RootField::Script { module: true },
					"style" => RootField::Style,
					"comments" => RootField::Comments,
					"list" => RootField::EmptyList,
					"null" => RootField::Null,
					other => return Err(format!("the document cannot hold {other}")),
				};
				fields.push(DocField::Field(keep(field), holds, omit));
				*at += 1;
			}
		}
	}
	Ok(fields)
}

fn tokens(line: &str) -> Vec<String> {
	let mut out = Vec::new();
	for word in line.split_whitespace() {
		let mut rest = word;
		while !rest.is_empty() {
			if let Some(i) = rest.find(['[', ']', '|', '{', '}']) {
				if i > 0 {
					out.push(rest[..i].to_string());
				}
				out.push(rest[i..i + 1].to_string());
				rest = &rest[i + 1..];
			} else {
				out.push(rest.to_string());
				rest = "";
			}
		}
	}
	out
}

fn entry(name: &str) -> Option<Entry> {
	Some(match name {
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
		_ => return None,
	})
}

/// `attribute` or `attribute:value`, the value an attribute needs.
fn attribute_spec(spec: &str) -> (&'static str, Option<&'static str>) {
	match spec.split_once(':') {
		Some((attribute, value)) => (keep(attribute), Some(keep(value))),
		None => (keep(spec), None),
	}
}

struct Reader<'a> {
	tokens: &'a [String],
	at: usize,
}

impl Reader<'_> {
	fn peek(&self) -> Option<&str> {
		self.tokens.get(self.at).map(String::as_str)
	}

	fn next(&mut self) -> Option<&str> {
		let token = self.tokens.get(self.at)?;
		self.at += 1;
		Some(token)
	}

	/// `-> field[?] [chain field] [declares a b:outside]`, up to the end of the alternative.
	fn body(&mut self) -> Result<Body, String> {
		let mut body = Body {
			field: "",
			omit: false,
			chain: None,
			declares: Vec::new(),
		};
		let mut declaring = false;
		while let Some(token) = self.peek() {
			match token {
				"]" | "|" | "}" => break,
				"chain" => {
					self.at += 1;
					body.chain = Some(keep(self.next().ok_or("chain needs the nested block's field")?));
				}
				"declares" => {
					declaring = true;
					self.at += 1;
				}
				word if declaring => {
					let (field, outside) = match word.strip_suffix(":outside") {
						Some(field) => (field, true),
						None => (word, false),
					};
					body.declares.push(Declare {
						field: keep(field),
						outside,
					});
					self.at += 1;
				}
				word if body.field.is_empty() => {
					let (field, omit) = match word.strip_suffix('?') {
						Some(field) => (field, true),
						None => (word, false),
					};
					body.field = keep(field);
					body.omit = omit;
					self.at += 1;
				}
				other => return Err(format!("unexpected {other} in a body")),
			}
		}
		if body.field.is_empty() {
			return Err("a body needs a field after ->".into());
		}
		Ok(body)
	}

	/// Items up to `]`, `}`, `|` or the end; a `->` starts the body.
	fn items(&mut self) -> Result<(Vec<Item>, Option<Body>), String> {
		let mut items = Vec::new();
		let mut body = None;
		while let Some(token) = self.peek() {
			match token {
				"]" | "}" | "|" => break,
				"->" => {
					self.at += 1;
					body = Some(self.body()?);
				}
				"[" | "{" => {
					let required = token == "{";
					let closer = if required { "}" } else { "]" };
					self.at += 1;
					let mut alternatives = Vec::new();
					loop {
						let (items, body) = self.items()?;
						alternatives.push(Alternative { items, body });
						match self.next() {
							Some("|") => continue,
							Some(token) if token == closer => break,
							_ => return Err("a group is not closed".into()),
						}
					}
					items.push(Item::Group {
						alternatives,
						required,
						after: &[],
					});
				}
				token => {
					let item = match token.split_once('=') {
						Some((field, kind)) if !field.is_empty() => {
							let (field, omit) = match field.strip_suffix('?') {
								Some(field) => (field, true),
								None => (field, false),
							};
							Item::Entry {
								field: keep(field),
								entry: entry(kind).ok_or_else(|| format!("no entry named {kind}"))?,
								omit,
								stops: Stops::NONE,
							}
						}
						_ => Item::Literal(keep(token)),
					};
					items.push(item);
					self.at += 1;
				}
			}
		}
		Ok((items, body))
	}

	fn form(&mut self) -> Result<Form, String> {
		let (mut items, body) = self.items()?;
		if let Some(extra) = self.peek() {
			return Err(format!("unexpected {extra} after a form"));
		}
		resolve(&mut items, &[]);
		let mut entries = Vec::new();
		collect_entries(&items, &mut entries);
		let mut bodies = Vec::new();
		if let Some(body) = &body {
			bodies.push(body);
		}
		collect_alternative_bodies(&items, &mut bodies);
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
		Ok(Form { items, body, entries })
	}
}

impl Grammar {
	/// Reads a grammar from its text; the message names the line that could not be read.
	pub fn read(text: &str) -> Result<Grammar, String> {
		let mut grammar = Grammar {
			name: "",
			document: DocumentRule {
				ty: "Document",
				fields: vec![DocField::Field("children", RootField::Fragment, false)],
			},
			delimiters: ("{", "}"),
			attribute_expressions: false,
			attribute_shorthand: false,
			sigils: None,
			autoclose: false,
			trim: false,
			void: Vec::new(),
			fragment: None,
			fragment_scope: false,
			element_fields: ElementFields {
				name: "name",
				attributes: "attributes",
				children: "children",
			},
			text: TextRule {
				ty: "Text",
				data: "data",
				raw: None,
			},
			comment: CommentRule {
				ty: "Comment",
				data: "data",
			},
			verbatim: None,
			elements: Vec::new(),
			script: None,
			style: None,
			directive_syntax: None,
			shorthands: Vec::new(),
			directives: Vec::new(),
			spread: None,
			blocks: Vec::new(),
			tags: Vec::new(),
			declaration: None,
			expression: None,
		};
		for (number, line) in text.lines().enumerate() {
			// a comment is `//` at the start of the line or after whitespace
			let line = match line.find("//") {
				Some(i) if i == 0 || line[..i].ends_with(char::is_whitespace) => &line[..i],
				_ => line,
			};
			// the lines that spell punctuation keep their words whole
			let raw = line.trim_start().starts_with("delimiters ") || line.trim_start().starts_with("directives ");
			let tokens = if raw {
				line.split_whitespace().map(str::to_string).collect()
			} else {
				tokens(line)
			};
			if tokens.is_empty() {
				continue;
			}
			let indented = line.starts_with([' ', '\t']);
			grammar
				.line(&tokens, indented)
				.map_err(|message| format!("grammar line {}: {message}", number + 1))?;
		}
		if grammar.name.is_empty() {
			return Err("a grammar starts with its host's name".into());
		}
		for block in &mut grammar.blocks {
			let mut entries = block.open.entries.clone();
			let mut bodies = Vec::new();
			collect_bodies(&block.open, &mut bodies);
			for branch in &block.branches {
				entries.extend_from_slice(&branch.form.entries);
				collect_bodies(&branch.form, &mut bodies);
			}
			block.entries = entries;
			block.bodies = bodies;
		}
		Ok(grammar)
	}

	fn line(&mut self, tokens: &[String], indented: bool) -> Result<(), String> {
		let word = |i: usize| {
			tokens
				.get(i)
				.map(String::as_str)
				.ok_or_else(|| "the line ends early".to_string())
		};
		let head = tokens[0].as_str();
		if indented {
			let block = self.blocks.last_mut().ok_or("an indented line belongs to a block")?;
			let mut reader = Reader {
				tokens: &tokens[1..],
				at: 0,
			};
			match head {
				"open" => block.open = reader.form()?,
				"branch" => {
					let mut words = Vec::new();
					while let Some(token) = reader.peek() {
						if token.contains('=') || matches!(token, "[" | "{" | "->") {
							break;
						}
						words.push(keep(token));
						reader.at += 1;
					}
					if words.is_empty() {
						return Err("a branch needs its word".into());
					}
					let form = reader.form()?;
					if form.body.is_none() {
						return Err("a branch needs a body".into());
					}
					block.branches.push(BranchRule { words, form });
				}
				other => return Err(format!("unexpected {other} in a block")),
			}
			return Ok(());
		}
		match head {
			"host" => self.name = keep(word(1)?),
			"document" => {
				let mut at = 2;
				let fields = doc_fields(tokens, &mut at)?;
				if at < tokens.len() {
					return Err(format!("unexpected {} on the document", tokens[at]));
				}
				self.document = DocumentRule {
					ty: keep(word(1)?),
					fields,
				};
			}
			"delimiters" => self.delimiters = (keep(word(1)?), keep(word(2)?)),
			"attributes" => {
				for token in &tokens[1..] {
					match token.as_str() {
						"expressions" => self.attribute_expressions = true,
						"shorthand" => self.attribute_shorthand = true,
						other => return Err(format!("unexpected {other} on attributes")),
					}
				}
			}
			"sigils" => {
				let mut sigils = Sigils {
					open: "",
					branch: "",
					close: "",
					tag: "",
				};
				for token in &tokens[1..] {
					match token.split_once('=') {
						Some(("open", s)) => sigils.open = keep(s),
						Some(("branch", s)) => sigils.branch = keep(s),
						Some(("close", s)) => sigils.close = keep(s),
						Some(("tag", s)) => sigils.tag = keep(s),
						_ => return Err(format!("unexpected {token} on sigils")),
					}
				}
				if [sigils.open, sigils.branch, sigils.close, sigils.tag].contains(&"") {
					return Err("sigils need open, branch, close and tag".into());
				}
				self.sigils = Some(sigils);
			}
			"autoclose" => self.autoclose = true,
			"trim" => self.trim = true,
			"void" => self.void = tokens[1..].iter().map(|name| keep(name)).collect(),
			"fragment" => {
				self.fragment = Some((keep(word(1)?), keep(word(2)?)));
				for token in &tokens[3..] {
					match token.as_str() {
						"scope" => self.fragment_scope = true,
						other => return Err(format!("unexpected {other} on fragment")),
					}
				}
			}
			"elements" => {
				for token in &tokens[1..] {
					match token.split_once('=') {
						Some(("name", field)) => self.element_fields.name = keep(field),
						Some(("attributes", field)) => self.element_fields.attributes = keep(field),
						Some(("children", field)) => self.element_fields.children = keep(field),
						_ => return Err(format!("unexpected {token} on elements")),
					}
				}
			}
			"text" => {
				let mut rule = TextRule {
					ty: keep(word(1)?),
					data: "data",
					raw: None,
				};
				for token in &tokens[2..] {
					match token.split_once('=') {
						Some((field, "data")) => rule.data = keep(field),
						Some((field, "raw")) => rule.raw = Some(keep(field)),
						_ => return Err(format!("unexpected {token} on text")),
					}
				}
				self.text = rule;
			}
			"comment" => {
				let mut rule = CommentRule {
					ty: keep(word(1)?),
					data: "data",
				};
				for token in &tokens[2..] {
					match token.split_once('=') {
						Some((field, "data")) => rule.data = keep(field),
						_ => return Err(format!("unexpected {token} on comment")),
					}
				}
				self.comment = rule;
			}
			"verbatim" => self.verbatim = Some(keep(word(1)?)),
			"element" => {
				let name = match word(1)? {
					"component-name" => Match::Component,
					"*" => Match::Any,
					name => Match::Exact(keep(name)),
				};
				let mut rule = ElementRule {
					name,
					ty: keep(word(2)?),
					this: None,
					root: false,
					once: false,
					inside: None,
					outside: None,
					raw: false,
					rcdata: false,
				};
				let mut i = 3;
				while i < tokens.len() {
					match tokens[i].as_str() {
						"root" => rule.root = true,
						"once" => rule.once = true,
						"raw" => rule.raw = true,
						"rcdata" => rule.rcdata = true,
						"inside" => {
							i += 1;
							rule.inside = Some(keep(word(i)?));
						}
						"outside" => {
							i += 1;
							rule.outside = Some(keep(word(i)?));
						}
						flag => match flag.strip_prefix("this=") {
							Some(spec) => {
								rule.this = Some(match spec.strip_suffix(":text") {
									Some(field) => (keep(field), true),
									None => (keep(spec), false),
								})
							}
							None => return Err(format!("unexpected {flag} on an element")),
						},
					}
					i += 1;
				}
				self.elements.push(rule);
			}
			"script" => {
				let mut module = Vec::new();
				let mut typescript = Vec::new();
				for token in &tokens[2..] {
					let (list, spec) = match (token.strip_prefix("module="), token.strip_prefix("typescript=")) {
						(Some(spec), _) => (&mut module, spec),
						(_, Some(spec)) => (&mut typescript, spec),
						_ => return Err(format!("unexpected {token} on a script")),
					};
					list.push(attribute_spec(spec));
				}
				self.script = Some(ScriptRule {
					name: keep(word(1)?),
					module,
					typescript,
				});
			}
			"style" => self.style = Some(keep(word(1)?)),
			"directives" => {
				let mut syntax = DirectiveSyntax {
					prefix: None,
					arg: ":",
					modifier: "|",
					dynamic: None,
					name_field: None,
					arg_field: None,
					modifiers_field: None,
					raw_field: None,
					unique: false,
				};
				for token in &tokens[1..] {
					match token.split_once('=') {
						Some(("prefix", prefix)) => syntax.prefix = Some(keep(prefix)),
						Some(("arg", sep)) => syntax.arg = keep(sep),
						Some(("modifier", sep)) => syntax.modifier = keep(sep),
						Some(("dynamic", brackets)) if brackets.len() == 2 => {
							syntax.dynamic = Some((keep(&brackets[..1]), keep(&brackets[1..])))
						}
						Some(("field:name", field)) => syntax.name_field = Some(keep(field)),
						Some(("field:arg", field)) => syntax.arg_field = Some(keep(field)),
						Some(("field:modifiers", field)) => syntax.modifiers_field = Some(keep(field)),
						Some(("field:raw", field)) => syntax.raw_field = Some(keep(field)),
						Some(("unique", "raw")) => syntax.unique = true,
						_ => return Err(format!("unexpected {token} on directives")),
					}
				}
				self.directive_syntax = Some(syntax);
			}
			"shorthand" => self.shorthands.push(Shorthand {
				token: keep(word(1)?),
				name: keep(word(2)?),
				modifiers: tokens[3..]
					.iter()
					.map(|modifier| keep(modifier.strip_prefix('.').unwrap_or(modifier)))
					.collect(),
			}),
			"directive" => {
				let name = match word(1)? {
					"*" => Match::Any,
					name => Match::Exact(keep(name)),
				};
				let mut flags = Vec::new();
				let mut unique = Unique::No;
				let mut declares = None;
				// the trailing words: flags, then what the directive declares
				let mut trailing = |tokens: &[String]| {
					for (i, token) in tokens.iter().enumerate() {
						match token.as_str() {
							"declares" => {
								declares = Some(tokens[i + 1..].iter().map(|name| keep(name)).collect());
								break;
							}
							"unique" => unique = Unique::Kind,
							"unique:attribute" => unique = Unique::Attribute,
							token => match token.strip_prefix('!') {
								Some(name) => flags.push((keep(name), false)),
								None => flags.push((keep(token), true)),
							},
						}
					}
				};
				let value = match word(3)? {
					"expression" => DirectiveValue::Expression {
						optional: false,
						name: false,
					},
					"expression?" => DirectiveValue::Expression {
						optional: true,
						name: false,
					},
					"expression?name" => DirectiveValue::Expression {
						optional: true,
						name: true,
					},
					"pattern" => DirectiveValue::Pattern {
						optional: false,
						name: false,
					},
					"pattern?" => DirectiveValue::Pattern {
						optional: true,
						name: false,
					},
					"pattern?name" => DirectiveValue::Pattern {
						optional: true,
						name: true,
					},
					"value" => DirectiveValue::Value,
					_ => {
						// a form, then the flags after it: `!flag` is off
						let end = tokens
							.iter()
							.rposition(|t| t.contains('=') || matches!(t.as_str(), "[" | "]" | "{" | "}" | "|"))
							.map_or(3, |i| i + 1)
							.max(3);
						let mut reader = Reader {
							tokens: &tokens[3..end],
							at: 0,
						};
						let form = reader.form()?;
						trailing(&tokens[end..]);
						DirectiveValue::Form(form)
					}
				};
				if !matches!(value, DirectiveValue::Form(_)) {
					trailing(&tokens[4..]);
				}
				self.directives.push(DirectiveRule {
					name,
					ty: keep(word(2)?),
					value,
					flags,
					unique,
					declares,
				});
			}
			"spread" => self.spread = Some(keep(word(1)?)),
			"block" => {
				let mut chain_flag = None;
				for token in &tokens[3..] {
					match token.strip_prefix("chain=") {
						Some(field) => chain_flag = Some(keep(field)),
						None => return Err(format!("unexpected {token} on a block")),
					}
				}
				self.blocks.push(BlockRule {
					name: keep(word(1)?),
					ty: keep(word(2)?),
					open: Form::default(),
					branches: Vec::new(),
					chain_flag,
					entries: Vec::new(),
					bodies: Vec::new(),
				})
			}
			"tag" | "declaration" | "expression" => {
				let named = head == "tag";
				let attribute = named && tokens.last().is_some_and(|t| t == "attribute");
				let end = tokens.len() - usize::from(attribute);
				let mut reader = Reader {
					tokens: &tokens[if named { 3 } else { 2 }..end],
					at: 0,
				};
				let rule = TagRule {
					name: if named { keep(word(1)?) } else { "" },
					ty: keep(word(if named { 2 } else { 1 })?),
					form: reader.form()?,
					attribute,
				};
				match head {
					"tag" => self.tags.push(rule),
					"declaration" => self.declaration = Some(rule),
					_ => self.expression = Some(rule),
				}
			}
			other => return Err(format!("unexpected {other}")),
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
	#[test]
	fn a_body_declares_only_what_the_form_reads() {
		let good = "host t\ndelimiters { }\nsigils open=# branch=: close=/ tag=@\nfragment Fragment nodes\nelements name=name attributes=attributes children=fragment\ntext Text data=data\nblock each EachBlock\n  open expression=expression as context=pattern -> body declares context\n";
		assert!(
			super::Grammar::read(good).is_ok(),
			"{:?}",
			super::Grammar::read(good).err()
		);
		let typo = good.replace("declares context", "declares contexxt");
		let error = super::Grammar::read(&typo).unwrap_err();
		assert!(error.contains("contexxt"), "{error}");
	}

	use super::*;

	#[test]
	fn reads_the_svelte_grammar() {
		let grammar = Grammar::read(include_str!("../../hosts/svelte.grammar")).unwrap();
		assert_eq!(grammar.name, "svelte");
		assert_eq!(grammar.document.ty, "Root");
		assert!(
			matches!(&grammar.document.fields[5], DocField::Scope(inner) if matches!(inner[0], DocField::Field("instance", RootField::Script { module: false }, true)))
		);
		assert_eq!(grammar.directive("let").unwrap().declares, Some(vec![]));
		assert_eq!(grammar.fragment, Some(("Fragment", "nodes")));
		assert!(grammar.fragment_scope);
		assert!(grammar.attribute_expressions && grammar.attribute_shorthand && grammar.autoclose && grammar.trim);
		assert_eq!(grammar.sigils.as_ref().unwrap().tag, "@");
		assert!(grammar.tag("attach").unwrap().attribute && !grammar.tag("html").unwrap().attribute);
		assert!(grammar.is_void("br") && grammar.is_void("!DOCTYPE") && !grammar.is_void("div"));
		assert_eq!(grammar.elements.len(), 17);
		assert!(grammar.element("textarea").unwrap().rcdata);
		assert_eq!(grammar.directives.len(), 10);
		let syntax = grammar.directive_syntax.as_ref().unwrap();
		assert_eq!(
			(syntax.arg, syntax.modifier, syntax.arg_field),
			(":", "|", Some("name"))
		);
		assert_eq!(grammar.directive("bind").unwrap().unique, Unique::Attribute);
		assert_eq!(
			grammar.directive("in").unwrap().flags,
			[("intro", true), ("outro", false)]
		);
		let each = grammar.block("each").unwrap();
		assert_eq!(each.ty, "EachBlock");
		let body = each.open.body.as_ref().unwrap();
		assert_eq!(body.field, "body");
		assert_eq!(body.declares.len(), 2);
		assert!(
			matches!(each.open.items[1], Item::Group { ref alternatives, required: false, .. } if alternatives.len() == 1)
		);
		let await_ = grammar.block("await").unwrap();
		let Item::Group { alternatives, .. } = &await_.open.items[1] else {
			panic!()
		};
		assert_eq!(alternatives.len(), 2);
		assert_eq!(alternatives[0].body.as_ref().unwrap().field, "then");
		assert_eq!(await_.branches[1].words, ["catch"]);
		let if_ = grammar.block("if").unwrap();
		assert_eq!(if_.chain_flag, Some("elseif"));
		assert_eq!(if_.branches[0].form.body.as_ref().unwrap().chain, Some("consequent"));
		assert_eq!(grammar.script.as_ref().unwrap().typescript, [("lang", Some("ts"))]);
		assert_eq!(grammar.element("Foo.Bar").unwrap().ty, "Component");
		assert_eq!(grammar.element("div").unwrap().ty, "RegularElement");
		assert_eq!(grammar.element("svelte:head").unwrap().ty, "SvelteHead");
		assert!(Grammar::read("element div").is_err());
	}

	#[test]
	fn reads_the_vue_grammar() {
		let grammar = Grammar::read(include_str!("../../hosts/vue.grammar")).unwrap();
		assert_eq!(grammar.delimiters, ("{{", "}}"));
		assert!(!grammar.attribute_expressions && !grammar.autoclose && grammar.fragment.is_none());
		assert_eq!(grammar.element_fields.children, "children");
		let syntax = grammar.directive_syntax.as_ref().unwrap();
		assert_eq!(
			(syntax.prefix, syntax.modifier, syntax.dynamic),
			(Some("v-"), ".", Some(("[", "]")))
		);
		assert_eq!(grammar.shorthands.len(), 4);
		assert_eq!(grammar.shorthands[3].modifiers, ["prop"]);
		let for_ = grammar.directive("for").unwrap();
		let DirectiveValue::Form(form) = &for_.value else {
			panic!()
		};
		assert!(matches!(form.items[1], Item::Group { required: true, .. }));
		assert_eq!(grammar.directive("anything").unwrap().name, Match::Any);
		assert_eq!(for_.declares, Some(vec!["value", "key", "index"]));
		assert_eq!(grammar.verbatim, Some("v-pre"));
	}
}
