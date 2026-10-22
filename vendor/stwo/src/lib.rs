#![allow(incomplete_features)]
// The SIMD/FFT kernels rely on entire `unsafe fn` bodies being unsafe by design.
// Rust 2024 makes `unsafe_op_in_unsafe_fn` deny-by-default; opt back into the
// pre-2024 behavior rather than wrapping every operation in an `unsafe {}` block.
#![allow(unsafe_op_in_unsafe_fn)]
#![cfg_attr(all(target_arch = "x86_64", target_feature = "avx512f"), feature(stdarch_x86_avx512))]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(feature = "prover", feature(iter_array_chunks, portable_simd, slice_ptr_get))]
pub mod core;

/// stwo-soroban patch: bumps an operation counter when the `op-count` feature is on.
#[cfg(feature = "op-count")]
#[macro_export]
macro_rules! op_count {
    ($c:ident) => {
        $crate::core::op_count::$c.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    };
}
#[cfg(not(feature = "op-count"))]
#[macro_export]
macro_rules! op_count {
    ($c:ident) => {};
}

#[cfg(feature = "prover")]
pub mod prover;
#[cfg(feature = "tracing")]
pub mod tracing;
