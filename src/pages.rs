use std::convert::TryInto;
use std::io::Error;
use std::ptr;

#[cfg(windows)]
use winapi::um::winnt;

#[cfg(unix)]
lazy_static! {
	pub static ref PAGE_SIZE: usize = {
		use libc::{sysconf, _SC_PAGE_SIZE};

		let pg = unsafe { sysconf(_SC_PAGE_SIZE) };
		assert!(pg > 0);

		pg.try_into().unwrap()
	};
}

#[cfg(windows)]
lazy_static! {
	pub static ref PAGE_SIZE: usize = {
		use std::mem::MaybeUninit;
		use winapi::um::sysinfoapi::{SYSTEM_INFO, GetSystemInfo};

		let mut si = MaybeUninit::<SYSTEM_INFO>::uninit();
		unsafe { GetSystemInfo(si.as_mut_ptr()); }
		unsafe { si.assume_init() }.dwPageSize.try_into().unwrap()
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

pub fn page_align(offset: usize) -> usize {
	align(offset, *PAGE_SIZE)
}

pub unsafe fn zero(addr: *mut u8, size: usize) {
	std::intrinsics::volatile_set_memory(addr, 0, size);
}

#[cfg(unix)]
pub unsafe fn allocate(size: usize, prot: Protection) -> Result<*mut u8, Error> {
	use libc::{mmap, MAP_PRIVATE, MAP_ANON, MAP_FAILED};
	use std::os::raw::c_int;

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

	match VirtualFree(addr as *mut c_void, size, MEM_DECOMMIT) {
		0 => Err(Error::last_os_error()),
		_ => Ok(()),
	}
}

#[cfg(unix)]
pub unsafe fn release(addr: *mut u8, size: usize) -> Result<(), Error> {
	use libc::munmap;
	use std::ffi::c_void;

	match munmap(addr as *mut c_void, size) {
		0 => Ok(()),
		_ => Err(Error::last_os_error()),
	}
}

#[cfg(windows)]
pub unsafe fn release(addr: *mut u8, _size: usize) -> Result<(), Error> {
	use winapi::ctypes::c_void;
	use winapi::um::memoryapi::VirtualFree;
	use winapi::um::winnt::MEM_RELEASE;

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

	match mlock(addr as *mut c_void, size) {
		0 => Ok(()),
		_ => Err(Error::last_os_error()),
	}
}

#[cfg(windows)]
pub unsafe fn lock(addr: *mut u8, size: usize) -> Result<(), Error> {
	use winapi::ctypes::c_void;
	use winapi::um::memoryapi::VirtualLock;

	match VirtualLock(addr as *mut c_void, size) {
		0 => Err(Error::last_os_error()),
		_ => Ok(()),
	}
}

#[cfg(unix)]
pub unsafe fn unlock(addr: *mut u8, size: usize) -> Result<(), Error> {
	use libc::munlock;
	use std::ffi::c_void;

	match munlock(addr as *mut c_void, size) {
		0 => Ok(()),
		_ => Err(Error::last_os_error()),
	}
}

#[cfg(windows)]
pub unsafe fn unlock(addr: *mut u8, size: usize) -> Result<(), Error> {
	use winapi::ctypes::c_void;
	use winapi::um::memoryapi::VirtualUnlock;

	match VirtualUnlock(addr as *mut c_void, size) {
		0 => Err(Error::last_os_error()),
		_ => Ok(()),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_page_size() {
		assert!(is_power_of_two(*PAGE_SIZE));

		// No modern architecture has a page size < 4096 bytes
		assert!(*PAGE_SIZE >= 4096);
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
}
