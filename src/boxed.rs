use crate::alloc::Sensitive;
use crate::pages::{Protection, page_align, protect, zero};
use crate::guard::{Guard, Protectable};

use std::io::Error;

type InnerBox<T> = std::boxed::Box<T, Sensitive>;
type Box<T> = Guard<InnerBox<T>>;

unsafe fn box_raw_ptr<T>(boxed: &InnerBox<T>) -> *mut u8 {
	debug_assert!(std::mem::size_of::<T>() > 0);

	(&**boxed as *const T).cast::<u8>() as *mut u8
}

fn box_protect<T>(boxed: &InnerBox<T>, prot: Protection) -> Result<(), std::io::Error> {
	if std::mem::size_of::<T>() > 0 {
		unsafe { protect(box_raw_ptr(boxed), page_align(std::mem::size_of::<T>()), prot) }
	} else {
		Ok(())
	}
}

impl<T> Protectable for InnerBox<T> {
	fn lock(&self) -> Result<(), Error> {
		box_protect(self, Protection::NoAccess)
	}

	fn unlock(&self) -> Result<(), Error> {
		box_protect(self, Protection::ReadOnly)
	}

	fn unlock_mut(&mut self) -> Result<(), Error> {
		box_protect(self, Protection::ReadWrite)
	}
}

impl<T> Box<T> {
	pub unsafe fn raw_ptr(&self) -> *mut u8 {
		box_raw_ptr(self.inner())
	}

	fn new_without_clear(source: T) -> Self {
		let mut guard = Guard::from_inner(std::boxed::Box::new_in(source, Sensitive));
		guard.mutate(|boxed| boxed.lock().unwrap());
		guard
	}

	pub fn new(mut source: T) -> Self {
		let ptr: *mut T = &mut source;
		let guard = Self::new_without_clear(source);

		// Clear out source
		unsafe { zero(ptr, 1); }

		guard
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
