//! The WebAssembly module: the same five entry points, with the request's switches as a
//! comma-separated list of the names in `teasel::json::FLAGS`, each returning ESTree JSON.

use teasel::Until;
use teasel::json::{Entry, Prepared, Request};
use wasm_bindgen::prelude::wasm_bindgen;

fn request(flags: &str) -> Request {
	let mut request = Request::new(Entry::Program, 0);
	for flag in flags.split(',') {
		request.set(flag);
	}
	request
}

fn until(word: Option<String>) -> Option<Until> {
	match word.as_deref() {
		Some("as") => Some(Until::As),
		Some("in") => Some(Until::In),
		_ => None,
	}
}

fn run(source: &str, entry: Entry, offset: f64, flags: &str) -> String {
	Prepared::new(source.to_owned(), request(flags)).parse(entry, offset, None)
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

	pub fn parse(&self) -> String {
		self.prepared.parse(Entry::Program, 0.0, None)
	}

	/// `until` is `"as"` or `"in"`: the word operator this expression stops before.
	#[wasm_bindgen(js_name = parseExpressionAt)]
	pub fn parse_expression_at(&self, offset: f64, until_word: Option<String>) -> String {
		self.prepared.parse(Entry::Expression, offset, until(until_word))
	}

	#[wasm_bindgen(js_name = parsePatternAt)]
	pub fn parse_pattern_at(&self, offset: f64) -> String {
		self.prepared.parse(Entry::Pattern, offset, None)
	}

	#[wasm_bindgen(js_name = parseParamsAt)]
	pub fn parse_params_at(&self, offset: f64) -> String {
		self.prepared.parse(Entry::Params, offset, None)
	}

	#[wasm_bindgen(js_name = parseStatementAt)]
	pub fn parse_statement_at(&self, offset: f64) -> String {
		self.prepared.parse(Entry::Statement, offset, None)
	}
}

#[wasm_bindgen]
pub fn parse(source: &str, flags: &str) -> String {
	run(source, Entry::Program, 0.0, flags)
}

#[wasm_bindgen(js_name = parseExpressionAt)]
pub fn parse_expression_at(source: &str, offset: f64, flags: &str) -> String {
	run(source, Entry::Expression, offset, flags)
}

#[wasm_bindgen(js_name = parsePatternAt)]
pub fn parse_pattern_at(source: &str, offset: f64, flags: &str) -> String {
	run(source, Entry::Pattern, offset, flags)
}

#[wasm_bindgen(js_name = parseParamsAt)]
pub fn parse_params_at(source: &str, offset: f64, flags: &str) -> String {
	run(source, Entry::Params, offset, flags)
}

#[wasm_bindgen(js_name = parseStatementAt)]
pub fn parse_statement_at(source: &str, offset: f64, flags: &str) -> String {
	run(source, Entry::Statement, offset, flags)
}
