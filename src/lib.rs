#![feature(
	allocator_api,
	cell_update,
	core_intrinsics,
	maybe_uninit_extra,
	nonnull_slice_from_raw_parts,
)]

pub mod pages;
pub mod alloc;
pub mod boxed;
pub mod vec;

mod refs;
