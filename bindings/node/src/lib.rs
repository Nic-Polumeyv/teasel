//! The Node addon: a source as UTF-8 bytes, the request's switches as bits and what to parse,
//! answered as the packed token stream the package's `decode.js` turns into ESTree objects, or
//! as the error in JSON. The words stay in one buffer the addon owns, read in place by the
//! package before its next call, as the WebAssembly module's are.

use std::cell::{Cell, RefCell};

use napi::bindgen_prelude::{Either, FromNapiValue, ToNapiValue, Uint8Array, Uint32Array};
use napi::{Env, sys};
use napi_derive::napi;
use teasel::json::{Entry, Prepared, Request};

thread_local! {
	/// The last answer's words.
	static WORDS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
	/// Every buffer the words outgrew, kept so a view a caller held on to reads old words and
	/// never freed memory.
	static OUTGROWN: RefCell<Vec<Vec<u32>>> = const { RefCell::new(Vec::new()) };
	/// The JavaScript view over the whole of `WORDS`, made once per buffer: a view made per call
	/// would have V8 count its bytes as new memory every time and collect for it.
	static VIEW: Cell<sys::napi_ref> = const { Cell::new(std::ptr::null_mut()) };
}

type Answer = napi::Result<Either<Uint32Array, String>>;

/// `bits` are the request's switches, bit `i` being `json::FLAGS[i]`; the source is UTF-8 as
/// the package encodes it.
fn prepared(source: &[u8], bits: u32) -> Prepared {
	let mut request = Request::new(Entry::Program, 0);
	request.set_bits(bits);
	Prepared::new(String::from_utf8_lossy(source).into_owned(), request)
}

fn status(status: sys::napi_status, what: &str) -> napi::Result<()> {
	if status == sys::Status::napi_ok {
		Ok(())
	} else {
		Err(napi::Error::new(napi::Status::from(status), what))
	}
}

/// The words as the view of the addon's buffer, or the error answer as JSON.
fn answer(env: &Env, result: Result<Vec<u32>, String>) -> Answer {
	let words = match result {
		Ok(words) => words,
		Err(json) => return Ok(Either::B(json)),
	};
	WORDS.with(|w| {
		let mut w = w.borrow_mut();
		let grown = w.capacity() < words.len();
		if grown {
			let mut room = vec![0; words.len().max(w.capacity() * 2)];
			room.clear();
			OUTGROWN.with(|o| o.borrow_mut().push(std::mem::replace(&mut *w, room)));
		}
		w.clear();
		w.extend_from_slice(&words);
		let mut value = std::ptr::null_mut();
		let reference = VIEW.get();
		if reference.is_null() || grown {
			if !reference.is_null() {
				status(unsafe { sys::napi_delete_reference(env.raw(), reference) }, "delete the view")?;
			}
			// the buffer outlives every view: it is never freed, only outgrown
			let view = unsafe { Uint32Array::with_external_data(w.as_mut_ptr(), w.capacity(), |_, _| {}) };
			value = unsafe { ToNapiValue::to_napi_value(env.raw(), view)? };
			let mut reference = std::ptr::null_mut();
			status(unsafe { sys::napi_create_reference(env.raw(), value, 1, &mut reference) }, "keep the view")?;
			VIEW.set(reference);
		} else {
			status(unsafe { sys::napi_get_reference_value(env.raw(), reference, &mut value) }, "get the view")?;
		}
		Ok(Either::A(unsafe { Uint32Array::from_napi_value(env.raw(), value)? }))
	})
}

/// One parse of `entry` (`json::Entry` by index) at a UTF-16 `offset`; `until` says the host's
/// `as` follows the expression.
#[napi(catch_unwind)]
pub fn parse_at(env: Env, source: Uint8Array, bits: u32, entry: u32, offset: f64, until: bool) -> Answer {
	answer(&env, prepared(&source, bits).binary(Entry::from_index(entry), offset, until))
}

/// The same answer as JSON text, for checking the decoder against.
#[napi(catch_unwind)]
pub fn parse_at_json(source: Uint8Array, bits: u32, entry: u32, offset: f64, until: bool) -> String {
	prepared(&source, bits).parse(Entry::from_index(entry), offset, until)
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
	pub fn new(source: Uint8Array, bits: u32) -> Self {
		Self {
			prepared: prepared(&source, bits),
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
