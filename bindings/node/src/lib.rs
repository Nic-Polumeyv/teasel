use std::cell::Cell;

use napi::bindgen_prelude::{Either, FromNapiValue, ToNapiValue, Uint8Array, Uint32Array};
use napi::{Env, sys};
use napi_derive::napi;
use teasel::Entry;
use teasel::json::{Prepared, Request};

thread_local! {
	static WORDS: Cell<(*mut u32, usize)> = const { Cell::new((std::ptr::null_mut(), 0)) };
	static VIEW: Cell<(sys::napi_env, sys::napi_ref)> = const { Cell::new((std::ptr::null_mut(), std::ptr::null_mut())) };
}

type Answer = napi::Result<Either<Uint32Array, String>>;

fn status(status: sys::napi_status, what: &str) -> napi::Result<()> {
	if status == sys::Status::napi_ok {
		Ok(())
	} else {
		Err(napi::Error::new(napi::Status::from(status), what))
	}
}

// the view's finalizer owns the buffer, so an outgrown one lives while JavaScript holds its view
// the view spans the whole buffer: past an answer's words it shows the previous answer's
fn answer(env: &Env, result: Result<Vec<u32>, String>) -> Answer {
	let words = match result {
		Ok(words) => words,
		Err(json) => return Ok(Either::B(json)),
	};
	let (mut ptr, capacity) = WORDS.get();
	let (view_env, mut reference) = VIEW.get();
	let fits = capacity >= words.len() && (capacity <= 1 << 16 || capacity <= 4 * words.len());
	let mut value = std::ptr::null_mut();
	if reference.is_null() || view_env != env.raw() || !fits {
		if !reference.is_null() && view_env == env.raw() {
			VIEW.set((std::ptr::null_mut(), std::ptr::null_mut()));
			status(
				unsafe { sys::napi_delete_reference(env.raw(), reference) },
				"delete the view",
			)?;
		}
		let mut buffer = vec![0u32; (words.len() * 2).max(1 << 16)];
		ptr = buffer.as_mut_ptr();
		let capacity = buffer.capacity();
		std::mem::forget(buffer);
		let view = unsafe {
			Uint32Array::with_external_data(ptr, capacity, move |ptr, len| {
				drop(Vec::from_raw_parts(ptr, len, capacity))
			})
		};
		value = unsafe { ToNapiValue::to_napi_value(env.raw(), view)? };
		reference = std::ptr::null_mut();
		status(
			unsafe { sys::napi_create_reference(env.raw(), value, 1, &mut reference) },
			"keep the view",
		)?;
		VIEW.set((env.raw(), reference));
		WORDS.set((ptr, capacity));
	} else {
		status(
			unsafe { sys::napi_get_reference_value(env.raw(), reference, &mut value) },
			"get the view",
		)?;
	}
	unsafe { std::ptr::copy_nonoverlapping(words.as_ptr(), ptr, words.len()) };
	teasel::estree::recycle(words);
	Ok(Either::A(unsafe { Uint32Array::from_napi_value(env.raw(), value)? }))
}

#[napi]
pub fn constants() -> Vec<&'static str> {
	teasel::estree::constants()
}

#[napi]
pub fn shapes() -> Vec<u32> {
	teasel::estree::shapes()
}

#[napi]
pub struct Source {
	prepared: Prepared<'static>,
}

#[napi]
impl Source {
	// the bytes V8 encoded, made valid UTF-8 in place where they are not
	#[napi(constructor)]
	pub fn new(source: Uint8Array, bits: u32) -> Self {
		Self {
			prepared: Prepared::from_bytes(source.to_vec(), Request::from_bits(bits)),
		}
	}

	#[napi(catch_unwind, ts_return_type = "Uint32Array | string")]
	pub fn parse(&self, env: Env, entry: u32, offset: f64, end: Option<f64>, stop: String) -> Answer {
		answer(&env, self.prepared.binary(Entry::from_index(entry), offset, end, &stop))
	}
}
