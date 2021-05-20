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

		// Attempt to lock memory
		let _ = unsafe { lock(base, size) };

		// Allow read‐write access
		if unsafe { protect(base, size, Protection::ReadWrite).is_err() } {
			let _ = unsafe { release(addr, full) };
			return Err(AllocError);
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
		const LIMIT: usize = 4194304;

		use rand::prelude::*;

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
