#![feature(
	allocator_api,
	core_intrinsics,
	maybe_uninit_slice,
	slice_ptr_get,
	slice_ptr_len,
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
