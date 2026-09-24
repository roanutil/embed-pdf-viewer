//! G11's pure gate: the two decisions the mutators rest on.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-write
//!
//! Both are pure and both are silent when wrong, which is why they are
//! pinned before the mutators that use them.
//!
//! A wrong APPEARANCE verdict either destroys a stream a foreign producer
//! wrote, which cannot be recomputed, or keeps one that no longer matches the
//! dictionary it is supposed to draw. A wrong IMPACT verdict either makes
//! every client re-read after every edit, or lets one go on dereferencing an
//! index that now points at a different annotation.

use epdf_engine::features::annotation::write::appearance::{
    AppearanceImpact, appearance_impact_of,
};
use epdf_engine::features::annotation::write::impact::{
    AnnotationMutationKind, compute_mutation_impact,
};
use epdf_engine::identity::{AnnotationStableId, PageObjectNumber};
use epdf_engine::revision::{PageState, RevisionToken, WeakAnnotationState};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;
const SESSION: &str = "<session>";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    page_object_number: u32,
    appearance: Vec<AppearanceCase>,
    mutations: Vec<MutationCase>,
}

#[derive(Debug, Deserialize)]
struct AppearanceCase {
    name: String,
    covers: String,
    current: serde_json::Value,
    patch: serde_json::Value,
    impact: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MutationCase {
    name: String,
    covers: String,
    mutation: String,
    weak_before: serde_json::Value,
    weak_after: serde_json::Value,
    generation_before: u64,
    generation_after: u64,
    changed: Vec<AnnotationStableId>,
    /// Present when the TypeScript produced a verdict.
    #[serde(default)]
    meta: Option<serde_json::Value>,
    /// Present instead when it threw.
    #[serde(default)]
    threw: Option<String>,
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

fn mutation_kind(name: &str) -> AnnotationMutationKind {
    match name {
        "create" => AnnotationMutationKind::Create,
        "update" => AnnotationMutationKind::Update,
        "delete" => AnnotationMutationKind::Delete,
        "move" => AnnotationMutationKind::Move,
        other => panic!("the vector names a mutation {other} this runner does not map"),
    }
}

fn weak_state(value: &serde_json::Value) -> WeakAnnotationState {
    match value["kind"].as_str() {
        Some("known") => WeakAnnotationState::Known {
            has_any_weak_annotations: value["hasAnyWeakAnnotations"]
                .as_bool()
                .expect("a known state carries its flag"),
        },
        Some("unknown") => WeakAnnotationState::Unknown,
        other => panic!("the vector names a weak state {other:?} this runner does not map"),
    }
}

fn page_state(page: u32, generation: u64, weak: &serde_json::Value) -> PageState {
    let page = PageObjectNumber::new(page).expect("a page object number");
    PageState {
        page_object_number: page,
        revision: RevisionToken::new(SESSION, page, generation),
        weak_annotation_state: weak_state(weak),
    }
}

#[test]
fn the_appearance_verdicts_agree_with_the_typescript_engine() {
    let file = load();
    assert!(file.appearance.len() >= 20, "the case list shrank");

    let mut seen = std::collections::BTreeSet::new();
    for case in &file.appearance {
        let mine = impact_name(appearance_impact_of(&case.current, &case.patch));
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
}

#[test]
fn the_mutation_impacts_agree_with_the_typescript_engine() {
    let file = load();
    assert!(file.mutations.len() >= 6, "the case list shrank");
    let mut refetched = 0usize;
    let mut quiet = 0usize;

    for case in &file.mutations {
        let before = page_state(file.page_object_number, case.generation_before, &case.weak_before);
        let after = page_state(file.page_object_number, case.generation_after, &case.weak_after);
        let mine = compute_mutation_impact(
            mutation_kind(&case.mutation),
            &before,
            &after,
            case.changed.clone(),
        );

        match (&mine, &case.meta, &case.threw) {
            (Ok(meta), Some(theirs), None) => {
                assert_eq!(
                    serde_json::to_value(meta).expect("serialize"),
                    *theirs,
                    "{}: {}",
                    case.name,
                    case.covers,
                );
                if meta.should_refetch.is_some() {
                    refetched += 1;
                } else {
                    quiet += 1;
                }
            }
            (Err(_), None, Some(_)) => {}
            (Ok(_), None, Some(message)) => {
                panic!("{}: TypeScript refused this and Rust did not: {message}", case.name)
            }
            (Err(error), Some(_), None) => {
                panic!("{}: Rust refused this and TypeScript did not: {error}", case.name)
            }
            _ => panic!("{}: the vector carries neither a verdict nor a refusal", case.name),
        }
    }

    assert!(refetched > 0, "no case in the list asks for a refetch");
    assert!(quiet > 0, "every case in the list asks for a refetch");
}

#[test]
fn only_delete_and_move_shift_an_index() {
    // The doctrine the whole envelope rests on, stated where it can be read
    // rather than inferred from six cases. A create APPENDS, so the new
    // annotation lands past everything that was already there.
    assert!(!AnnotationMutationKind::Create.shifts_existing_indices());
    assert!(!AnnotationMutationKind::Update.shifts_existing_indices());
    assert!(AnnotationMutationKind::Delete.shifts_existing_indices());
    assert!(AnnotationMutationKind::Move.shifts_existing_indices());
}

#[test]
fn an_index_shifting_mutation_needs_a_scanned_page() {
    // The TypeScript throws here and this returns an error, which is the one
    // deliberate difference. A caller that asks what a delete invalidated
    // without having scanned the page has a bug, and unwinding a mutation's
    // bookkeeping through a panic would leave the session in a shape nobody
    // has reasoned about.
    let file = load();
    let before = page_state(file.page_object_number, 1, &serde_json::json!({ "kind": "unknown" }));
    let after = page_state(file.page_object_number, 2, &serde_json::json!({ "kind": "unknown" }));

    for kind in [AnnotationMutationKind::Delete, AnnotationMutationKind::Move] {
        let refused = compute_mutation_impact(kind, &before, &after, Vec::new())
            .expect_err("an unknown weak state cannot answer for an index-shifting mutation");
        assert_eq!(refused.kind, kind);
    }

    // And the non-shifting pair never consults it, so an unknown state is
    // not an error for them.
    for kind in [AnnotationMutationKind::Create, AnnotationMutationKind::Update] {
        compute_mutation_impact(kind, &before, &after, Vec::new()).expect("never consulted");
    }
}

#[test]
fn a_translation_is_verified_rather_than_claimed() {
    // The theorem this classifier exists to check. A caller saying "this is
    // just a move" is not evidence; the geometry is. Stated here on a case
    // the vector does not carry, so the property is pinned independently of
    // the case list.
    let current = serde_json::json!({
        "subtype": "polyline",
        "rect": { "left": 0.0, "bottom": 0.0, "right": 100.0, "top": 50.0 },
        "vertices": [{ "x": 0.0, "y": 0.0 }, { "x": 100.0, "y": 50.0 }],
    });

    // The rect moves and the vertices move with it by the same delta.
    let honest = serde_json::json!({
        "subtype": "polyline",
        "rect": { "left": 7.0, "bottom": -3.0, "right": 107.0, "top": 47.0 },
        "vertices": [{ "x": 7.0, "y": -3.0 }, { "x": 107.0, "y": 47.0 }],
    });
    assert_eq!(appearance_impact_of(&current, &honest), AppearanceImpact::Translation);

    // The same rect move with ONE vertex off by a point. Below any visible
    // difference and above the epsilon, which is exactly the case a
    // tolerance that was too loose would wave through.
    let nearly = serde_json::json!({
        "subtype": "polyline",
        "rect": { "left": 7.0, "bottom": -3.0, "right": 107.0, "top": 47.0 },
        "vertices": [{ "x": 7.0, "y": -3.0 }, { "x": 108.0, "y": 47.0 }],
    });
    assert_eq!(appearance_impact_of(&current, &nearly), AppearanceImpact::Regenerate);

    // And a shift that is within the tolerance is still a translation: the
    // epsilon exists because these numbers round-trip through f32.
    let rounded = serde_json::json!({
        "subtype": "polyline",
        "rect": { "left": 7.0, "bottom": -3.0, "right": 107.0, "top": 47.0 },
        "vertices": [{ "x": 7.0, "y": -3.0 }, { "x": 107.0005, "y": 47.0 }],
    });
    assert_eq!(appearance_impact_of(&current, &rounded), AppearanceImpact::Translation);
}
