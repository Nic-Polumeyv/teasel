use std::cell::{Cell, RefCell};
use std::rc::Rc;
use teasel::host::Grammar;
use teasel::json::{Prepared, error_json};
use teasel::{Entry, Options};

thread_local! {
	static TEXT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
	/// The count of the tree's buffers, then each one's address, room in bytes and element size; all zero before any parse.
	static TREE: Cell<[u32; 148]> = const { Cell::new([0; 148]) };
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
	if len == 0 {
		return std::ptr::NonNull::dangling().as_ptr();
	}
	let layout = std::alloc::Layout::array::<u8>(len as usize).unwrap();
	let ptr = unsafe { std::alloc::alloc(layout) };
	if ptr.is_null() {
		std::alloc::handle_alloc_error(layout);
	}
	ptr
}

/// # Safety
/// `ptr` is `capacity` bytes from `alloc`, `len` of them the source; they are taken over here.
/// `flags` is the option word. The handle is 0 when the source is not UTF-8, the error as JSON at
/// `text_ptr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn source_new(ptr: *mut u8, len: u32, capacity: u32, flags: u32) -> u32 {
	let source = unsafe { Vec::from_raw_parts(ptr, len as usize, capacity as usize) };
	match String::from_utf8(source) {
		Ok(source) => Box::into_raw(Box::new(Prepared::new(source, Options(flags)))) as u32,
		Err(_) => {
			text(error_json("the source is not UTF-8", 0));
			0
		}
	}
}

/// # Safety
/// `ptr` is `capacity` bytes from `alloc`, `len` of them a host language's grammar as JSON;
/// they are taken over here. The handle is 0 when the grammar cannot be read, the error as JSON
/// at `text_ptr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plan_new(ptr: *mut u8, len: u32, capacity: u32) -> u32 {
	let grammar = unsafe { Vec::from_raw_parts(ptr, len as usize, capacity as usize) };
	let grammar = std::str::from_utf8(&grammar).map_err(|_| "the grammar is not UTF-8".to_string());
	match grammar.and_then(teasel::json::grammar) {
		Ok(grammar) => Box::into_raw(Box::new(grammar)) as u32,
		Err(message) => {
			text(error_json(&message, 0));
			0
		}
	}
}

#[unsafe(no_mangle)]
pub extern "C" fn plan_children(handle: u32) {
	text(teasel::json::children(unsafe { &*(handle as *const Rc<Grammar>) }));
}
#[unsafe(no_mangle)]
pub extern "C" fn plan_free(handle: u32) {
	drop(unsafe { Box::from_raw(handle as *mut Rc<Grammar>) });
}

#[unsafe(no_mangle)]
pub extern "C" fn source_free(handle: u32) {
	drop(unsafe { Box::from_raw(handle as *mut Prepared<'static>) });
}

fn source(handle: u32) -> &'static Prepared<'static> {
	unsafe { &*(handle as *const Prepared<'static>) }
}

// 0: words at `words_ptr`; 1: an error as JSON at `text_ptr`
fn answer(result: Result<(), String>) -> u32 {
	match result {
		Ok(()) => 0,
		Err(error) => {
			text(error);
			1
		}
	}
}

/// # Safety
/// `ptr` is `capacity` bytes from `alloc`, `len` of them the stop tokens; they are taken over here.
/// `plan` is a handle from `plan_new`, or 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn source_parse(
	handle: u32,
	entry: u32,
	offset: f64,
	end: f64,
	has_end: u32,
	ptr: *mut u8,
	len: u32,
	capacity: u32,
	plan: u32,
) -> u32 {
	let stop = unsafe { Vec::from_raw_parts(ptr, len as usize, capacity as usize) };
	let end = (has_end == 1).then_some(end);
	let grammar = (plan != 0).then(|| unsafe { &**(plan as *const Rc<Grammar>) });
	let (Some(entry), Ok(stop)) = (Entry::from_index(entry), std::str::from_utf8(&stop)) else {
		return answer(Err(error_json("an entry and UTF-8 stop tokens expected", 0)));
	};
	answer(source(handle).in_place(entry, offset, end, stop, grammar))
}

#[unsafe(no_mangle)]
pub extern "C" fn words_ptr() -> *const u32 {
	teasel::json::words(|w| w.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn words_len() -> u32 {
	teasel::json::words(|w| w.len() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn text_ptr() -> *const u8 {
	TEXT.with(|t| t.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn text_len() -> u32 {
	TEXT.with(|t| t.borrow().len() as u32)
}

fn text(json: String) {
	TEXT.with(|t| {
		let mut t = t.borrow_mut();
		t.clear();
		t.extend_from_slice(json.as_bytes());
	});
}

// the count of views, then each view's address, its room in bytes and its elements' size; zero before any parse
#[unsafe(no_mangle)]
pub extern "C" fn tree() -> *const u32 {
	let mut out = [0u32; 148];
	let mut count = 0;
	let found = teasel::json::tree(&mut |_, buffer| {
		if let Some(buffer) = buffer {
			out[1 + 3 * count..][..3].copy_from_slice(&[
				buffer.as_ptr() as u32,
				buffer.capacity_bytes() as u32,
				buffer.element().size() as u32,
			]);
		}
		count += 1;
	});
	if found.is_some() {
		out[0] = count as u32;
	}
	TREE.set(out);
	TREE.with(|t| t.as_ptr() as *const u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn layout() {
	text(teasel::json::layout_json());
}
