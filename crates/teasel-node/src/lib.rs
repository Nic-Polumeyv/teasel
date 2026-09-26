//! The Node addon: the same five operations as the WebAssembly module, over a handle to the
//! prepared source.

mod node_api;

use std::cell::Cell;
use std::ffi::{CString, c_void};
use std::ptr::null_mut;
use std::rc::Rc;

use node_api::{CallbackInfo, Env, OK, Ref, Status, Value};
use teasel::Entry;
use teasel::handed::{Element, Raw};
use teasel::host::Grammar;
use teasel::json::{Prepared, Request};

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

fn bytes(env: Env, value: Value) -> Result<Vec<u8>> {
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
		return Ok(Vec::new());
	}
	Ok(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) }.to_vec())
}

fn string(env: Env, value: Value) -> Result<String> {
	let mut length = 0;
	check(
		unsafe { node_api::napi_get_value_string_utf8(env, value, null_mut(), 0, &mut length) },
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
	out(0.0, "a number", |r| unsafe {
		node_api::napi_get_value_double(env, value, r)
	})
}

fn optional(env: Env, value: Value) -> Result<Option<f64>> {
	if kind(env, value)? == node_api::UNDEFINED {
		Ok(None)
	} else {
		number(env, value).map(Some)
	}
}

fn kind(env: Env, value: Value) -> Result<i32> {
	out(0, "a value", |r| unsafe { node_api::napi_typeof(env, value, r) })
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

fn external(env: Env, value: Value, what: &str) -> Result<*mut c_void> {
	out(null_mut(), what, |r| unsafe {
		node_api::napi_get_value_external(env, value, r)
	})
}

fn handle(env: Env, value: Value) -> Result<*mut Prepared<'static>> {
	let data = external(env, value, "a source")?;
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
		let mut result = null_mut();
		check(
			unsafe {
				node_api::napi_create_external(
					env,
					Box::into_raw(Box::new(prepared)).cast(),
					None,
					null_mut(),
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

unsafe extern "C" fn drop_plan(_: Env, data: *mut c_void, _: *mut c_void) {
	drop(unsafe { Box::from_raw(data.cast::<Rc<Grammar>>()) });
}

// the grammar a parse reads a document by, or undefined
fn grammar_of(env: Env, value: Value) -> Result<Option<&'static Grammar>> {
	if kind(env, value)? == node_api::UNDEFINED {
		return Ok(None);
	}
	let data = external(env, value, "a plan")?;
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
		fresh(env);
		match prepared.in_place(entry, offset, end, &stop, grammar) {
			Ok(()) => view(env),
			Err(json) => text(env, &json),
		}
	})
}

// the views of the last parse's tree, in the layout's order, `undefined` for a table the parse did not
// fill, its first element whether the tree is the TypeScript one; undefined before any. The array is kept: a call
// sets the views whose buffers moved since the last, and the answer's words say when one has
unsafe extern "C" fn tree(env: Env, _: CallbackInfo) -> Value {
	guard(env, || {
		fresh(env);
		let mut moved: Vec<(u32, *mut u8, usize, usize, i32)> = Vec::new();
		let mut count = 0;
		let typescript = teasel::json::tree(&mut |_, buffer| {
			count += 1;
			if let Some(buffer) = buffer
				&& let Some((ptr, bytes, align)) = buffer.release()
			{
				moved.push((count, ptr, bytes, align, kind_of(buffer.element())));
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
		// the view owns the buffer's allocation and frees it when JavaScript lets the view go: the array
		// lets go of the view of a buffer that moved
		for (i, ptr, bytes, align, kind) in moved {
			check(
				unsafe { node_api::napi_set_element(env, array, i, external_view(env, ptr, bytes, align, kind)?) },
				"an element",
			)?;
		}
		Ok(array)
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
			null_mut()
		}
	}
}

// the hint is the allocation's bytes over its alignment's power of two
unsafe extern "C" fn release(_: Env, data: *mut c_void, hint: *mut c_void) {
	unsafe { teasel::handed::free(data.cast(), hint.addr() >> 4, 1 << (hint.addr() & 15)) };
}

fn external_view(env: Env, ptr: *mut u8, bytes: usize, align: usize, kind: i32) -> Result<Value> {
	let mut array = null_mut();
	check(
		unsafe {
			node_api::napi_create_external_arraybuffer(
				env,
				ptr.cast(),
				bytes,
				Some(release),
				std::ptr::without_provenance_mut(bytes << 4 | align.trailing_zeros() as usize),
				&mut array,
			)
		},
		"the view's buffer",
	)?;
	let mut value = null_mut();
	check(
		unsafe {
			node_api::napi_create_typedarray(env, kind, bytes / node_api::element_size(kind), array, 0, &mut value)
		},
		"the view",
	)?;
	Ok(value)
}

// the view owns the buffer's allocation and frees it when JavaScript lets the view go; a buffer
// that outgrew its allocation gets a new view, and the old one stays with what it showed
fn view_of(env: Env, slot: usize, buffer: &mut dyn Raw, kind: i32) -> Result<Value> {
	let (_, mut refs) = VIEW.get();
	let mut value = null_mut();
	if let Some((ptr, bytes, align)) = buffer.release() {
		if !refs[slot].is_null() {
			let old = std::mem::replace(&mut refs[slot], null_mut());
			VIEW.set((env, refs));
			check(unsafe { node_api::napi_delete_reference(env, old) }, "the old view")?;
		}
		value = external_view(env, ptr, bytes, align, kind)?;
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
		let value = view_of(env, 0, words, node_api::UINT32_ARRAY)?;
		// past 256 KB, a buffer four times too big is left to its view
		if words.capacity() > 1 << 16 && words.capacity() > 4 * words.len() {
			words.renew(2 * words.len());
		}
		Ok(value)
	})
}

// a view of another environment holds an allocation that environment's end frees
fn fresh(env: Env) {
	let (view_env, refs) = VIEW.get();
	if view_env != env && refs.iter().any(|reference| !reference.is_null()) {
		VIEW.set((null_mut(), [null_mut(); VIEWS]));
		teasel::json::words(|words| words.renew(0));
		teasel::json::renew_trees();
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
		for (name, callback) in [
			(c"create", create as unsafe extern "C" fn(Env, CallbackInfo) -> Value),
			(c"parse", parse),
			(c"plan", plan),
			(c"free", free),
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
