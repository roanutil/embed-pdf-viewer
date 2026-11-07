//! Two jobs.
//!
//! The text tables: `packages/core/text/src/joining.generated.ts` holds
//! Unicode's joining types as the engine's shaper reads them, and
//! `packages/core/fonts/src/last-resort.generated.ts` the last-resort program
//! (its hash and its bytes in base64). The generated TypeScript is the one
//! copy of that data; this script reads it, so the Rust and the TypeScript
//! can't drift apart. It writes `$OUT_DIR/joining.rs` (`JOINING_TYPES`),
//! `$OUT_DIR/last-resort.ttf` (the program, decoded) and
//! `$OUT_DIR/last_resort.rs` (`LAST_RESORT_HASH`, and `LAST_RESORT_PROGRAM`
//! including those bytes). It reads the generators' exact layout and panics on
//! a line it doesn't expect, rather than guess.
//!
//! The rpath: re-emitted for this crate's own test binaries. The conformance
//! harness (`harness` feature) depends on `plugin-conformance`, whose fake
//! engine links libembedpdf through `epdf-engine`. Cargo does not propagate
//! `cargo:rustc-link-arg` to a dependent package's test targets, so without
//! this line the tests link and then fail at load with
//! `Library not loaded: @rpath/libembedpdf.dylib`. Without the harness the
//! crate is pure and links nothing, so it needs no rpath.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    if std::env::var_os("CARGO_FEATURE_HARNESS").is_some() {
        build_support::emit_rpath_only();
    }
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    // The nearest ancestor holding the packages, as `build_support::lib_root`
    // finds `packages/engine/runtime`, so the crate's depth doesn't matter.
    let packages = manifest
        .ancestors()
        .map(|dir| dir.join("packages/core"))
        .find(|dir| dir.join("text/src").is_dir())
        .unwrap_or_else(|| panic!("no packages/core/text above {}", manifest.display()));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));

    let joining = packages.join("text/src/joining.generated.ts");
    std::fs::write(out.join("joining.rs"), joining_rs(&joining)).expect("write joining.rs");

    let last_resort = packages.join("fonts/src/last-resort.generated.ts");
    let (hash, program) = last_resort_of(&last_resort);
    std::fs::write(out.join("last-resort.ttf"), decode_base64(&program))
        .expect("write last-resort.ttf");
    std::fs::write(
        out.join("last_resort.rs"),
        format!(
            "/// SHA-256 of the last-resort program, hex.\n\
             pub const LAST_RESORT_HASH: &str = {hash:?};\n\
             /// The last-resort program: a box for every character.\n\
             pub(crate) static LAST_RESORT_PROGRAM: &[u8] = \
             include_bytes!(concat!(env!(\"OUT_DIR\"), \"/last-resort.ttf\"));\n"
        ),
    )
    .expect("write last_resort.rs");
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

/// The last-resort program's hash and base64, from their two exports.
fn last_resort_of(path: &Path) -> (String, String) {
    let text = read(path);
    let constant = |name: &str| {
        let prefix = format!("export const {name} = '");
        text.lines()
            .find_map(|line| line.strip_prefix(&prefix)?.strip_suffix("';"))
            .unwrap_or_else(|| panic!("{}: no {name}", path.display()))
            .to_string()
    };
    (constant("LAST_RESORT_HASH"), constant("LAST_RESORT_PROGRAM"))
}

/// Padded base64 to bytes, as `base64.ts` decodes it.
fn decode_base64(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    assert!(text.is_ascii() && text.len().is_multiple_of(4), "base64 of whole groups");
    let value = |byte: u8| ALPHABET.iter().position(|&c| c == byte).map_or(0, |at| at as u32);
    let padding = text.bytes().rev().take_while(|&byte| byte == b'=').count();
    let length = text.len() / 4 * 3 - padding;
    let mut out = Vec::with_capacity(length);
    for group in text.as_bytes().chunks(4) {
        let chunk = group.iter().fold(0, |chunk, &byte| (chunk << 6) | value(byte));
        for byte in [chunk >> 16, (chunk >> 8) & 0xff, chunk & 0xff] {
            if out.len() < length {
                out.push(byte as u8);
            }
        }
    }
    out
}
