use std::cell::Cell;

use napi::bindgen_prelude::{Either, FromNapiValue, ToNapiValue, Uint8Array, Uint32Array};
use napi::{Env, sys};
use napi_derive::napi;
use teasel::Entry;
use teasel::json::{Prepared, Request};

thread_local! {
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

// the view owns the words' allocation and frees it when JavaScript lets the view go; the answer
// is written into it in place, and one the words outgrew stays with the view that shows it
fn answer(env: &Env, result: Result<(), String>) -> Answer {
	if let Err(json) = result {
		return Ok(Either::B(json));
	}
	let (_, mut reference) = VIEW.get();
	let mut value = std::ptr::null_mut();
	teasel::json::words(|words| -> napi::Result<()> {
		if let Some((ptr, capacity)) = words.release() {
			if !reference.is_null() {
				VIEW.set((std::ptr::null_mut(), std::ptr::null_mut()));
				status(
					unsafe { sys::napi_delete_reference(env.raw(), reference) },
					"delete the view",
				)?;
			}
			let view = unsafe {
				Uint32Array::with_external_data(ptr, capacity, move |ptr, len| drop(Vec::from_raw_parts(ptr, 0, len)))
			};
			value = unsafe { ToNapiValue::to_napi_value(env.raw(), view)? };
			reference = std::ptr::null_mut();
			status(
				unsafe { sys::napi_create_reference(env.raw(), value, 1, &mut reference) },
				"keep the view",
			)?;
			VIEW.set((env.raw(), reference));
		} else {
			status(
				unsafe { sys::napi_get_reference_value(env.raw(), reference, &mut value) },
				"get the view",
			)?;
		}
		// past 256 KB, a buffer four times too big is left to its view
		if words.capacity() > 1 << 16 && words.capacity() > 4 * words.len() {
			words.renew(2 * words.len());
		}
		Ok(())
	})?;
	Ok(Either::A(unsafe { Uint32Array::from_napi_value(env.raw(), value)? }))
}

// a view of another environment holds an allocation that environment's end frees
fn fresh(env: &Env) {
	let (view_env, reference) = VIEW.get();
	if !reference.is_null() && view_env != env.raw() {
		VIEW.set((std::ptr::null_mut(), std::ptr::null_mut()));
		teasel::json::words(|words| words.renew(0));
	}
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
	// the bytes V8 encoded, made valid UTF-8 where they are not; the options as their names; the
	// grammar of the host language the whole source is a document of, or nothing
	#[napi(constructor)]
	pub fn new(source: Uint8Array, options: String, host: String) -> napi::Result<Self> {
		let mut prepared = Prepared::from_bytes(source.to_vec(), Request::from_names(&options));
		if !host.is_empty() {
			prepared = prepared.host(&host).map_err(napi::Error::from_reason)?;
		}
		Ok(Self { prepared })
	}

	#[napi(catch_unwind, ts_return_type = "Uint32Array | string")]
	pub fn parse(&self, env: Env, entry: u32, offset: f64, end: Option<f64>, stop: String) -> Answer {
		fresh(&env);
		answer(&env, self.prepared.binary(Entry::from_index(entry), offset, end, &stop))
	}
}
