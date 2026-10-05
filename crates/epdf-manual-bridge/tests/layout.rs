//! `include/epdf_manual.h` is written by hand. Every struct in it carries
//! `_Static_assert`s on its size and field offsets; this checks the Rust
//! `#[repr(C)]` side against the same numbers, and asks the C compiler to
//! check the header's side.

use std::collections::BTreeMap;
use std::mem::{offset_of, size_of};
use std::path::PathBuf;
use std::process::Command;

use epdf_manual_bridge::*;

fn header() -> (PathBuf, String) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("include/epdf_manual.h");
    let text = std::fs::read_to_string(&path).unwrap();
    (path, text)
}

/// The Rust layout, keyed the way the header's asserts spell it:
/// `Type` for a size, `Type.field` for an offset.
fn rust_layout() -> BTreeMap<&'static str, usize> {
    macro_rules! layout {
        ($($ty:ident { $($field:ident),* })*) => {{
            let mut map = BTreeMap::new();
            $(
                map.insert(stringify!($ty), size_of::<$ty>());
                $( map.insert(concat!(stringify!($ty), ".", stringify!($field)), offset_of!($ty, $field)); )*
            )*
            map
        }};
    }
    layout! {
        EpdfError { kind, code, detail }
        EpdfPoint { x, y }
        EpdfRect { x, y, width, height }
        EpdfQuad { upper_start, upper_end, lower_start, lower_end }
        EpdfSegment { quad, rect, advance }
        EpdfSearchHit {
            page_object_number, page_index, char_start, char_count, bounds, segment_offset,
            segment_count, snippet_text, snippet_len, snippet_match_start,
            snippet_match_length, has_bounds, has_snippet
        }
        EpdfHits { hits, hit_count, segments, segment_count, owner }
        EpdfPixels { data, len, width, height, stride, format, scale, owner }
        EpdfSearchStarted { text, operation_id }
        EpdfSearchProgress { scanned, total, hit_count, operation_id }
        EpdfSearchCompleted { hit_count, operation_id }
        EpdfSearchCancelled { operation_id, reason }
        EpdfSearchFailed { code, message, operation_id }
        EpdfSearchActiveHitChanged { index, hits }
    }
}

/// `_Static_assert(sizeof(T) == N` and `_Static_assert(offsetof(T, f) == N`.
fn header_layout(text: &str) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("_Static_assert(") else { continue };
        // `offsetof(T, f) == N, "message");`: split at the message.
        let (expr, _) = rest.split_once(", \"").unwrap();
        let (lhs, n) = expr.split_once("==").unwrap();
        let n: usize = n.trim().parse().unwrap();
        let lhs = lhs.trim();
        let key = if let Some(t) = lhs.strip_prefix("sizeof(") {
            t.trim_end_matches(')').to_string()
        } else if let Some(args) = lhs.strip_prefix("offsetof(") {
            let (t, f) = args.trim_end_matches(')').split_once(',').unwrap();
            format!("{}.{}", t.trim(), f.trim())
        } else {
            panic!("an assert this test cannot read: {line}");
        };
        map.insert(key, n);
    }
    map
}

#[test]
fn every_struct_in_the_header_matches_its_repr_c_twin() {
    let (_, text) = header();
    let header = header_layout(&text);
    let rust = rust_layout();
    let rust: BTreeMap<String, usize> = rust.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    assert_eq!(header, rust);
}

#[test]
fn the_c_compiler_accepts_the_header_and_its_asserts() {
    let (path, _) = header();
    let Ok(output) =
        Command::new("cc").args(["-fsyntax-only", "-std=c11", "-x", "c"]).arg(&path).output()
    else {
        eprintln!("no cc on PATH: the C side of the layout is unchecked");
        return;
    };
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn the_header_names_the_constants_rust_uses() {
    let (_, text) = header();
    for (name, value) in [
        ("EPDF_ERROR_OK", EPDF_ERROR_OK as i64),
        ("EPDF_ERROR_KERNEL", EPDF_ERROR_KERNEL as i64),
        ("EPDF_ERROR_CLOSED", EPDF_ERROR_CLOSED as i64),
        ("EPDF_PIXEL_FORMAT_RGBA8", EPDF_PIXEL_FORMAT_RGBA8 as i64),
        ("EPDF_SEARCH_EVENT_STARTED", EPDF_SEARCH_EVENT_STARTED as i64),
        ("EPDF_SEARCH_EVENT_PROGRESS", EPDF_SEARCH_EVENT_PROGRESS as i64),
        ("EPDF_SEARCH_EVENT_COMPLETED", EPDF_SEARCH_EVENT_COMPLETED as i64),
        ("EPDF_SEARCH_EVENT_CANCELLED", EPDF_SEARCH_EVENT_CANCELLED as i64),
        ("EPDF_SEARCH_EVENT_FAILED", EPDF_SEARCH_EVENT_FAILED as i64),
        ("EPDF_SEARCH_EVENT_ACTIVE_HIT_CHANGED", EPDF_SEARCH_EVENT_ACTIVE_HIT_CHANGED as i64),
        ("EPDF_SEARCH_EVENT_CLEARED", EPDF_SEARCH_EVENT_CLEARED as i64),
        ("EPDF_SEARCH_EVENT_END", EPDF_SEARCH_EVENT_END as i64),
    ] {
        let line = format!("#define {name} {value}");
        assert!(text.contains(&line), "the header lacks `{line}`");
    }
}
