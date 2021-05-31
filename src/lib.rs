#![feature(
	allocator_api,
	available_concurrency,
	cell_update,
	core_intrinsics,
	maybe_uninit_extra,
	nonnull_slice_from_raw_parts,
	slice_ptr_get,
	slice_ptr_len,
)]

pub mod auxiliary;
pub mod pages;
pub mod alloc;
pub mod boxed;
pub mod vec;

mod traits;
mod guard;
