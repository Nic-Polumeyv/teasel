//! What the addon takes from Node-API, declared here. The host process provides the functions:
//! resolved when the addon is loaded on Linux and macOS, looked up by name on Windows, where a
//! library cannot leave an import open.

use std::ffi::{c_char, c_void};

pub type Env = *mut c_void;
pub type Value = *mut c_void;
pub type CallbackInfo = *mut c_void;
pub type Ref = *mut c_void;
pub type Status = i32;
pub type Callback = Option<unsafe extern "C" fn(Env, CallbackInfo) -> Value>;
pub type Finalize = Option<unsafe extern "C" fn(Env, *mut c_void, *mut c_void)>;

pub const OK: Status = 0;
pub const UNDEFINED: i32 = 0;
pub const UINT8_ARRAY: i32 = 1;
pub const UINT32_ARRAY: i32 = 6;

macro_rules! api {
	($(fn $name:ident($($arg:ident: $ty:ty),*) -> Status;)*) => {
		#[cfg(not(windows))]
		unsafe extern "C" {
			$(pub fn $name($($arg: $ty),*) -> Status;)*
		}

		#[cfg(windows)]
		#[allow(non_upper_case_globals)]
		mod symbols {
			use std::sync::atomic::AtomicUsize;
			$(pub static $name: AtomicUsize = AtomicUsize::new(0);)*
		}

		$(
			#[cfg(windows)]
			pub unsafe fn $name($($arg: $ty),*) -> Status {
				let at = symbols::$name.load(std::sync::atomic::Ordering::Relaxed);
				let f: unsafe extern "C" fn($($ty),*) -> Status = unsafe { std::mem::transmute(at) };
				unsafe { f($($arg),*) }
			}
		)*

		#[cfg(windows)]
		pub unsafe fn load() {
			let host = unsafe { GetModuleHandleW(std::ptr::null()) };
			$(symbols::$name.store(
				unsafe { GetProcAddress(host, concat!(stringify!($name), "\0").as_ptr().cast()) } as usize,
				std::sync::atomic::Ordering::Relaxed,
			);)*
		}
	};
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
	fn GetModuleHandleW(name: *const u16) -> *mut c_void;
	fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
}

api! {
	fn napi_create_function(env: Env, name: *const c_char, length: usize, cb: Callback, data: *mut c_void, result: *mut Value) -> Status;
	fn napi_set_named_property(env: Env, object: Value, name: *const c_char, value: Value) -> Status;
	fn napi_get_cb_info(env: Env, info: CallbackInfo, argc: *mut usize, argv: *mut Value, this: *mut Value, data: *mut *mut c_void) -> Status;
	fn napi_typeof(env: Env, value: Value, result: *mut i32) -> Status;
	fn napi_get_typedarray_info(env: Env, array: Value, kind: *mut i32, length: *mut usize, data: *mut *mut c_void, buffer: *mut Value, offset: *mut usize) -> Status;
	fn napi_get_value_string_utf8(env: Env, value: Value, buffer: *mut c_char, size: usize, length: *mut usize) -> Status;
	fn napi_get_value_double(env: Env, value: Value, result: *mut f64) -> Status;
	fn napi_create_external(env: Env, data: *mut c_void, finalize: Finalize, hint: *mut c_void, result: *mut Value) -> Status;
	fn napi_get_value_external(env: Env, value: Value, result: *mut *mut c_void) -> Status;
	fn napi_create_external_arraybuffer(env: Env, data: *mut c_void, bytes: usize, finalize: Finalize, hint: *mut c_void, result: *mut Value) -> Status;
	fn napi_create_typedarray(env: Env, kind: i32, length: usize, buffer: Value, offset: usize, result: *mut Value) -> Status;
	fn napi_create_reference(env: Env, value: Value, count: u32, result: *mut Ref) -> Status;
	fn napi_delete_reference(env: Env, reference: Ref) -> Status;
	fn napi_get_reference_value(env: Env, reference: Ref, result: *mut Value) -> Status;
	fn napi_create_string_utf8(env: Env, text: *const c_char, length: usize, result: *mut Value) -> Status;
	fn napi_create_array_with_length(env: Env, length: usize, result: *mut Value) -> Status;
	fn napi_set_element(env: Env, array: Value, index: u32, value: Value) -> Status;
	fn napi_create_uint32(env: Env, value: u32, result: *mut Value) -> Status;
	fn napi_throw_error(env: Env, code: *const c_char, message: *const c_char) -> Status;
	fn napi_get_undefined(env: Env, result: *mut Value) -> Status;
}
