//! Auxiliary functions

/// Check if `num` is a non‐zero power of two
#[must_use]
pub const fn is_power_of_two(num: usize) -> bool {
	num != 0 && (num & (num - 1)) == 0
}

/// Align `offset` to a multiple of `align`
///
/// `align` must be a non‐zero power of two.
#[must_use]
pub const fn align(offset: usize, align: usize) -> usize {
	debug_assert!(is_power_of_two(align));

	(offset + (align - 1)) & !(align - 1)
}

/// Securely zero‐out memory
///
/// # Safety
///
/// `addr` must be [valid](std::ptr#safety) for writes and properly aligned.
pub unsafe fn zero<T>(addr: *mut T, count: usize) {
	debug_assert_eq!(addr.align_offset(std::mem::align_of::<T>()), 0);
	std::intrinsics::volatile_set_memory(addr, 0, count);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn power_of_two() {
		let mut p = 2;

		while p < usize::MAX / 2 {
			assert!(is_power_of_two(p));
			p *= 2;
		}
	}

	#[test]
	fn not_power_of_two() {
		let mut p = 2;

		while p <= 4194304 {
			for q in p + 1 .. p * 2 {
				assert!(!is_power_of_two(q));
			}

			p *= 2;
		}
	}

	#[test]
	fn alignment() {
		assert_eq!(align(0, 4096), 0);

		for i in 1..4096 {
			assert_eq!(align(i, 4096), 4096);
		}
	}
}
