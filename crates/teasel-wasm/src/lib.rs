use std::cell::{Cell, RefCell};
use std::rc::Rc;
use teasel::Options;
use teasel::host::Grammar;

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
/// `ptr` is `capacity` bytes from `alloc`, `len` of them a host language's grammar as JSON;
/// they are taken over here. The handle is 0 when the grammar cannot be read, what is wrong at
/// `text_ptr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plan_new(ptr: *mut u8, len: u32, capacity: u32) -> u32 {
	let grammar = unsafe { Vec::from_raw_parts(ptr, len as usize, capacity as usize) };
	let grammar = std::str::from_utf8(&grammar).map_err(|_| "the grammar is not UTF-8".to_string());
	match grammar.and_then(teasel::json::grammar) {
		Ok(grammar) => Box::into_raw(Box::new(grammar)) as u32,
		Err(message) => {
			text(message);
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

/// # Safety
/// `ptr` is `capacity` bytes from `alloc`, `len` of them the source, and `stop_ptr` is
/// `stop_capacity` bytes from `alloc`, `stop_len` of them the stop tokens; both are taken over
/// here. `flags` is the option word; `plan` is a handle from `plan_new`, or 0. 0: words at
/// `words_ptr`; 1: what is wrong with the call at `text_ptr`, for the front end to throw.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn parse(
	ptr: *mut u8,
	len: u32,
	capacity: u32,
	flags: u32,
	entry: f64,
	offset: f64,
	stop_ptr: *mut u8,
	stop_len: u32,
	stop_capacity: u32,
	plan: u32,
) -> u32 {
	let source = unsafe { Vec::from_raw_parts(ptr, len as usize, capacity as usize) };
	let stop = unsafe { Vec::from_raw_parts(stop_ptr, stop_len as usize, stop_capacity as usize) };
	let grammar = (plan != 0).then(|| unsafe { &**(plan as *const Rc<Grammar>) });
	let source = std::str::from_utf8(&source).map_err(|_| "the source is not UTF-8".to_string());
	let stop = std::str::from_utf8(&stop).map_err(|_| "the stop tokens are not UTF-8".to_string());
	let parsed = source.and_then(|source| {
		let entry = teasel::json::entry(entry)?;
		teasel::json::in_place(source, Options(flags), entry, offset, stop?, grammar)
	});
	match parsed {
		Ok(()) => 0,
		Err(message) => {
			text(message);
			1
		}
	}
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
