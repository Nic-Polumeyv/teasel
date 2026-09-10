//! The CSS of a style element: rules, at-rules and declarations as host nodes, selectors in
//! full, values as text with their comments listed.

use super::*;

/// A comment read out of the CSS: where it sits, and its offset into the value it interrupted.
struct CssComment {
	start: u32,
	end: u32,
	position: Option<u32>,
}

fn ident_char(c: char) -> bool {
	c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// `\` and one to six hex digits, then one whitespace or CRLF: the length of the sequence.
fn unicode_escape(rest: &str) -> Option<usize> {
	let hex = rest.strip_prefix('\\')?;
	let digits = hex.bytes().take_while(|b| b.is_ascii_hexdigit()).take(6).count();
	if digits == 0 {
		return None;
	}
	let after = &hex[digits..];
	let tail = if after.starts_with("\r\n") {
		2
	} else {
		after.chars().next().filter(|&c| is_space(c)).map_or(0, char::len_utf8)
	};
	Some(1 + digits + tail)
}

/// The `<an+b> [of]` of `:nth-child(...)`: its length, when it stands at the start of `rest`.
fn nth_of(rest: &str) -> Option<usize> {
	fn digits(s: &str) -> usize {
		s.bytes().take_while(|b| b.is_ascii_digit()).count()
	}
	fn spaces(s: &str) -> usize {
		s.len() - s.trim_start_matches(is_space).len()
	}
	// `\s*[+-]\s*\d+`
	fn offset(s: &str, signs: &[u8]) -> Option<usize> {
		let mut i = spaces(s);
		if !signs.contains(s.as_bytes().get(i)?) {
			return None;
		}
		i += 1;
		i += spaces(&s[i..]);
		let n = digits(&s[i..]);
		(n > 0).then_some(i + n)
	}
	// what the pattern may be followed by
	fn suffix(s: &str) -> Option<usize> {
		let n = spaces(s);
		if matches!(s.as_bytes().get(n), Some(b',') | Some(b')')) {
			return Some(0);
		}
		if n == 0 {
			return None;
		}
		let after = s[n..].strip_prefix("of")?;
		let m = spaces(after);
		if m > 0 {
			return Some(n + 2 + m);
		}
		matches!(after.as_bytes().first(), Some(b'.' | b'#' | b'[' | b'*' | b':' | b'&')).then_some(n + 2)
	}
	let mut heads: Vec<usize> = Vec::new();
	if rest.starts_with("even") {
		heads.push(4);
	} else if rest.starts_with("odd") {
		heads.push(3);
	} else if let Some(after) = rest.strip_prefix('-') {
		let n = digits(after);
		if after[n..].starts_with('n')
			&& let Some(o) = offset(&after[n + 1..], b"+")
		{
			heads.push(1 + n + 1 + o);
		}
	} else {
		let plus = usize::from(rest.starts_with('+'));
		let s = &rest[plus..];
		let n = digits(s);
		if n > 0 {
			heads.push(plus + n);
		}
		if s[n..].starts_with('n') {
			let base = plus + n + 1;
			match offset(&s[n + 1..], b"+-") {
				Some(o) => {
					heads.push(base + o);
					heads.push(base);
				}
				None => heads.push(base),
			}
		}
	}
	heads
		.into_iter()
		.find_map(|head| suffix(&rest[head..]).map(|tail| head + tail))
}

/// `\d+(\.\d+)?%` at the start of `rest`: its length.
fn percentage(rest: &str) -> Option<usize> {
	let n = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
	if n == 0 {
		return None;
	}
	let mut i = n;
	if rest[i..].starts_with('.') {
		let m = rest[i + 1..].bytes().take_while(|b| b.is_ascii_digit()).count();
		if m > 0 {
			i += 1 + m;
		}
	}
	rest[i..].starts_with('%').then_some(i + 1)
}

fn combinator(rest: &str) -> Option<&'static str> {
	["||", "+", "~", ">"].into_iter().find(|c| rest.starts_with(c))
}

impl<'a, E: Extension> Walker<'a, E> {
	/// A style element's sheet, from the content at the cursor through the closing tag.
	pub(super) fn style_sheet(&mut self, start: u32, name: &str, attributes: Vec<NodeId>) -> Result<NodeId> {
		let closer = format!("</{name}");
		let content_start = self.at;
		let mut comments = Vec::new();
		let mut children = Vec::new();
		loop {
			self.css_space(&mut comments, true)?;
			if self.matches(&closer) || self.at >= self.len() {
				break;
			}
			children.push(if self.matches("@") {
				self.at_rule(&mut comments)?
			} else {
				self.rule(&mut comments)?
			});
		}
		let content_end = self.at;
		self.expect(&closer)?;
		self.space();
		self.expect(">")?;
		let end = self.at;
		let comments: Vec<NodeId> = comments
			.into_iter()
			.map(|comment| {
				let mut fields = vec![("value", Value::Slice(comment.start + 2, comment.end - 2))];
				if let Some(position) = comment.position {
					fields.push(("position", Value::Int(position)));
				}
				self.host("CSSComment", comment.start, comment.end, fields, None, true)
			})
			.collect();
		let attributes = self.list(&attributes);
		let children = self.list(&children);
		let comments = self.list(&comments);
		let content = self.host(
			"",
			content_start,
			content_end,
			vec![
				("styles", Value::Slice(content_start, content_end)),
				("comment", Value::Null),
			],
			None,
			true,
		);
		Ok(self.host(
			"StyleSheet",
			start,
			end,
			vec![
				("attributes", Value::Nodes(attributes)),
				("children", Value::Nodes(children)),
				("comments", Value::Nodes(comments)),
				("content", Value::Node(content)),
			],
			None,
			true,
		))
	}

	/// Whitespace, comments and HTML comment markers; `capture` keeps the comments.
	fn css_space(&mut self, comments: &mut Vec<CssComment>, capture: bool) -> Result<()> {
		self.space();
		while self.matches("/*") || self.matches("<!--") {
			if self.matches("/*") {
				let comment = self.css_comment()?;
				if capture {
					comments.push(comment);
				}
			}
			if self.eat("<!--") {
				match self.rest().find("-->") {
					Some(i) => self.at += i as u32 + 3,
					None => return fail(self.len(), self.len(), Code::Expected, Some("-->")),
				}
			}
			self.space();
		}
		Ok(())
	}

	fn css_comment(&mut self) -> Result<CssComment> {
		let start = self.at;
		self.expect("/*")?;
		match self.rest().find("*/") {
			Some(i) => self.at += i as u32 + 2,
			None => return fail(self.len(), self.len(), Code::Expected, Some("*/")),
		}
		Ok(CssComment {
			start,
			end: self.at,
			position: None,
		})
	}

	fn at_rule(&mut self, comments: &mut Vec<CssComment>) -> Result<NodeId> {
		let start = self.at;
		self.expect("@")?;
		let name = self.css_identifier()?;
		let prelude = self.css_value(comments, true)?;
		let block = if self.matches("{") {
			Value::Node(self.css_block(comments)?)
		} else {
			self.expect(";")?;
			Value::Null
		};
		let name = self.intern(&name);
		let prelude = self.intern(&prelude);
		Ok(self.host(
			"Atrule",
			start,
			self.at,
			vec![
				("name", Value::Str(name)),
				("prelude", Value::Str(prelude)),
				("block", block),
			],
			None,
			true,
		))
	}

	fn rule(&mut self, comments: &mut Vec<CssComment>) -> Result<NodeId> {
		let start = self.at;
		let prelude = self.selector_list(comments, false)?;
		let block = self.css_block(comments)?;
		Ok(self.host(
			"Rule",
			start,
			self.at,
			vec![("prelude", Value::Node(prelude)), ("block", Value::Node(block))],
			None,
			true,
		))
	}

	fn selector_list(&mut self, comments: &mut Vec<CssComment>, inside_pseudo: bool) -> Result<NodeId> {
		let mut children = Vec::new();
		self.css_space(comments, true)?;
		let start = self.at;
		while self.at < self.len() {
			children.push(self.selector(comments, inside_pseudo)?);
			let end = self.at;
			self.css_space(comments, true)?;
			if self.matches(if inside_pseudo { ")" } else { "{" }) {
				let children = self.list(&children);
				return Ok(self.host(
					"SelectorList",
					start,
					end,
					vec![("children", Value::Nodes(children))],
					None,
					true,
				));
			}
			self.expect(",")?;
			self.css_space(comments, true)?;
		}
		fail(self.len(), self.len(), Code::UnexpectedEof, None)
	}

	fn selector(&mut self, comments: &mut Vec<CssComment>, inside_pseudo: bool) -> Result<NodeId> {
		let list_start = self.at;
		let mut children = Vec::new();
		let mut combinator_node: Option<NodeId> = None;
		let mut selectors: Vec<NodeId> = Vec::new();
		let mut relative_start = self.at;
		let closer = if inside_pseudo { ")" } else { "{" };
		while self.at < self.len() {
			let start = self.at;
			if self.eat("&") {
				let name = self.intern("&");
				selectors.push(self.host(
					"NestingSelector",
					start,
					self.at,
					vec![("name", Value::Str(name))],
					None,
					true,
				));
			} else if self.eat("*") {
				let mut fields = Vec::new();
				let mut name = String::from("*");
				if self.eat("|") {
					let namespace = self.intern("*");
					fields.push(("namespace", Value::Str(namespace)));
					name = if self.eat("*") {
						String::from("*")
					} else {
						self.css_identifier()?
					};
				}
				let name = self.intern(&name);
				fields.insert(0, ("name", Value::Str(name)));
				selectors.push(self.host("TypeSelector", start, self.at, fields, None, true));
			} else if self.eat("#") {
				let name = self.css_identifier()?;
				let name = self.intern(&name);
				selectors.push(self.host(
					"IdSelector",
					start,
					self.at,
					vec![("name", Value::Str(name))],
					None,
					true,
				));
			} else if self.eat(".") {
				let name = self.css_identifier()?;
				let name = self.intern(&name);
				selectors.push(self.host(
					"ClassSelector",
					start,
					self.at,
					vec![("name", Value::Str(name))],
					None,
					true,
				));
			} else if self.eat("::") {
				let name = self.css_identifier()?;
				let name = self.intern(&name);
				let mut fields = vec![("name", Value::Str(name))];
				if self.eat("(") {
					let args = self.selector_list(comments, true)?;
					self.expect(")")?;
					fields.push(("args", Value::Node(args)));
				}
				selectors.push(self.host("PseudoElementSelector", start, self.at, fields, None, true));
			} else if self.eat(":") {
				let name = self.css_identifier()?;
				let name = self.intern(&name);
				let args = if self.eat("(") {
					let args = self.selector_list(comments, true)?;
					self.expect(")")?;
					Value::Node(args)
				} else {
					Value::Null
				};
				selectors.push(self.host(
					"PseudoClassSelector",
					start,
					self.at,
					vec![("name", Value::Str(name)), ("args", args)],
					None,
					true,
				));
			} else if self.eat("[") {
				self.space();
				let name = self.css_identifier()?;
				self.space();
				let matcher = {
					let rest = self.rest();
					let len = if rest.starts_with(['~', '^', '$', '*', '|']) && rest[1..].starts_with('=') {
						2
					} else if rest.starts_with('=') {
						1
					} else {
						0
					};
					let matcher = &rest[..len];
					self.at += len as u32;
					(len > 0).then_some(matcher)
				};
				let mut value = None;
				if matcher.is_some() {
					self.space();
					value = Some(self.css_attribute_value()?);
				}
				self.space();
				let flags = {
					let len = self.rest().bytes().take_while(|b| b.is_ascii_alphabetic()).count();
					let flags = &self.rest()[..len];
					self.at += len as u32;
					(len > 0).then_some(flags)
				};
				self.space();
				self.expect("]")?;
				let name = self.intern(&name);
				let matcher = matcher.map_or(Value::Null, |m| Value::Str(self.intern(m)));
				let value = value.map_or(Value::Null, |v| Value::Str(self.intern(&v)));
				let flags = flags.map_or(Value::Null, |f| Value::Str(self.intern(f)));
				selectors.push(self.host(
					"AttributeSelector",
					start,
					self.at,
					vec![
						("name", Value::Str(name)),
						("matcher", matcher),
						("value", value),
						("flags", flags),
					],
					None,
					true,
				));
			} else if let Some(len) = nth_of(self.rest()).filter(|_| inside_pseudo) {
				self.at += len as u32;
				selectors.push(self.host(
					"Nth",
					start,
					self.at,
					vec![("value", Value::Slice(start, self.at))],
					None,
					true,
				));
			} else if let Some(len) = percentage(self.rest()) {
				self.at += len as u32;
				selectors.push(self.host(
					"Percentage",
					start,
					self.at,
					vec![("value", Value::Slice(start, self.at))],
					None,
					true,
				));
			} else if combinator(self.rest()).is_none() {
				let mut name = self.css_identifier()?;
				let mut fields = Vec::new();
				if self.eat("|") {
					let namespace = self.intern(&name);
					fields.push(("namespace", Value::Str(namespace)));
					name = if self.eat("*") {
						String::from("*")
					} else {
						self.css_identifier()?
					};
				}
				let name = self.intern(&name);
				fields.insert(0, ("name", Value::Str(name)));
				selectors.push(self.host("TypeSelector", start, self.at, fields, None, true));
			}
			let index = self.at;
			self.css_space(comments, false)?;
			if self.matches(",") || self.matches(closer) {
				self.at = index;
				let relative = self.relative_selector(combinator_node, &selectors, relative_start, index);
				children.push(relative);
				let children = self.list(&children);
				return Ok(self.host(
					"ComplexSelector",
					list_start,
					index,
					vec![("children", Value::Nodes(children))],
					None,
					true,
				));
			}
			self.at = index;
			if let Some(next) = self.css_combinator()? {
				if !selectors.is_empty() {
					let relative = self.relative_selector(combinator_node, &selectors, relative_start, index);
					children.push(relative);
				}
				selectors.clear();
				combinator_node = Some(next);
				relative_start = self.tree().node(next).start;
				self.space();
				if self.matches(",") || self.matches(closer) {
					return fail(self.at, self.at, Code::Expected, Some("a selector"));
				}
			}
		}
		fail(self.len(), self.len(), Code::UnexpectedEof, None)
	}

	fn relative_selector(&mut self, combinator: Option<NodeId>, selectors: &[NodeId], start: u32, end: u32) -> NodeId {
		let selectors = self.list(selectors);
		self.host(
			"RelativeSelector",
			start,
			end,
			vec![
				("combinator", combinator.map_or(Value::Null, Value::Node)),
				("selectors", Value::Nodes(selectors)),
			],
			None,
			true,
		)
	}

	fn css_combinator(&mut self) -> Result<Option<NodeId>> {
		let start = self.at;
		self.space();
		let index = self.at;
		if let Some(name) = combinator(self.rest()) {
			self.at += name.len() as u32;
			let end = self.at;
			self.space();
			let name = self.intern(name);
			return Ok(Some(self.host(
				"Combinator",
				index,
				end,
				vec![("name", Value::Str(name))],
				None,
				true,
			)));
		}
		if self.at != start {
			let name = self.intern(" ");
			return Ok(Some(self.host(
				"Combinator",
				start,
				self.at,
				vec![("name", Value::Str(name))],
				None,
				true,
			)));
		}
		Ok(None)
	}

	fn css_block(&mut self, comments: &mut Vec<CssComment>) -> Result<NodeId> {
		let start = self.at;
		self.expect("{")?;
		let mut children = Vec::new();
		while self.at < self.len() {
			self.css_space(comments, true)?;
			if self.matches("}") {
				break;
			}
			children.push(self.block_item(comments)?);
		}
		self.expect("}")?;
		let children = self.list(&children);
		Ok(self.host(
			"Block",
			start,
			self.at,
			vec![("children", Value::Nodes(children))],
			None,
			true,
		))
	}

	/// A declaration, a rule or an at-rule: a look ahead to the next `{` or `;` tells which.
	fn block_item(&mut self, comments: &mut Vec<CssComment>) -> Result<NodeId> {
		if self.matches("@") {
			return self.at_rule(comments);
		}
		let start = self.at;
		self.css_value(&mut Vec::new(), false)?;
		let opens = self.byte() == Some(b'{');
		self.at = start;
		if opens {
			self.rule(comments)
		} else {
			self.declaration(comments)
		}
	}

	fn declaration(&mut self, comments: &mut Vec<CssComment>) -> Result<NodeId> {
		let start = self.at;
		let len = self
			.rest()
			.find(|c: char| is_space(c) || c == ':')
			.unwrap_or(self.rest().len());
		let property = &self.src[start as usize..start as usize + len];
		self.at += len as u32;
		self.space();
		self.eat(":");
		let index = self.at;
		self.space();
		let value = self.css_value(comments, true)?;
		if value.is_empty() && !property.starts_with("--") {
			return fail(start, index, Code::Expected, Some("a declaration value"));
		}
		let end = self.at;
		if !self.matches("}") {
			self.expect(";")?;
		}
		let value = self.intern(&value);
		Ok(self.host(
			"Declaration",
			start,
			end,
			vec![
				("property", Value::Slice(start, start + len as u32)),
				("value", Value::Str(value)),
			],
			None,
			true,
		))
	}

	/// Text up to a `;`, `{` or `}` outside quotes and `url(...)`, its comments taken out.
	fn css_value(&mut self, comments: &mut Vec<CssComment>, capture: bool) -> Result<String> {
		let mut value = String::new();
		// the value's length in UTF-16 units, where a comment's position is measured
		let mut units = 0u32;
		let mut positions: Vec<usize> = Vec::new();
		let mut escaped = false;
		let mut in_url = false;
		let mut quote: Option<char> = None;
		while let Some(c) = self.char() {
			if escaped {
				value.push('\\');
				value.push(c);
				units += 1 + c.len_utf16() as u32;
				escaped = false;
				self.at += c.len_utf8() as u32;
				continue;
			} else if c == '\\' {
				escaped = true;
				self.at += 1;
				continue;
			} else if Some(c) == quote {
				quote = None;
			} else if c == ')' {
				in_url = false;
			} else if quote.is_none() && (c == '"' || c == '\'') {
				quote = Some(c);
			} else if c == '(' && value.ends_with("url") {
				in_url = true;
			} else if (c == ';' || c == '{' || c == '}') && !in_url && quote.is_none() {
				let leading = value.trim_start_matches(is_space);
				let leading = value[..value.len() - leading.len()].encode_utf16().count() as u32;
				for &i in &positions {
					let position = comments[i].position.unwrap_or(0);
					comments[i].position = Some(position.saturating_sub(leading));
				}
				return Ok(value.trim_matches(is_space).to_string());
			} else if c == '/' && !in_url && quote.is_none() && self.rest()[1..].starts_with('*') {
				let mut comment = self.css_comment()?;
				if capture {
					comment.position = Some(units);
					comments.push(comment);
					positions.push(comments.len() - 1);
				}
				continue;
			}
			value.push(c);
			units += c.len_utf16() as u32;
			self.at += c.len_utf8() as u32;
		}
		fail(self.len(), self.len(), Code::UnexpectedEof, None)
	}

	/// `foo`, `'foo bar'` or `"foo bar"`.
	fn css_attribute_value(&mut self) -> Result<String> {
		let mut value = String::new();
		let mut escaped = false;
		let quote = if self.eat("\"") {
			Some('"')
		} else if self.eat("'") {
			Some('\'')
		} else {
			None
		};
		while let Some(c) = self.char() {
			if escaped {
				value.push('\\');
				value.push(c);
				escaped = false;
			} else if c == '\\' {
				escaped = true;
			} else if quote.map_or(is_space(c) || c == ']', |q| c == q) {
				if quote.is_some() {
					self.at += 1;
				}
				return Ok(value.trim_matches(is_space).to_string());
			} else {
				value.push(c);
			}
			self.at += c.len_utf8() as u32;
		}
		fail(self.len(), self.len(), Code::UnexpectedEof, None)
	}

	/// An identifier as CSS Syntax spells one, escapes decoded.
	fn css_identifier(&mut self) -> Result<String> {
		let start = self.at;
		let rest = self.rest();
		let digit_first = rest
			.strip_prefix('-')
			.unwrap_or(rest)
			.starts_with(|c: char| c.is_ascii_digit());
		if digit_first {
			return fail(start, start, Code::Expected, Some("a valid CSS identifier"));
		}
		let mut identifier = String::new();
		while let Some(c) = self.char() {
			if c == '\\' {
				if let Some(len) = unicode_escape(self.rest()) {
					let hex = self.rest()[1..len].trim_end_matches(is_space).trim_end_matches("\r\n");
					let code = u32::from_str_radix(hex.trim_end(), 16).unwrap_or(0xfffd);
					let character = char::from_u32(code).unwrap_or('\u{fffd}');
					if character == '\\' {
						identifier.push_str("\\\\");
					} else {
						identifier.push(character);
					}
					self.at += len as u32;
				} else {
					identifier.push('\\');
					self.at += 1;
					if let Some(next) = self.char() {
						identifier.push(next);
						self.at += next.len_utf8() as u32;
					}
				}
			} else if c as u32 >= 160 || ident_char(c) {
				identifier.push(c);
				self.at += c.len_utf8() as u32;
			} else {
				break;
			}
		}
		if identifier.is_empty() {
			return fail(start, start, Code::Expected, Some("a valid CSS identifier"));
		}
		Ok(identifier)
	}
}
