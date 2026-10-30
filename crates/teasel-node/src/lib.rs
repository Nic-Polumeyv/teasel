//! The Node addon: the same operations as the WebAssembly module, the source read where V8
//! keeps its bytes.

mod node_api;

use std::cell::Cell;
use std::ffi::{CString, c_void};
use std::ptr::null_mut;
use std::rc::Rc;

use node_api::{CallbackInfo, Env, OK, Ref, Status, Value};
use teasel::Options;
use teasel::handed::{Allocation, Element, Raw};
use teasel::host::Grammar;
use teasel::json;

// RUSTFLAGS in the environment replaces .cargo/config.toml's flags, and nothing loads the addon on a Windows without the redistributable
#[cfg(all(target_env = "msvc", not(target_feature = "crt-static")))]
compile_error!("the addon links the C runtime statically: build with .cargo/config.toml's rustflags");

/// What JavaScript holds: the view of the words, then the views of the JavaScript tree and of
/// the TypeScript one, each an array.
const VIEWS: usize = 3;

thread_local! {
	static VIEW: Cell<(Env, [Ref; VIEWS])> = const { Cell::new((null_mut(), [null_mut(); VIEWS])) };
}

type Result<T> = std::result::Result<T, String>;

fn check(status: Status, what: &str) -> Result<()> {
	if status == OK {
		Ok(())
	} else {
		Err(format!("{what}: Node-API status {status}"))
	}
}

fn out<T>(mut slot: T, what: &str, f: impl FnOnce(*mut T) -> Status) -> Result<T> {
	check(f(&mut slot), what)?;
	Ok(slot)
}

fn args<const N: usize>(env: Env, info: CallbackInfo) -> Result<[Value; N]> {
	let mut argv = [null_mut(); N];
	let mut argc = N;
	check(
		unsafe { node_api::napi_get_cb_info(env, info, &mut argc, argv.as_mut_ptr(), null_mut(), null_mut()) },
		"arguments",
	)?;
	if argc < N {
		return Err(format!("{N} arguments expected"));
	}
	Ok(argv)
}

fn bytes<'a>(env: Env, value: Value) -> Result<&'a [u8]> {
	let (mut kind, mut length, mut data) = (0, 0, null_mut());
	check(
		unsafe {
			node_api::napi_get_typedarray_info(env, value, &mut kind, &mut length, &mut data, null_mut(), null_mut())
		},
		"a Uint8Array",
	)?;
	if kind != node_api::UINT8_ARRAY {
		return Err("a Uint8Array expected".into());
	}
	if length == 0 || data.is_null() {
		return Ok(&[]);
	}
	Ok(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) })
}

fn string(env: Env, value: Value) -> Result<String> {
	let mut length = 0;
	check(
		unsafe { node_api::napi_get_value_string_utf8(env, value, null_mut(), 0, &mut length) },
		"a string",
	)?;
	if length == 0 {
		return Ok(String::new());
	}
	let mut buffer = vec![0u8; length + 1];
	check(
		unsafe {
			node_api::napi_get_value_string_utf8(env, value, buffer.as_mut_ptr().cast(), length + 1, &mut length)
		},
		"a string",
	)?;
	buffer.truncate(length);
	String::from_utf8(buffer).map_err(|_| "a string expected".into())
}

fn source_text<'a>(env: Env, value: Value) -> Result<&'a str> {
	std::str::from_utf8(bytes(env, value)?).map_err(|_| "the source is not UTF-8".into())
}

fn number(env: Env, value: Value) -> Result<f64> {
	out(0.0, "a number", |r| unsafe {
		node_api::napi_get_value_double(env, value, r)
	})
}

// anything but a number is absent, read straight: asking V8 what the value is first costs as much as the read
fn optional(env: Env, value: Value) -> Option<f64> {
	let mut result = 0.0;
	(unsafe { node_api::napi_get_value_double(env, value, &mut result) } == OK).then_some(result)
}

fn text(env: Env, text: &str) -> Result<Value> {
	out(null_mut(), "a string", |r| unsafe {
		node_api::napi_create_string_utf8(env, text.as_ptr().cast(), text.len(), r)
	})
}

fn undefined(env: Env) -> Result<Value> {
	out(null_mut(), "undefined", |r| unsafe {
		node_api::napi_get_undefined(env, r)
	})
}

// the grammar of a host language as JSON, read once; V8 lets go of it with the external
unsafe extern "C" fn plan(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [definition] = args::<1>(env, info)?;
		let grammar = teasel::json::grammar(&string(env, definition)?)?;
		let mut result = null_mut();
		check(
			unsafe {
				node_api::napi_create_external(
					env,
					Box::into_raw(Box::new(grammar)).cast(),
					Some(drop_plan),
					null_mut(),
					&mut result,
				)
			},
			"a plan",
		)?;
		Ok(result)
	})
}

unsafe extern "C" fn children(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [plan] = args::<1>(env, info)?;
		text(
			env,
			&teasel::json::children(grammar_of(env, plan).ok_or("a plan expected")?),
		)
	})
}

unsafe extern "C" fn drop_plan(_: Env, data: *mut c_void, _: *mut c_void) {
	drop(unsafe { Box::from_raw(data.cast::<Rc<Grammar>>()) });
}

// the grammar a parse reads a document by; anything else is none
fn grammar_of(env: Env, value: Value) -> Option<&'static Grammar> {
	let mut data = null_mut();
	if unsafe { node_api::napi_get_value_external(env, value, &mut data) } != OK || data.is_null() {
		return None;
	}
	Some(unsafe { &**data.cast::<Rc<Grammar>>() })
}

unsafe extern "C" fn parse(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [source, flags, entry, plan] = args::<4>(env, info)?;
		let (source, options) = (source_text(env, source)?, Options(number(env, flags)? as u32));
		let (entry, grammar) = (json::entry(number(env, entry)?)?, grammar_of(env, plan));
		in_env(env, || {
			json::whole(source, options, entry, grammar);
			view(env)
		})
	})
}

unsafe extern "C" fn parse_at(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [source, flags, entry, offset, end, stop] = args::<6>(env, info)?;
		let (source, options) = (source_text(env, source)?, Options(number(env, flags)? as u32));
		let (entry, offset, end, stop) = (
			json::entry(number(env, entry)?)?,
			number(env, offset)?,
			optional(env, end),
			string(env, stop)?,
		);
		answer(env, || json::at(source, options, entry, offset, end, &stop))
	})
}

fn answer(env: Env, parse: impl FnOnce() -> Result<()>) -> Result<Value> {
	in_env(env, || match parse() {
		Ok(()) => view(env),
		Err(json) => text(env, &json),
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
			let mut array = null_mut();
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
	out(null_mut(), "a number", |r| unsafe {
		node_api::napi_create_uint32(env, value, r)
	})
}

unsafe extern "C" fn layout(env: Env, _: CallbackInfo) -> Value {
	guard(env, || text(env, &teasel::json::layout_json()))
}

// a panic must not cross into the host; it and any error become a thrown Error
fn guard(env: Env, f: impl FnOnce() -> Result<Value> + std::panic::UnwindSafe) -> Value {
	let result = match std::panic::catch_unwind(f) {
		Ok(result) => result,
		Err(panic) => {
			let message = panic
				.downcast_ref::<&str>()
				.map(|s| s.to_string())
				.or_else(|| panic.downcast_ref::<String>().cloned())
				.unwrap_or_else(|| "panic".into());
			// a payload's drop can panic again, and a second panic aborts
			std::mem::forget(panic);
			teasel::json::reset_session();
			Err(message)
		}
	};
	match result {
		Ok(value) => value,
		Err(message) => {
			let message = CString::new(message.replace('\0', " ")).unwrap();
			unsafe { node_api::napi_throw_error(env, std::ptr::null(), message.as_ptr()) };
			null_mut()
		}
	}
}

fn allocate(layout: std::alloc::Layout) -> Allocation {
	let (env, _) = VIEW.get();
	assert!(!env.is_null(), "buffer allocation needs a live environment");
	let result = (|| {
		let (mut data, mut buffer) = (null_mut(), null_mut());
		check(
			unsafe { node_api::napi_create_buffer(env, layout.size(), &mut data, &mut buffer) },
			"allocating a buffer",
		)?;
		let ptr = std::ptr::NonNull::new(data.cast::<u8>()).ok_or("null buffer allocation")?;
		let (mut array, mut offset) = (null_mut(), 0);
		check(
			unsafe {
				node_api::napi_get_typedarray_info(
					env,
					buffer,
					null_mut(),
					null_mut(),
					null_mut(),
					&mut array,
					&mut offset,
				)
			},
			"the buffer's backing store",
		)?;
		assert_eq!(offset, 0, "a buffer must cover its backing store");
		let reference = out(null_mut(), "keeping a buffer", |r| unsafe {
			node_api::napi_create_reference(env, array, 1, r)
		})?;
		Ok(Allocation {
			ptr,
			token: std::ptr::NonNull::new(reference).ok_or("null buffer reference")?,
		})
	})();
	result.unwrap_or_else(|error: String| {
		eprintln!("{error}");
		std::alloc::handle_alloc_error(layout)
	})
}

// a session never mixes environments (`fresh` resets it first), so the token's is the current one
fn release(reference: std::ptr::NonNull<c_void>) {
	unsafe { node_api::napi_delete_reference(VIEW.get().0, reference.as_ptr()) };
}

fn buffer_view(env: Env, buffer: &mut dyn Raw) -> Result<Option<Value>> {
	let Some(reference) = buffer.allocation() else {
		return Ok(None);
	};
	let mut array = null_mut();
	check(
		unsafe { node_api::napi_get_reference_value(env, reference.as_ptr(), &mut array) },
		"the view's buffer",
	)?;
	let mut value = null_mut();
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
	let mut value = null_mut();
	if let Some(next) = buffer_view(env, buffer)? {
		if !refs[slot].is_null() {
			let old = std::mem::replace(&mut refs[slot], null_mut());
			VIEW.set((env, refs));
			check(unsafe { node_api::napi_delete_reference(env, old) }, "the old view")?;
		}
		value = next;
		let mut reference = null_mut();
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
		VIEW.set((env, [null_mut(); VIEWS]));
	}
}

fn in_env<R>(env: Env, f: impl FnOnce() -> R) -> R {
	fresh(env);
	unsafe { teasel::handed::allocating(allocate, release, f) }
}

unsafe extern "C" fn cleanup(data: *mut c_void) {
	if VIEW.get().0 == data.cast() {
		fresh(null_mut());
	}
}

/// # Safety
/// Called by Node once per load with a live `env` and the module's `exports` object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn napi_register_module_v1(env: Env, exports: Value) -> Value {
	// without Node-API there is nothing to throw with: Node reports the module as not registered
	#[cfg(windows)]
	if !unsafe { node_api::load() } {
		eprintln!("teasel: this process does not provide Node-API");
		return null_mut();
	}
	guard(env, || {
		check(
			unsafe { node_api::napi_add_env_cleanup_hook(env, cleanup, env.cast()) },
			"environment cleanup",
		)?;
		for (name, callback) in [
			(c"parse", parse as unsafe extern "C" fn(Env, CallbackInfo) -> Value),
			(c"parseAt", parse_at),
			(c"plan", plan),
			(c"children", children),
			(c"tree", tree),
			(c"layout", layout),
		] {
			let mut function = null_mut();
			check(
				unsafe {
					node_api::napi_create_function(
						env,
						name.as_ptr(),
						name.count_bytes(),
						Some(callback),
						null_mut(),
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
