use crate::traits::Protectable;

use std::default::Default;
use std::fmt;
use std::ops::{Deref, DerefMut, Index, IndexMut, Drop};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Guard<T: Protectable>(AtomicUsize, T);
pub struct Ref<'t, T: Protectable>(pub &'t Guard<T>);
pub struct RefMut<'t, T: Protectable>(pub &'t mut Guard<T>);

impl<T: Protectable> Guard<T> {
	const ACC: usize = usize::MAX / 2 + 1;
	const REF: usize = !Self::ACC;
	const MUT: usize = usize::MAX & Self::REF;
	const MAX: usize = (usize::MAX & Self::REF) - 1;

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
		|refs| if refs == 1 {
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

	pub unsafe fn inner(&self) -> &T {
		&self.1
	}

	pub unsafe fn inner_mut(&mut self) -> &mut T {
		&mut self.1
	}

	pub fn mutate<M, R>(&mut self, mut mutation: M) -> &Self
		where M: FnMut(&mut T) -> R {
		debug_assert_eq!(self.0.swap(Self::ACC | Self::MUT, Ordering::AcqRel), 0);
		mutation(&mut self.1);
		debug_assert_eq!(self.0.swap(0, Ordering::AcqRel), Self::ACC | Self::MUT);
		self
	}

	pub fn borrow(&self) -> Ref<'_, T> {
		Ref(self.acquire())
	}

	pub fn borrow_mut(&mut self) -> RefMut<'_, T> {
		RefMut(self.acquire_mut())
	}
}

impl<T: Protectable + Default> Default for Guard<T> {
	fn default() -> Self {
		Self(AtomicUsize::default(), T::default())
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
	pub fn inner(&self) -> &T {
		&self.0.1
	}
}

impl<T: Protectable + Deref> Deref for Ref<'_, T> {
	type Target = T::Target;

	fn deref(&self) -> &Self::Target {
		&*self.0.1
	}
}

impl<T: Protectable + Index<I>, I> Index<I> for Ref<'_, T> {
	type Output = T::Output;

	fn index(&self, index: I) -> &Self::Output {
		&self.0.1[index]
	}
}

impl<T: Protectable> Drop for Ref<'_, T> {
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
	pub fn inner(&self) -> &T {
		&self.0.1
	}

	pub fn inner_mut(&mut self) -> &mut T {
		&mut self.0.1
	}
}

impl<T: Protectable + Deref> Deref for RefMut<'_, T> {
	type Target = T::Target;

	fn deref(&self) -> &Self::Target {
		&*self.0.1
	}
}

impl<T: Protectable + Index<I>, I> Index<I> for RefMut<'_, T> {
	type Output = T::Output;

	fn index(&self, index: I) -> &Self::Output {
		&self.0.1[index]
	}
}

impl<T: Protectable + Index<I> + IndexMut<I>, I> IndexMut<I> for RefMut<'_, T> {
	fn index_mut(&mut self, index: I) -> &mut Self::Output {
		&mut self.0.1[index]
	}
}

impl<T: Protectable + Deref + DerefMut> DerefMut for RefMut<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut *self.0.1
	}
}

impl<T: Protectable> Drop for RefMut<'_, T> {
	fn drop(&mut self) {
		self.0.release_mut();
	}
}

impl<T: Protectable + fmt::Debug> fmt::Debug for RefMut<'_, T> {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt.debug_tuple("RefMut").field(&self.0).finish()
	}
}
