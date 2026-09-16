//! A buffer a front end reads in place: it hands its allocation over as a view and grows on a
//! fresh one, so the view keeps what it saw.

/// A `Vec` whose allocation can be handed to a caller; the caller frees it as a `Vec<T>` of the
/// capacity `release` gave. Once handed over, the buffer never reallocates in place.
pub struct Handed<T> {
	vec: Vec<T>,
	owned: bool,
	/// The least capacity a fresh allocation gets.
	least: usize,
}

impl<T: Copy> Handed<T> {
	pub fn new(least: usize) -> Self {
		Handed {
			vec: Vec::new(),
			owned: true,
			least,
		}
	}

	pub fn with_capacity(capacity: usize, least: usize) -> Self {
		Handed {
			vec: Vec::with_capacity(capacity),
			owned: true,
			least,
		}
	}

	pub fn as_ptr(&self) -> *const T {
		self.vec.as_ptr()
	}

	pub fn capacity(&self) -> usize {
		self.vec.capacity()
	}

	pub fn clear(&mut self) {
		self.vec.clear();
	}

	pub fn truncate(&mut self, len: usize) {
		self.vec.truncate(len);
	}

	#[inline(always)]
	fn room(&mut self, more: usize) {
		if self.vec.capacity() - self.vec.len() < more {
			self.grow(more);
		}
	}

	#[inline(always)]
	pub fn push(&mut self, value: T) {
		self.room(1);
		self.vec.push(value);
	}

	#[inline(always)]
	pub fn extend_from_slice(&mut self, values: &[T]) {
		self.room(values.len());
		self.vec.extend_from_slice(values);
	}

	fn grow(&mut self, more: usize) {
		let cap = (self.vec.capacity() * 2).max(self.vec.len() + more).max(self.least);
		let mut next = Vec::with_capacity(cap);
		next.extend_from_slice(&self.vec);
		let old = std::mem::replace(&mut self.vec, next);
		if !self.owned {
			std::mem::forget(old);
		}
		self.owned = true;
	}

	/// Hands the allocation to the caller, who frees it as a `Vec<T>` of that capacity; None
	/// when a caller already holds it.
	pub fn release(&mut self) -> Option<(*mut T, usize)> {
		if !self.owned {
			return None;
		}
		if self.vec.capacity() == 0 {
			self.grow(0);
		}
		self.owned = false;
		Some((self.vec.as_mut_ptr(), self.vec.capacity()))
	}

	/// Continues on a fresh allocation of at least `cap`, empty.
	pub fn renew(&mut self, cap: usize) {
		let old = std::mem::replace(&mut self.vec, Vec::with_capacity(cap.max(self.least)));
		if !self.owned {
			std::mem::forget(old);
		}
		self.owned = true;
	}
}

impl<T: Copy> Default for Handed<T> {
	fn default() -> Self {
		Self::new(0)
	}
}

impl<T> Drop for Handed<T> {
	fn drop(&mut self) {
		if !self.owned {
			std::mem::forget(std::mem::take(&mut self.vec));
		}
	}
}

impl<T> std::ops::Deref for Handed<T> {
	type Target = [T];
	fn deref(&self) -> &[T] {
		&self.vec
	}
}

impl<T> std::ops::DerefMut for Handed<T> {
	fn deref_mut(&mut self) -> &mut [T] {
		&mut self.vec
	}
}

impl<T: std::fmt::Debug> std::fmt::Debug for Handed<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.vec.fmt(f)
	}
}

impl<T: Copy> Extend<T> for Handed<T> {
	fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
		let iter = iter.into_iter();
		match iter.size_hint() {
			// an exact size fills the room in one copy; a loose one must not let the Vec grow itself
			(lower, Some(upper)) if lower == upper => {
				self.room(upper);
				self.vec.extend(iter);
			}
			_ => {
				for value in iter {
					self.push(value);
				}
			}
		}
	}
}
