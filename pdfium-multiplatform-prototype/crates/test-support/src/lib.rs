//! The one reader of vectors/report-page4.json, which build/generate-vectors.mjs
//! wrote once from the shipping TypeScript path and which nothing regenerates.
//! A dev-dependency only: no production crate depends on this.
use std::path::{Path, PathBuf};

pub struct Vectors {
    pub page_index: u32,
    pub page_count: u32,
    pub width: f32,
    pub height: f32,
    pub char_count: u32,
    pub text: String,
    pub search_query: String,
    pub search_case_sensitive: bool,
    pub search_hit_count: usize,
    pub search_first_hits: Vec<(u32, u32)>,
    pub render_scale: f32,
    pub render_width: u32,
    pub render_height: u32,
    pub render_stride: u32,
    pub golden: Vec<u8>,
    pub tolerance: f64,
    pub max_tolerance: u8,
}

/// The scaffold root, from this crate's own manifest directory.
pub fn scaffold_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn fixture_bytes() -> Vec<u8> {
    let path = scaffold_root().join("fixtures/report.pdf");
    std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

pub fn vectors() -> Vectors {
    let root = scaffold_root();
    let raw = std::fs::read_to_string(root.join("vectors/report-page4.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let size = &v["size"];
    let search = &v["search"];
    let render = &v["render"];

    Vectors {
        page_index: v["pageIndex"].as_u64().unwrap() as u32,
        page_count: v["pageCount"].as_u64().unwrap() as u32,
        width: size["width"].as_f64().unwrap() as f32,
        height: size["height"].as_f64().unwrap() as f32,
        char_count: v["charCount"].as_u64().unwrap() as u32,
        text: v["text"].as_str().unwrap().to_string(),
        search_query: search["query"].as_str().unwrap().to_string(),
        search_case_sensitive: search["caseSensitive"].as_bool().unwrap(),
        search_hit_count: search["hitCount"].as_u64().unwrap() as usize,
        search_first_hits: search["firstHits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| {
                (
                    hit["charIndex"].as_u64().unwrap() as u32,
                    hit["charCount"].as_u64().unwrap() as u32,
                )
            })
            .collect(),
        render_scale: render["scale"].as_f64().unwrap() as f32,
        render_width: render["width"].as_u64().unwrap() as u32,
        render_height: render["height"].as_u64().unwrap() as u32,
        render_stride: render["stride"].as_u64().unwrap() as u32,
        golden: std::fs::read(root.join("vectors").join(render["golden"].as_str().unwrap())).unwrap(),
        tolerance: render["meanAbsDiffTolerance"].as_f64().unwrap(),
        max_tolerance: render["maxAbsDiffTolerance"].as_u64().unwrap() as u8,
    }
}

/// Mean absolute per-byte difference. Rendering is compared this way rather
/// than by hash because a hash pins float and rasterization behaviour across
/// four architectures and fails for reasons unrelated to this code.
pub fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len(), "buffer lengths differ: {} vs {}", a.len(), b.len());
    let total: u64 = a.iter().zip(b).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    total as f64 / a.len() as f64
}

/// Largest single-byte absolute difference. `mean_abs_diff` averages over the
/// whole buffer, so one badly wrong region (a missing glyph, say, which
/// produces deltas near 255) can hide under a mean-based tolerance as long as
/// most of the buffer matches. This bounds the worst byte instead.
pub fn max_abs_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len(), "buffer lengths differ: {} vs {}", a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}
