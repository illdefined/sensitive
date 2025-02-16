#![allow(internal_features)]
#![feature(
	allocator_api,
	core_intrinsics,
	maybe_uninit_slice,
	ptr_as_ref_unchecked,
	slice_ptr_get,
	sync_unsafe_cell,
)]

pub mod auxiliary;
pub mod pages;
pub mod alloc;
pub mod boxed;
pub mod vec;

#[cfg(feature = "string")]
pub mod string;

mod traits;
mod guard;
