use crate::alloc::Sensitive;
use crate::pages::{Protection, page_align, protect, zero};
use crate::refs::RefCount;

use std::cmp::PartialEq;
use std::default::Default;
use std::fmt;
use std::ops::{Deref, DerefMut, Drop};

pub struct Vec<T> {
	vec: std::vec::Vec<T, Sensitive>,
	refs: RefCount
}

#[derive(Debug)]
pub struct Ref<'t, T>(&'t Vec<T>);

#[derive(Debug)]
pub struct RefMut<'t, T>(&'t mut Vec<T>);

impl<T> Vec<T> {
	const CMP_MIN: usize = 32;

	fn eq_slice<U>(a: &Vec<T>, b: &[U]) -> bool
		where T: PartialEq<U> {
		if a.capacity() == 0 {
			debug_assert!(a.is_empty());
			b.is_empty()
		} else {
			assert!(a.capacity() >= Self::CMP_MIN);

			b.iter().take(a.capacity()).enumerate().fold(true, |d, (i, e)| {
				d & unsafe { a.as_ptr().add(i).read() == *e }
			}) & (a.len() == b.len())
		}
	}

	unsafe fn raw_ptr(&self) -> *mut u8 {
		debug_assert!(self.capacity() > 0);

		self.vec.as_ptr().cast::<u8>() as *mut u8
	}

	fn protect(&self, prot: Protection) -> Result<(), std::io::Error> {
		if self.capacity() > 0 {
			unsafe { protect(self.raw_ptr(), page_align(self.capacity()), prot) }
		} else {
			Ok(())
		}
	}

	pub fn new() -> Self {
		let outer = Self {
			vec: std::vec::Vec::new_in(Sensitive),
			refs: RefCount::default()
		};

		debug_assert!(outer.capacity() == 0);

		outer
	}

	fn with_capacity_unprotected(capacity: usize) -> Self {
		Self {
			vec: std::vec::Vec::with_capacity_in(capacity, Sensitive),
			refs: RefCount::default()
		}
	}

	pub fn with_capacity(capacity: usize) -> Self {
		let outer = Self::with_capacity_unprotected(capacity);

		outer.protect(Protection::NoAccess).unwrap();
		outer
	}

	pub fn borrow(&self) -> Ref<'_, T> {
		self.refs.acquire(|| self.protect(Protection::ReadOnly).unwrap());

		Ref(self)
	}

	pub fn borrow_mut(&mut self) -> RefMut<'_, T> {
		self.refs.acquire_mut(|| self.protect(Protection::ReadWrite).unwrap());

		RefMut(self)
	}

	pub fn capacity(&self) -> usize {
		self.vec.capacity()
	}

	pub fn reserve(&mut self, capacity: usize) {
		self.vec.reserve(capacity);
		self.refs.mutate(|| self.protect(Protection::NoAccess).unwrap());
	}

	pub fn reserve_exact(&mut self, capacity: usize) {
		self.vec.reserve_exact(capacity);
		self.refs.mutate(|| self.protect(Protection::NoAccess).unwrap());
	}

	pub fn len(&self) -> usize {
		self.vec.len()
	}

	pub fn is_empty(&self) -> bool {
		self.vec.is_empty()
	}

	pub unsafe fn set_len(&mut self, len: usize) {
		self.vec.set_len(len)
	}

	pub fn as_ptr(&self) -> *const T {
		self.vec.as_ptr()
	}

	pub fn as_mut_ptr(&mut self) -> *mut T {
		self.vec.as_mut_ptr()
	}
}

impl<T> Default for Vec<T> {
	fn default() -> Self {
		Self::new()
	}
}

impl<T> From<&mut [T]> for Vec<T> {
	fn from(source: &mut [T]) -> Self {
		let len = source.len();
		let mut outer = Self::with_capacity_unprotected(len);

		unsafe {
			outer.as_mut_ptr().copy_from_nonoverlapping(source.as_ptr(), len);
			outer.set_len(len);
			zero(source.as_mut_ptr(), len);
		}

		outer.protect(Protection::NoAccess).unwrap();

		outer
	}
}

impl<T> From<std::vec::Vec<T>> for Vec<T> {
	fn from(mut source: std::vec::Vec<T>) -> Self {
		Self::from(source.as_mut_slice())
	}
}

impl From<&mut str> for Vec<u8> {
	fn from(source: &mut str) -> Self {
		Self::from(unsafe { source.as_bytes_mut() })
	}
}

impl From<std::string::String> for Vec<u8> {
	fn from(mut source: String) -> Self {
		Self::from(source.as_mut_str())
	}
}

impl<T, U> PartialEq<[U]> for Vec<T>
	where T: PartialEq<U> {
	fn eq(&self, other: &[U]) -> bool {
		self.borrow() == other
	}
}

impl<T, U> PartialEq<&[U]> for Vec<T>
	where T: PartialEq<U> {
	fn eq(&self, other: &&[U]) -> bool {
		&self.borrow() == other
	}
}

impl<T, U, const N: usize> PartialEq<[U; N]> for Vec<T>
	where T: PartialEq<U> {
	fn eq(&self, other: &[U; N]) -> bool {
		&self.borrow() == other
	}
}

impl PartialEq<&str> for Vec<u8> {
	fn eq(&self, other: &&str) -> bool {
		&self.borrow() == other
	}
}

impl<T> fmt::Debug for Vec<T> {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt.debug_struct("Vec")
			.field("refs", &self.refs)
			.finish_non_exhaustive()
	}
}

impl<T> Drop for Vec<T> {
	fn drop(&mut self) {
		self.protect(Protection::ReadWrite).unwrap();
	}
}

impl<T> Deref for Ref<'_, T> {
	type Target = [T];

	fn deref(&self) -> &Self::Target {
		self.0.vec.as_slice()
	}
}

impl<T, U> PartialEq<[U]> for Ref<'_, T>
	where T: PartialEq<U> {
	fn eq(&self, other: &[U]) -> bool {
		Vec::<T>::eq_slice(self.0, other)
	}
}

impl<T, U> PartialEq<&[U]> for Ref<'_, T>
	where T: PartialEq<U> {
	fn eq(&self, other: &&[U]) -> bool {
		self == *other
	}
}


impl<T, U, const N: usize> PartialEq<[U; N]> for Ref<'_, T>
	where T: PartialEq<U> {
	fn eq(&self, other: &[U; N]) -> bool {
		self == other as &[U]
	}
}

impl PartialEq<&str> for Ref<'_, u8> {
	fn eq(&self, other: &&str) -> bool {
		self == other.as_bytes()
	}
}

impl<T> Drop for Ref<'_, T> {
	fn drop(&mut self) {
		self.0.refs.release(
			|| self.0.protect(Protection::NoAccess).unwrap(),
			|| self.0.protect(Protection::ReadOnly).unwrap());
	}
}

impl<T> RefMut<'_, T> {
	pub fn push(&mut self, value: T) {
		self.0.vec.push(value);
	}

	pub fn pop(&mut self) -> Option<T> {
		self.0.vec.pop()
	}

	pub fn shrink_to_fit(&mut self) {
		self.0.vec.shrink_to_fit();
	}
}

impl<T> Deref for RefMut<'_, T> {
	type Target = [T];

	fn deref(&self) -> &Self::Target {
		self.0.vec.as_slice()
	}
}

impl<T> DerefMut for RefMut<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		self.0.vec.as_mut_slice()
	}
}

impl<T, U> PartialEq<[U]> for RefMut<'_, T>
	where T: PartialEq<U> {
	fn eq(&self, other: &[U]) -> bool {
		Vec::<T>::eq_slice(self.0, other)
	}
}

impl<T, U> PartialEq<&[U]> for RefMut<'_, T>
	where T: PartialEq<U> {
	fn eq(&self, other: &&[U]) -> bool {
		self == *other
	}
}


impl<T, U, const N: usize> PartialEq<[U; N]> for RefMut<'_, T>
	where T: PartialEq<U> {
	fn eq(&self, other: &[U; N]) -> bool {
		self == other as &[U]
	}
}

impl PartialEq<&str> for RefMut<'_, u8> {
	fn eq(&self, other: &&str) -> bool {
		self == other.as_bytes()
	}
}

impl<T> Drop for RefMut<'_, T> {
	fn drop(&mut self) {
		self.0.refs.release_mut(|| self.0.protect(Protection::NoAccess).unwrap());
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(unix)]
	#[test]
	fn test_protection() {
		use bulletproof::Bulletproof;

		let mut test = Vec::<u8>::new();
		test.borrow_mut().push(0xff);

		let bp = unsafe { Bulletproof::new() };
		let ptr = unsafe { test.raw_ptr() };

		assert_eq!(unsafe { bp.load(ptr) }, Err(()));

		{
			let immutable = test.borrow();
			assert_eq!(immutable[0], 0xff);
			assert_eq!(unsafe { bp.store(ptr, &0x55) }, Err(()));
		}

		assert_eq!(unsafe { bp.load(ptr) }, Err(()));

		{
			let mut mutable = test.borrow_mut();
			assert_eq!(mutable[0], 0xff);
			mutable[0] = 0x55;
			assert_eq!(mutable[0], 0x55);
		}

		assert_eq!(unsafe { bp.load(ptr) }, Err(()));
	}

	#[test]
	fn test_vec_seq() {
		const LIMIT: usize = 4194304;

		let mut test: Vec<usize> = Vec::new();

		{
			let mut mutable = test.borrow_mut();

			for i in 0..LIMIT {
				mutable.push(i);
			}
		}

		{
			let immutable = test.borrow();

			for i in 0..LIMIT {
				assert_eq!(immutable[i], i);
			}
		}
	}

	#[test]
	fn test_vec_rng() {
		use rand::prelude::*;

		const LIMIT: usize = 4194304;

		let mut rng = rand::thread_rng();
		let mut test: Vec<u8> = Vec::new();

		let mut mutable = test.borrow_mut();

		for i in 0..LIMIT {
			let rand = rng.gen();

			mutable.push(rand);
			assert_eq!(mutable[i], rand);
		}

		for _ in 0..LIMIT {
			assert!(mutable.pop().is_some());
			mutable.shrink_to_fit();
		}
	}

	#[test]
	fn test_eq() {
		assert_eq!(Vec::<u8>::from(vec![]), []);
		assert_eq!(Vec::<u8>::from(vec![0x00]), [0x00]);

		assert_ne!(Vec::<u8>::from(vec![]), [0x00]);
		assert_ne!(Vec::<u8>::from(vec![0x00]), []);
		assert_ne!(Vec::<u8>::from(vec![0x00]), [0x55]);

		assert_eq!(Vec::from("".to_string()), "");
		assert_eq!(Vec::from("Some secret".to_string()), "Some secret");

		assert_ne!(Vec::from("Warum Thunfische das?".to_string()), "");
	}
}
