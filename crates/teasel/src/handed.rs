use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;

pub struct Allocation {
	pub ptr: NonNull<u8>,
	pub token: NonNull<c_void>,
}

type Allocate = fn(Layout) -> Allocation;
type Release = fn(NonNull<c_void>);

thread_local! {
	static ALLOCATE: Cell<Option<Allocate>> = const { Cell::new(None) };
	static RELEASE: Cell<Option<Release>> = const { Cell::new(None) };
}

/// # Safety
/// Each hook result must own a distinct writable allocation matching the layout, valid on this thread until
/// `release` gets its token; `release` stays in force on this thread after `f` returns.
pub unsafe fn allocating<R>(allocate: Allocate, release: Release, f: impl FnOnce() -> R) -> R {
	struct Restore(Option<Allocate>);
	impl Drop for Restore {
		fn drop(&mut self) {
			ALLOCATE.set(self.0);
		}
	}
	RELEASE.set(Some(release));
	let _restore = Restore(ALLOCATE.replace(Some(allocate)));
	f()
}

pub struct Handed<T: Copy> {
	ptr: NonNull<T>,
	len: usize,
	capacity: usize,
	least: usize,
	token: Option<NonNull<c_void>>,
	viewed: bool,
}

impl<T: Copy> Handed<T> {
	pub fn new(least: usize) -> Self {
		Handed {
			ptr: NonNull::dangling(),
			len: 0,
			capacity: if size_of::<T>() == 0 { usize::MAX } else { 0 },
			least,
			token: None,
			viewed: false,
		}
	}

	pub fn with_capacity(capacity: usize, least: usize) -> Self {
		let mut buffer = Self::new(least);
		let layout = Layout::array::<T>(capacity).expect("buffer capacity overflow");
		if layout.size() != 0 {
			let ptr = if let Some(allocate) = ALLOCATE.get() {
				let allocation = allocate(layout);
				buffer.token = Some(allocation.token);
				allocation.ptr
			} else {
				NonNull::new(unsafe { alloc(layout) }).unwrap_or_else(|| handle_alloc_error(layout))
			};
			assert!(ptr.as_ptr().addr().is_multiple_of(layout.align()), "buffer alignment");
			buffer.ptr = ptr.cast();
			buffer.capacity = capacity;
		}
		buffer
	}

	pub fn as_ptr(&self) -> *const T {
		self.ptr.as_ptr()
	}

	pub fn capacity(&self) -> usize {
		self.capacity
	}

	pub fn clear(&mut self) {
		self.len = 0;
	}

	pub fn truncate(&mut self, len: usize) {
		self.len = self.len.min(len);
	}

	pub fn resize(&mut self, len: usize, value: T) {
		self.truncate(len);
		self.room(len - self.len);
		while self.len < len {
			unsafe { self.ptr.as_ptr().add(self.len).write(value) };
			self.len += 1;
		}
	}

	#[inline(always)]
	fn room(&mut self, more: usize) {
		if self.capacity - self.len < more {
			self.grow(more);
		}
	}

	#[inline(always)]
	pub fn push(&mut self, value: T) {
		self.room(1);
		unsafe { self.ptr.as_ptr().add(self.len).write(value) };
		self.len += 1;
	}

	#[inline(always)]
	pub fn extend_from_slice(&mut self, values: &[T]) {
		self.room(values.len());
		unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), self.ptr.as_ptr().add(self.len), values.len()) };
		self.len += values.len();
	}

	fn grow(&mut self, more: usize) {
		let cap = self
			.capacity
			.saturating_mul(2)
			.max(self.len.checked_add(more).expect("buffer capacity overflow"))
			.max(self.least);
		let mut next = Self::with_capacity(cap, self.least);
		next.extend_from_slice(self);
		*self = next;
	}

	pub fn renew(&mut self, cap: usize) {
		*self = Self::with_capacity(cap.max(self.least), self.least);
	}
}

impl<T: Copy> Default for Handed<T> {
	fn default() -> Self {
		Self::new(0)
	}
}

impl<T: Copy> Drop for Handed<T> {
	fn drop(&mut self) {
		if let Some(token) = self.token {
			RELEASE.get().expect("a lent buffer's release")(token);
		} else if self.capacity != 0 && size_of::<T>() != 0 {
			unsafe { dealloc(self.ptr.as_ptr().cast(), Layout::array::<T>(self.capacity).unwrap()) };
		}
	}
}

impl<T: Copy> std::ops::Deref for Handed<T> {
	type Target = [T];
	fn deref(&self) -> &[T] {
		unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
	}
}

impl<T: Copy> std::ops::DerefMut for Handed<T> {
	fn deref_mut(&mut self) -> &mut [T] {
		unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
	}
}

impl<T: Copy + std::fmt::Debug> std::fmt::Debug for Handed<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		(**self).fmt(f)
	}
}

impl<T: Copy> Extend<T> for Handed<T> {
	fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
		let iter = iter.into_iter();
		self.room(iter.size_hint().0);
		for value in iter {
			self.push(value);
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
	U8,
	U32,
	F64,
}

impl Element {
	pub fn size(self) -> usize {
		match self {
			Element::U8 => 1,
			Element::U32 => 4,
			Element::F64 => 8,
		}
	}
}

pub trait Raw {
	fn element(&self) -> Element;
	/// The length in the elements a front end reads, a record being its words.
	fn elements(&self) -> usize {
		self.len_bytes() / self.element().size()
	}
	fn as_ptr(&self) -> *const u8;
	fn len_bytes(&self) -> usize;
	fn capacity_bytes(&self) -> usize;
	fn allocation(&mut self) -> Option<NonNull<c_void>>;
}

impl<T: Copy + 'static> Raw for Handed<T> {
	fn element(&self) -> Element {
		use std::any::TypeId;
		let ty = TypeId::of::<T>();
		if ty == TypeId::of::<u8>() {
			Element::U8
		} else if ty == TypeId::of::<f64>() {
			Element::F64
		} else {
			const { assert!(size_of::<T>().is_multiple_of(4) || size_of::<T>() == 1) };
			Element::U32
		}
	}

	fn as_ptr(&self) -> *const u8 {
		self.ptr.as_ptr().cast()
	}

	fn len_bytes(&self) -> usize {
		self.len * size_of::<T>()
	}

	fn capacity_bytes(&self) -> usize {
		self.capacity * size_of::<T>()
	}

	fn allocation(&mut self) -> Option<NonNull<c_void>> {
		if self.viewed {
			return None;
		}
		if self.capacity == 0 {
			self.grow(1);
		}
		self.viewed = true;
		Some(self.token.expect("a view needs an allocation hook"))
	}
}

/// Visits the buffers of a tree a front end reads in place, by name; None for a table the parse
/// did not fill.
pub struct Views<'f>(pub &'f mut dyn FnMut(&'static str, Option<&mut dyn Raw>));

impl Views<'_> {
	pub fn push(&mut self, name: &'static str, buffer: &mut dyn Raw) {
		(self.0)(name, Some(buffer));
	}

	pub fn none(&mut self, name: &'static str) {
		(self.0)(name, None);
	}
}
