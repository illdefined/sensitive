use libc::c_int;
use std::alloc::{Allocator, AllocError, Layout, handle_alloc_error};
use std::convert::TryInto;
use std::ffi::c_void;
use std::ptr::{self, NonNull};

pub struct Sensitive;

lazy_static! {
	static ref PAGE_SIZE: usize = unsafe { libc::sysconf(libc::_SC_PAGE_SIZE).try_into().unwrap() };
}

impl Sensitive {
	fn align(offset: usize, align: usize) -> usize {
		debug_assert!(align != 0 && (align & (align - 1)) == 0);

		(offset + (align - 1)) & !(align - 1)
	}

	/// Align offset on page boundary
	fn page_align(offset: usize) -> usize {
		Self::align(offset, *PAGE_SIZE)
	}

	fn mmap_anonymous(size: usize) -> Result<*mut u8, AllocError> {
		match unsafe {
			libc::mmap(ptr::null_mut(), size, libc::PROT_NONE, libc::MAP_PRIVATE | libc::MAP_ANON, -1, 0)
		} {
			libc::MAP_FAILED => Err(AllocError),
			addr => Ok(addr as *mut u8),
		}
	}

	fn munmap(addr: *mut u8, size: usize) -> Result<(), AllocError> {
		match unsafe { libc::munmap(addr as *mut c_void, size) } {
			0 => Ok(()),
			_ => Err(AllocError),
		}
	}

	fn mprotect(addr: *mut u8, size: usize, prot: c_int) -> Result<(), AllocError> {
		match unsafe { libc::mprotect(addr as *mut c_void, size, prot) } {
			0 => Ok(()),
			_ => Err(AllocError),
		}
	}

	fn mlock(addr: *mut u8, size: usize) -> Result<(), AllocError> {
		match unsafe { libc::mlock(addr as *mut c_void, size) } {
			0 => Ok(()),
			_ => Err(AllocError),
		}
	}
}

unsafe impl Allocator for Sensitive {
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		// Refuse allocation if alignment requirement exceeds page size
		if layout.align() >= *PAGE_SIZE {
			return Err(AllocError);
		}

		// Allocate size + two guard pages
		let size = Self::page_align(layout.size());
		let full = size + 2 * *PAGE_SIZE;

		let addr = Self::mmap_anonymous(full)?;
		let base = unsafe { addr.add(*PAGE_SIZE) };

		// Attempt to lock memory
		let _ = Self::mlock(base, size);

		// Allow read‐write access
		if Self::mprotect(base, size, libc::PROT_READ | libc::PROT_WRITE).is_err() {
			Self::munmap(addr, full)?;
			return Err(AllocError);
		}

		Ok(NonNull::slice_from_raw_parts(unsafe { NonNull::new_unchecked(base) }, size))
	}

	fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		self.allocate(layout)
	}

	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		debug_assert!(layout.align() <= *PAGE_SIZE);

		let size = Self::page_align(layout.size());
		let full = size + 2 * *PAGE_SIZE;

		// Zero memory before returning to OS
		std::intrinsics::volatile_set_memory(ptr.as_ptr(), 0, layout.size());

		let addr = ptr.as_ptr().sub(*PAGE_SIZE);

		if Self::munmap(addr, full).is_err() {
			handle_alloc_error(layout);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn align() {
		assert_eq!(Sensitive::align(0, 4096), 0);

		for i in 1..4096 {
			assert_eq!(Sensitive::align(i, 4096), 4096);
		}
	}

	#[test]
	fn test_vec() {
		use rand::prelude::*;

		let mut rng = rand::thread_rng();
		let mut test: Vec<u8, _> = Vec::new_in(Sensitive);

		for i in 0..4194304 {
			let rand = rng.gen();

			test.push(rand);
			assert_eq!(test[i], rand);
		}
	}
}
