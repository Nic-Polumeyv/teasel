//! The Node addon: each function takes the source and acorn-shaped options and returns ESTree
//! JSON, which the package's JavaScript turns into objects and errors.

use napi_derive::napi;
use teasel::json::{self, Entry, Request};

#[napi(object)]
#[derive(Default)]
pub struct Options {
	/// `"module"` or `"script"`; a module by default.
	pub source_type: Option<String>,
	pub typescript: Option<bool>,
	pub comments: Option<bool>,
	pub preserve_parens: Option<bool>,
	pub allow_return_outside_function: Option<bool>,
	pub allow_await_outside_function: Option<bool>,
	pub allow_super_outside_method: Option<bool>,
	pub allow_undeclared_exports: Option<bool>,
}

fn request(entry: Entry, offset: u32, options: Option<Options>) -> Request {
	let options = options.unwrap_or_default();
	Request {
		entry,
		offset,
		typescript: options.typescript.unwrap_or(false),
		comments: options.comments.unwrap_or(false),
		options: teasel::Options {
			module: options.source_type.as_deref() != Some("script"),
			preserve_parens: options.preserve_parens.unwrap_or(false),
			allow_return_outside_function: options.allow_return_outside_function.unwrap_or(false),
			allow_await_outside_function: options.allow_await_outside_function.unwrap_or(false),
			allow_super_outside_method: options.allow_super_outside_method.unwrap_or(false),
			allow_undeclared_exports: options.allow_undeclared_exports.unwrap_or(false),
		},
	}
}

/// Byte offset of a UTF-16 offset, so callers can pass acorn-style positions.
fn byte_offset(source: &str, utf16: u32) -> u32 {
	let mut units = 0;
	for (byte, c) in source.char_indices() {
		if units >= utf16 as usize {
			return byte as u32;
		}
		units += c.len_utf16();
	}
	source.len() as u32
}

#[napi]
pub fn parse(source: String, options: Option<Options>) -> String {
	json::parse(&source, &request(Entry::Program, 0, options))
}

#[napi]
pub fn parse_expression_at(source: String, offset: u32, options: Option<Options>) -> String {
	let offset = byte_offset(&source, offset);
	json::parse(&source, &request(Entry::Expression, offset, options))
}

#[napi]
pub fn parse_pattern_at(source: String, offset: u32, options: Option<Options>) -> String {
	let offset = byte_offset(&source, offset);
	json::parse(&source, &request(Entry::Pattern, offset, options))
}

#[napi]
pub fn parse_params_at(source: String, offset: u32, options: Option<Options>) -> String {
	let offset = byte_offset(&source, offset);
	json::parse(&source, &request(Entry::Params, offset, options))
}

#[napi]
pub fn parse_statement_at(source: String, offset: u32, options: Option<Options>) -> String {
	let offset = byte_offset(&source, offset);
	json::parse(&source, &request(Entry::Statement, offset, options))
}
