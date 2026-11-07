//! The joining table: `packages/core/text/src/joining.generated.ts` holds
//! Unicode's joining types as the engine's shaper reads them. The generated
//! TypeScript is the one copy of that data; this script reads it, so the Rust
//! and the TypeScript can't drift apart. It writes `$OUT_DIR/joining.rs`
//! (`JOINING_TYPES`). It reads the generator's exact layout and panics on a
//! line it doesn't expect, rather than guess.
//!
//! No rpath: the crate has no conformance harness of its own (the form
//! painter's vectors replay it), so it links nothing.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    // The nearest ancestor holding the package, as `build_support::lib_root`
    // finds `packages/engine/runtime`, so the crate's depth doesn't matter.
    let joining = manifest
        .ancestors()
        .map(|dir| dir.join("packages/core/text/src/joining.generated.ts"))
        .find(|file| file.is_file())
        .unwrap_or_else(|| panic!("no packages/core/text above {}", manifest.display()));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("joining.rs"), joining_rs(&joining)).expect("write joining.rs");
}

fn read(path: &Path) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// `JOINING_TYPES` as a Rust table: each `[first, last, type]` row of the
/// generated array, in its order.
fn joining_rs(path: &Path) -> String {
    let text = read(path);
    let mut rows = String::new();
    let mut count = 0;
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("export const JOINING_TYPES") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line == "];" {
            inside = false;
            continue;
        }
        let row = line
            .trim()
            .strip_prefix('[')
            .and_then(|row| row.strip_suffix("],"))
            .unwrap_or_else(|| panic!("{}: unexpected line {line:?}", path.display()));
        let parts: Vec<&str> = row.split(", ").collect();
        let [first, last, kind] = parts[..] else {
            panic!("{}: unexpected row {line:?}", path.display());
        };
        let number = |hex: &str| {
            u32::from_str_radix(hex.strip_prefix("0x").expect("hex"), 16).expect("hex number")
        };
        let kind = kind.trim_matches('\'');
        assert!(
            ["U", "L", "R", "D", "C", "T"].contains(&kind),
            "{}: joining type {kind:?}",
            path.display()
        );
        writeln!(rows, "    (0x{:x}, 0x{:x}, JoiningType::{kind}),", number(first), number(last))
            .unwrap();
        count += 1;
    }
    assert!(count > 0, "{}: no joining types", path.display());
    format!(
        "/// Each listed character's joining type, as ranges in order: `(first, last, type)`.\n\
         static JOINING_TYPES: [(u32, u32, JoiningType); {count}] = [\n{rows}];\n"
    )
}
