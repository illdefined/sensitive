//! Atomically reference‐counted access guard

use crate::traits::Protectable;

use std::intrinsics::likely;
use std::fmt;
use std::ops::{Deref, DerefMut, Index, IndexMut, Drop};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Atomically reference‐counted access guard
pub struct Guard<T: Protectable>(AtomicUsize, T);

/// Reference to immutably borrowed guarded value
pub struct Ref<'t, T: Protectable>(pub &'t Guard<T>);

/// Reference to mutably borrowed guarded value
pub struct RefMut<'t, T: Protectable>(pub &'t mut Guard<T>);

impl<T: Protectable> Guard<T> {
	const ACC: usize = usize::MAX / 2 + 1;
	const REF: usize = !Self::ACC;
	const MUT: usize = usize::MAX & Self::REF;
	const MAX: usize = (usize::MAX & Self::REF) - 1;

	#[inline]
	pub fn from_inner(inner: T) -> Self {
		Self(AtomicUsize::default(), inner)
	}

	fn acquire(&self) -> &Self {
		// Increment ref counter
		let mut refs = self.0.fetch_add(1, Ordering::AcqRel);

		// Panic before overflow
		assert!(refs & Self::REF < Self::MAX);

		if refs == 0 {
			// First acquisition
			self.1.unlock().unwrap();

			// Mark accessible
			self.0.fetch_or(Self::ACC, Ordering::Release);
		} else {
			while refs & Self::ACC == 0 {
				std::thread::yield_now();
				refs = self.0.load(Ordering::Acquire);
			}
		}

		self
	}

	fn release(&self) -> &Self {
		// Last release?
		if self.0.fetch_update(Ordering::AcqRel, Ordering::Acquire,
		|refs| if refs & Self::REF == 1 {
			// Panic on illegal access modification
			debug_assert_ne!(refs & Self::ACC, 0);

			// Mark inaccessible
			Some(refs & !Self::ACC)
		} else {
			// Panic before underflow
			debug_assert!(refs & Self::REF > 0);

			// Decrement ref counter
			Some(refs - 1)
		}).unwrap() & Self::REF == 1 {  // Last release?
		self.0.fetch_update(Ordering::AcqRel, Ordering::Acquire,
		|refs| if likely(refs == 1) {
				// Last release
				self.1.lock().unwrap();

				Some(0)
			} else {
				// Panic on illegal access modification
				debug_assert_eq!(refs & Self::ACC, 0);

				// Reacquisition
				self.1.unlock().unwrap();

				Some((refs - 1) | Self::ACC)
			}).unwrap();
		}

		self
	}

	fn acquire_mut(&mut self) -> &mut Self {
		debug_assert_eq!(self.0.swap(Self::ACC | Self::MUT, Ordering::AcqRel), 0);
		self.1.unlock_mut().unwrap();
		self
	}

	fn release_mut(&mut self) -> &Self {
		debug_assert_eq!(self.0.swap(0, Ordering::AcqRel), Self::ACC | Self::MUT);
		self.1.lock().unwrap();
		self
	}

	#[inline]
	pub(crate) unsafe fn inner(&self) -> &T {
		&self.1
	}

	#[inline]
	pub(crate) unsafe fn inner_mut(&mut self) -> &mut T {
		&mut self.1
	}

	pub(crate) fn mutate<M, R>(&mut self, mut mutation: M) -> &Self
		where M: FnMut(&mut T) -> R {
		debug_assert_eq!(self.0.swap(Self::ACC | Self::MUT, Ordering::AcqRel), 0);
		mutation(&mut self.1);
		debug_assert_eq!(self.0.swap(0, Ordering::AcqRel), Self::ACC | Self::MUT);
		self
	}

	#[inline]
	pub fn borrow(&self) -> Ref<'_, T> {
		Ref(self.acquire())
	}

	#[inline]
	pub fn borrow_mut(&mut self) -> RefMut<'_, T> {
		RefMut(self.acquire_mut())
	}
}

impl<T: Protectable> fmt::Debug for Guard<T> {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		let refs = self.0.load(Ordering::Acquire);

		write!(fmt, "Guard(")?;
		if refs & Self::ACC != 0 {
			write!(fmt, "ACC ")?;
		}

		if refs & Self::REF == Self::MUT {
			write!(fmt, "MUT")?;
		} else {
			write!(fmt, "{}", refs & Self::REF)?;
		}

		write!(fmt, ", {})", std::any::type_name::<T>())
	}
}

impl<T: Protectable> Ref<'_, T> {
	#[inline]
	pub fn inner(&self) -> &T {
		&self.0.1
	}
}

impl<T: Protectable + Deref> Deref for Ref<'_, T> {
	type Target = T::Target;

	#[inline]
	fn deref(&self) -> &Self::Target {
		&*self.0.1
	}
}

impl<T: Protectable + Index<I>, I> Index<I> for Ref<'_, T> {
	type Output = T::Output;

	#[inline]
	fn index(&self, index: I) -> &Self::Output {
		&self.0.1[index]
	}
}

impl<T: Protectable> Drop for Ref<'_, T> {
	#[inline]
	fn drop(&mut self) {
		self.0.release();
	}
}

impl<T: Protectable + fmt::Debug> fmt::Debug for Ref<'_, T> {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt.debug_tuple("Ref").field(&self.0).finish()
	}
}

impl<T: Protectable> RefMut<'_, T> {
	#[inline]
	pub fn inner(&self) -> &T {
		&self.0.1
	}

	#[inline]
	pub fn inner_mut(&mut self) -> &mut T {
		&mut self.0.1
	}
}

impl<T: Protectable + Deref> Deref for RefMut<'_, T> {
	type Target = T::Target;

	#[inline]
	fn deref(&self) -> &Self::Target {
		&*self.0.1
	}
}

impl<T: Protectable + Index<I>, I> Index<I> for RefMut<'_, T> {
	type Output = T::Output;

	#[inline]
	fn index(&self, index: I) -> &Self::Output {
		&self.0.1[index]
	}
}

impl<T: Protectable + Index<I> + IndexMut<I>, I> IndexMut<I> for RefMut<'_, T> {
	#[inline]
	fn index_mut(&mut self, index: I) -> &mut Self::Output {
		&mut self.0.1[index]
	}
}

impl<T: Protectable + Deref + DerefMut> DerefMut for RefMut<'_, T> {
	#[inline]
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut *self.0.1
	}
}

impl<T: Protectable> Drop for RefMut<'_, T> {
	#[inline]
	fn drop(&mut self) {
		self.0.release_mut();
	}
}

impl<T: Protectable + fmt::Debug> fmt::Debug for RefMut<'_, T> {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt.debug_tuple("RefMut").field(&self.0).finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Error;

	struct Dummy;

	impl Protectable for Dummy {
		fn lock(&self) -> Result<(), Error> {
			Ok(())
		}

		fn unlock(&self) -> Result<(), Error> {
			Ok(())
		}

		fn unlock_mut(&mut self) -> Result<(), Error> {
			Ok(())
		}
	}

	#[test]
	#[should_panic]
	fn underflow() {
		let guard = Guard::from_inner(Dummy);
		guard.release();
	}

	#[test]
	#[should_panic]
	fn overflow() {
		let guard = Guard(AtomicUsize::new(Guard::<Dummy>::MAX), Dummy);
		guard.acquire();
	}

	#[test]
	fn immutable() {
		let guard = Guard::from_inner(Dummy);

		for _ in 0..1024 {
			guard.acquire();
		}

		for _ in 0..1024 {
			guard.release();
		}
	}

	#[test]
	fn mutable() {
		let mut guard = Guard::from_inner(Dummy);

		for _ in 0..1024 {
			guard.acquire_mut();
			guard.release_mut();
		}
	}

	#[test]
	#[should_panic]
	fn mutable_multiple() {
		let mut guard = Guard::from_inner(Dummy);
		guard.acquire_mut();
		guard.acquire_mut();
	}

	#[test]
	fn borrow() {
		const LIMIT: usize = 1024;

		let guard = Guard::from_inner(Dummy);

		{
			let mut refs = std::vec::Vec::with_capacity(LIMIT);

			for _ in 0..LIMIT {
				refs.push(guard.borrow());
			}
		}

		assert_eq!(guard.0.into_inner(), 0);
	}

	#[test]
	fn borrow_mut() {
		let mut guard = Guard::from_inner(Dummy);

		{
			guard.borrow_mut();
		}

		assert_eq!(guard.0.into_inner(), 0);
	}

	#[test]
	fn concurrent() {
		use std::cmp::max;
		use std::sync::{Arc, Barrier};
		use std::thread;

		const LIMIT: usize = 262144;

		let guard = Arc::new(Guard::from_inner(Dummy));
		let concurrency = max(16, 2 * thread::available_parallelism().unwrap().get());
		let barrier = Arc::new(Barrier::new(concurrency));
		let mut threads = std::vec::Vec::with_capacity(concurrency);

		for _ in 0..concurrency {
			let barrier = barrier.clone();
			let guard = guard.clone();

			threads.push(thread::spawn(move || {
				barrier.wait();

				for _ in 0..LIMIT {
					guard.borrow();
					thread::yield_now();
				}
			}));
		}

		for thread in threads {
			thread.join().unwrap();
		}

		assert_eq!(Arc::try_unwrap(guard).unwrap().0.into_inner(), 0);
	}
}
