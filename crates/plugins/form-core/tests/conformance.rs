//! `vectors/core/form-painter.json`, generated from `packages/core/form` by
//! `tooling/plugin-conformance/src/suites/form-painter.ts`.

use std::collections::BTreeSet;

use epdf_form_core::harness::functions;
use plugin_conformance::{Registry, assert_suite, load};

const FILE: &str = "core/form-painter.json";

#[test]
fn the_painter_matches_the_typescript() {
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

/// Every family the painter draws, and every check style, is drawn by some
/// case, and every standard font lays text out, so a family or a face the
/// generator stopped writing fails here rather than passing by omission. No
/// case throws: the painter has no failure the vectors record.
#[test]
fn the_vectors_cover_every_family_style_and_face() {
    let cases = load(FILE)["cases"].as_array().expect("cases").clone();
    assert!(cases.iter().all(|case| case["outcome"].get("returned").is_some()));
    let families: BTreeSet<String> = cases
        .iter()
        .filter(|case| case["fn"] == "widgetDrawing")
        .map(|case| case["args"][1]["family"].as_str().expect("family").to_string())
        .collect();
    let all: BTreeSet<String> =
        ["text", "checkbox", "radio", "combobox", "listbox", "pushbutton", "signature", "unknown"]
            .into_iter()
            .map(String::from)
            .collect();
    assert_eq!(families, all);
    let styles: BTreeSet<String> = cases
        .iter()
        .filter(|case| case["fn"] == "widgetDrawing")
        .filter_map(|case| case["args"][0]["checkStyle"].as_str().map(String::from))
        .collect();
    assert_eq!(styles.len(), 6, "{styles:?}");
    let faces: BTreeSet<String> = cases
        .iter()
        .filter(|case| case["fn"] == "widgetDrawing" && case["args"][1]["family"] == "text")
        .filter_map(|case| case["args"][0]["fontFamily"].as_str().map(String::from))
        .collect();
    for font in epdf_fonts::StandardFont::ALL {
        assert!(faces.contains(font.as_str()), "{}", font.as_str());
    }
}
