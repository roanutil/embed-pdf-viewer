//! Document, Page and TextPage exist to be neither `Send` nor `Sync`: every
//! PDFium handle is confined to the thread that created it. Nothing here
//! runs at test time; the check is compile-time. Today that guarantee rests
//! on the absence of an `unsafe impl Send`/`unsafe impl Sync`, which is easy
//! to add by accident later (or in a merge) without anyone noticing. This
//! turns that absence into an assertion this file fails to *compile* against
//! if it's ever violated.
//!
//! Hand-rolled rather than pulling in `static_assertions`, using the same
//! trick that crate uses under the hood and that `impls`/`negative_impl`-style
//! checks rely on: a trait implemented unconditionally for every type, and
//! again, under a second marker type, for every type that also implements
//! the trait being checked against. If the type under test already
//! implements that trait, both impls apply and the call below has two
//! candidates to resolve, which is a compile error (E0283, "type annotations
//! needed") rather than a runtime failure. Verified against a standalone
//! `unsafe impl Send for <toy type>` during development: it turns exactly
//! this construct from `cargo build` succeeding into E0283.
macro_rules! assert_not_impl {
    ($x:ty: $($t:path),+ $(,)?) => {
        const _: fn() = || {
            trait AmbiguousIfImpl<A> {
                fn some_item() {}
            }

            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}

            struct Invocation;

            #[allow(dead_code)]
            impl<T: ?Sized $(+ $t)+> AmbiguousIfImpl<Invocation> for T {}

            // Only compiles if exactly one of the two impls above applies to
            // `$x`, i.e. if `$x` does NOT implement every trait in `$t`.
            let _ = <$x as AmbiguousIfImpl<_>>::some_item;
        };
    };
}

assert_not_impl!(pdfium::Document: Send);
assert_not_impl!(pdfium::Document: Sync);
assert_not_impl!(pdfium::Page<'static>: Send);
assert_not_impl!(pdfium::Page<'static>: Sync);
assert_not_impl!(pdfium::TextPage<'static>: Send);
assert_not_impl!(pdfium::TextPage<'static>: Sync);

// A no-op #[test] so `cargo test` reports this file as an exercised target
// rather than a silently-skipped one; the real check already ran at compile
// time, above.
#[test]
fn document_page_and_text_page_are_thread_confined() {}
