//! Memory page functions

use crate::traits::{AsPages, Protectable};

use std::convert::TryInto;
use std::intrinsics::likely;
use std::io::Error;
use std::marker::PhantomData;
use std::mem::{MaybeUninit, ManuallyDrop};
use std::ops::Range;
use std::ptr::{self, NonNull};
use std::sync::Once;

#[cfg(windows)]
use winapi::um::winnt;

#[cfg(unix)]
use std::ffi::c_void;

#[cfg(windows)]
use winapi::ctypes::c_void;

#[cfg(doc)]
/// Page access protection
pub enum Protection {
	/// Pages may not be accessed
	NoAccess,

	/// Pages may be read
	ReadOnly,

	/// Pages may be read and written
	ReadWrite
}

#[cfg(all(unix, not(doc)))]
#[repr(i32)]
pub enum Protection {
	NoAccess = libc::PROT_NONE,
	ReadOnly = libc::PROT_READ,
	ReadWrite = libc::PROT_READ | libc::PROT_WRITE,
}

#[cfg(all(windows, not(doc)))]
#[repr(u32)]
pub enum Protection {
	NoAccess = winnt::PAGE_NOACCESS,
	ReadOnly = winnt::PAGE_READONLY,
	ReadWrite = winnt::PAGE_READWRITE,
}

/// Memory pages
#[must_use]
#[derive(Debug)]
pub struct Pages<'t>(NonNull<[u8]>, PhantomData<&'t ()>);

/// Memory page allocation
#[must_use]
#[derive(Debug)]
pub struct Allocation(NonNull<[u8]>);

/// Guarded memory page allocation
#[must_use]
#[derive(Debug)]
pub struct GuardedAlloc<const N: usize = 1>(Allocation);

static INIT: Once = Once::new();

static mut PAGE_SIZE: MaybeUninit<usize> = MaybeUninit::uninit();

#[cfg(windows)]
static mut GRANULARITY: MaybeUninit<usize> = MaybeUninit::uninit();

fn init() {
	#[cfg(unix)]
	INIT.call_once(|| {
		use libc::{sysconf, _SC_PAGESIZE};

		let pg = unsafe { sysconf(_SC_PAGESIZE) };
		assert!(pg > 0);

		unsafe { PAGE_SIZE.write(pg.try_into().unwrap()); }
	});

	#[cfg(windows)]
	INIT.call_once(|| {
		use winapi::um::sysinfoapi::{SYSTEM_INFO, GetSystemInfo};

		let mut si = MaybeUninit::<SYSTEM_INFO>::uninit();
		unsafe { GetSystemInfo(si.as_mut_ptr()); }

		unsafe { PAGE_SIZE.write(si.assume_init().dwPageSize.try_into().unwrap()) };
		unsafe { GRANULARITY.write(si.assume_init().dwAllocationGranularity.try_into().unwrap()) };
	});
}

impl<'t> Pages<'t> {
	#[must_use]
	pub fn granularity() -> usize {
		init();

		unsafe { PAGE_SIZE.assume_init() }
	}

	#[must_use]
	pub fn align(offset: usize) -> usize {
		offset.next_multiple_of(Self::granularity())
	}

	/// Create [`Pages`] from [non‐null](NonNull) raw [`u8`] slice
	///
	/// # Safety
	///
	/// `slice` must be aligned to and have a size that is a multiple of the [page size](Self::granularity).
	pub unsafe fn from_slice(slice: NonNull<[u8]>) -> Pages<'t> {
		// Assert correct alignment
		debug_assert_eq!(slice.as_ptr().cast::<u8>().align_offset(Self::granularity()), 0);
		debug_assert_eq!(Self::align(slice.len()), slice.len());

		Self(slice, PhantomData)
	}

	/// Create [`Pages`] from [non‐null](NonNull) [`u8`] pointer and length
	///
	/// # Safety
	///
	/// `ptr` must be aligned to a multiple of the [page size](Self::granularity) and `size` must be a multiple of the
	/// [page size](Self::granularity).
	pub unsafe fn from_raw_parts(ptr: NonNull<u8>, size: usize) -> Pages<'t> {
		Self::from_slice(NonNull::slice_from_raw_parts(ptr, size))
	}

	/// Create [`Pages`] from pointer and length
	///
	/// # Safety
	///
	/// `ptr` must be non‐null and aligned to a multiple of the [page size](Self::granularity) and `size` must be a
	/// multiple of the [page size](Self::granularity).
	///
	/// # Panics
	///
	/// May panic if `ptr` is null or if `ptr` or `size` are not properly aligned.
	pub unsafe fn from_ptr<T>(ptr: *mut T, size: usize) -> Pages<'t> {
		Self::from_raw_parts(NonNull::new(ptr.cast::<u8>()).unwrap(), size)
	}

	#[must_use]
	pub fn as_ptr<T>(&self) -> *mut T {
		debug_assert!(std::mem::align_of::<T>() < Self::granularity());
		self.0.as_ptr().cast::<T>()
	}

	#[must_use] #[inline]
	pub const fn into_slice(self) -> NonNull<[u8]> {
		self.0
	}

	#[must_use] #[inline]
	pub fn size(&self) -> usize {
		self.0.len()
	}

	#[must_use]
	pub fn len(&self) -> usize {
		self.size() / Self::granularity()
	}

	#[must_use] #[inline]
	pub fn is_empty(&self) -> bool {
		self.size() == 0
	}

	#[allow(clippy::missing_errors_doc)]
	pub fn protect(&self, prot: Protection) -> Result<(), Error> {
		#[cfg(unix)] {
			use libc::mprotect;
			use std::os::raw::c_int;

			match unsafe { mprotect(self.as_ptr::<c_void>(), self.0.len(), prot as c_int) } {
				0 => Ok(()),
				_ => Err(Error::last_os_error()),
			}
		}

		#[cfg(windows)] {
			use winapi::shared::minwindef::DWORD;
			use winapi::um::memoryapi::VirtualProtect;

			let mut old = MaybeUninit::<DWORD>::uninit();
			match unsafe { VirtualProtect(self.as_ptr::<c_void>(), self.0.len(), prot as DWORD, old.as_mut_ptr()) } {
				0 => Err(Error::last_os_error()),
				_ => Ok(()),
			}
		}
	}

	#[allow(clippy::missing_errors_doc)]
	pub fn lock(&self) -> Result<(), Error> {
		#[cfg(unix)] {
			use libc::mlock;

			match unsafe { mlock(self.as_ptr::<c_void>(), self.0.len()) } {
				0 => Ok(()),
				_ => Err(Error::last_os_error()),
			}
		}

		#[cfg(windows)] {
			use winapi::um::memoryapi::VirtualLock;

			match unsafe { VirtualLock(self.as_ptr::<c_void>(), self.0.len()) } {
				0 => Err(Error::last_os_error()),
				_ => Ok(()),
			}
		}
	}

	#[allow(clippy::missing_errors_doc)]
	pub fn unlock(&self) -> Result<(), Error> {
		#[cfg(unix)] {
			use libc::munlock;

			match unsafe { munlock(self.as_ptr::<c_void>(), self.0.len()) } {
				0 => Ok(()),
				_ => Err(Error::last_os_error()),
			}
		}

		#[cfg(windows)] {
			use winapi::um::memoryapi::VirtualUnlock;

			match unsafe { VirtualUnlock(self.as_ptr::<c_void>(), self.0.len()) } {
				0 => Err(Error::last_os_error()),
				_ => Ok(()),
			}
		}
	}

	#[must_use]
	pub fn pages(&'t self, range: Range<usize>) -> Option<Pages<'t>> {
		if likely(range.start < self.len() && range.end <= self.len()) {
			Some(unsafe {
				Self::from_ptr(self.as_ptr::<u8>().add(range.start * Self::granularity()),
				(range.end - range.start - 1) * Self::granularity())
			})
		} else {
			None
		}
	}
}

impl Allocation {
	#[must_use]
	pub fn granularity() -> usize {
		#[cfg(unix)] {
			Pages::granularity()
		}

		#[cfg(windows)] {
			init();

			unsafe { GRANULARITY.assume_init() }
		}
	}

	#[must_use]
	pub fn align(offset: usize) -> usize {
		offset.next_multiple_of(Self::granularity())
	}

	/// Create [`Allocation`] from [non‐null](NonNull) raw [`u8`] slice
	///
	/// # Safety
	///
	/// `slice` must have been previously generated by a call to [`into_slice`](Self::into_slice) and must not be
	/// aliased by any other [`Allocation`].
	pub unsafe fn from_slice(slice: NonNull<[u8]>) -> Self {
		// Assert correct alignment
		debug_assert_eq!(slice.as_ptr().cast::<u8>().align_offset(Self::granularity()), 0);
		debug_assert_eq!(Self::align(slice.len()), slice.len());

		Self(slice)
	}

	/// Create [`Allocation`] from [non‐null](NonNull) [`u8`] pointer and length
	///
	/// # Safety
	///
	/// `ptr` must have been previously generated by a call to [`into_ptr`](Self::into_ptr) and must not be aliased
	/// by any other [`Allocation`]. `size` must match the size of the original `Allocation`.
	pub unsafe fn from_raw_parts(ptr: NonNull<u8>, size: usize) -> Self {
		Self::from_slice(NonNull::slice_from_raw_parts(ptr, size))
	}

	/// Create [`Allocation`] from pointer and length
	///
	/// # Safety
	///
	/// `ptr` must have been previously generated by a call to [`into_ptr`](Self::into_ptr) and must not be aliased
	/// by any other [`Allocation`]. `size` must match the size of the original `Allocation`.
	///
	/// # Panics
	///
	/// May panic if `ptr` is null or `ptr` or `size` are not properly aligned.
	pub unsafe fn from_ptr<T>(ptr: *mut T, size: usize) -> Self {
		Self::from_raw_parts(NonNull::new(ptr.cast::<u8>()).unwrap(), size)
	}

	#[must_use]
	pub fn as_ptr<T>(&self) -> *mut T {
		debug_assert!(std::mem::align_of::<T>() < Self::granularity());
		self.0.as_ptr().cast::<T>()
	}

	#[must_use] #[inline]
	pub fn into_ptr<T>(self) -> *mut T {
		ManuallyDrop::new(self).as_ptr()
	}

	#[must_use] #[inline]
	pub fn into_slice(self) -> NonNull<[u8]> {
		ManuallyDrop::new(self).0
	}

	#[must_use] #[inline]
	pub fn size(&self) -> usize {
		self.0.len()
	}

	#[must_use]
	pub fn len(&self) -> usize {
		self.size() / Pages::granularity()
	}

	#[must_use] #[inline]
	pub fn is_empty(&self) -> bool {
		self.size() == 0
	}

	#[allow(clippy::missing_errors_doc)]
	pub fn new(size: usize, prot: Protection) -> Result<Self, Error> {
		let size = Self::align(size);

		#[cfg(unix)] {
			use libc::{mmap, MAP_PRIVATE, MAP_ANON, MAP_FAILED};
			use std::os::raw::c_int;

			match unsafe { mmap(ptr::null_mut(), size, prot as c_int, MAP_PRIVATE | MAP_ANON, -1, 0) } {
				MAP_FAILED => Err(Error::last_os_error()),
				addr => Ok(unsafe { Self::from_ptr(addr, size) }),
			}
		}

		#[cfg(windows)] {
			use winapi::shared::minwindef::DWORD;
			use winapi::shared::ntdef::NULL;
			use winapi::um::memoryapi::VirtualAlloc;
			use winapi::um::winnt::{MEM_COMMIT, MEM_RESERVE};

			match unsafe { VirtualAlloc(ptr::null_mut(), size, MEM_COMMIT | MEM_RESERVE, prot as DWORD) } {
				NULL => Err(Error::last_os_error()),
				addr => Ok(unsafe { Self::from_ptr(addr, size) }),
			}
		}
	}

	/// # Panics
	///
	/// May panic if `size` is not smaller than the current size.
	#[allow(clippy::missing_errors_doc)]
	pub fn shrink(self, size: usize) -> Result<Self, Error> {
		assert!(size < self.0.len());

		let size = Pages::align(size);
		let diff = self.0.len() - size;

		if diff > 0 {
			#[cfg(unix)] {
				use libc::munmap;

				match unsafe { munmap(self.as_ptr::<u8>().add(size).cast::<c_void>(), diff) } {
					0 => Ok(unsafe { Self::from_ptr(self.into_ptr::<c_void>(), size) }),
					_ => Err(Error::last_os_error()),
				}
			}

			#[cfg(windows)] {
				use winapi::um::memoryapi::VirtualFree;
				use winapi::um::winnt::MEM_DECOMMIT;

				match unsafe { VirtualFree(self.as_ptr::<u8>().add(size).cast::<c_void>(), diff, MEM_DECOMMIT) } {
					0 => Err(Error::last_os_error()),
					_ => Ok(unsafe { Self::from_ptr(self.into_ptr::<c_void>(), size) }),
				}
			}
		} else {
			Ok(self)
		}
	}

	#[must_use]
	pub fn pages(&self, range: Range<usize>) -> Option<Pages> {
		if likely(range.start < self.len() && range.end <= self.len()) {
			Some(unsafe {
				Pages::from_ptr(self.as_ptr::<u8>().add(range.start * Pages::granularity()),
				(range.end - range.start) * Pages::granularity())
			})
		} else {
			None
		}
	}
}

impl Drop for Allocation {
	fn drop(&mut self) {
		#[cfg(unix)] {
			use libc::munmap;

			assert_eq!(unsafe { munmap(self.as_ptr::<c_void>(), self.0.len()) }, 0,
			"{}", Error::last_os_error());
		}

		#[cfg(windows)] {
			use winapi::um::memoryapi::VirtualFree;
			use winapi::um::winnt::MEM_RELEASE;

			assert_ne!(unsafe { VirtualFree(self.as_ptr::<c_void>(), 0, MEM_RELEASE) }, 0,
			"{}", Error::last_os_error());
		}
	}
}

impl<const N: usize> GuardedAlloc<N> {
	pub const GUARD_PAGES: usize = N;

	#[must_use]
	pub fn guard_size() -> usize {
		Self::GUARD_PAGES * Pages::granularity()
	}

	#[must_use]
	pub fn outer_size(size: usize) -> usize {
		Allocation::align(size + 2 * Self::guard_size())
	}

	#[must_use]
	pub fn inner_size(size: usize) -> usize {
		Self::outer_size(size) - 2 * Self::guard_size()
	}

	#[allow(clippy::missing_errors_doc)]
	pub fn new(size: usize, prot: Protection) -> Result<Self, Error> {
		let alloc = Self(Allocation::new(Self::outer_size(size), Protection::NoAccess)?);

		if likely(!alloc.inner().is_empty()) {
			alloc.inner().protect(prot)?;
		}

		Ok(alloc)
	}

	#[allow(clippy::missing_panics_doc)]
	pub fn inner(&self) -> Pages {
		self.0.pages(Self::GUARD_PAGES .. self.0.len() - Self::GUARD_PAGES).unwrap()
	}

	/// Create [`GuardedAlloc`] from [non‐null](NonNull) raw [`u8`] slice
	///
	/// # Safety
	///
	/// `slice` must have been previously generated by a call to [`into_slice`](Self::into_slice) and must not be
	/// aliased by any other [`GuardedAlloc`].
	pub unsafe fn from_raw_parts(base: NonNull<u8>, inner: usize) -> Self {
		debug_assert_eq!(base.as_ptr().align_offset(Pages::granularity()), 0);

		let ptr = base.as_ptr().sub(Self::guard_size());
		let outer = Self::outer_size(inner);

		debug_assert_eq!(ptr.align_offset(Allocation::granularity()), 0);
		debug_assert_eq!(Allocation::align(outer), outer);

		Self(Allocation::from_ptr(ptr, outer))
	}

	/// Create [`GuardedAlloc`] from pointer and length
	///
	/// # Safety
	///
	/// `ptr` must have been previously generated by a call to [`into_slice`](Self::into_slice) and must not be
	/// aliased by any other [`GuardedAlloc`]. `size` must match the size of the original [`GuardedAlloc`].
	///
	/// # Panics
	///
	/// May panic if `base` is null or if `base` or `inner` are not properly aligned.
	pub unsafe fn from_ptr<T>(base: *mut T, inner: usize) -> Self {
		Self::from_raw_parts(NonNull::new(base.cast::<u8>()).unwrap(), inner)
	}

	#[must_use] #[allow(clippy::missing_panics_doc)]
	pub fn into_slice(self) -> NonNull<[u8]> {
		let len = self.0.len();
		ManuallyDrop::new(self.0).pages(Self::GUARD_PAGES .. len - Self::GUARD_PAGES).unwrap().into_slice()
	}

	pub fn into_pages(self) -> Pages<'static> {
		unsafe { Pages::from_slice(self.into_slice()) }
	}

	#[allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]
	pub fn shrink(self, size: usize) -> Result<Self, Error> {
		let outer = Self::outer_size(size);

		if outer < self.0.size() {
			let pages = outer / Pages::granularity();
			self.0.pages(pages - Self::GUARD_PAGES .. pages).unwrap().protect(Protection::NoAccess)?;
			Ok(Self(self.0.shrink(outer)?))
		} else {
			Ok(self)
		}
	}
}

impl<T: AsPages> Protectable for T {
	fn lock(&self) -> Result<(), Error> {
		if let Some(pages) = self.as_pages() {
			pages.protect(Protection::NoAccess)?;
		}

		Ok(())
	}

	fn unlock(&self) -> Result<(), Error> {
		if let Some(pages) = self.as_pages() {
			pages.protect(Protection::ReadOnly)?;
		}

		Ok(())
	}

	fn unlock_mut(&mut self) -> Result<(), Error> {
		if let Some(pages) = self.as_pages() {
			pages.protect(Protection::ReadWrite)?;
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn page_size() {
		assert!(Pages::granularity().is_power_of_two());

		// No modern architecture has a page size < 4096 bytes
		assert!(Pages::granularity() >= 4096);
	}

	#[test]
	fn alloc_size() {
		assert!(Allocation::granularity().is_power_of_two());
		assert!(Allocation::granularity() >= Pages::granularity());
		assert_eq!(Pages::align(Allocation::granularity()), Allocation::granularity());
	}

	fn raw_range(range: std::ops::Range<usize>, samples: usize) {
		use rand::SeedableRng;
		use rand::distr::{Distribution, Uniform};

		let mut rng = rand_xoshiro::Xoshiro256PlusPlus::from_os_rng();
		let dist = Uniform::try_from(range).unwrap();

		for _ in 0..samples {
			let size = dist.sample(&mut rng);

			eprintln!("Allocating {} bytes", size);

			let alloc = Allocation::new(size, Protection::ReadWrite).unwrap();

			assert!(alloc.size() >= size);

			let slice = unsafe { std::slice::from_raw_parts_mut(alloc.as_ptr::<u8>(), alloc.size()) };

			for elem in slice.iter() {
				assert_eq!(*elem, 0);
			}

			slice.fill(0x55);

			for elem in slice.iter() {
				assert_eq!(*elem, 0x55);
			}
		}
	}

	#[test]
	fn raw_tiny() {
		raw_range(1..4096, 4095);
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
	fn raw_protection() {
		use bulletproof::Bulletproof;

		let size = Allocation::granularity();
		let bp = unsafe { Bulletproof::new() };
		let alloc = Allocation::new(size, Protection::NoAccess).unwrap();
		let ptr = alloc.as_ptr::<u8>();

		for i in 0..size {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
			assert_eq!(unsafe { bp.store(ptr.add(i), &0xff) }, Err(()));
		}

		alloc.pages(0 .. size / Pages::granularity()).unwrap().protect(Protection::ReadOnly).unwrap();

		for i in 0..size {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
			assert_eq!(unsafe { bp.store(ptr.add(i), &0x55) }, Err(()));
		}

		alloc.pages(0 .. size / Pages::granularity()).unwrap().protect(Protection::ReadWrite).unwrap();

		for i in 0..size {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
			assert_eq!(unsafe { bp.store(ptr.add(i), &0x55) }, Ok(()));
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0x55));
		}
	}

	#[cfg(target_os = "linux")]
	#[test]
	fn raw_shrink() {
		use bulletproof::Bulletproof;

		let size_0 = std::cmp::max(Allocation::granularity(), 2 * Pages::granularity());
		let bp = unsafe { Bulletproof::new() };
		let alloc_0 = Allocation::new(size_0, Protection::ReadWrite).unwrap();
		assert_eq!(alloc_0.size(), size_0);

		let ptr = alloc_0.as_ptr::<u8>();

		for i in 0..size_0 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
			assert_eq!(unsafe { bp.store(ptr.add(i), &0x55) }, Ok(()));
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0x55));
		}

		let size_1 = size_0 - Pages::granularity();

		let alloc_1 = alloc_0.shrink(size_1).unwrap();
		assert_eq!(alloc_1.size(), size_1);

		// Ensure TLB flush
		std::thread::yield_now();

		for i in 0..size_1 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0x55));
		}

		for i in size_1 .. size_0 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}
	}

	fn guarded_range(range: std::ops::Range<usize>, samples: usize) {
		use rand::SeedableRng;
		use rand::distr::{Distribution, Uniform};

		let mut rng = rand_xoshiro::Xoshiro256PlusPlus::from_os_rng();
		let dist = Uniform::try_from(range).unwrap();

		for _ in 0..samples {
			let size = dist.sample(&mut rng);

			eprintln!("Allocating {} bytes", size);

			let alloc = GuardedAlloc::<1>::new(size, Protection::ReadWrite).unwrap();

			assert!(alloc.inner().size() >= size);

			let slice = unsafe { std::slice::from_raw_parts_mut(alloc.inner().as_ptr::<u8>(), alloc.inner().size()) };

			for elem in slice.iter() {
				assert_eq!(*elem, 0);
			}

			slice.fill(0x55);

			for elem in slice.iter() {
				assert_eq!(*elem, 0x55);
			}
		}
	}

	#[test]
	fn guarded_tiny() {
		guarded_range(0..4096, 4096);
	}

	#[test]
	fn guarded_small() {
		guarded_range(4096..65536, 256);
	}

	#[test]
	fn guarded_medium() {
		guarded_range(65536..4194304, 64);
	}

	#[test]
	fn guarded_large() {
		guarded_range(4194304..16777216, 16);
	}

	#[test]
	fn guarded_huge() {
		guarded_range(4194304..268435456, 4);
	}

	#[cfg(target_os = "linux")]
	#[test]
	fn guarded_guard() {
		use bulletproof::Bulletproof;

		let size = Allocation::granularity();
		let bp = unsafe { Bulletproof::new() };
		let alloc = GuardedAlloc::<1>::new(size, Protection::ReadWrite).unwrap();
		let ptr = alloc.inner().as_ptr::<u8>();

		// Preceding guard
		for i in 1 ..= GuardedAlloc::<1>::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.sub(i)) }, Err(()));
		}

		for i in 0 .. size {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
			assert_eq!(unsafe { bp.store(ptr.add(i), &0x55) }, Ok(()));
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0x55));
		}

		// Trailing guard
		for i in size .. GuardedAlloc::<1>::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}
	}

	#[cfg(target_os = "linux")]
	#[test]
	fn guarded_shrink() {
		use crate::pages::Allocation;
		use bulletproof::Bulletproof;

		let size_0 = std::cmp::max(Allocation::granularity(), 2 * GuardedAlloc::<1>::guard_size());

		let bp = unsafe { Bulletproof::new() };
		let alloc_0 = GuardedAlloc::<1>::new(size_0, Protection::ReadWrite).unwrap();
		let ptr = alloc_0.inner().as_ptr::<u8>();

		for i in 0..size_0 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// Original guard
		for i in size_0 .. GuardedAlloc::<1>::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}

		let size_1 = size_0 - GuardedAlloc::<1>::guard_size();
		let alloc_1 = alloc_0.shrink(size_1).unwrap();

		// Allocation should not move
		assert_eq!(alloc_1.inner().as_ptr::<u8>(), ptr);

		// Ensure TLB flush
		std::thread::yield_now();

		for i in 0 .. size_1 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// New guard
		for i in size_1 .. GuardedAlloc::<1>::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}
	}
}
