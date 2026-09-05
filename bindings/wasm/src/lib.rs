//! The WebAssembly module: the same five entry points, with the request's switches as a
//! comma-separated list of the names in `teasel::json::FLAGS`, each returning ESTree JSON.

use teasel::json::{self, Entry, Request};
use wasm_bindgen::prelude::wasm_bindgen;

fn run(source: &str, entry: Entry, offset: f64, flags: &str) -> String {
	let offset = match json::byte_offset(source, offset) {
		Ok(offset) => offset,
		Err(message) => return json::error_json(&message, 0),
	};
	let mut request = Request::new(entry, offset);
	for flag in flags.split(',') {
		request.set(flag);
	}
	json::parse(source, &request)
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
