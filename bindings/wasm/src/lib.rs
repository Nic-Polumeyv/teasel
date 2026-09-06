//! The WebAssembly module: the same entry points as the Node addon over a plain ABI, no glue.
//! The host writes the source into the module's memory once, and reads each answer, the packed
//! token stream of `teasel::estree::Binary`, straight out of it: the words stay where the parser
//! left them, in a buffer reused from one parse to the next, until the next call.

use std::cell::RefCell;
use teasel::json::{Entry, Prepared, Request};

thread_local! {
	/// The last answer's words.
	static WORDS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
	/// The last answer's text: an error as JSON, or the constant names.
	static TEXT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Room for `len` bytes the host fills in; `source_new` takes them back.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
	let mut bytes = Vec::<u8>::with_capacity(len as usize);
	let ptr = bytes.as_mut_ptr();
	std::mem::forget(bytes);
	ptr
}

/// A source held with its switches, `bits` being the request's with bit `i` for
/// `json::FLAGS[i]`.
///
/// # Safety
/// The bytes are `capacity` from `alloc`, filled up to `len`, and are taken over here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn source_new(ptr: *mut u8, len: u32, capacity: u32, bits: u32) -> u32 {
	let source = unsafe { Vec::from_raw_parts(ptr, len as usize, capacity as usize) };
	let mut request = Request::new(Entry::Program, 0);
	request.set_bits(bits);
	let prepared = Prepared::new(String::from_utf8_lossy(&source).into_owned(), request);
	Box::into_raw(Box::new(prepared)) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn source_free(handle: u32) {
	drop(unsafe { Box::from_raw(handle as *mut Prepared) });
}

fn source(handle: u32) -> &'static Prepared {
	unsafe { &*(handle as *const Prepared) }
}

/// Keeps an answer: 0 for words the host reads with `words_ptr` and `words_len`, 1 for an error
/// the host reads with `text_ptr` and `text_len`.
fn answer(result: Result<Vec<u32>, String>) -> u32 {
	match result {
		Ok(words) => {
			WORDS.with(|w| {
				let mut w = w.borrow_mut();
				w.clear();
				w.extend_from_slice(&words);
			});
			0
		}
		Err(error) => {
			TEXT.with(|t| {
				let mut t = t.borrow_mut();
				t.clear();
				t.extend_from_slice(error.as_bytes());
			});
			1
		}
	}
}

/// Parses `entry` (`json::Entry` by index) from a UTF-16 `offset`; `until` is 1 when the host's
/// `as` follows the expression.
#[unsafe(no_mangle)]
pub extern "C" fn source_parse(handle: u32, entry: u32, offset: f64, until: u32) -> u32 {
	answer(source(handle).binary(Entry::from_index(entry), offset, until == 1))
}

/// Parses the program spanning `start..end` of the source; a negative `end` means its end.
#[unsafe(no_mangle)]
pub extern "C" fn source_parse_range(handle: u32, start: f64, end: f64) -> u32 {
	answer(source(handle).binary_range(start, (end >= 0.0).then_some(end)))
}

#[unsafe(no_mangle)]
pub extern "C" fn words_ptr() -> *const u32 {
	WORDS.with(|w| w.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn words_len() -> u32 {
	WORDS.with(|w| w.borrow().len() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn text_ptr() -> *const u8 {
	TEXT.with(|t| t.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn text_len() -> u32 {
	TEXT.with(|t| t.borrow().len() as u32)
}

/// The constant strings numbered so far, as a JSON list in the text, which a token stream refers
/// to by id.
#[unsafe(no_mangle)]
pub extern "C" fn constants() {
	let names = teasel::estree::constants();
	let mut json = String::from("[");
	for (i, name) in names.iter().enumerate() {
		if i > 0 {
			json.push(',');
		}
		teasel::estree::write_json_string(&mut json, name);
	}
	json.push(']');
	TEXT.with(|t| {
		let mut t = t.borrow_mut();
		t.clear();
		t.extend_from_slice(json.as_bytes());
	});
}
