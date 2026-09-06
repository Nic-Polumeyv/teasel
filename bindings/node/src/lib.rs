//! The Node addon: each function takes the source and acorn-shaped options and returns ESTree
//! JSON, which the package's JavaScript turns into objects and errors.

use napi_derive::napi;
use teasel::json::{Entry, Prepared, Request};

#[napi(object)]
#[derive(Default)]
pub struct Options {
	/// `"script"` (the default, as in acorn) or `"module"`.
	pub source_type: Option<String>,
	pub typescript: Option<bool>,
	/// With `typescript`: erase it from the output.
	pub erase: Option<bool>,
	pub comments: Option<bool>,
	pub locations: Option<bool>,
	pub preserve_parens: Option<bool>,
	pub allow_return_outside_function: Option<bool>,
	pub allow_await_outside_function: Option<bool>,
	pub allow_super_outside_method: Option<bool>,
	pub allow_undeclared_exports: Option<bool>,
	/// `"as"`: the host's `as` follows the expression, which ends at the last top-level one.
	pub until: Option<String>,
}

fn request(options: Option<Options>) -> Request {
	let options = options.unwrap_or_default();
	let mut request = Request::new(Entry::Program, 0);
	let flags = [
		("typescript", options.typescript),
		("erase", options.erase),
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
	if options.until.as_deref() == Some("as") {
		request.set("untilAs");
	}
	request
}

fn run(source: String, entry: Entry, offset: f64, options: Option<Options>) -> String {
	Prepared::new(source, request(options)).parse(entry, offset, false)
}

#[napi(catch_unwind)]
pub fn parse(source: String, options: Option<Options>) -> String {
	run(source, Entry::Program, 0.0, options)
}

#[napi(catch_unwind)]
pub fn parse_expression_at(source: String, offset: f64, options: Option<Options>) -> String {
	run(source, Entry::Expression, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_pattern_at(source: String, offset: f64, options: Option<Options>) -> String {
	run(source, Entry::Pattern, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_params_at(source: String, offset: f64, options: Option<Options>) -> String {
	run(source, Entry::Params, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_statement_at(source: String, offset: f64, options: Option<Options>) -> String {
	run(source, Entry::Statement, offset, options)
}

/// A source held with its options, so many parses out of it pay for its tables once.
#[napi]
pub struct Source {
	prepared: Prepared,
}

#[napi]
impl Source {
	#[napi(constructor)]
	pub fn new(source: String, options: Option<Options>) -> Self {
		Self {
			prepared: Prepared::new(source, request(options)),
		}
	}

	/// The whole source, or the program spanning `start..end` of it.
	#[napi(catch_unwind)]
	pub fn parse(&self, start: Option<f64>, end: Option<f64>) -> String {
		match (start, end) {
			(None, None) => self.prepared.parse(Entry::Program, 0.0, false),
			(start, end) => self.prepared.parse_range(start.unwrap_or(0.0), end.unwrap_or(f64::NAN)),
		}
	}

	/// `until` is `"as"` when the host's `as` follows the expression.
	#[napi(catch_unwind)]
	pub fn parse_expression_at(&self, offset: f64, until: Option<String>) -> String {
		self.prepared
			.parse(Entry::Expression, offset, until.as_deref() == Some("as"))
	}

	#[napi(catch_unwind)]
	pub fn parse_pattern_at(&self, offset: f64) -> String {
		self.prepared.parse(Entry::Pattern, offset, false)
	}

	#[napi(catch_unwind)]
	pub fn parse_params_at(&self, offset: f64) -> String {
		self.prepared.parse(Entry::Params, offset, false)
	}

	#[napi(catch_unwind)]
	pub fn parse_statement_at(&self, offset: f64) -> String {
		self.prepared.parse(Entry::Statement, offset, false)
	}
}
