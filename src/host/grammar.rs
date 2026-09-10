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
	/// `pattern = expression`, a const declaration the host spells without the keyword.
	Const,
	/// An identifier kept as its name, which the body still declares.
	Name,
	/// Identifiers separated by commas, possibly none.
	Identifiers,
}

/// One step of a form.
#[derive(Clone, Debug)]
pub enum Item {
	Literal(&'static str),
	/// `field=entry`; `field?=entry` leaves the field out when the entry was not read, where the
	/// plain form gives it null.
	Entry { field: &'static str, entry: Entry, omit: bool },
	/// `[ a | b ]`: at most one alternative, tried in order.
	Optional(Vec<Alternative>),
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
}

#[derive(Clone, Debug)]
pub struct ScriptRule {
	pub name: &'static str,
	/// Attributes that make the script the module one, each with the text value it needs, if any.
	pub module: Vec<(&'static str, Option<&'static str>)>,
	/// Attributes that make the document TypeScript, the same way.
	pub typescript: Vec<(&'static str, Option<&'static str>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectiveValue {
	Expression {
		optional: bool,
		/// Without a value, the directive's own name is the expression: `bind:value`.
		name: bool,
	},
	/// The attribute value as it is, text and expressions.
	Value,
}

#[derive(Clone, Debug)]
pub struct DirectiveRule {
	pub prefix: &'static str,
	pub ty: &'static str,
	pub value: DirectiveValue,
	pub flags: Vec<(&'static str, bool)>,
}

#[derive(Clone, Debug)]
pub struct BlockRule {
	pub name: &'static str,
	pub ty: &'static str,
	pub open: Form,
	pub branches: Vec<BranchRule>,
	/// The boolean field that says the block was opened by a chained branch.
	pub chain_flag: Option<&'static str>,
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
}

/// What a field of the document's root holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootField {
	Fragment,
	/// The script, the module one when `module`.
	Script { module: bool },
	Style,
	/// Every comment read.
	Comments,
	EmptyList,
	Null,
}

#[derive(Clone, Debug)]
pub struct DocumentRule {
	pub ty: &'static str,
	/// Each field, what it holds, and whether it is left out rather than null when there is nothing.
	pub fields: Vec<(&'static str, RootField, bool)>,
}

#[derive(Debug)]
pub struct Grammar {
	pub name: &'static str,
	pub document: DocumentRule,
	pub elements: Vec<ElementRule>,
	pub script: Option<ScriptRule>,
	pub style: Option<&'static str>,
	pub directives: Vec<DirectiveRule>,
	pub spread: Option<&'static str>,
	pub attach: Option<&'static str>,
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

fn tokens(line: &str) -> Vec<String> {
	let mut out = Vec::new();
	for word in line.split_whitespace() {
		let mut rest = word;
		while !rest.is_empty() {
			if let Some(i) = rest.find(['[', ']', '|']) {
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
		"const" => Entry::Const,
		"identifiers" => Entry::Identifiers,
		"name" => Entry::Name,
		_ => return None,
	})
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
				"]" | "|" => break,
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

	/// Items up to `]`, `|` or the end; a `->` starts the body.
	fn items(&mut self) -> Result<(Vec<Item>, Option<Body>), String> {
		let mut items = Vec::new();
		let mut body = None;
		while let Some(token) = self.peek() {
			match token {
				"]" | "|" => break,
				"->" => {
					self.at += 1;
					body = Some(self.body()?);
				}
				"[" => {
					self.at += 1;
					let mut alternatives = Vec::new();
					loop {
						let (items, body) = self.items()?;
						alternatives.push(Alternative { items, body });
						match self.next() {
							Some("|") => continue,
							Some("]") => break,
							_ => return Err("an optional group is not closed".into()),
						}
					}
					items.push(Item::Optional(alternatives));
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
		let (items, body) = self.items()?;
		if let Some(extra) = self.peek() {
			return Err(format!("unexpected {extra} after a form"));
		}
		Ok(Form { items, body })
	}
}

impl Grammar {
	/// Reads a grammar from its text; the message names the line that could not be read.
	pub fn read(text: &str) -> Result<Grammar, String> {
		let mut grammar = Grammar {
			name: "",
			document: DocumentRule {
				ty: "Document",
				fields: vec![("fragment", RootField::Fragment, false)],
			},
			elements: Vec::new(),
			script: None,
			style: None,
			directives: Vec::new(),
			spread: None,
			attach: None,
			blocks: Vec::new(),
			tags: Vec::new(),
			declaration: None,
			expression: None,
		};
		for (number, line) in text.lines().enumerate() {
			let line = line.split('#').next().unwrap_or("");
			let tokens = tokens(line);
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
		Ok(grammar)
	}

	fn line(&mut self, tokens: &[String], indented: bool) -> Result<(), String> {
		let word = |i: usize| tokens.get(i).map(String::as_str).ok_or_else(|| format!("the line ends early"));
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
						if token.contains('=') || matches!(token, "[" | "->") {
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
				let mut fields = Vec::new();
				for token in &tokens[2..] {
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
					fields.push((keep(field), holds, omit));
				}
				self.document = DocumentRule {
					ty: keep(word(1)?),
					fields,
				};
			}
			"element" => {
				let name = match word(1)? {
					"component" => Match::Component,
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
				};
				let mut i = 3;
				while i < tokens.len() {
					match tokens[i].as_str() {
						"root" => rule.root = true,
						"once" => rule.once = true,
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
					list.push(match spec.split_once(':') {
						Some((attribute, value)) => (keep(attribute), Some(keep(value))),
						None => (keep(spec), None),
					});
				}
				self.script = Some(ScriptRule {
					name: keep(word(1)?),
					module,
					typescript,
				});
			}
			"style" => self.style = Some(keep(word(1)?)),
			"directive" => {
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
					"value" => DirectiveValue::Value,
					other => return Err(format!("unexpected value {other} on a directive")),
				};
				let mut rule = DirectiveRule {
					prefix: keep(word(1)?),
					ty: keep(word(2)?),
					value,
					flags: Vec::new(),
				};
				for flag in &tokens[4..] {
					match flag.strip_prefix('!') {
						Some(name) => rule.flags.push((keep(name), false)),
						None => rule.flags.push((keep(flag), true)),
					}
				}
				self.directives.push(rule);
			}
			"spread" => self.spread = Some(keep(word(1)?)),
			"attach" => self.attach = Some(keep(word(1)?)),
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
				})
			}
			"tag" | "declaration" | "expression" => {
				let named = head == "tag";
				let mut reader = Reader {
					tokens: &tokens[if named { 3 } else { 2 }..],
					at: 0,
				};
				let rule = TagRule {
					name: if named { keep(word(1)?) } else { "" },
					ty: keep(word(if named { 2 } else { 1 })?),
					form: reader.form()?,
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

	pub fn directive(&self, prefix: &str) -> Option<&DirectiveRule> {
		self.directives.iter().find(|rule| rule.prefix == prefix)
	}

	pub fn block(&self, name: &str) -> Option<&BlockRule> {
		self.blocks.iter().find(|rule| rule.name == name)
	}

	pub fn tag(&self, name: &str) -> Option<&TagRule> {
		self.tags.iter().find(|rule| rule.name == name)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reads_the_svelte_grammar() {
		let grammar = Grammar::read(include_str!("../../hosts/svelte.grammar")).unwrap();
		assert_eq!(grammar.name, "svelte");
		assert_eq!(grammar.document.ty, "Root");
		assert_eq!(grammar.document.fields[5], ("instance", RootField::Script { module: false }, true));
		assert_eq!(grammar.elements.len(), 14);
		assert_eq!(grammar.directives.len(), 10);
		let each = grammar.block("each").unwrap();
		assert_eq!(each.ty, "EachBlock");
		let body = each.open.body.as_ref().unwrap();
		assert_eq!(body.field, "body");
		assert_eq!(body.declares.len(), 2);
		assert!(matches!(each.open.items[1], Item::Optional(ref alternatives) if alternatives.len() == 1));
		let await_ = grammar.block("await").unwrap();
		let Item::Optional(alternatives) = &await_.open.items[1] else { panic!() };
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
}
