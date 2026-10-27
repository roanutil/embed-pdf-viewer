//! G11's pure gate: the appearance decision the update rests on.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-write
//!
//! The decision is pure and silent when wrong, which is why it is pinned
//! apart from the update that uses it. A wrong verdict either destroys a
//! stream a foreign producer wrote, which cannot be recomputed, or keeps one
//! that no longer matches the dictionary it is supposed to draw.
//!
//! The update classifies the patch it resolved (9d8ebf857): a drawn kind's
//! `rect` is first resolved into the shape that puts the drawing there. The
//! cases whose patch names such a `rect` record that `resolvedPatch`, and it
//! is what is classified here, after checking the resolution gives it.
//!
//! The weak-reference impact cases are gone with upstream's weak references
//! and `computeMutationImpact` (bc0057f3a).

use epdf_engine::features::annotation::write::appearance::{
    AppearanceImpact, appearance_impact_of,
};
use epdf_engine::features::annotation::write::shape_for_rect::pdf_resolve_rect_command;
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    appearance: Vec<AppearanceCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppearanceCase {
    name: String,
    covers: String,
    current: serde_json::Value,
    patch: serde_json::Value,
    /// The patch the update classifies, when its `rect` resolves into the
    /// shape.
    #[serde(default)]
    resolved_patch: Option<serde_json::Value>,
    impact: String,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("annotation").join("write.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("write.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "annotation.write");
    file
}

fn impact_name(impact: AppearanceImpact) -> &'static str {
    match impact {
        AppearanceImpact::Inert => "inert",
        AppearanceImpact::Translation => "translation",
        AppearanceImpact::Regenerate => "regenerate",
    }
}

#[test]
fn the_appearance_verdicts_agree_with_the_typescript_engine() {
    let file = load();
    assert!(file.appearance.len() >= 20, "the case list shrank");

    let mut seen = std::collections::BTreeSet::new();
    let mut resolved = 0usize;
    for case in &file.appearance {
        let patch = match &case.resolved_patch {
            Some(expected) => {
                // The rect command gives the recorded resolution.
                let mine = pdf_resolve_rect_command(
                    case.current.as_object().expect("a read"),
                    case.patch.as_object().expect("a patch").clone(),
                )
                .unwrap_or_else(|e| panic!("{}: {e}", case.name));
                assert_eq!(
                    test_support::canonical_numbers(&serde_json::Value::Object(mine)),
                    test_support::canonical_numbers(expected),
                    "{}: the resolved patch",
                    case.name,
                );
                resolved += 1;
                expected
            }
            None => &case.patch,
        };
        let mine = impact_name(appearance_impact_of(&case.current, patch));
        assert_eq!(mine, case.impact, "{}: {}", case.name, case.covers);
        seen.insert(case.impact.as_str());
    }

    // All three verdicts have to occur, or a classifier that answered one
    // thing always would pass.
    assert_eq!(
        seen,
        ["inert", "regenerate", "translation"].into_iter().collect(),
        "the case list no longer produces every verdict",
    );
    assert_eq!(resolved, 3, "the cases that resolve a rect moved");
}

#[test]
fn a_translation_is_verified_rather_than_claimed() {
    // The theorem this classifier exists to check. A caller saying "this is
    // just a move" is not evidence; the geometry is. Stated here on a case
    // the vector does not carry, so the property is pinned independently of
    // the case list. A drawn kind's move shows in its points: its `rect` is
    // the engine's, resolved into them before the verdict.
    let current = serde_json::json!({
        "subtype": "polyline",
        "rect": { "left": 0.0, "bottom": 0.0, "right": 100.0, "top": 50.0 },
        "vertices": [{ "x": 0.0, "y": 0.0 }, { "x": 100.0, "y": 50.0 }],
    });

    // Every vertex moves by the same delta.
    let honest = serde_json::json!({
        "subtype": "polyline",
        "vertices": [{ "x": 7.0, "y": -3.0 }, { "x": 107.0, "y": 47.0 }],
    });
    assert_eq!(appearance_impact_of(&current, &honest), AppearanceImpact::Translation);

    // One vertex off by a point. Below any visible difference and above the
    // epsilon, which is exactly the case a tolerance that was too loose
    // would wave through.
    let nearly = serde_json::json!({
        "subtype": "polyline",
        "vertices": [{ "x": 7.0, "y": -3.0 }, { "x": 108.0, "y": 47.0 }],
    });
    assert_eq!(appearance_impact_of(&current, &nearly), AppearanceImpact::Regenerate);

    // A shift within the tolerance is still a translation: the epsilon
    // exists because these numbers round-trip through f32.
    let rounded = serde_json::json!({
        "subtype": "polyline",
        "vertices": [{ "x": 7.0, "y": -3.0 }, { "x": 107.0005, "y": 47.0 }],
    });
    assert_eq!(appearance_impact_of(&current, &rounded), AppearanceImpact::Translation);

    // The rect sent beside the points is no geometry of a polyline's: it
    // is resolved away first, and here it is a style change.
    let with_rect = serde_json::json!({
        "subtype": "polyline",
        "rect": { "left": 7.0, "bottom": -3.0, "right": 107.0, "top": 47.0 },
        "vertices": [{ "x": 7.0, "y": -3.0 }, { "x": 107.0, "y": 47.0 }],
    });
    assert_eq!(appearance_impact_of(&current, &with_rect), AppearanceImpact::Regenerate);
}
