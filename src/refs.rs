#[derive(Default, Clone, Copy, Debug)]
pub struct RefCount(usize);

impl RefCount {
	const MSB: usize = usize::MAX / 2 + 1;

	pub fn get(self) -> usize {
		self.0 & !Self::MSB
	}

	pub fn get_mut(self) -> bool {
		self.0 & Self::MSB == Self::MSB
	}

	pub fn acquire(self) -> Self {
		assert!(self.get() < Self::MSB);
		Self(self.0 + 1)
	}

	pub fn release(self) -> Self {
		debug_assert!(self.get() > 0);
		Self(self.0 - 1)
	}

	pub fn acquire_mut(self) -> Self {
		debug_assert!(!self.get_mut());
		Self(self.acquire().0 | Self::MSB)
	}

	pub fn release_mut(self) -> Self {
		debug_assert!(self.get_mut());
		Self(self.release().0 & !Self::MSB)
	}
}
