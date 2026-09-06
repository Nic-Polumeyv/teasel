//! The WebAssembly module: the same five entry points, with the request's switches as a
//! comma-separated list of the names in `teasel::json::FLAGS`, each returning ESTree JSON.

use teasel::json::{Entry, Prepared, Request};
use wasm_bindgen::prelude::wasm_bindgen;

fn request(flags: &str) -> Request {
	let mut request = Request::new(Entry::Program, 0);
	for flag in flags.split(',') {
		request.set(flag);
	}
	request
}

fn run(source: String, entry: Entry, offset: f64, flags: &str) -> String {
	Prepared::new(source, request(flags)).parse(entry, offset, false)
}

/// A source held with its switches, so many parses out of it pay for its tables once.
#[wasm_bindgen]
pub struct Source {
	prepared: Prepared,
}

#[wasm_bindgen]
impl Source {
	#[wasm_bindgen(constructor)]
	pub fn new(source: String, flags: &str) -> Source {
		Source {
			prepared: Prepared::new(source, request(flags)),
		}
	}

	/// The whole source, or the program spanning `start..end` of it.
	pub fn parse(&self, start: Option<f64>, end: Option<f64>) -> String {
		match (start, end) {
			(None, None) => self.prepared.parse(Entry::Program, 0.0, false),
			(start, end) => self.prepared.parse_range(start.unwrap_or(0.0), end),
		}
	}

	/// `until` is `"as"` when the host's `as` follows the expression.
	#[wasm_bindgen(js_name = parseExpressionAt)]
	pub fn parse_expression_at(&self, offset: f64, until: Option<String>) -> String {
		self.prepared
			.parse(Entry::Expression, offset, until.as_deref() == Some("as"))
	}

	#[wasm_bindgen(js_name = parsePatternAt)]
	pub fn parse_pattern_at(&self, offset: f64) -> String {
		self.prepared.parse(Entry::Pattern, offset, false)
	}

	#[wasm_bindgen(js_name = parseParamsAt)]
	pub fn parse_params_at(&self, offset: f64) -> String {
		self.prepared.parse(Entry::Params, offset, false)
	}

	#[wasm_bindgen(js_name = parseStatementAt)]
	pub fn parse_statement_at(&self, offset: f64) -> String {
		self.prepared.parse(Entry::Statement, offset, false)
	}
}

#[wasm_bindgen]
pub fn parse(source: String, flags: &str) -> String {
	run(source, Entry::Program, 0.0, flags)
}

#[wasm_bindgen(js_name = parseExpressionAt)]
pub fn parse_expression_at(source: String, offset: f64, flags: &str) -> String {
	run(source, Entry::Expression, offset, flags)
}

#[wasm_bindgen(js_name = parsePatternAt)]
pub fn parse_pattern_at(source: String, offset: f64, flags: &str) -> String {
	run(source, Entry::Pattern, offset, flags)
}

#[wasm_bindgen(js_name = parseParamsAt)]
pub fn parse_params_at(source: String, offset: f64, flags: &str) -> String {
	run(source, Entry::Params, offset, flags)
}

#[wasm_bindgen(js_name = parseStatementAt)]
pub fn parse_statement_at(source: String, offset: f64, flags: &str) -> String {
	run(source, Entry::Statement, offset, flags)
}
