//! The WebAssembly module: the same five entry points, with options as a JSON string, each
//! returning ESTree JSON.

use teasel::json::{self, Entry, Request};
use wasm_bindgen::prelude::wasm_bindgen;

/// Reads acorn-shaped options out of their JSON without a JSON library: every option is a flat
/// key with a boolean or a short string.
fn request(entry: Entry, offset: u32, options: &str) -> Request {
	let flag = |key: &str| options.contains(&format!("\"{key}\":true"));
	Request {
		entry,
		offset,
		typescript: flag("typescript"),
		comments: flag("comments"),
		options: teasel::Options {
			module: !options.contains("\"sourceType\":\"script\""),
			preserve_parens: flag("preserveParens"),
			allow_return_outside_function: flag("allowReturnOutsideFunction"),
			allow_await_outside_function: flag("allowAwaitOutsideFunction"),
			allow_super_outside_method: flag("allowSuperOutsideMethod"),
			allow_undeclared_exports: flag("allowUndeclaredExports"),
		},
	}
}

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

#[wasm_bindgen]
pub fn parse(source: &str, options: &str) -> String {
	json::parse(source, &request(Entry::Program, 0, options))
}

#[wasm_bindgen(js_name = parseExpressionAt)]
pub fn parse_expression_at(source: &str, offset: u32, options: &str) -> String {
	json::parse(
		source,
		&request(Entry::Expression, byte_offset(source, offset), options),
	)
}

#[wasm_bindgen(js_name = parsePatternAt)]
pub fn parse_pattern_at(source: &str, offset: u32, options: &str) -> String {
	json::parse(source, &request(Entry::Pattern, byte_offset(source, offset), options))
}

#[wasm_bindgen(js_name = parseParamsAt)]
pub fn parse_params_at(source: &str, offset: u32, options: &str) -> String {
	json::parse(source, &request(Entry::Params, byte_offset(source, offset), options))
}

#[wasm_bindgen(js_name = parseStatementAt)]
pub fn parse_statement_at(source: &str, offset: u32, options: &str) -> String {
	json::parse(source, &request(Entry::Statement, byte_offset(source, offset), options))
}
