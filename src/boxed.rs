use crate::auxiliary::zero;
use crate::pages::{Pages, GuardedAlloc};
use crate::alloc::Sensitive;
use crate::guard::Guard;
use crate::traits::{AsPages, Protectable};

pub(crate) type InnerBox<T> = std::boxed::Box<T, Sensitive>;
pub type Box<T> = Guard<InnerBox<T>>;

impl<T> AsPages for InnerBox<T> {
	fn as_pages(&self) -> Option<Pages> {
		if std::mem::size_of::<T>() > 0 {
			Some(unsafe { GuardedAlloc::<{ Sensitive::GUARD_PAGES }>::from_ptr(&**self as *const T as *mut T, std::mem::size_of::<T>()).into_pages() })
		} else {
			None
		}
	}
}

impl<T> Box<T> {
	pub(crate) fn new_without_clear(source: T) -> Self {
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

impl<T: Default> Default for Box<T> {
	fn default() -> Self {
		Self::new_without_clear(T::default())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(target_os = "linux")]
	#[test]
	fn protection() {
		use bulletproof::Bulletproof;

		let mut test = Box::<u32>::new(0x55555555);
		let bp = unsafe { Bulletproof::new() };

		let ptr = unsafe { &mut **test.inner_mut() } as *mut u32;

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
