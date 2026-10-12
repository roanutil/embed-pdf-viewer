//! The signature, stamp and redaction crates build with their `harness`
//! feature off, as a host that ships them builds them, with no warnings.
//! Every test and vector runs with the harness on, so code only the
//! harness reaches (a crate-private event table, review finding M24) looks
//! used there and dead here.
//!
//! It runs a nested `cargo check` into its own target directory, which
//! rebuilds every dependency from cold, so it is ignored by default:
//!
//! ```sh
//! cargo test -p epdf-signature-core --test no_harness_build -- --ignored
//! ```
//!
//! The same check by hand, per crate:
//!
//! ```sh
//! RUSTFLAGS="-D warnings" CARGO_TARGET_DIR=target/no-harness \
//!   cargo check -p epdf-plugin-signature --no-default-features
//! ```

use std::path::Path;
use std::process::Command;

const CRATES: [&str; 4] =
    ["epdf-signature-core", "epdf-plugin-signature", "epdf-plugin-stamp", "epdf-plugin-redaction"];

#[test]
#[ignore = "a nested cargo build; run with --ignored"]
fn the_plugins_build_without_the_harness() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for name in CRATES {
        let output = Command::new(&cargo)
            .current_dir(&workspace)
            .env("RUSTFLAGS", "-D warnings")
            .env("CARGO_TARGET_DIR", workspace.join("target/no-harness"))
            .args(["check", "--quiet", "-p", name, "--no-default-features"])
            .output()
            .expect("run cargo");
        assert!(
            output.status.success(),
            "{name} does not build without its harness:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
