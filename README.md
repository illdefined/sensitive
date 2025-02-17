## Synopsis

This is a library of memory allocators and data structures to handle sensitive information, especially when interfacing with foreign and unsafe code.

It currently features:

- An interface to the system’s [page allocator](https://docs.rs/sensitive/*/sensitive/pages/),
- a simple [memory allocator](https://docs.rs/sensitive/*/sensitive/alloc/) implementing the [`Allocator`](https://doc.rust-lang.org/nightly/std/alloc/trait.Allocator.html) trait, and
- access‐guarded wrappers around [`Box`](https://docs.rs/sensitive/*/sensitive/boxed/), [`Vec`](https://docs.rs/sensitive/*/sensitive/vec/) and [`String`](https://docs.rs/sensitive/*/sensitive/string/).

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
sensitive = "0.10"
```

The `force-mlock` feature may be used to force allocations to be memory‐resident: If the memory cannot be locked, the allocation will fail. Without this feature, locking is attempted, but failures are ignored.

## Implementation notes

This code relies heavily on experimental nightly‐only APIs.

## Intellectual property

This work is dedicated to the public domain under the terms of the
[CC0 1.0 licence](https://creativecommons.org/publicdomain/zero/1.0/).

The author holds no patent rights related to this work.
