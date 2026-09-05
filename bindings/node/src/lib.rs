//! The Node addon: each function takes the source and acorn-shaped options and returns ESTree
//! JSON, which the package's JavaScript turns into objects and errors.

use napi_derive::napi;
use teasel::json::{self, Entry, Request};

#[napi(object)]
#[derive(Default)]
pub struct Options {
	/// `"script"` (the default, as in acorn) or `"module"`.
	pub source_type: Option<String>,
	pub typescript: Option<bool>,
	pub comments: Option<bool>,
	pub locations: Option<bool>,
	pub preserve_parens: Option<bool>,
	pub allow_return_outside_function: Option<bool>,
	pub allow_await_outside_function: Option<bool>,
	pub allow_super_outside_method: Option<bool>,
	pub allow_undeclared_exports: Option<bool>,
}

fn run(source: &str, entry: Entry, offset: f64, options: Option<Options>) -> String {
	let offset = match json::byte_offset(source, offset) {
		Ok(offset) => offset,
		Err(message) => return json::error_json(&message, 0),
	};
	let options = options.unwrap_or_default();
	let mut request = Request::new(entry, offset);
	let flags = [
		("typescript", options.typescript),
		("comments", options.comments),
		("locations", options.locations),
		("preserveParens", options.preserve_parens),
		("allowReturnOutsideFunction", options.allow_return_outside_function),
		("allowAwaitOutsideFunction", options.allow_await_outside_function),
		("allowSuperOutsideMethod", options.allow_super_outside_method),
		("allowUndeclaredExports", options.allow_undeclared_exports),
	];
	for (flag, on) in flags {
		if on == Some(true) {
			request.set(flag);
		}
	}
	if options.source_type.as_deref() != Some("module") {
		request.set("script");
	}
	json::parse(source, &request)
}

#[napi(catch_unwind)]
pub fn parse(source: String, options: Option<Options>) -> String {
	run(&source, Entry::Program, 0.0, options)
}

#[napi(catch_unwind)]
pub fn parse_expression_at(source: String, offset: f64, options: Option<Options>) -> String {
	run(&source, Entry::Expression, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_pattern_at(source: String, offset: f64, options: Option<Options>) -> String {
	run(&source, Entry::Pattern, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_params_at(source: String, offset: f64, options: Option<Options>) -> String {
	run(&source, Entry::Params, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_statement_at(source: String, offset: f64, options: Option<Options>) -> String {
	run(&source, Entry::Statement, offset, options)
}
