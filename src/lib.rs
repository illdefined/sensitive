#![feature(allocator_api, nonnull_slice_from_raw_parts, core_intrinsics)]

#[macro_use]
extern crate lazy_static;

pub mod pages;
pub mod alloc;
