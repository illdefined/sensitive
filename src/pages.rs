use std::convert::TryInto;
use std::io::Error;
use std::ptr;

#[cfg(windows)]
use winapi::um::winnt;

#[cfg(unix)]
lazy_static! {
	pub static ref GRANULARITY: usize = {
		use libc::{sysconf, _SC_PAGESIZE};

		let pg = unsafe { sysconf(_SC_PAGESIZE) };
		assert!(pg > 0);

		pg.try_into().unwrap()
	};
}

#[cfg(windows)]
lazy_static! {
	pub static ref GRANULARITY: usize = {
		use std::mem::MaybeUninit;
		use winapi::um::sysinfoapi::{SYSTEM_INFO, GetSystemInfo};

		let mut si = MaybeUninit::<SYSTEM_INFO>::uninit();
		unsafe { GetSystemInfo(si.as_mut_ptr()); }
		unsafe { si.assume_init() }.dwAllocationGranularity.try_into().unwrap()
	};
}

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

pub fn is_power_of_two(num: usize) -> bool {
	num != 0 && (num & (num - 1)) == 0
}

pub fn align(offset: usize, align: usize) -> usize {
	debug_assert!(is_power_of_two(align));

	(offset + (align - 1)) & !(align - 1)
}

pub fn alloc_align(offset: usize) -> usize {
	align(offset, *GRANULARITY)
}

pub unsafe fn zero(addr: *mut u8, size: usize) {
	std::intrinsics::volatile_set_memory(addr, 0, size);
}

#[cfg(unix)]
pub unsafe fn allocate(size: usize, prot: Protection) -> Result<*mut u8, Error> {
	use libc::{mmap, MAP_PRIVATE, MAP_ANON, MAP_FAILED};
	use std::os::raw::c_int;

	debug_assert_eq!(alloc_align(size), size);

	match mmap(ptr::null_mut(), size, prot as c_int, MAP_PRIVATE | MAP_ANON, -1, 0) {
		MAP_FAILED => Err(Error::last_os_error()),
		addr => Ok(addr as *mut u8),
	}
}

#[cfg(windows)]
pub unsafe fn allocate(size: usize, prot: Protection) -> Result<*mut u8, Error> {
	use winapi::shared::minwindef::DWORD;
	use winapi::shared::ntdef::NULL;
	use winapi::um::memoryapi::VirtualAlloc;
	use winapi::um::winnt::{MEM_COMMIT, MEM_RESERVE};

	debug_assert_eq!(alloc_align(size), size);

	match VirtualAlloc(ptr::null_mut(), size, MEM_COMMIT | MEM_RESERVE, prot as DWORD) {
		NULL => Err(Error::last_os_error()),
		addr => Ok(addr as *mut u8),
	}
}

#[cfg(unix)]
pub unsafe fn uncommit(addr: *mut u8, size: usize) -> Result<(), Error> {
	release(addr, size)
}

#[cfg(windows)]
pub unsafe fn uncommit(addr: *mut u8, size: usize) -> Result<(), Error> {
	use winapi::ctypes::c_void;
	use winapi::um::memoryapi::VirtualFree;
	use winapi::um::winnt::MEM_DECOMMIT;

	debug_assert_eq!(addr.align_offset(*GRANULARITY), 0);
	debug_assert_eq!(alloc_align(size), size);

	match VirtualFree(addr as *mut c_void, size, MEM_DECOMMIT) {
		0 => Err(Error::last_os_error()),
		_ => Ok(()),
	}
}

#[cfg(unix)]
pub unsafe fn release(addr: *mut u8, size: usize) -> Result<(), Error> {
	use libc::munmap;
	use std::ffi::c_void;

	debug_assert_eq!(addr.align_offset(*GRANULARITY), 0);
	debug_assert_eq!(alloc_align(size), size);

	match munmap(addr as *mut c_void, size) {
		0 => Ok(()),
		_ => Err(Error::last_os_error()),
	}
}

#[cfg(windows)]
pub unsafe fn release(addr: *mut u8, size: usize) -> Result<(), Error> {
	use winapi::ctypes::c_void;
	use winapi::um::memoryapi::VirtualFree;
	use winapi::um::winnt::MEM_RELEASE;

	debug_assert_eq!(addr.align_offset(*GRANULARITY), 0);
	debug_assert_eq!(alloc_align(size), size);

	match VirtualFree(addr as *mut c_void, 0, MEM_RELEASE) {
		0 => Err(Error::last_os_error()),
		_ => Ok(()),
	}
}

#[cfg(unix)]
pub unsafe fn protect(addr: *mut u8, size: usize, prot: Protection) -> Result<(), Error> {
	use libc::mprotect;
	use std::ffi::c_void;
	use std::os::raw::c_int;

	debug_assert_eq!(addr.align_offset(*GRANULARITY), 0);
	debug_assert_eq!(alloc_align(size), size);

	match mprotect(addr as *mut c_void, size, prot as c_int) {
		0 => Ok(()),
		_ => Err(Error::last_os_error()),
	}
}

#[cfg(windows)]
pub unsafe fn protect(addr: *mut u8, size: usize, prot: Protection) -> Result<(), Error> {
	use std::mem::MaybeUninit;
	use winapi::ctypes::c_void;
	use winapi::shared::minwindef::DWORD;
	use winapi::um::memoryapi::VirtualProtect;

	debug_assert_eq!(addr.align_offset(*GRANULARITY), 0);
	debug_assert_eq!(alloc_align(size), size);

	let mut old = MaybeUninit::<DWORD>::uninit();
	match VirtualProtect(addr as *mut c_void, size, prot as DWORD, old.as_mut_ptr()) {
		0 => Err(Error::last_os_error()),
		_ => Ok(()),
	}
}

#[cfg(unix)]
pub unsafe fn lock(addr: *mut u8, size: usize) -> Result<(), Error> {
	use libc::mlock;
	use std::ffi::c_void;

	debug_assert_eq!(addr.align_offset(*GRANULARITY), 0);
	debug_assert_eq!(alloc_align(size), size);

	match mlock(addr as *mut c_void, size) {
		0 => Ok(()),
		_ => Err(Error::last_os_error()),
	}
}

#[cfg(windows)]
pub unsafe fn lock(addr: *mut u8, size: usize) -> Result<(), Error> {
	use winapi::ctypes::c_void;
	use winapi::um::memoryapi::VirtualLock;

	debug_assert_eq!(addr.align_offset(*GRANULARITY), 0);
	debug_assert_eq!(alloc_align(size), size);

	match VirtualLock(addr as *mut c_void, size) {
		0 => Err(Error::last_os_error()),
		_ => Ok(()),
	}
}

#[cfg(unix)]
pub unsafe fn unlock(addr: *mut u8, size: usize) -> Result<(), Error> {
	use libc::munlock;
	use std::ffi::c_void;

	debug_assert_eq!(addr.align_offset(*GRANULARITY), 0);
	debug_assert_eq!(alloc_align(size), size);

	match munlock(addr as *mut c_void, size) {
		0 => Ok(()),
		_ => Err(Error::last_os_error()),
	}
}

#[cfg(windows)]
pub unsafe fn unlock(addr: *mut u8, size: usize) -> Result<(), Error> {
	use winapi::ctypes::c_void;
	use winapi::um::memoryapi::VirtualUnlock;

	debug_assert_eq!(addr.align_offset(*GRANULARITY), 0);
	debug_assert_eq!(alloc_align(size), size);

	match VirtualUnlock(addr as *mut c_void, size) {
		0 => Err(Error::last_os_error()),
		_ => Ok(()),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_alloc_size() {
		assert!(is_power_of_two(*GRANULARITY));

		// No modern architecture has a page size < 4096 bytes
		assert!(*GRANULARITY >= 4096);
	}

	#[test]
	fn test_is_power_of_two() {
		let mut p = 2;

		while p < usize::MAX / 2 {
			assert!(is_power_of_two(p));
			p *= 2;
		}
	}

	#[test]
	fn test_not_is_power_of_two() {
		let mut p = 2;

		while p <= 4194304 {
			for q in p + 1 .. p * 2 {
				assert!(!is_power_of_two(q));
			}

			p *= 2;
		}
	}

	#[test]
	fn test_align() {
		assert_eq!(align(0, 4096), 0);

		for i in 1..4096 {
			assert_eq!(align(i, 4096), 4096);
		}
	}

	#[cfg(unix)]
	#[test]
	fn test_protection() {
		use bulletproof::Bulletproof;

		let size = *GRANULARITY;
		let bp = unsafe { Bulletproof::new() };
		let buf = unsafe { allocate(size, Protection::NoAccess) }.unwrap();

		for i in 0..size {
			assert_eq!(unsafe { bp.load(buf.add(i)) }, Err(()));
			assert_eq!(unsafe { bp.store(buf.add(i), &0xff) }, Err(()));
		}

		unsafe { protect(buf, size, Protection::ReadOnly) }.unwrap();

		for i in 0..size {
			assert_eq!(unsafe { bp.load(buf.add(i)) }, Ok(0));
			assert_eq!(unsafe { bp.store(buf.add(i), &0x55) }, Err(()));
		}

		unsafe { protect(buf, size, Protection::ReadWrite) }.unwrap();

		for i in 0..size {
			assert_eq!(unsafe { bp.load(buf.add(i)) }, Ok(0));
			assert_eq!(unsafe { bp.store(buf.add(i), &0x55) }, Ok(()));
			assert_eq!(unsafe { bp.load(buf.add(i)) }, Ok(0x55));
		}

		unsafe { release(buf, size) }.unwrap();
	}
}
