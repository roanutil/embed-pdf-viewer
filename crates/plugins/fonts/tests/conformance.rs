//! `vectors/core/fonts.json`, generated from `packages/core/fonts` by
//! `tooling/plugin-conformance/src/suites/fonts.ts`.

use std::collections::BTreeSet;

use epdf_fonts::harness::functions;
use plugin_conformance::{Registry, assert_suite, load};

const FILE: &str = "core/fonts.json";

#[test]
fn fonts_match_the_typescript() {
    assert_suite(FILE, &Registry::new(), &functions());
}

/// The table dispatches exactly the functions the vectors call.
#[test]
fn the_table_names_only_what_the_vectors_call() {
    let called: BTreeSet<String> = load(FILE)["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|case| case["fn"].as_str().expect("fn").to_string())
        .collect();
    let table: BTreeSet<String> = functions().iter().map(|(name, _)| name.to_string()).collect();
    assert_eq!(functions().len(), table.len(), "duplicate table entry");
    assert_eq!(table, called);
}

/// Every face is replayed by every per-face entry, so a face the generator
/// stopped writing fails here rather than passing by omission.
#[test]
fn the_vectors_cover_all_fourteen_faces() {
    let cases = load(FILE)["cases"].as_array().expect("cases").clone();
    for function in
        ["standardFontMetrics", "standardFontProgram", "standardFontProgramLoad", "standardFontCss"]
    {
        let fonts: BTreeSet<&str> = cases
            .iter()
            .filter(|case| case["fn"] == function && case["outcome"].get("returned").is_some())
            .map(|case| case["args"][0].as_str().expect("font"))
            .collect();
        let all: BTreeSet<&str> =
            epdf_fonts::StandardFont::ALL.iter().map(|f| f.as_str()).collect();
        assert_eq!(fonts, all, "{function}");
    }
}
