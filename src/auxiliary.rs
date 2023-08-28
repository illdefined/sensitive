//! Auxiliary functions

/// Securely zero‐out memory
///
/// # Safety
///
/// `addr` must be [valid](std::ptr#safety) for writes and properly aligned.
pub unsafe fn zero<T>(addr: *mut T, count: usize) {
	debug_assert_eq!(addr.align_offset(std::mem::align_of::<T>()), 0);
	std::intrinsics::volatile_set_memory(addr, 0, count);
}
