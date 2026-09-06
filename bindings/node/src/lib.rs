//! The Node addon: a source, the request's switches as bits and what to parse, answered as the
//! packed token stream the package's `decode.js` turns into ESTree objects, or as the error in
//! JSON.

// The answer goes into a buffer JavaScript owns, which only the compat API creates; the
// replacement it points at makes external buffers, slower to hand over and to collect.
#![allow(deprecated)]

use napi::bindgen_prelude::Either;
use napi::{Env, JsArrayBuffer, JsArrayBufferValue};
use napi_derive::napi;
use teasel::json::{Entry, Prepared, Request};

type Answer = napi::Result<Either<JsArrayBuffer, String>>;

/// `bits` are the request's switches, bit `i` being `json::FLAGS[i]`.
fn prepared(source: String, bits: u32) -> Prepared {
	let mut request = Request::new(Entry::Program, 0);
	request.set_bits(bits);
	Prepared::new(source, request)
}

/// The packed token stream as a buffer JavaScript owns, or the error answer as JSON.
fn answer(env: &Env, result: Result<Vec<u32>, String>) -> Answer {
	match result {
		Ok(words) => {
			let mut buffer = env.create_arraybuffer(words.len() * 4)?;
			fill(&mut buffer, &words);
			Ok(Either::A(buffer.into_raw()))
		}
		Err(json) => Ok(Either::B(json)),
	}
}

/// One parse of `entry` (`json::Entry` by index) at a UTF-16 `offset`; `until` says the host's
/// `as` follows the expression.
#[napi(catch_unwind)]
pub fn parse_at(env: Env, source: String, bits: u32, entry: u32, offset: f64, until: bool) -> Answer {
	answer(&env, prepared(source, bits).binary(Entry::from_index(entry), offset, until))
}

/// The same answer as JSON text, for checking the decoder against.
#[napi(catch_unwind)]
pub fn parse_at_json(source: String, bits: u32, entry: u32, offset: f64, until: bool) -> String {
	prepared(source, bits).parse(Entry::from_index(entry), offset, until)
}

/// The constant strings numbered so far, which a token stream refers to by id.
#[napi]
pub fn constants() -> Vec<&'static str> {
	teasel::estree::constants()
}

/// A source held with its switches, so many parses out of it pay for its tables once.
#[napi]
pub struct Source {
	prepared: Prepared,
}

#[napi]
impl Source {
	#[napi(constructor)]
	pub fn new(source: String, bits: u32) -> Self {
		Self {
			prepared: prepared(source, bits),
		}
	}

	#[napi(catch_unwind)]
	pub fn parse_at(&self, env: Env, entry: u32, offset: f64, until: bool) -> Answer {
		answer(&env, self.prepared.binary(Entry::from_index(entry), offset, until))
	}

	/// The program spanning `start..end` of the source, `end` defaulting to its end.
	#[napi(catch_unwind)]
	pub fn parse_range(&self, env: Env, start: f64, end: Option<f64>) -> Answer {
		answer(&env, self.prepared.binary_range(start, end))
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
