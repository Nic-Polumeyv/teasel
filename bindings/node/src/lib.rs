//! The Node addon: the same five operations as the WebAssembly module, over a handle to the
//! prepared source.

mod node_api;

use std::cell::Cell;
use std::ffi::{CString, c_void};

use node_api::{CallbackInfo, Env, OK, Ref, Status, Value};
use teasel::Entry;
use teasel::json::{Prepared, Request};

thread_local! {
	static VIEW: Cell<(Env, Ref)> = const { Cell::new((std::ptr::null_mut(), std::ptr::null_mut())) };
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
	Ok(String::from_utf8_lossy(&buffer).into_owned())
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

// the bytes V8 encoded, made valid UTF-8 where they are not; the options as their names; the
// grammar of the host language the whole source is a document of, or nothing
unsafe extern "C" fn create(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [source, names, host] = args::<3>(env, info)?;
		let mut prepared = Prepared::from_bytes(bytes(env, source)?, Request::from_names(&string(env, names)?));
		let host = string(env, host)?;
		if !host.is_empty() {
			prepared = prepared.host(&host)?;
		}
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

unsafe extern "C" fn free(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [source] = args::<1>(env, info)?;
		drop(unsafe { Box::from_raw(handle(env, source)?) });
		undefined(env)
	})
}

unsafe extern "C" fn parse(env: Env, info: CallbackInfo) -> Value {
	guard(env, || {
		let [source, entry, offset, end, stop] = args::<5>(env, info)?;
		let prepared = unsafe { &*handle(env, source)? };
		let entry = Entry::from_index(number(env, entry)? as u32);
		let (offset, end, stop) = (number(env, offset)?, optional(env, end)?, string(env, stop)?);
		fresh(env);
		match prepared.binary(entry, offset, end, &stop) {
			Ok(()) => view(env),
			Err(json) => text(env, &json),
		}
	})
}

unsafe extern "C" fn constants(env: Env, _: CallbackInfo) -> Value {
	guard(env, || {
		let names = teasel::estree::constants();
		let mut array = std::ptr::null_mut();
		check(
			unsafe { node_api::napi_create_array_with_length(env, names.len(), &mut array) },
			"an array",
		)?;
		for (i, name) in names.iter().enumerate() {
			check(
				unsafe { node_api::napi_set_element(env, array, i as u32, text(env, name)?) },
				"an element",
			)?;
		}
		Ok(array)
	})
}

unsafe extern "C" fn shapes(env: Env, _: CallbackInfo) -> Value {
	guard(env, || {
		let words = teasel::estree::shapes();
		let mut array = std::ptr::null_mut();
		check(
			unsafe { node_api::napi_create_array_with_length(env, words.len(), &mut array) },
			"an array",
		)?;
		for (i, word) in words.iter().enumerate() {
			let mut value = std::ptr::null_mut();
			check(
				unsafe { node_api::napi_create_uint32(env, *word, &mut value) },
				"a number",
			)?;
			check(
				unsafe { node_api::napi_set_element(env, array, i as u32, value) },
				"an element",
			)?;
		}
		Ok(array)
	})
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

unsafe extern "C" fn release(_: Env, data: *mut c_void, capacity: *mut c_void) {
	drop(unsafe { Vec::from_raw_parts(data.cast::<u32>(), 0, capacity as usize) });
}

// the view owns the words' allocation and frees it when JavaScript lets the view go; the answer
// is written into it in place, and one the words outgrew stays with the view that shows it
fn view(env: Env) -> Result<Value> {
	let (_, mut reference) = VIEW.get();
	let mut value = std::ptr::null_mut();
	teasel::json::words(|words| -> Result<()> {
		if let Some((ptr, capacity)) = words.release() {
			if !reference.is_null() {
				VIEW.set((std::ptr::null_mut(), std::ptr::null_mut()));
				check(
					unsafe { node_api::napi_delete_reference(env, reference) },
					"the old view",
				)?;
			}
			let mut buffer = std::ptr::null_mut();
			check(
				unsafe {
					node_api::napi_create_external_arraybuffer(
						env,
						ptr.cast(),
						capacity * 4,
						Some(release),
						capacity as *mut c_void,
						&mut buffer,
					)
				},
				"the answer's buffer",
			)?;
			check(
				unsafe {
					node_api::napi_create_typedarray(env, node_api::UINT32_ARRAY, capacity, buffer, 0, &mut value)
				},
				"the view",
			)?;
			reference = std::ptr::null_mut();
			check(
				unsafe { node_api::napi_create_reference(env, value, 1, &mut reference) },
				"keeping the view",
			)?;
			VIEW.set((env, reference));
		} else {
			check(
				unsafe { node_api::napi_get_reference_value(env, reference, &mut value) },
				"the view",
			)?;
		}
		// past 256 KB, a buffer four times too big is left to its view
		if words.capacity() > 1 << 16 && words.capacity() > 4 * words.len() {
			words.renew(2 * words.len());
		}
		Ok(())
	})?;
	Ok(value)
}

// a view of another environment holds an allocation that environment's end frees
fn fresh(env: Env) {
	let (view_env, reference) = VIEW.get();
	if !reference.is_null() && view_env != env {
		VIEW.set((std::ptr::null_mut(), std::ptr::null_mut()));
		teasel::json::words(|words| words.renew(0));
	}
}

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
			(c"free", free),
			(c"constants", constants),
			(c"shapes", shapes),
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
