use crate::auxiliary::zero;
use crate::alloc::Sensitive;
use crate::guard::{Guard, Ref, RefMut};
use crate::traits::{Pages, Protectable};

use std::cmp::{PartialEq, min, max};
use std::default::Default;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

pub(crate) type InnerVec<T> = std::vec::Vec<T, Sensitive>;
pub type Vec<T> = Guard<InnerVec<T>>;

impl<T> Pages for InnerVec<T> {
	fn pages(&self) -> Option<NonNull<[u8]>> {
		if self.capacity() > 0 {
			Some(NonNull::slice_from_raw_parts(
				NonNull::new(self.as_ptr().cast::<u8>() as *mut u8).unwrap(),
				Sensitive::inner_size(self.capacity() * std::mem::size_of::<T>())))
		} else {
			None
		}
	}
}

impl<T> Vec<T> {
	const CMP_MIN: usize = 32;

	fn eq_slice<U: Copy + Into<usize>>(a: &Vec<T>, b: &[U]) -> bool
		where T: Copy + Into<usize> {
		if a.capacity() == 0 {
			debug_assert!(a.is_empty());
			b.is_empty()
		} else {
			debug_assert!(a.capacity() >= Self::CMP_MIN);

			b.iter().take(a.capacity()).enumerate().fold(0, |d, (i, e)| {
				d | unsafe { a.as_ptr().add(i).read().into() ^ (*e).into() }
			}) | (max(a.len(), b.len()) - min(a.len(), b.len())) == 0
		}
	}

	pub fn new() -> Self {
		let guard = Guard::from_inner(std::vec::Vec::new_in(Sensitive));
		debug_assert!(guard.capacity() == 0);
		guard
	}

	pub(crate) fn with_capacity_unprotected(capacity: usize) -> Self {
		Guard::from_inner(std::vec::Vec::with_capacity_in(capacity, Sensitive))
	}

	pub fn with_capacity(capacity: usize) -> Self {
		let mut guard = Self::with_capacity_unprotected(capacity);
		guard.mutate(|vec| vec.lock().unwrap());
		guard
	}

	pub fn capacity(&self) -> usize {
		unsafe { self.inner().capacity() }
	}

	pub fn reserve(&mut self, capacity: usize) {
		self.mutate(|vec| {
			vec.reserve(capacity);
			vec.lock().unwrap();
		});
	}

	pub fn reserve_exact(&mut self, capacity: usize) {
		self.mutate(|vec| {
			vec.reserve_exact(capacity);
			vec.lock().unwrap();
		});
	}

	pub fn len(&self) -> usize {
		unsafe { self.inner().len() }
	}

	pub fn is_empty(&self) -> bool {
		unsafe { self.inner().is_empty() }
	}

	pub unsafe fn set_len(&mut self, len: usize) {
		self.inner_mut().set_len(len);
	}

	pub fn as_ptr(&self) -> *const T {
		unsafe { self.inner().as_ptr() }
	}

	pub fn as_mut_ptr(&mut self) -> *mut T {
		unsafe { self.inner_mut().as_mut_ptr() }
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
		let mut guard = Self::with_capacity_unprotected(len);

		unsafe {
			guard.as_mut_ptr().copy_from_nonoverlapping(source.as_ptr(), len);
			guard.set_len(len);
			zero(source.as_mut_ptr(), len);
			guard.inner().lock().unwrap();
		}

		guard
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

impl<T: Copy + Into<usize>, U: Copy + Into<usize>> PartialEq<[U]> for Vec<T> {
	fn eq(&self, other: &[U]) -> bool {
		self.borrow() == other
	}
}

impl<T: Copy + Into<usize>, U: Copy + Into<usize>> PartialEq<&[U]> for Vec<T> {
	fn eq(&self, other: &&[U]) -> bool {
		&self.borrow() == other
	}
}

impl<T: Copy + Into<usize>, U: Copy + Into<usize>, const N: usize> PartialEq<[U; N]> for Vec<T> {
	fn eq(&self, other: &[U; N]) -> bool {
		&self.borrow() == other
	}
}

impl PartialEq<&str> for Vec<u8> {
	fn eq(&self, other: &&str) -> bool {
		&self.borrow() == other
	}
}

impl<T: Copy + Into<usize>, U: Copy + Into<usize>> PartialEq<[U]> for Ref<'_, InnerVec<T>> {
	fn eq(&self, other: &[U]) -> bool {
		Vec::<T>::eq_slice(self.0, other)
	}
}

impl<T: Copy + Into<usize>, U: Copy + Into<usize>> PartialEq<&[U]> for Ref<'_, InnerVec<T>> {
	fn eq(&self, other: &&[U]) -> bool {
		self == *other
	}
}

impl<T> Ref<'_, InnerVec<T>> {
	pub fn as_slice(&self) -> &[T] {
		unsafe { self.0.inner() }.as_slice()
	}
}

impl<T: Copy + Into<usize>, U: Copy + Into<usize>, const N: usize> PartialEq<[U; N]> for Ref<'_, InnerVec<T>> {
	fn eq(&self, other: &[U; N]) -> bool {
		self == other as &[U]
	}
}

impl PartialEq<&str> for Ref<'_, InnerVec<u8>> {
	fn eq(&self, other: &&str) -> bool {
		self == other.as_bytes()
	}
}

impl<T> RefMut<'_, InnerVec<T>> {
	pub fn as_slice(&self) -> &[T] {
		unsafe { self.0.inner() }.as_slice()
	}

	pub fn len(&self) -> usize {
		unsafe { self.0.inner() }.len()
	}

	pub fn push(&mut self, value: T) {
		self.inner_mut().push(value);
	}

	pub fn pop(&mut self) -> Option<T> {
		self.inner_mut().pop()
	}

	pub fn shrink_to_fit(&mut self) {
		self.inner_mut().shrink_to_fit();
	}

	pub fn extend<I>(&mut self, iter: I)
		where I: IntoIterator<Item = T> {
		self.inner_mut().extend(iter);
	}

	pub fn spare_capacity_mut(&mut self) -> &mut [MaybeUninit<T>] {
		self.inner_mut().spare_capacity_mut()
	}

	pub fn reserve(&mut self, capacity: usize) {
		self.inner_mut().reserve(capacity);
	}

	pub fn reserve_exact(&mut self, capacity: usize) {
		self.inner_mut().reserve_exact(capacity);
	}

	pub unsafe fn set_len(&mut self, len: usize) {
		self.inner_mut().set_len(len);
	}
}

impl<T: Clone> RefMut<'_, InnerVec<T>> {
	pub fn resize(&mut self, len: usize, value: T) {
		self.inner_mut().resize(len, value);
	}
}

impl<T: Copy + Into<usize>, U: Copy + Into<usize>> PartialEq<[U]> for RefMut<'_, InnerVec<T>> {
	fn eq(&self, other: &[U]) -> bool {
		Vec::<T>::eq_slice(self.0, other)
	}
}

impl<T: Copy + Into<usize>, U: Copy + Into<usize>> PartialEq<&[U]> for RefMut<'_, InnerVec<T>> {
	fn eq(&self, other: &&[U]) -> bool {
		self == *other
	}
}

impl<T: Copy + Into<usize>, U: Copy + Into<usize>, const N: usize> PartialEq<[U; N]> for RefMut<'_, InnerVec<T>> {
	fn eq(&self, other: &[U; N]) -> bool {
		self == other as &[U]
	}
}

impl PartialEq<&str> for RefMut<'_, InnerVec<u8>> {
	fn eq(&self, other: &&str) -> bool {
		self == other.as_bytes()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(target_os = "linux")]
	#[test]
	fn protection() {
		use bulletproof::Bulletproof;

		let mut test = Vec::<u8>::new();
		test.borrow_mut().push(0xff);

		let bp = unsafe { Bulletproof::new() };
		let ptr = test.as_mut_ptr();

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
	fn vec_seq() {
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
	fn vec_rng() {
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
	fn eq() {
		assert_eq!(Vec::<u8>::from(vec![]), [] as [u8; 0]);
		assert_eq!(Vec::<u8>::from(vec![0x00]), [0u8]);

		assert_ne!(Vec::<u8>::from(vec![]), [0u8]);
		assert_ne!(Vec::<u8>::from(vec![0x00]), [] as [u8; 0]);
		assert_ne!(Vec::<u8>::from(vec![0x00]), [0x55u8]);

		assert_eq!(Vec::from("".to_string()), "");
		assert_eq!(Vec::from("Some secret".to_string()), "Some secret");

		assert_ne!(Vec::from("Warum Thunfische das?".to_string()), "");
	}

	#[test]
	fn concurrent() {
		use std::cmp::max;
		use std::sync::{Arc, Barrier};
		use std::thread;

		const LIMIT: usize = 4194304;

		let mut test: Vec<usize> = Vec::new();

		{
			let mut mutable = test.borrow_mut();

			for i in 0..LIMIT {
				mutable.push(i);
			}
		}

		let concurrency = max(16, 2 * thread::available_concurrency().unwrap().get());
		let barrier = Arc::new(Barrier::new(concurrency));
		let vec = Arc::new(test);
		let mut threads = std::vec::Vec::with_capacity(concurrency);

		for _ in 0..concurrency {
			let barrier = barrier.clone();
			let vec = vec.clone();

			threads.push(thread::spawn(move || {
				barrier.wait();

				for i in 0..LIMIT {
					let immutable = vec.borrow();
					assert_eq!(immutable[i], i);
				}
			}));
		}

		for thread in threads {
			thread.join().unwrap();
		}
	}

	#[test]
	fn concurrent_rw() {
		use std::cmp::max;
		use std::sync::{Arc, Barrier, RwLock};
		use std::thread;

		const LIMIT: usize = 32768;

		let mut test: Vec<usize> = Vec::new();

		{
			let mut mutable = test.borrow_mut();

			for _ in 0..LIMIT {
				mutable.push(0);
			}
		}

		let concurrency = max(16, 2 * thread::available_concurrency().unwrap().get());
		let barrier = Arc::new(Barrier::new(concurrency));
		let lock = Arc::new(RwLock::new(test));
		let mut threads = std::vec::Vec::with_capacity(concurrency);

		for _ in 0..concurrency {
			let barrier = barrier.clone();
			let lock = lock.clone();

			threads.push(thread::spawn(move || {
				barrier.wait();

				for i in 0..LIMIT {
					{
						let mut vec = lock.write().unwrap();
						let mut mutable = vec.borrow_mut();

						mutable[i] = i;
					}

					{
						let vec = lock.read().unwrap();
						let immutable = vec.borrow();

						assert_eq!(immutable[i], i);
					}
				}
			}));
		}

		for thread in threads {
			thread.join().unwrap();
		}
	}
}
