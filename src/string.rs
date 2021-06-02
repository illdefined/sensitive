use crate::auxiliary::zero;
use crate::guard;
use crate::vec::{InnerVec, Vec};

use std::convert::From;
use std::iter::FromIterator;
use std::mem::MaybeUninit;
use std::str::Chars;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::decompose_canonical;

#[derive(Debug)]
pub struct String(Vec<u8>);

#[derive(Debug)]
pub struct Ref<'t>(guard::Ref<'t, InnerVec<u8>>);

#[derive(Debug)]
pub struct RefMut<'t>(guard::RefMut<'t, InnerVec<u8>>);

impl String {
	pub fn new() -> Self {
		Self(Vec::new())
	}

	pub fn with_capacity(capacity: usize) -> Self {
		Self(Vec::with_capacity(capacity))
	}

	pub fn capacity(&self) -> usize {
		self.0.capacity()
	}

	pub fn len(&self) -> usize {
		self.0.len()
	}

	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}

	pub fn reserve(&mut self, capacity: usize) {
		self.0.reserve(capacity);
	}

	pub fn reserve_exact(&mut self, capacity: usize) {
		self.0.reserve_exact(capacity);
	}

	pub fn borrow(&self) -> Ref<'_> {
		Ref(self.0.borrow())
	}

	pub fn borrow_mut(&mut self) -> RefMut<'_> {
		RefMut(self.0.borrow_mut())
	}
}

impl FromIterator<char> for String {
	fn from_iter<I>(into: I) -> Self
		where I: IntoIterator<Item = char> {
		let iter = into.into_iter();
		let (lower, upper) = iter.size_hint();
		let mut string = Self::with_capacity(upper.unwrap_or(lower));

		{
			let mut mutable = string.borrow_mut();

			for ch in iter {
				mutable.push(ch);
			}
		}

		string
	}
}

impl From<&str> for String {
	fn from(source: &str) -> Self {
		let iter = source.nfd();
		let (lower, upper) = iter.size_hint();
		let mut string = Self::with_capacity(upper.unwrap_or(lower));

		{
			let mut mutable = string.borrow_mut();

			for decomp in iter {
				mutable.reserve(decomp.len_utf8());
				decomp.encode_utf8(unsafe { MaybeUninit::slice_assume_init_mut(mutable.0.spare_capacity_mut()) });
				unsafe { mutable.0.set_len(mutable.0.len() + decomp.len_utf8()); }
			}
		}

		string
	}
}

impl From<std::string::String> for String {
	fn from(mut source: std::string::String) -> Self {
		let string = Self::from(source.as_str());

		// Zero out source
		unsafe { zero(source.as_mut_ptr(), source.len()); }

		string
	}
}

impl Ref<'_> {
	pub fn as_bytes(&self) -> &[u8] {
		self.0.as_slice()
	}

	pub fn as_str(&self) -> &str {
		unsafe { std::str::from_utf8_unchecked(self.0.as_slice()) }
	}

	pub fn chars(&self) -> Chars<'_> {
		self.as_str().chars()
	}
}

impl RefMut<'_> {
	pub fn as_bytes(&self) -> &[u8] {
		self.0.as_slice()
	}

	pub fn as_str(&self) -> &str {
		unsafe { std::str::from_utf8_unchecked(self.0.as_slice()) }
	}

	pub fn chars(&self) -> Chars<'_> {
		self.as_str().chars()
	}

	pub fn reserve(&mut self, capacity: usize) {
		self.0.0.reserve(capacity);
	}

	pub fn reserve_exact(&mut self, capacity: usize) {
		self.0.0.reserve_exact(capacity);
	}

	pub fn push(&mut self, ch: char) {
		decompose_canonical(ch, |decomp| {
			self.0.0.reserve(decomp.len_utf8());
			decomp.encode_utf8(unsafe { MaybeUninit::slice_assume_init_mut(self.0.spare_capacity_mut()) });
			unsafe { self.0.0.set_len(self.0.len() + decomp.len_utf8()); }
		});
	}

	pub fn push_str(&mut self, string: &str) {
		let iter = string.nfd();
		let (lower, upper) = iter.size_hint();

		self.0.0.reserve(upper.unwrap_or(lower));

		for decomp in iter {
			self.0.0.reserve(decomp.len_utf8());
			decomp.encode_utf8(unsafe { MaybeUninit::slice_assume_init_mut(self.0.spare_capacity_mut()) });
			unsafe { self.0.0.set_len(self.0.len() + decomp.len_utf8()); }
		}
	}

	pub fn pop(&mut self) -> Option<char> {
		let ch = self.chars().rev().next()?;
		unsafe { self.0.0.set_len(self.0.len() - ch.len_utf8()); }
		Some(ch)
	}
}