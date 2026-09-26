//! The Node addon: the same five operations as the WebAssembly module, over a handle to the
//! prepared source.

mod node_api;

use std::cell::Cell;
use std::ffi::{CString, c_void};
use std::rc::Rc;

use node_api::{CallbackInfo, Env, OK, Ref, Status, Value};
use teasel::Entry;
use teasel::handed::{Allocation, Element, Raw};
use teasel::host::Grammar;
use teasel::json::{Prepared, Request};

/// What JavaScript holds: the view of the words, then the views of the JavaScript tree and of
/// the TypeScript one, each an array.
const VIEWS: usize = 3;

thread_local! {
	static VIEW: Cell<(Env, [Ref; VIEWS])> = const { Cell::new((std::ptr::null_mut(), [std::ptr::null_mut(); VIEWS])) };
}

type Result<T> = std::result::Result<T, String>;

fn check(status: Status, what: &str) -> Result<()> {
	if status == OK {
		Ok(())
	} else {
		Err(format!("{what}: Node-API status {status}"))
	}
}

fn args<const N: usize>(env: Env, info: CallbackInfo) -> Result<[Value; N]> {
	let mut argv = [std::ptr::null_mut(); N];
	let mut argc = N;
	check(
		unsafe {
			node_api::napi_get_cb_info(
				env,
				info,
				&mut argc,
				argv.as_mut_ptr(),
				std::ptr::null_mut(),
				std::ptr::null_mut(),
			)
		},
		"arguments",
	)?;
	if argc < N {
		return Err(format!("{N} arguments expected"));
	}
	Ok(argv)
}

fn bytes(env: Env, value: Value) -> Result<Vec<u8>> {
	let (mut kind, mut length, mut data) = (0, 0, std::ptr::null_mut());
	check(
		unsafe {
			node_api::napi_get_typedarray_info(
				env,
				value,
				&mut kind,
				&mut length,
				&mut data,
				std::ptr::null_mut(),
				std::ptr::null_mut(),
			)
		},
		"a Uint8Array",
	)?;
	if kind != node_api::UINT8_ARRAY {
		return Err("a Uint8Array expected".into());
	}
	if length == 0 || data.is_null() {
		return Ok(Vec::new());
	}
	Ok(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) }.to_vec())
}

fn string(env: Env, value: Value) -> Result<String> {
	let mut length = 0;
	check(
		unsafe { node_api::napi_get_value_string_utf8(env, value, std::ptr::null_mut(), 0, &mut length) },
		"a string",
	)?;
	let mut buffer = vec![0u8; length + 1];
	check(
		unsafe {
			node_api::napi_get_value_string_utf8(env, value, buffer.as_mut_ptr().cast(), length + 1, &mut length)
		},
		"a string",
	)?;
	buffer.truncate(length);
	Ok(String::from_utf8(buffer).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

fn number(env: Env, value: Value) -> Result<f64> {
	let mut result = 0.0;
	check(
		unsafe { node_api::napi_get_value_double(env, value, &mut result) },
		"a number",
	)?;
	Ok(result)
}

fn optional(env: Env, value: Value) -> Result<Option<f64>> {
	let mut kind = 0;
	check(unsafe { node_api::napi_typeof(env, value, &mut kind) }, "a value")?;
	if kind == node_api::UNDEFINED {
		Ok(None)
	} else {
		number(env, value).map(Some)
	}
}

fn text(env: Env, text: &str) -> Result<Value> {
	let mut result = std::ptr::null_mut();
	check(
		unsafe { node_api::napi_create_string_utf8(env, text.as_ptr().cast(), text.len(), &mut result) },
		"a string",
	)?;
	Ok(result)
}

fn undefined(env: Env) -> Result<Value> {
	let mut result = std::ptr::null_mut();
	check(unsafe { node_api::napi_get_undefined(env, &mut result) }, "undefined")?;
	Ok(result)
}

fn handle(env: Env, value: Value) -> Result<*mut Prepared<'static>> {
	let mut data = std::ptr::null_mut();
	check(
		unsafe { node_api::napi_get_value_external(env, value, &mut data) },
		"a source",
	)?;
	if data.is_null() {
		return Err("a source expected".into());
	}
	Ok(data.cast())
}

// the bytes V8 encoded, made valid UTF-8 where they are not; the options as one flag word
unsafe extern "C" fn create(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [source, flags] = args::<2>(env, info)?;
		let prepared = Prepared::from_bytes(bytes(env, source)?, Request::from_flags(number(env, flags)? as u32));
		let mut result = std::ptr::null_mut();
		check(
			unsafe {
				node_api::napi_create_external(
					env,
					Box::into_raw(Box::new(prepared)).cast(),
					None,
					std::ptr::null_mut(),
					&mut result,
				)
			},
			"a source",
		)?;
		Ok(result)
	})
}

// the grammar of a host language, read once; V8 lets go of it with the external
unsafe extern "C" fn plan(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [text] = args::<1>(env, info)?;
		let grammar = teasel::json::grammar(&string(env, text)?)?;
		let mut result = std::ptr::null_mut();
		check(
			unsafe {
				node_api::napi_create_external(
					env,
					Box::into_raw(Box::new(grammar)).cast(),
					Some(drop_plan),
					std::ptr::null_mut(),
					&mut result,
				)
			},
			"a plan",
		)?;
		Ok(result)
	})
}

unsafe extern "C" fn drop_plan(_: Env, data: *mut c_void, _: *mut c_void) {
	drop(unsafe { Box::from_raw(data.cast::<Rc<Grammar>>()) });
}

// the grammar a parse reads a document by, or undefined
fn grammar_of(env: Env, value: Value) -> Result<Option<&'static Grammar>> {
	let mut kind = 0;
	check(unsafe { node_api::napi_typeof(env, value, &mut kind) }, "a value")?;
	if kind == node_api::UNDEFINED {
		return Ok(None);
	}
	let mut data = std::ptr::null_mut();
	check(
		unsafe { node_api::napi_get_value_external(env, value, &mut data) },
		"a plan",
	)?;
	if data.is_null() {
		return Err("a plan expected".into());
	}
	Ok(Some(unsafe { &**data.cast::<Rc<Grammar>>() }))
}

unsafe extern "C" fn free(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [source] = args::<1>(env, info)?;
		drop(unsafe { Box::from_raw(handle(env, source)?) });
		undefined(env)
	})
}

unsafe extern "C" fn parse(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [source, entry, offset, end, stop, plan] = args::<6>(env, info)?;
		let prepared = unsafe { &*handle(env, source)? };
		let entry = Entry::from_index(number(env, entry)? as u32);
		let (offset, end, stop) = (number(env, offset)?, optional(env, end)?, string(env, stop)?);
		let grammar = grammar_of(env, plan)?;
		in_env(env, || match prepared.in_place(entry, offset, end, &stop, grammar) {
			Ok(()) => view(env),
			Err(json) => text(env, &json),
		})
	})
}

// the views of the last parse's tree, in the layout's order, `undefined` for a table the parse did not
// fill, its first element whether the tree is the TypeScript one; undefined before any. The array is kept: a call
// sets the views whose buffers moved since the last, and the answer's words say when one has
unsafe extern "C" fn tree(env: Env, _: CallbackInfo) -> Value {
	guard(env, || {
		in_env(env, || {
			let mut moved = Vec::new();
			let mut count = 0;
			let typescript = teasel::json::tree(&mut |_, buffer| {
				count += 1;
				if let Some(buffer) = buffer {
					moved.push((count, buffer_view(env, buffer)));
				}
			});
			let Some(typescript) = typescript else {
				return undefined(env);
			};
			let slot = 1 + typescript as usize;
			let (_, mut refs) = VIEW.get();
			let mut array = std::ptr::null_mut();
			if refs[slot].is_null() {
				check(
					unsafe { node_api::napi_create_array_with_length(env, 1 + count as usize, &mut array) },
					"an array",
				)?;
				check(
					unsafe { node_api::napi_set_element(env, array, 0, uint32(env, typescript as u32)?) },
					"an element",
				)?;
				check(
					unsafe { node_api::napi_create_reference(env, array, 1, &mut refs[slot]) },
					"keeping the views",
				)?;
				VIEW.set((env, refs));
			} else {
				check(
					unsafe { node_api::napi_get_reference_value(env, refs[slot], &mut array) },
					"the views",
				)?;
			}
			for (i, value) in moved {
				if let Some(value) = value? {
					check(
						unsafe { node_api::napi_set_element(env, array, i, value) },
						"an element",
					)?;
				}
			}
			Ok(array)
		})
	})
}

fn kind_of(element: Element) -> i32 {
	match element {
		Element::U8 => node_api::UINT8_ARRAY,
		Element::U32 => node_api::UINT32_ARRAY,
		Element::F64 => node_api::FLOAT64_ARRAY,
	}
}

fn uint32(env: Env, value: u32) -> Result<Value> {
	let mut result = std::ptr::null_mut();
	check(
		unsafe { node_api::napi_create_uint32(env, value, &mut result) },
		"a number",
	)?;
	Ok(result)
}

unsafe extern "C" fn layout(env: Env, _: CallbackInfo) -> Value {
	guard(env, || text(env, &teasel::json::layout_json()))
}

// a panic must not cross into the host; it and any error become a thrown Error
fn guard(env: Env, f: impl FnOnce() -> Result<Value> + std::panic::UnwindSafe) -> Value {
	let result = match std::panic::catch_unwind(f) {
		Ok(result) => result,
		Err(panic) => Err(panic
			.downcast_ref::<&str>()
			.map(|s| s.to_string())
			.or_else(|| panic.downcast_ref::<String>().cloned())
			.unwrap_or_else(|| "panic".into())),
	};
	match result {
		Ok(value) => value,
		Err(message) => {
			let message = CString::new(message.replace('\0', " ")).unwrap();
			unsafe { node_api::napi_throw_error(env, std::ptr::null(), message.as_ptr()) };
			std::ptr::null_mut()
		}
	}
}

struct Reference {
	env: Env,
	reference: Ref,
}

impl Drop for Reference {
	fn drop(&mut self) {
		unsafe { node_api::napi_delete_reference(self.env, self.reference) };
	}
}

fn allocate(layout: std::alloc::Layout) -> Allocation {
	let (env, _) = VIEW.get();
	assert!(!env.is_null(), "buffer allocation needs a live environment");
	let result = (|| {
		let (mut data, mut buffer, mut array, mut reference) = (
			std::ptr::null_mut(),
			std::ptr::null_mut(),
			std::ptr::null_mut(),
			std::ptr::null_mut(),
		);
		check(
			unsafe { node_api::napi_create_buffer(env, layout.size(), &mut data, &mut buffer) },
			"allocating a buffer",
		)?;
		let ptr = std::ptr::NonNull::new(data.cast::<u8>()).ok_or("null buffer allocation")?;
		let mut offset = 0;
		check(
			unsafe {
				node_api::napi_get_typedarray_info(
					env,
					buffer,
					std::ptr::null_mut(),
					std::ptr::null_mut(),
					std::ptr::null_mut(),
					&mut array,
					&mut offset,
				)
			},
			"the buffer's backing store",
		)?;
		assert_eq!(offset, 0, "a buffer must cover its backing store");
		check(
			unsafe { node_api::napi_create_reference(env, array, 1, &mut reference) },
			"keeping a buffer",
		)?;
		Ok(Allocation {
			ptr,
			owner: Box::new(Reference { env, reference }),
		})
	})();
	result.unwrap_or_else(|error: String| {
		eprintln!("{error}");
		std::alloc::handle_alloc_error(layout)
	})
}

fn buffer_view(env: Env, buffer: &mut dyn Raw) -> Result<Option<Value>> {
	let Some(owner) = buffer.allocation() else {
		return Ok(None);
	};
	let owner = owner.downcast_ref::<Reference>().expect("a buffer's reference");
	assert_eq!(owner.env, env, "a buffer belongs to its environment");
	let mut array = std::ptr::null_mut();
	check(
		unsafe { node_api::napi_get_reference_value(env, owner.reference, &mut array) },
		"the view's buffer",
	)?;
	let mut value = std::ptr::null_mut();
	let kind = kind_of(buffer.element());
	check(
		unsafe {
			node_api::napi_create_typedarray(
				env,
				kind,
				buffer.capacity_bytes() / node_api::element_size(kind),
				array,
				0,
				&mut value,
			)
		},
		"the view",
	)?;
	Ok(Some(value))
}

fn view_of(env: Env, slot: usize, buffer: &mut dyn Raw) -> Result<Value> {
	let (_, mut refs) = VIEW.get();
	let mut value = std::ptr::null_mut();
	if let Some(next) = buffer_view(env, buffer)? {
		if !refs[slot].is_null() {
			let old = std::mem::replace(&mut refs[slot], std::ptr::null_mut());
			VIEW.set((env, refs));
			check(unsafe { node_api::napi_delete_reference(env, old) }, "the old view")?;
		}
		value = next;
		let mut reference = std::ptr::null_mut();
		check(
			unsafe { node_api::napi_create_reference(env, value, 1, &mut reference) },
			"keeping the view",
		)?;
		refs[slot] = reference;
		VIEW.set((env, refs));
	} else {
		check(
			unsafe { node_api::napi_get_reference_value(env, refs[slot], &mut value) },
			"the view",
		)?;
	}
	Ok(value)
}

// the answer is written into the words in place; one the words outgrew stays with the view that shows it
fn view(env: Env) -> Result<Value> {
	teasel::json::words(|words| {
		let value = view_of(env, 0, words)?;
		// past 256 KB, a buffer four times too big is left to its view
		if words.capacity() > 1 << 16 && words.capacity() > 4 * words.len() {
			words.renew(2 * words.len());
		}
		Ok(value)
	})
}

fn fresh(env: Env) {
	let (view_env, refs) = VIEW.get();
	if view_env != env {
		teasel::json::reset_session();
		for reference in refs {
			if !reference.is_null() {
				unsafe { node_api::napi_delete_reference(view_env, reference) };
			}
		}
		VIEW.set((env, [std::ptr::null_mut(); VIEWS]));
	}
}

fn in_env<R>(env: Env, f: impl FnOnce() -> R) -> R {
	fresh(env);
	unsafe { teasel::handed::allocating(allocate, f) }
}

unsafe extern "C" fn cleanup(data: *mut c_void) {
	if VIEW.get().0 == data.cast() {
		fresh(std::ptr::null_mut());
	}
}

/// # Safety
/// Called by Node once per load with a live `env` and the module's `exports` object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_register_module_v1(env: Env, exports: Value) -> Value {
	#[cfg(windows)]
	unsafe {
		node_api::load()
	};
	guard(env, || {
		check(
			unsafe { node_api::napi_add_env_cleanup_hook(env, cleanup, env.cast()) },
			"environment cleanup",
		)?;
		for (name, callback) in [
			(c"create", create as unsafe extern "C" fn(Env, CallbackInfo) -> Value),
			(c"parse", parse),
			(c"plan", plan),
			(c"free", free),
			(c"tree", tree),
			(c"layout", layout),
		] {
			let mut function = std::ptr::null_mut();
			check(
				unsafe {
					node_api::napi_create_function(
						env,
						name.as_ptr(),
						name.count_bytes(),
						Some(callback),
						std::ptr::null_mut(),
						&mut function,
					)
				},
				"a function",
			)?;
			check(
				unsafe { node_api::napi_set_named_property(env, exports, name.as_ptr(), function) },
				"an export",
			)?;
		}
		Ok(exports)
	})
}
