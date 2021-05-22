use crate::pages::*;

use std::alloc::{Allocator, AllocError, Layout, handle_alloc_error};
use std::ptr::NonNull;

pub struct Sensitive;

unsafe impl Allocator for Sensitive {
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		// Refuse allocation if alignment requirement exceeds allocation granularity
		if layout.align() >= *GRANULARITY {
			return Err(AllocError);
		}

		// Allocate size + two guard allocations
		let size = alloc_align(layout.size());
		let full = size + 2 * *GRANULARITY;

		let addr = unsafe { allocate(full, Protection::NoAccess).or(Err(AllocError))? };
		let base = unsafe { addr.add(*GRANULARITY) };

		if size > 0 {
			// Attempt to lock memory
			let _ = unsafe { lock(base, size) };

			// Allow read‐write access
			if unsafe { protect(base, size, Protection::ReadWrite).is_err() } {
				let _ = unsafe { release(addr, full) };
				return Err(AllocError);
			}
		}

		Ok(NonNull::slice_from_raw_parts(unsafe { NonNull::new_unchecked(base) }, size))
	}

	fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		self.allocate(layout)
	}

	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		debug_assert!(layout.align() <= *GRANULARITY);

		let size = alloc_align(layout.size());
		let full = size + 2 * *GRANULARITY;

		// Zero memory before returning to OS
		zero(ptr.as_ptr(), layout.size());

		let addr = ptr.as_ptr().sub(*GRANULARITY);

		if release(addr, full).is_err() {
			handle_alloc_error(layout);
		}
	}

	unsafe fn shrink(&self, ptr: NonNull<u8>, old: Layout, new: Layout) -> Result<NonNull<[u8]>, AllocError> {
		// Refuse allocation if alignment requirement exceeds allocation granularity
		if new.align() >= *GRANULARITY {
			return Err(AllocError);
		}

		// Zero memory before shrinking
		zero(ptr.as_ptr().add(new.size()), old.size() - new.size());

		// Uncommit pages as needed
		let size_old = alloc_align(old.size());
		let size_new = alloc_align(new.size());

		if size_old - size_new >= *GRANULARITY {
			let tail = ptr.as_ptr().add(size_new);
			let diff = size_old - size_new;

			// Uncommit pages and protect new guard allocation
			if uncommit(tail.add(*GRANULARITY), diff).is_err()
				|| protect(tail, *GRANULARITY, Protection::NoAccess).is_err() {
				handle_alloc_error(new);
			}
		}

		Ok(NonNull::slice_from_raw_parts(ptr, size_new))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn test_raw_range(range: std::ops::Range<usize>, samples: usize) {
		use rand::distributions::{Distribution, Uniform};

		let mut rng = rand::thread_rng();
		let dist = Uniform::from(range);

		for _ in 0..samples {
			let size = dist.sample(&mut rng);

			eprintln!("Allocating {} bytes", size);

			let layout = Layout::from_size_align(size, 1).unwrap();
			let alloc = Sensitive.allocate(layout).unwrap();

			for i in 0..size {
				let ptr = unsafe { alloc.cast::<u8>().as_ptr().add(i) };
				assert_eq!(unsafe { ptr.read() }, 0);
				unsafe { ptr.write(0x55) };
				assert_eq!(unsafe { ptr.read() }, 0x55);
			}

			unsafe { Sensitive.deallocate(alloc.cast::<u8>(), layout); }
		}
	}

	#[test]
	fn test_raw_tiny() {
		test_raw_range(0..4096, 4096);
	}

	#[test]
	fn test_raw_small() {
		test_raw_range(4096..65536, 256);
	}

	#[test]
	fn test_raw_medium() {
		test_raw_range(65536..4194304, 64);
	}

	#[test]
	fn test_raw_large() {
		test_raw_range(4194304..16777216, 16);
	}

	#[test]
	fn test_raw_huge() {
		test_raw_range(4194304..268435456, 4);
	}

	#[cfg(unix)]
	#[test]
	fn test_raw_guard() {
		use bulletproof::Bulletproof;

		let size = alloc_align(4194304);

		let bp = unsafe { Bulletproof::new() };
		let layout = Layout::from_size_align(size, 1).unwrap();
		let alloc = Sensitive.allocate(layout).unwrap();
		let ptr = alloc.cast::<u8>().as_ptr();

		// Preceding guard
		for i in 1..=*GRANULARITY {
			assert_eq!(unsafe { bp.load(ptr.sub(i)) }, Err(()));
		}

		for i in 0..size {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// Trailing guard
		for i in size + 1 .. *GRANULARITY {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}

		unsafe { Sensitive.deallocate(alloc.cast::<u8>(), layout); }
	}

	#[cfg(unix)]
	#[test]
	fn test_raw_shrink() {
		use bulletproof::Bulletproof;

		let size = 2 * *GRANULARITY;

		let bp = unsafe { Bulletproof::new() };
		let layout_0 = Layout::from_size_align(size, 1).unwrap();
		let alloc_0 = Sensitive.allocate(layout_0).unwrap();
		let ptr = alloc_0.cast::<u8>().as_ptr();

		for i in 0..size {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// Original guard
		for i in size + 1 .. *GRANULARITY {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}

		let layout_1 = Layout::from_size_align(size / 2, 1).unwrap();
		let alloc_1 = unsafe {
			Sensitive.shrink(alloc_0.cast::<u8>(), layout_0, layout_1)
		}.unwrap();

		// Allocation should not move
		assert_eq!(alloc_0.cast::<u8>(), alloc_1.cast::<u8>());

		for i in 0 .. size / 2 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// New guard
		for i in size / 2 + 1 .. *GRANULARITY {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}

		unsafe { Sensitive.deallocate(alloc_1.cast::<u8>(), layout_1); }
	}

	#[test]
	fn test_vec_seq() {
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
	fn test_vec_rng() {
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
