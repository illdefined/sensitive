use crate::auxiliary::zero;
use crate::pages::{Pages, GuardedAlloc, Protection};

use std::alloc::{Allocator, AllocError, Layout, handle_alloc_error};
use std::ptr::NonNull;

pub struct Sensitive;

impl Sensitive {
	pub(crate) const GUARD_PAGES: usize = 1;
}

unsafe impl Allocator for Sensitive {
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		// Refuse allocation if alignment requirement exceeds page size
		if layout.align() >= Pages::granularity() {
			return Err(AllocError);
		}

		let alloc = GuardedAlloc::<{ Sensitive::GUARD_PAGES }>::new(layout.size(), Protection::ReadWrite).map_err(|_| AllocError)?;

		if !alloc.inner().is_empty() {
			// Attempt to lock memory
			let _ = alloc.inner().lock();
		}

		Ok(alloc.into_slice())
	}

	fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		self.allocate(layout)
	}

	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		debug_assert!(layout.align() <= Pages::granularity());

		let alloc = GuardedAlloc::<{ Self::GUARD_PAGES }>::from_raw_parts(ptr, layout.size());

		if !alloc.inner().is_empty() {
			// Allow read‐write access before zeroing
			if alloc.inner().protect(Protection::ReadWrite).is_err() {
				handle_alloc_error(layout);
			}

			// Zero memory before returning to OS
			zero(ptr.as_ptr(), layout.size());
		}
	}

	unsafe fn shrink(&self, ptr: NonNull<u8>, old: Layout, new: Layout) -> Result<NonNull<[u8]>, AllocError> {
		// Refuse allocation if alignment requirement exceeds page size
		if new.align() >= Pages::granularity() {
			return Err(AllocError);
		}

		debug_assert!(new.size() < old.size());
		let mut alloc = GuardedAlloc::<{ Self::GUARD_PAGES }>::from_raw_parts(ptr, old.size());

		// Uncommit pages as needed
		let inner_old = GuardedAlloc::<{ Self::GUARD_PAGES }>::inner_size(old.size());
		let inner_new = GuardedAlloc::<{ Self::GUARD_PAGES }>::inner_size(new.size());

		if inner_old - inner_new > 0 {
			let tail = ptr.as_ptr().add(inner_new);
			let diff = inner_old - inner_new;

			// Allow read‐write access before zeroing
			if Pages::from_ptr(tail, diff + GuardedAlloc::<{ Self::GUARD_PAGES }>::guard_size())
			.protect(Protection::ReadWrite).is_err() {
				handle_alloc_error(new);
			}

			// Zero memory before uncommiting
			zero(tail, diff + GuardedAlloc::<{ Self::GUARD_PAGES }>::guard_size());

			// Shrink allocation
			alloc = match alloc.shrink(GuardedAlloc::<{ Self::GUARD_PAGES }>::outer_size(inner_new)) {
				Ok(alloc) => alloc,
				Err(_) => handle_alloc_error(new)
			}
		}

		Ok(alloc.into_slice())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn raw_range(range: std::ops::Range<usize>, samples: usize) {
		use rand::distributions::{Distribution, Uniform};

		let mut rng = rand::thread_rng();
		let dist = Uniform::from(range);

		for _ in 0..samples {
			let size = dist.sample(&mut rng);

			eprintln!("Allocating {} bytes", size);

			let layout = Layout::from_size_align(size, 1).unwrap();
			let alloc = Sensitive.allocate(layout).unwrap();

			for i in 0..size / 2 {
				let ptr = unsafe { alloc.cast::<u8>().as_ptr().add(i) };
				assert_eq!(unsafe { ptr.read() }, 0);
				unsafe { ptr.write(0x55) };
				assert_eq!(unsafe { ptr.read() }, 0x55);
			}

			unsafe { Sensitive.deallocate(alloc.cast::<u8>(), layout); }
		}
	}

	#[test]
	fn raw_tiny() {
		raw_range(0..4096, 4096);
	}

	#[test]
	fn raw_small() {
		raw_range(4096..65536, 256);
	}

	#[test]
	fn raw_medium() {
		raw_range(65536..4194304, 64);
	}

	#[test]
	fn raw_large() {
		raw_range(4194304..16777216, 16);
	}

	#[test]
	fn raw_huge() {
		raw_range(4194304..268435456, 4);
	}

	#[cfg(target_os = "linux")]
	#[test]
	fn raw_guard() {
		use crate::pages::Allocation;
		use bulletproof::Bulletproof;

		let size = Allocation::align(4194304);

		let bp = unsafe { Bulletproof::new() };
		let layout = Layout::from_size_align(size, 1).unwrap();
		let alloc = Sensitive.allocate(layout).unwrap();
		let ptr = alloc.cast::<u8>().as_ptr();

		// Preceding guard
		for i in 1..=GuardedAlloc::<{ Sensitive::GUARD_PAGES }>::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.sub(i)) }, Err(()));
		}

		for i in 0..size {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// Trailing guard
		for i in size + 1 .. GuardedAlloc::<{ Sensitive::GUARD_PAGES }>::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}

		unsafe { Sensitive.deallocate(alloc.cast::<u8>(), layout); }
	}

	#[cfg(target_os = "linux")]
	#[test]
	fn raw_shrink() {
		use crate::pages::Allocation;
		use bulletproof::Bulletproof;

		let size_0 = std::cmp::max(Allocation::granularity(), 2 * GuardedAlloc::<{ Sensitive::GUARD_PAGES }>::guard_size());

		let bp = unsafe { Bulletproof::new() };
		let layout_0 = Layout::from_size_align(size_0, 1).unwrap();
		let alloc_0 = Sensitive.allocate(layout_0).unwrap();
		let ptr = alloc_0.cast::<u8>().as_ptr();

		for i in 0..size_0 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// Original guard
		for i in size_0 + 1 .. GuardedAlloc::<{ Sensitive::GUARD_PAGES }>::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}

		let size_1 = size_0 - GuardedAlloc::<{ Sensitive::GUARD_PAGES }>::guard_size();

		let layout_1 = Layout::from_size_align(size_1, 1).unwrap();
		let alloc_1 = unsafe {
			Sensitive.shrink(alloc_0.cast::<u8>(), layout_0, layout_1)
		}.unwrap();

		// Allocation should not move
		assert_eq!(alloc_0.cast::<u8>(), alloc_1.cast::<u8>());

		for i in 0 .. size_1 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// New guard
		for i in size_1 + 1 .. GuardedAlloc::<{ Sensitive::GUARD_PAGES }>::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}

		unsafe { Sensitive.deallocate(alloc_1.cast::<u8>(), layout_1); }
	}

	#[test]
	fn vec_seq() {
		const LIMIT: usize = 4194304;

		let mut test: Vec<usize, _> = Vec::new_in(Sensitive);

		for i in 0..LIMIT {
			test.push(i);
		}

		for i in 0..LIMIT {
			assert_eq!(test[i], i);
		}
	}

	#[test]
	fn vec_rng() {
		use rand::prelude::*;

		const LIMIT: usize = 4194304;

		let mut rng = rand::thread_rng();
		let mut test: Vec<u8, _> = Vec::new_in(Sensitive);

		for i in 0..LIMIT {
			let rand = rng.gen();

			test.push(rand);
			assert_eq!(test[i], rand);
		}

		for _ in 0..LIMIT {
			assert!(test.pop().is_some());
			test.shrink_to_fit();
		}
	}
}
