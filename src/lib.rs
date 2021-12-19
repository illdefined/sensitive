#![feature(
	allocator_api,
	available_parallelism,
	core_intrinsics,
	maybe_uninit_extra,
	maybe_uninit_slice,
	nonnull_slice_from_raw_parts,
	slice_ptr_get,
	slice_ptr_len,
	vec_spare_capacity,
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
