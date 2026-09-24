//! G4: the render geometry agrees with the TypeScript.
//!
//! The arithmetic that turns a PDF region into a device raster, before any
//! pixel exists. Pixels are checked against a fixture; this is checked against
//! vectors, because a matrix that is subtly wrong produces a picture that is
//! subtly wrong and nobody notices by looking at it.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run render

use epdf_engine::features::render::geometry::{
    RenderViewport, device_size, display_rect_to_device_matrix, pdf_rect_to_display_rect,
};
use epdf_engine::geom::{PdfRect, PdfRotation};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
struct VectorFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    kind: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Rect {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
}

impl From<&Rect> for PdfRect {
    fn from(r: &Rect) -> Self {
        PdfRect { left: r.left, bottom: r.bottom, right: r.right, top: r.top }
    }
}

#[derive(Debug, Deserialize)]
struct Size {
    width: u32,
    height: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Viewport {
    Scale { scale: f64 },
    Width { width: f64 },
}

impl From<&Viewport> for RenderViewport {
    fn from(v: &Viewport) -> Self {
        match v {
            Viewport::Scale { scale } => RenderViewport::Scale { scale: *scale },
            Viewport::Width { width } => RenderViewport::Width { width: *width },
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    covers: String,
    rect: Rect,
    page_height: f64,
    rotation: u16,
    viewport: Viewport,
    display_rect: Rect,
    size: Size,
    matrix: Vec<f64>,
}

fn rotation(degrees: u16) -> PdfRotation {
    match degrees {
        90 => PdfRotation::Clockwise90,
        180 => PdfRotation::Clockwise180,
        270 => PdfRotation::Clockwise270,
        _ => PdfRotation::None,
    }
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("render").join("geometry.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("geometry.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "render.geometry");
    file
}

#[test]
fn the_display_flip_agrees_with_typescript() {
    // Checked on its own because it feeds the other two: a wrong flip makes
    // every size and every matrix wrong for a reason neither would name.
    let file = load();
    for case in &file.cases {
        let actual = pdf_rect_to_display_rect((&case.rect).into(), case.page_height);
        let expected: PdfRect = (&case.display_rect).into();
        assert_eq!(actual, expected, "{}: {}", case.name, case.covers);
    }
}

#[test]
fn device_sizes_agree_with_typescript() {
    let file = load();
    assert!(file.cases.len() >= 20, "the corpus shrank");
    for case in &file.cases {
        let actual = device_size(
            (&case.display_rect).into(),
            rotation(case.rotation),
            (&case.viewport).into(),
        )
        .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(actual, (case.size.width, case.size.height), "{}: {}", case.name, case.covers,);
    }
}

#[test]
fn device_matrices_agree_with_typescript() {
    let file = load();
    for case in &file.cases {
        let matrix = display_rect_to_device_matrix(
            (&case.display_rect).into(),
            rotation(case.rotation),
            case.size.width,
            case.size.height,
        );
        let actual: Vec<f64> = matrix
            .to_array()
            .iter()
            .map(|cell| test_support::corpus::quantize_units(*cell) as f64 / 10_000.0)
            .collect();
        assert_eq!(actual.len(), case.matrix.len(), "{}: the matrix changed shape", case.name,);
        for (index, (got, want)) in actual.iter().zip(&case.matrix).enumerate() {
            assert_eq!(
                got, want,
                "{}: matrix cell {index} is {got}, TypeScript said {want} ({})",
                case.name, case.covers,
            );
        }
    }
}

#[test]
fn a_viewport_that_cannot_produce_pixels_is_refused() {
    // Not in the vectors: the TypeScript throws, and a thrown error has no
    // shape to record. Checked here so the refusal exists on both sides.
    let rect = PdfRect { left: 0.0, bottom: 0.0, right: 100.0, top: 100.0 };
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            device_size(rect, PdfRotation::None, RenderViewport::Scale { scale }).is_err(),
            "scale {scale} should be refused",
        );
    }
    for width in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            device_size(rect, PdfRotation::None, RenderViewport::Width { width }).is_err(),
            "width {width} should be refused",
        );
    }
}

#[test]
fn a_region_with_no_area_is_refused_rather_than_rendered_empty() {
    use epdf_engine::error::EngineErrorCode;
    use epdf_engine::features::render::geometry::{RenderTarget, resolve_target};

    // The matrix would divide by zero, and a caller asking for a zero-area
    // region has a bug worth naming.
    for rect in [
        PdfRect { left: 10.0, bottom: 10.0, right: 10.0, top: 20.0 },
        PdfRect { left: 10.0, bottom: 10.0, right: 20.0, top: 10.0 },
    ] {
        let refused =
            resolve_target(RenderTarget::Rect { rect }, 612.0, 792.0).expect_err("zero area");
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
    }
    // And a whole-page target is the page box.
    assert_eq!(
        resolve_target(RenderTarget::Page, 612.0, 792.0).expect("page"),
        PdfRect { left: 0.0, bottom: 0.0, right: 612.0, top: 792.0 },
    );
}

#[test]
fn an_inverted_rect_is_normalized_rather_than_refused() {
    use epdf_engine::features::render::geometry::{RenderTarget, resolve_target};

    // A caller that wrote its corners the other way round meant the same
    // region, and normalizing is what the TypeScript does.
    let inverted = PdfRect { left: 200.0, bottom: 300.0, right: 100.0, top: 150.0 };
    assert_eq!(
        resolve_target(RenderTarget::Rect { rect: inverted }, 612.0, 792.0).expect("normalized"),
        PdfRect { left: 100.0, bottom: 150.0, right: 200.0, top: 300.0 },
    );
}
