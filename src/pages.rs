use crate::auxiliary::align;
use crate::traits::{AsPages, Protectable};

use std::convert::TryInto;
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

#[cfg(unix)]
#[repr(i32)]
pub enum Protection {
	NoAccess = libc::PROT_NONE,
	ReadOnly = libc::PROT_READ,
	ReadWrite = libc::PROT_READ | libc::PROT_WRITE,
}

#[cfg(windows)]
#[repr(u32)]
pub enum Protection {
	NoAccess = winnt::PAGE_NOACCESS,
	ReadOnly = winnt::PAGE_READONLY,
	ReadWrite = winnt::PAGE_READWRITE,
}

#[derive(Debug)]
pub struct Pages<'t>(NonNull<[u8]>, PhantomData<&'t ()>);

#[derive(Debug)]
pub struct Allocation(NonNull<[u8]>);

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
	pub fn granularity() -> usize {
		init();

		unsafe { PAGE_SIZE.assume_init() }
	}

	pub fn align(offset: usize) -> usize {
		align(offset, Self::granularity())
	}

	pub unsafe fn from_slice(slice: NonNull<[u8]>) -> Pages<'t> {
		// Assert correct alignment
		debug_assert_eq!(slice.as_ptr().cast::<u8>().align_offset(Self::granularity()), 0);
		debug_assert_eq!(Self::align(slice.len()), slice.len());

		Self(slice, PhantomData)
	}

	pub unsafe fn from_raw_parts(ptr: NonNull<u8>, size: usize) -> Pages<'t> {
		Self::from_slice(NonNull::slice_from_raw_parts(ptr, size))
	}

	pub unsafe fn from_ptr<T>(ptr: *mut T, size: usize) -> Pages<'t> {
		Self::from_raw_parts(NonNull::new(ptr.cast::<u8>()).unwrap(), size)
	}

	pub unsafe fn as_ptr<T>(&self) -> *mut T {
		self.0.as_ptr().cast::<T>()
	}

	pub fn into_slice(self) -> NonNull<[u8]> {
		self.0
	}

	pub fn size(&self) -> usize {
		self.0.len()
	}

	pub fn len(&self) -> usize {
		self.size() / Self::granularity()
	}

	pub fn is_empty(&self) -> bool {
		self.size() == 0
	}

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

	pub fn pages(&'t self, range: Range<usize>) -> Option<Pages<'t>> {
		if range.start < self.len() && range.end <= self.len() {
			let ptr = unsafe { self.as_ptr::<u8>() };
			Some(unsafe {
				Self::from_ptr(ptr.add(range.start * Self::granularity()),
				(range.end - range.start - 1) * Self::granularity())
			})
		} else {
			None
		}
	}
}

impl Allocation {
	pub fn granularity() -> usize {
		#[cfg(unix)] {
			Pages::granularity()
		}

		#[cfg(windows)] {
			init();

			unsafe { GRANULARITY.assume_init() }
		}
	}

	pub fn align(offset: usize) -> usize {
		align(offset, Self::granularity())
	}

	pub unsafe fn from_slice(slice: NonNull<[u8]>) -> Allocation {
		// Assert correct alignment
		debug_assert_eq!(slice.as_ptr().cast::<u8>().align_offset(Self::granularity()), 0);
		debug_assert_eq!(Self::align(slice.len()), slice.len());

		Self(slice)
	}

	pub unsafe fn from_raw_parts(ptr: NonNull<u8>, size: usize) -> Allocation {
		Self::from_slice(NonNull::slice_from_raw_parts(ptr, size))
	}

	pub unsafe fn from_ptr<T>(ptr: *mut T, size: usize) -> Allocation {
		Self::from_raw_parts(NonNull::new(ptr.cast::<u8>()).unwrap(), size)
	}

	pub unsafe fn as_ptr<T>(&self) -> *mut T {
		self.0.as_ptr().cast::<T>()
	}

	pub fn into_ptr<T>(self) -> *mut T {
		unsafe { ManuallyDrop::new(self).as_ptr() }
	}

	pub fn into_slice(self) -> NonNull<[u8]> {
		ManuallyDrop::new(self).0
	}

	pub fn size(&self) -> usize {
		self.0.len()
	}

	pub fn len(&self) -> usize {
		self.size() / Pages::granularity()
	}

	pub fn is_empty(&self) -> bool {
		self.size() == 0
	}

	pub fn new(size: usize, prot: Protection) -> Result<Allocation, Error> {
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

	pub fn shrink(self, size: usize) -> Result<Allocation, Error> {
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

	pub fn pages(&self, range: Range<usize>) -> Option<Pages> {
		if range.start < self.len() && range.end <= self.len() {
			let ptr = unsafe { self.as_ptr::<u8>() };
			Some(unsafe {
				Pages::from_ptr(ptr.add(range.start * Pages::granularity()),
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

			if unsafe { munmap(self.as_ptr::<c_void>(), self.0.len()) } != 0 {
				panic!("{}", Error::last_os_error());
			}
		}

		#[cfg(windows)] {
			use winapi::um::memoryapi::VirtualFree;
			use winapi::um::winnt::MEM_RELEASE;

			if unsafe { VirtualFree(self.as_ptr::<c_void>(), 0, MEM_RELEASE) } == 0 {
				panic!("{}", Error::last_os_error());
			}
		}
	}
}

impl<const N: usize> GuardedAlloc<N> {
	pub const GUARD_PAGES: usize = N;

	pub fn guard_size() -> usize {
		Self::GUARD_PAGES * Pages::granularity()
	}

	pub fn outer_size(size: usize) -> usize {
		Allocation::align(size + 2 * Self::guard_size())
	}

	pub fn inner_size(size: usize) -> usize {
		Self::outer_size(size) - 2 * Self::guard_size()
	}

	pub fn new(size: usize, prot: Protection) -> Result<Self, Error> {
		let alloc = Self(Allocation::new(Self::outer_size(size), Protection::NoAccess)?);

		if !alloc.inner().is_empty() {
			alloc.inner().protect(prot)?;
		}

		Ok(alloc)
	}

	pub fn inner(&self) -> Pages {
		self.0.pages(Self::GUARD_PAGES .. self.0.len() - Self::GUARD_PAGES).unwrap()
	}

	pub unsafe fn from_raw_parts(base: NonNull<u8>, inner: usize) -> Self {
		debug_assert_eq!(base.as_ptr().align_offset(Pages::granularity()), 0);

		let ptr = base.as_ptr().sub(Self::guard_size());
		let outer = Self::outer_size(inner);

		debug_assert_eq!(ptr.align_offset(Allocation::granularity()), 0);
		debug_assert_eq!(Allocation::align(outer), outer);

		Self(Allocation::from_ptr(ptr, outer))
	}

	pub unsafe fn from_ptr<T>(base: *mut T, inner: usize) -> Self {
		Self::from_raw_parts(NonNull::new(base.cast::<u8>()).unwrap(), inner)
	}

	pub fn into_slice(self) -> NonNull<[u8]> {
		let len = self.0.len();
		ManuallyDrop::new(self.0).pages(Self::GUARD_PAGES .. len - Self::GUARD_PAGES).unwrap().into_slice()
	}

	pub fn into_pages(self) -> Pages<'static> {
		unsafe { Pages::from_slice(self.into_slice()) }
	}

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

	use crate::auxiliary::is_power_of_two;

	#[test]
	fn page_size() {
		assert!(is_power_of_two(Pages::granularity()));

		// No modern architecture has a page size < 4096 bytes
		assert!(Pages::granularity() >= 4096);
	}

	#[test]
	fn alloc_size() {
		assert!(is_power_of_two(Allocation::granularity()));
		assert!(Allocation::granularity() >= Pages::granularity());
		assert_eq!(Pages::align(Allocation::granularity()), Allocation::granularity());
	}

	fn raw_range(range: std::ops::Range<usize>, samples: usize) {
		use rand::distributions::{Distribution, Uniform};

		let mut rng = rand::thread_rng();
		let dist = Uniform::from(range);

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
		let ptr = unsafe { alloc.as_ptr::<u8>() };

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

		let ptr = unsafe { alloc_0.as_ptr::<u8>() };

		for i in 0..size_0 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
			assert_eq!(unsafe { bp.store(ptr.add(i), &0x55) }, Ok(()));
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0x55));
		}

		let size_1 = size_0 - Pages::granularity();

		let alloc_1 = alloc_0.shrink(size_1).unwrap();
		assert_eq!(alloc_1.size(), size_1);

		for i in 0..size_1 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0x55));
		}

		for i in size_1 .. size_0 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}
	}

	fn guarded_range(range: std::ops::Range<usize>, samples: usize) {
		use rand::distributions::{Distribution, Uniform};

		let mut rng = rand::thread_rng();
		let dist = Uniform::from(range);

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
		let ptr = unsafe { alloc.inner().as_ptr::<u8>() };

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
		let ptr = unsafe { alloc_0.inner().as_ptr::<u8>() };

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
		assert_eq!(unsafe { alloc_1.inner().as_ptr::<u8>() }, ptr);

		for i in 0 .. size_1 {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Ok(0));
		}

		// New guard
		for i in size_1 .. GuardedAlloc::<1>::guard_size() {
			assert_eq!(unsafe { bp.load(ptr.add(i)) }, Err(()));
		}
	}
}
