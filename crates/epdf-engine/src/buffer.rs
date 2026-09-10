//! Phase 0 buffer contract.
//!
//! Layout, version, alignment, stride, pixel format, alpha mode, color space,
//! allocator, release operation and lifetime. Lengths and multiplications are
//! validated before allocation, nothing is freed by guessed capacity, and heap
//! views are rebuilt after WASM memory growth.
