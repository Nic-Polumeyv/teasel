use std::cell::RefCell;
use teasel::json::{Entry, Prepared, Request};

thread_local! {
	static WORDS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
	static TEXT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
	if len == 0 {
		return std::ptr::NonNull::dangling().as_ptr();
	}
	unsafe { std::alloc::alloc(std::alloc::Layout::array::<u8>(len as usize).unwrap()) }
}

/// # Safety
/// `ptr` is `capacity` bytes from `alloc`, `len` of them written; they are taken over here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn source_new(ptr: *mut u8, len: u32, capacity: u32, bits: u32) -> u32 {
	let source = unsafe { Vec::from_raw_parts(ptr, len as usize, capacity as usize) };
	let mut request = Request::new(Entry::Program, 0);
	request.set_bits(bits);
	let source = String::from_utf8(source).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
	let prepared = Prepared::new(source, request);
	Box::into_raw(Box::new(prepared)) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn source_free(handle: u32) {
	drop(unsafe { Box::from_raw(handle as *mut Prepared) });
}

fn source(handle: u32) -> &'static Prepared {
	unsafe { &*(handle as *const Prepared) }
}

// 0: words at `words_ptr`; 1: an error as JSON at `text_ptr`
fn answer(result: Result<Vec<u32>, String>) -> u32 {
	match result {
		Ok(words) => {
			WORDS.with(|w| *w.borrow_mut() = words);
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

#[unsafe(no_mangle)]
pub extern "C" fn source_parse(handle: u32, entry: u32, offset: f64, until: u32) -> u32 {
	answer(source(handle).binary(Entry::from_index(entry), offset, until == 1))
}

#[unsafe(no_mangle)]
pub extern "C" fn source_parse_range(handle: u32, start: f64, end: f64, has_end: u32) -> u32 {
	answer(source(handle).binary_range(start, (has_end == 1).then_some(end)))
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

#[unsafe(no_mangle)]
pub extern "C" fn constants() {
	let json = teasel::json::constants_json();
	TEXT.with(|t| {
		let mut t = t.borrow_mut();
		t.clear();
		t.extend_from_slice(json.as_bytes());
	});
}
