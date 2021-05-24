use crate::alloc::Sensitive;
use crate::pages::{Protection, page_align, protect, zero};
use crate::refs::RefCount;

use std::default::Default;
use std::fmt;
use std::ops::{Deref, DerefMut, Drop};

pub struct Box<T> {
	boxed: std::boxed::Box<T, Sensitive>,
	refs: RefCount
}

#[derive(Debug)]
pub struct Ref<'t, T>(&'t Box<T>);

#[derive(Debug)]
pub struct RefMut<'t, T>(&'t mut Box<T>);

impl<T> Box<T> {
	unsafe fn raw_ptr(&self) -> *mut u8 {
		debug_assert!(std::mem::size_of::<T>() > 0);

		(&*self.boxed as *const T).cast::<u8>() as *mut u8
	}

	fn protect(&self, prot: Protection) -> Result<(), std::io::Error> {
		if std::mem::size_of::<T>() > 0 {
			unsafe { protect(self.raw_ptr(), page_align(std::mem::size_of::<T>()), prot) }
		} else {
			Ok(())
		}
	}

	fn new_without_clear(source: T) -> Self {
		let outer = Self {
			boxed: std::boxed::Box::new_in(source, Sensitive),
			refs: RefCount::default()
		};

		outer.protect(Protection::NoAccess).unwrap();
		outer
	}

	pub fn new(mut source: T) -> Self {
		let ptr: *mut T = &mut source;
		let outer = Self::new_without_clear(source);

		// Clear out source
		unsafe { zero(ptr, 1); }

		outer
	}

	pub fn borrow(&self) -> Ref<'_, T> {
		self.refs.acquire(|| self.protect(Protection::ReadOnly).unwrap());

		Ref(self)
	}

	pub fn borrow_mut(&mut self) -> RefMut<'_, T> {
		self.refs.acquire_mut(|| self.protect(Protection::ReadWrite).unwrap());

		RefMut(self)
	}
}

impl<T: Default> Default for Box<T> {
	fn default() -> Self {
		Self::new_without_clear(T::default())
	}
}

impl<T> fmt::Debug for Box<T> {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt.debug_struct("Box")
			.field("refs", &self.refs)
			.finish_non_exhaustive()
	}
}

impl<T> Drop for Box<T> {
	fn drop(&mut self) {
		self.protect(Protection::ReadWrite).unwrap();
	}
}

impl<T> Deref for Ref<'_, T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		self.0.boxed.as_ref()
	}
}

impl<T> Drop for Ref<'_, T> {
	fn drop(&mut self) {
		self.0.refs.release(
			|| self.0.protect(Protection::NoAccess).unwrap(),
			|| self.0.protect(Protection::ReadOnly).unwrap());
	}
}

impl<T> Deref for RefMut<'_, T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		self.0.boxed.as_ref()
	}
}

impl<T> DerefMut for RefMut<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		self.0.boxed.as_mut()
	}
}

impl<T> Drop for RefMut<'_, T> {
	fn drop(&mut self) {
		self.0.refs.release_mut(|| self.0.protect(Protection::NoAccess).unwrap());
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(unix)]
	#[test]
	fn test_protection() {
		use bulletproof::Bulletproof;

		let mut test = Box::<u32>::new(0x55555555);
		let bp = unsafe { Bulletproof::new() };

		let ptr = unsafe { test.raw_ptr() };

		assert_eq!(unsafe { bp.load(ptr) }, Err(()));

		{
			let immutable = test.borrow();
			assert_eq!(*immutable, 0x55555555);
			assert_eq!(unsafe { bp.store(ptr, &0x55) }, Err(()));
		}

		assert_eq!(unsafe { bp.load(ptr) }, Err(()));

		{
			let mut mutable = test.borrow_mut();
			assert_eq!(*mutable, 0x55555555);
			*mutable = 0xdeadbeef;
			assert_eq!(*mutable, 0xdeadbeef);
		}

		assert_eq!(unsafe { bp.load(ptr) }, Err(()));
	}
}
