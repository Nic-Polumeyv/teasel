//! A host grammar: what a template language puts around the JavaScript it embeds, read once
//! from the JSON a host hands over. Every form is a sequence of the host's own words and
//! punctuators around JavaScript entries; what may follow an entry is what ends it.

use super::wire::{self, Json, Wire, keep, wire};

wire! {
	/// A JavaScript entry inside a form.
	pub copy enum Entry {
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
}

wire! {
	/// One step of a form.
	pub enum Item {
		/// One of the host's words or punctuators.
		Literal(&'static str),
		/// A JavaScript entry read into a field; `omit` leaves the field out when the entry was not
		/// read, where the plain form gives it null.
		Entry {
			field: &'static str,
			entry: Entry,
			omit: bool,
		} derived {
			stops: Stops,
		},
		/// Alternatives tried in order: at most one, or exactly one when `required`.
		Group {
			alternatives: Vec<Alternative>,
			required: bool,
		} derived {
			/// The literals that may follow the group.
			after: &'static [&'static str],
		},
	}
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

wire! {
	pub struct Alternative {
		pub items: Vec<Item>,
		/// The body the block opens when this alternative was read, `[ then value=pattern -> then ]`.
		pub body: Option<Body>,
	}
}

wire! {
	/// What a block's body is: the field that holds it, and what the body's scope declares.
	pub struct Body {
		pub field: &'static str,
		/// The field is left out of blocks that never opened this body; otherwise it is null there.
		pub omit: bool,
		/// A branch that nests a new block of the same kind into the field, `{:else if}`: the field
		/// of the nested block that its own body fills.
		pub chain: Option<&'static str>,
		pub declares: Vec<Declare>,
	}
}

wire! {
	pub struct Declare {
		pub field: &'static str,
		/// Declared in the scope around the block rather than inside it: a snippet's name.
		pub outside: bool,
	}
}

wire! {
	pub struct Form {
		pub items: Vec<Item>,
		pub body: Option<Body>,
	} derived {
		/// Every entry the form can read, and whether its field is left out when it was not.
		pub entries: Vec<(&'static str, bool)>,
	}
}

wire! {
	pub copy enum Match {
		Exact(&'static str),
		/// A capitalized or dotted name.
		Component,
		Any,
	}
}

wire! {
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
}

wire! {
	pub struct ScriptRule {
		pub name: &'static str,
		/// Attributes that make the script the module one, each with the text value it needs, if any.
		pub module: Vec<(&'static str, Option<&'static str>)>,
		/// Attributes that make the document TypeScript, the same way.
		pub typescript: Vec<(&'static str, Option<&'static str>)>,
	}
}

wire! {
	/// How a directive's attribute name is spelled: `prefix name arg modifiers`, the name being the
	/// directive's own when there is no prefix.
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
}

wire! {
	/// A character standing for a directive's prefix and name, `:` for `v-bind`.
	pub struct Shorthand {
		pub token: &'static str,
		pub name: &'static str,
		pub modifiers: Vec<&'static str>,
	}
}

wire! {
	pub enum DirectiveValue {
		/// The one expression of the attribute value, `on:click={handler}`; `name` makes the
		/// directive's own argument the expression when there is no value: `bind:value`.
		Expression {
			optional: bool,
			name: bool,
		},
		/// The one pattern of the attribute value, `let:item={{ id }}`.
		Pattern {
			optional: bool,
			name: bool,
		},
		/// The attribute value as it is, text and expressions.
		Value,
		/// The attribute value read by a form, `v-for="item in items"`.
		Form(Form),
	}
}

wire! {
	/// Which names a directive may not repeat on an element.
	pub copy enum Unique {
		No,
		/// Its own argument, among directives of its kind.
		Kind,
		/// Its argument, among the plain attributes too.
		Attribute,
	}
}

wire! {
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
}

wire! {
	pub struct BlockRule {
		pub name: &'static str,
		pub ty: &'static str,
		pub open: Form,
		pub branches: Vec<BranchRule>,
		/// The boolean field that says the block was opened by a chained branch.
		pub chain_flag: Option<&'static str>,
	} derived {
		/// Every entry the block's forms can read, and every body they can open.
		pub entries: Vec<(&'static str, bool)>,
		pub bodies: Vec<(&'static str, bool)>,
	}
}

wire! {
	pub struct BranchRule {
		pub words: Vec<&'static str>,
		pub form: Form,
	}
}

wire! {
	pub struct TagRule {
		pub name: &'static str,
		pub ty: &'static str,
		pub form: Form,
		/// The tag stands among an element's attributes rather than in content.
		pub attribute: bool,
	}
}

wire! {
	/// The characters after the opening delimiter that make a tag a block, a branch, a close or a
	/// special tag: `{#if}`, `{:else}`, `{/if}`, `{@html}`.
	pub struct Sigils {
		pub open: &'static str,
		pub branch: &'static str,
		pub close: &'static str,
		pub tag: &'static str,
	}
}

wire! {
	/// What a field of the document's root holds.
	pub copy enum RootField {
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
}

wire! {
	/// A field of the document's root, or a scope around fields.
	pub enum DocField {
		/// A field, what it holds, and whether it is left out rather than null when there is nothing.
		Field {
			field: &'static str,
			holds: RootField,
			omit: bool,
		},
		Scope(Vec<DocField>),
	}
}

wire! {
	pub struct DocumentRule {
		pub ty: &'static str,
		pub fields: Vec<DocField>,
	}
}

wire! {
	/// The fields of every element node.
	pub struct ElementFields {
		pub name: &'static str,
		pub attributes: &'static str,
		pub children: &'static str,
	}
}

wire! {
	/// The type and fields of every text node: the text as read, and as written.
	pub struct TextRule {
		pub ty: &'static str,
		pub data: &'static str,
		pub raw: Option<&'static str>,
	}
}

wire! {
	pub struct CommentRule {
		pub ty: &'static str,
		pub data: &'static str,
	}
}

wire! {
	/// A host language: its document, its content, and the JavaScript inside them.
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

impl Form {
	// what the walker takes from a form is computed once, here
	fn finish(&mut self) -> Result<(), String> {
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
						body.field, declare.field
					));
				}
			}
		}
		self.entries = entries;
		Ok(())
	}
}

impl Grammar {
	/// Reads a grammar from its JSON; the message names the field that could not be read.
	pub fn read(text: &str) -> Result<Grammar, String> {
		let json = Json::parse(text)?;
		let mut grammar = <Grammar as Wire>::read(&json, "grammar")?;
		grammar.finish()?;
		Ok(grammar)
	}

	/// The TypeScript module of every type on the wire, for the side that writes it.
	pub fn wire_types() -> String {
		wire::module::<Grammar>("crates/teasel/src/host/grammar.rs")
	}

	fn finish(&mut self) -> Result<(), String> {
		if self.name.is_empty() {
			return Err("a grammar names its host".into());
		}
		for rule in &mut self.directives {
			if let DirectiveValue::Form(form) = &mut rule.value {
				form.finish()?;
			}
		}
		for rule in self
			.tags
			.iter_mut()
			.chain(&mut self.declaration)
			.chain(&mut self.expression)
		{
			rule.form.finish()?;
		}
		for block in &mut self.blocks {
			block.open.finish()?;
			let mut entries = block.open.entries.clone();
			let mut bodies = Vec::new();
			collect_bodies(&block.open, &mut bodies);
			for branch in &mut block.branches {
				branch.form.finish()?;
				if branch.form.body.is_none() {
					return Err(format!(
						"the {} branch of {} needs a body",
						branch.words.join(" "),
						block.name
					));
				}
				entries.extend_from_slice(&branch.form.entries);
				collect_bodies(&branch.form, &mut bodies);
			}
			block.entries = entries;
			block.bodies = bodies;
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
