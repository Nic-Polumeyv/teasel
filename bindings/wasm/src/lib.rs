use std::cell::RefCell;
use teasel::Entry;
use teasel::json::{Prepared, Request};

thread_local! {
	static TEXT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
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

// a panic must not trap the instance: it becomes an error, the next call still answers
fn guard(on_panic: u32, f: impl FnOnce() -> u32) -> u32 {
	match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
		Ok(answer) => answer,
		Err(panic) => {
			let message = panic
				.downcast_ref::<&str>()
				.map(|s| s.to_string())
				.or_else(|| panic.downcast_ref::<String>().cloned())
				.unwrap_or_else(|| "panic".into());
			text(teasel::json::error_json(&message, 0));
			on_panic
		}
	}
}

/// # Safety
/// `ptr`, `names` and `host` are each `capacity` bytes from `alloc`, `len` of them written: the
/// source, the option names and the host grammar, empty for none; all are taken over here. The
/// handle is 0 when the grammar cannot be read, the error as JSON at `text_ptr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn source_new(
	ptr: *mut u8,
	len: u32,
	capacity: u32,
	names: *mut u8,
	names_len: u32,
	names_capacity: u32,
	host: *mut u8,
	host_len: u32,
	host_capacity: u32,
) -> u32 {
	let source = unsafe { Vec::from_raw_parts(ptr, len as usize, capacity as usize) };
	let names = unsafe { Vec::from_raw_parts(names, names_len as usize, names_capacity as usize) };
	let host = unsafe { Vec::from_raw_parts(host, host_len as usize, host_capacity as usize) };
	guard(0, || {
		let mut prepared = Prepared::from_bytes(source, Request::from_names(&String::from_utf8_lossy(&names)));
		if !host.is_empty() {
			prepared = match prepared.host(&String::from_utf8_lossy(&host)) {
				Ok(prepared) => prepared,
				Err(message) => {
					text(teasel::json::error_json(&message, 0));
					return 0;
				}
			};
		}
		Box::into_raw(Box::new(prepared)) as u32
	})
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
) -> u32 {
	let stop = unsafe { Vec::from_raw_parts(ptr, len as usize, capacity as usize) };
	let end = (has_end == 1).then_some(end);
	guard(1, || {
		answer(source(handle).binary(Entry::from_index(entry), offset, end, &String::from_utf8_lossy(&stop)))
	})
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

#[unsafe(no_mangle)]
pub extern "C" fn constants() {
	text(teasel::json::constants_json());
}

#[unsafe(no_mangle)]
pub extern "C" fn shapes() {
	text(teasel::json::shapes_json());
}
