//! The benchmark control export exists only with the `bench` feature.

#[test]
fn bench_noop_returns_on_the_calling_thread() {
    epdf_ffi::bench_noop();
}
