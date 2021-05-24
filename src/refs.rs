use std::ops::{Fn, FnOnce};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default, Debug)]
pub struct RefCount(AtomicUsize);

impl RefCount {
	const ACC: usize = usize::MAX / 2 + 1;
	const REF: usize = !Self::ACC;
	const MUT: usize = usize::MAX & Self::REF;
	const MAX: usize = (usize::MAX & Self::REF) - 1;

	pub fn acquire<A>(&self, acquire: A)
		where A: FnOnce() {
		// Increment ref counter
		let mut refs = self.0.fetch_add(1, Ordering::AcqRel);

		// Panic before overflow
		assert!(refs & Self::REF < Self::MAX);

		if refs == 0 {
			// First acquisition
			acquire();

			// Mark accessible
			self.0.fetch_or(Self::ACC, Ordering::Release);
		} else {
			while refs & Self::ACC == 0 {
				std::thread::yield_now();
				refs = self.0.load(Ordering::Acquire);
			}
		}
	}

	pub fn release<R, A>(&self, release: R, acquire: A)
		where R: Fn(), A: Fn() {
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
				release();

				Some(0)
			} else {
				// Panic on illegal access modification
				debug_assert_eq!(refs & Self::ACC, 0);

				// Reacquisition
				acquire();

				Some((refs - 1) | Self::ACC)
			}).unwrap();
		}
	}

	pub fn acquire_mut<A>(&self, acquire: A)
		where A: FnOnce() {
		debug_assert_eq!(self.0.swap(Self::ACC | Self::MUT, Ordering::AcqRel), 0);
		acquire();
	}

	pub fn release_mut<A>(&self, release: A)
		where A: FnOnce() {
		debug_assert_eq!(self.0.swap(0, Ordering::AcqRel), Self::ACC | Self::MUT);
		release();
	}

	pub fn mutate<M>(&self, mutate: M)
		where M: FnOnce() {
		debug_assert_eq!(self.0.swap(Self::ACC | Self::MUT, Ordering::AcqRel), 0);
		mutate();
		debug_assert_eq!(self.0.swap(0, Ordering::AcqRel), Self::ACC | Self::MUT);
	}
}
