pub fn is_power_of_two(num: usize) -> bool {
	num != 0 && (num & (num - 1)) == 0
}

pub fn align(offset: usize, align: usize) -> usize {
	debug_assert!(is_power_of_two(align));

	(offset + (align - 1)) & !(align - 1)
}

pub unsafe fn zero<T>(addr: *mut T, count: usize) {
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
