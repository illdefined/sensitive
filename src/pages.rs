use std::convert::TryInto;
use std::io::Error;
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::Once;

#[cfg(windows)]
use winapi::um::winnt;

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

static INIT: Once = Once::new();

static mut PAGE_SIZE: MaybeUninit<usize> = MaybeUninit::uninit();

#[cfg(windows)]
static mut GRANULARITY: MaybeUninit<usize> = MaybeUninit::uninit();

pub fn is_power_of_two(num: usize) -> bool {
	num != 0 && (num & (num - 1)) == 0
}

pub fn align(offset: usize, align: usize) -> usize {
	debug_assert!(is_power_of_two(align));

	(offset + (align - 1)) & !(align - 1)
}

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

pub fn page_size() -> usize {
	init();

	unsafe { PAGE_SIZE.assume_init() }
}

pub fn granularity() -> usize {
	#[cfg(unix)] {
		page_size()
	}

	#[cfg(windows)] {
		init();

		unsafe { GRANULARITY.assume_init() }
	}
}

pub fn page_align(offset: usize) -> usize {
	align(offset, page_size())
}

pub fn alloc_align(offset: usize) -> usize {
	align(offset, granularity())
}

pub unsafe fn zero<T>(addr: *mut T, count: usize) {
	std::intrinsics::volatile_set_memory(addr, 0, count);
}


pub unsafe fn allocate(size: usize, prot: Protection) -> Result<*mut u8, Error> {
	debug_assert_eq!(alloc_align(size), size);

	#[cfg(unix)] {
		use libc::{mmap, MAP_PRIVATE, MAP_ANON, MAP_FAILED};
		use std::os::raw::c_int;

		match mmap(ptr::null_mut(), size, prot as c_int, MAP_PRIVATE | MAP_ANON, -1, 0) {
			MAP_FAILED => Err(Error::last_os_error()),
			addr => Ok(addr as *mut u8),
		}
	}

	#[cfg(windows)] {
		use winapi::shared::minwindef::DWORD;
		use winapi::shared::ntdef::NULL;
		use winapi::um::memoryapi::VirtualAlloc;
		use winapi::um::winnt::{MEM_COMMIT, MEM_RESERVE};

		match VirtualAlloc(ptr::null_mut(), size, MEM_COMMIT | MEM_RESERVE, prot as DWORD) {
			NULL => Err(Error::last_os_error()),
			addr => Ok(addr as *mut u8),
		}
	}
}

pub unsafe fn uncommit(addr: *mut u8, size: usize) -> Result<(), Error> {
	debug_assert_eq!(addr.align_offset(page_size()), 0);
	debug_assert_eq!(page_align(size), size);

	#[cfg(unix)] {
		release(addr, size)
	}

	#[cfg(windows)] {
		use winapi::ctypes::c_void;
		use winapi::um::memoryapi::VirtualFree;
		use winapi::um::winnt::MEM_DECOMMIT;

		match VirtualFree(addr as *mut c_void, size, MEM_DECOMMIT) {
			0 => Err(Error::last_os_error()),
			_ => Ok(()),
		}
	}
}

pub unsafe fn release(addr: *mut u8, size: usize) -> Result<(), Error> {
	debug_assert_eq!(addr.align_offset(granularity()), 0);
	debug_assert_eq!(alloc_align(size), size);

	#[cfg(unix)] {
		use libc::munmap;
		use std::ffi::c_void;

		match munmap(addr as *mut c_void, size) {
			0 => Ok(()),
			_ => Err(Error::last_os_error()),
		}
	}

	#[cfg(windows)] {
		use winapi::ctypes::c_void;
		use winapi::um::memoryapi::VirtualFree;
		use winapi::um::winnt::MEM_RELEASE;

		match VirtualFree(addr as *mut c_void, 0, MEM_RELEASE) {
			0 => Err(Error::last_os_error()),
			_ => Ok(()),
		}
	}
}

pub unsafe fn protect(addr: *mut u8, size: usize, prot: Protection) -> Result<(), Error> {
	debug_assert_eq!(addr.align_offset(page_size()), 0);
	debug_assert_eq!(page_align(size), size);

	#[cfg(unix)] {
		use libc::mprotect;
		use std::ffi::c_void;
		use std::os::raw::c_int;

		match mprotect(addr as *mut c_void, size, prot as c_int) {
			0 => Ok(()),
			_ => Err(Error::last_os_error()),
		}
	}

	#[cfg(windows)] {
		use winapi::ctypes::c_void;
		use winapi::shared::minwindef::DWORD;
		use winapi::um::memoryapi::VirtualProtect;

		let mut old = MaybeUninit::<DWORD>::uninit();
		match VirtualProtect(addr as *mut c_void, size, prot as DWORD, old.as_mut_ptr()) {
			0 => Err(Error::last_os_error()),
			_ => Ok(()),
		}
	}
}

pub unsafe fn lock(addr: *mut u8, size: usize) -> Result<(), Error> {
	debug_assert_eq!(addr.align_offset(page_size()), 0);
	debug_assert_eq!(page_align(size), size);

	#[cfg(unix)] {
		use libc::mlock;
		use std::ffi::c_void;

		match mlock(addr as *mut c_void, size) {
			0 => Ok(()),
			_ => Err(Error::last_os_error()),
		}
	}

	#[cfg(windows)] {
		use winapi::ctypes::c_void;
		use winapi::um::memoryapi::VirtualLock;

		match VirtualLock(addr as *mut c_void, size) {
			0 => Err(Error::last_os_error()),
			_ => Ok(()),
		}
	}
}

pub unsafe fn unlock(addr: *mut u8, size: usize) -> Result<(), Error> {
	debug_assert_eq!(addr.align_offset(granularity()), 0);
	debug_assert_eq!(alloc_align(size), size);

	#[cfg(unix)] {
		use libc::munlock;
		use std::ffi::c_void;

		match munlock(addr as *mut c_void, size) {
			0 => Ok(()),
			_ => Err(Error::last_os_error()),
		}
	}

	#[cfg(windows)] {
		use winapi::ctypes::c_void;
		use winapi::um::memoryapi::VirtualUnlock;

		debug_assert_eq!(addr.align_offset(granularity()), 0);
		debug_assert_eq!(alloc_align(size), size);

		match VirtualUnlock(addr as *mut c_void, size) {
			0 => Err(Error::last_os_error()),
			_ => Ok(()),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_page_size() {
		assert!(is_power_of_two(page_size()));

		// No modern architecture has a page size < 4096 bytes
		assert!(page_size() >= 4096);
	}

	#[test]
	fn test_alloc_size() {
		assert!(is_power_of_two(granularity()));
		assert!(granularity() >= page_size());
		assert_eq!(align(granularity(), page_size()), granularity());
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

		let size = granularity();
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

	#[cfg(unix)]
	#[test]
	fn test_uncommit() {
		use bulletproof::Bulletproof;

		let size = std::cmp::max(granularity(), 2 * page_size());
		let bp = unsafe { Bulletproof::new() };
		let buf = unsafe { allocate(size, Protection::ReadWrite) }.unwrap();

		for i in 0..size {
			assert_eq!(unsafe { bp.load(buf.add(i)) }, Ok(0));
			assert_eq!(unsafe { bp.store(buf.add(i), &0x55) }, Ok(()));
			assert_eq!(unsafe { bp.load(buf.add(i)) }, Ok(0x55));
		}

		unsafe { uncommit(buf.add(size - page_size()), page_size()) }.unwrap();

		for i in 0 .. size - page_size() {
			assert_eq!(unsafe { bp.load(buf.add(i)) }, Ok(0x55));
		}

		for i in size - page_size() + 1 .. size {
			assert_eq!(unsafe { bp.load(buf.add(i)) }, Err(()));
		}

		unsafe { release(buf, size - page_size()) }.unwrap();
	}
}
