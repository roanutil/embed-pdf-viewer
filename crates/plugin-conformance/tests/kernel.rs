//! K0: `packages/core/main` against `vectors/kernel/lifecycle.json`.

use plugin_conformance::{Registry, assert_suite};

#[test]
fn the_kernel_matches_the_typescript() {
    assert_suite("kernel/lifecycle.json", &Registry::new(), &Vec::new());
}
