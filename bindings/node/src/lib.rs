//! The Node addon: each function takes the source and acorn-shaped options and answers with a
//! packed token stream the package's `decode.js` turns into ESTree objects, or with the error
//! as JSON.

// The answer goes into a buffer JavaScript owns, which only the compat API creates; the
// replacement it points at makes external buffers, slower to hand over and to collect.
#![allow(deprecated)]

use napi::bindgen_prelude::Either;
use napi::{Env, JsArrayBuffer, JsArrayBufferValue};
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

/// The packed token stream as a buffer JavaScript owns, or the error answer as JSON.
fn answer(env: &Env, result: Result<Vec<u32>, String>) -> napi::Result<Either<JsArrayBuffer, String>> {
	match result {
		Ok(words) => {
			let mut buffer = env.create_arraybuffer(words.len() * 4)?;
			fill(&mut buffer, &words);
			Ok(Either::A(buffer.into_raw()))
		}
		Err(json) => Ok(Either::B(json)),
	}
}

/// The constant strings numbered so far, which a token stream refers to by id.
#[napi]
pub fn constants() -> Vec<&'static str> {
	teasel::estree::constants()
}

fn run(
	env: &Env,
	source: String,
	entry: Entry,
	offset: f64,
	options: Option<Options>,
) -> napi::Result<Either<JsArrayBuffer, String>> {
	answer(
		env,
		Prepared::new(source, request(options)).binary(entry, offset, false),
	)
}

fn run_json(source: String, entry: Entry, offset: f64, options: Option<Options>) -> String {
	Prepared::new(source, request(options)).parse(entry, offset, false)
}

#[napi(catch_unwind)]
pub fn parse(env: Env, source: String, options: Option<Options>) -> napi::Result<Either<JsArrayBuffer, String>> {
	run(&env, source, Entry::Program, 0.0, options)
}

#[napi(catch_unwind)]
pub fn parse_expression_at(
	env: Env,
	source: String,
	offset: f64,
	options: Option<Options>,
) -> napi::Result<Either<JsArrayBuffer, String>> {
	run(&env, source, Entry::Expression, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_pattern_at(
	env: Env,
	source: String,
	offset: f64,
	options: Option<Options>,
) -> napi::Result<Either<JsArrayBuffer, String>> {
	run(&env, source, Entry::Pattern, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_params_at(
	env: Env,
	source: String,
	offset: f64,
	options: Option<Options>,
) -> napi::Result<Either<JsArrayBuffer, String>> {
	run(&env, source, Entry::Params, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_statement_at(
	env: Env,
	source: String,
	offset: f64,
	options: Option<Options>,
) -> napi::Result<Either<JsArrayBuffer, String>> {
	run(&env, source, Entry::Statement, offset, options)
}

/// The same answers as JSON text, for checking the decoder against.
#[napi(catch_unwind)]
pub fn parse_json(source: String, options: Option<Options>) -> String {
	run_json(source, Entry::Program, 0.0, options)
}

#[napi(catch_unwind)]
pub fn parse_expression_at_json(source: String, offset: f64, options: Option<Options>) -> String {
	run_json(source, Entry::Expression, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_pattern_at_json(source: String, offset: f64, options: Option<Options>) -> String {
	run_json(source, Entry::Pattern, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_params_at_json(source: String, offset: f64, options: Option<Options>) -> String {
	run_json(source, Entry::Params, offset, options)
}

#[napi(catch_unwind)]
pub fn parse_statement_at_json(source: String, offset: f64, options: Option<Options>) -> String {
	run_json(source, Entry::Statement, offset, options)
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
	pub fn parse(&self, env: Env, start: Option<f64>, end: Option<f64>) -> napi::Result<Either<JsArrayBuffer, String>> {
		answer(
			&env,
			match (start, end) {
				(None, None) => self.prepared.binary(Entry::Program, 0.0, false),
				(start, end) => self.prepared.binary_range(start.unwrap_or(0.0), end),
			},
		)
	}

	/// `until` is `"as"` when the host's `as` follows the expression.
	#[napi(catch_unwind)]
	pub fn parse_expression_at(
		&self,
		env: Env,
		offset: f64,
		until: Option<String>,
	) -> napi::Result<Either<JsArrayBuffer, String>> {
		answer(
			&env,
			self.prepared
				.binary(Entry::Expression, offset, until.as_deref() == Some("as")),
		)
	}

	#[napi(catch_unwind)]
	pub fn parse_pattern_at(&self, env: Env, offset: f64) -> napi::Result<Either<JsArrayBuffer, String>> {
		answer(&env, self.prepared.binary(Entry::Pattern, offset, false))
	}

	#[napi(catch_unwind)]
	pub fn parse_params_at(&self, env: Env, offset: f64) -> napi::Result<Either<JsArrayBuffer, String>> {
		answer(&env, self.prepared.binary(Entry::Params, offset, false))
	}

	#[napi(catch_unwind)]
	pub fn parse_statement_at(&self, env: Env, offset: f64) -> napi::Result<Either<JsArrayBuffer, String>> {
		answer(&env, self.prepared.binary(Entry::Statement, offset, false))
	}
}

/// Words in the host's order, which `decode.js` reads back through typed arrays.
fn fill(buffer: &mut JsArrayBufferValue, words: &[u32]) {
	let bytes = buffer.as_mut();
	debug_assert_eq!(bytes.len(), words.len() * 4);
	for (bytes, word) in bytes.chunks_exact_mut(4).zip(words) {
		bytes.copy_from_slice(&word.to_ne_bytes());
	}
}
