use crate::auxiliary::zero;
use crate::pages::*;

use std::alloc::{Allocator, AllocError, Layout, handle_alloc_error};
use std::ptr::NonNull;

pub struct Sensitive;

impl Sensitive {
	pub const GUARD_PAGES: usize = 1;

	pub fn guard_size() -> usize {
		Self::GUARD_PAGES * page_size()
	}

	pub fn outer_size(size: usize) -> usize {
		alloc_align(size + 2 * Self::guard_size())
	}

	pub fn inner_size(size: usize) -> usize {
		Self::outer_size(size) - 2 * Self::guard_size()
	}
}

unsafe impl Allocator for Sensitive {
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		// Refuse allocation if alignment requirement exceeds page size
		if layout.align() >= page_size() {
			return Err(AllocError);
		}

		// Allocate size + two guard pages
		let outer = Self::outer_size(layout.size());
		let inner = Self::inner_size(layout.size());

		let addr = unsafe { allocate(outer, Protection::NoAccess).or(Err(AllocError))? };
		let base = unsafe { addr.add(Self::guard_size()) };

		if inner > 0 {
			// Attempt to lock memory
			let _ = unsafe { lock(base, inner) };

			// Allow read‐write access
			if unsafe { protect(base, inner, Protection::ReadWrite).is_err() } {
				let _ = unsafe { release(addr, outer) };
				return Err(AllocError);
			}
		}

		Ok(NonNull::slice_from_raw_parts(unsafe { NonNull::new_unchecked(base) }, inner))
	}

	fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		self.allocate(layout)
	}

	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		debug_assert!(layout.align() <= page_size());

		let outer = Self::outer_size(layout.size());

		if layout.size() > 0 {
			// Allow read‐write access before zeroing
			if protect(ptr.as_ptr(), page_align(layout.size()), Protection::ReadWrite).is_err() {
				handle_alloc_error(layout);
			}

			// Zero memory before returning to OS
			zero(ptr.as_ptr(), layout.size());
		}

		let addr = ptr.as_ptr().sub(Self::guard_size());

		if release(addr, outer).is_err() {
			handle_alloc_error(layout);
		}
	}

	unsafe fn shrink(&self, ptr: NonNull<u8>, old: Layout, new: Layout) -> Result<NonNull<[u8]>, AllocError> {
		// Refuse allocation if alignment requirement exceeds page size
		if new.align() >= page_size() {
			return Err(AllocError);
		}

		debug_assert!(new.size() < old.size());

		// Uncommit pages as needed
		let inner_old = page_align(old.size());
		let inner_new = page_align(new.size());

		if inner_old - inner_new >= page_size() {
			let tail = ptr.as_ptr().add(inner_new);
			let diff = inner_old - inner_new;

			// Allow read‐write access before zeroing
			if protect(tail, diff + Self::guard_size(), Protection::ReadWrite).is_err() {
				handle_alloc_error(new);
			}

			// Zero memory before uncommiting
			zero(tail, diff + Self::guard_size());

			// Uncommit pages and protect new guard page
			if uncommit(tail.add(Self::guard_size()), diff).is_err()
				|| protect(tail, Self::guard_size(), Protection::NoAccess).is_err() {
				handle_alloc_error(new);
			}
		}

		Ok(NonNull::slice_from_raw_parts(ptr, inner_new))
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

	#[cfg(target_os = "Linux")]
	#[test]
	fn raw_guard() {
		use bulletproof::Bulletproof;

		let size = alloc_align(4194304);

		let bp = unsafe { Bulletproof::new() };
		let layout = Layout::from_size_align(size, 1).unwrap();
		let alloc = Sensitive.allocate(layout).unwrap();
		let ptr = alloc.cast::<u8>().as_ptr();

		// Preceding guard
		for i in 1..=Sensitive::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.sub(i)) }, Err(()));
		}

		for i in 0..size {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// Trailing guard
		for i in size + 1 .. Sensitive::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}

		unsafe { Sensitive.deallocate(alloc.cast::<u8>(), layout); }
	}

	#[cfg(target_os = "linux")]
	#[test]
	fn raw_shrink() {
		use bulletproof::Bulletproof;

		let size = granularity() + Sensitive::guard_size();

		let bp = unsafe { Bulletproof::new() };
		let layout_0 = Layout::from_size_align(size, 1).unwrap();
		let alloc_0 = Sensitive.allocate(layout_0).unwrap();
		let ptr = alloc_0.cast::<u8>().as_ptr();

		for i in 0..size {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// Original guard
		for i in size + 1 .. Sensitive::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}

		let layout_1 = Layout::from_size_align(size - Sensitive::guard_size(), 1).unwrap();
		let alloc_1 = unsafe {
			Sensitive.shrink(alloc_0.cast::<u8>(), layout_0, layout_1)
		}.unwrap();

		// Allocation should not move
		assert_eq!(alloc_0.cast::<u8>(), alloc_1.cast::<u8>());

		for i in 0 .. size / 2 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// New guard
		for i in size / 2 + 1 .. Sensitive::guard_size() {
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
