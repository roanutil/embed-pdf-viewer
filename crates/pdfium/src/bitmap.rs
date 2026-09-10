//! `FPDFBitmap_*` handles: allocation, stride, pixel format, buffer lifetime.
//!
//! The buffer contract (layout, version, alignment, stride, pixel format, alpha
//! mode, color space, allocator, release operation, lifetime) is written down in
//! Phase 0 and enforced here: lengths and multiplications validated before
//! allocation, no freeing by guessed capacity.
