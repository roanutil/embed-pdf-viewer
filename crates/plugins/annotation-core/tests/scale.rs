//! Whole-document steps stay linear in the annotation count: hydration
//! reaps and removal filter through hash sets, as the TypeScript does
//! through `Set`. Each case here took seconds when membership was a linear
//! scan per annotation.

use std::time::Instant;

use epdf_annotation_core::*;

const N: usize = 20_000;

/// One template annotation from the reducer itself, stamped out `n` times
/// with engine refs so hydration may reap them.
fn annots(prefix: &str, n: usize) -> Vec<Annot> {
    let msg = Msg::CreateAnnot {
        page: PageRef::new(1),
        subtype: "square".into(),
        geom: Geom::Rect {
            rect: Rect { x: 0.0, y: 0.0, width: 5.0, height: 5.0 },
            ellipse: false,
            rot: None,
        },
        preset: None,
        props: None,
        flags: None,
        select: None,
    };
    let m = apply(&initial_model(), &msg).0;
    let template = m.by_id.values().next().expect("one annotation").clone();
    (0..n)
        .map(|i| {
            let id = format!("{prefix}{i}");
            Annot {
                id: id.clone(),
                ref_: Some(AnnotationRef::Nm { page: PageRef::new(1), nm: id }),
                ..template.clone()
            }
        })
        .collect()
}

#[test]
fn hydrating_over_a_large_document_reaps_in_linear_time() {
    let loaded = apply(&initial_model(), &Msg::Loaded { annots: annots("old", N) }).0;
    assert_eq!(loaded.order.len(), N);
    // Half the old annotations survive, half are reaped, and N/2 are new.
    let mut incoming = annots("old", N / 2);
    incoming.extend(annots("new", N / 2));

    let start = Instant::now();
    let hydrated = apply(&loaded, &Msg::Hydrated { annots: incoming, bump_ap: Some(false) }).0;
    let elapsed = start.elapsed();

    assert_eq!(hydrated.order.len(), N);
    assert_eq!(hydrated.order.first().map(String::as_str), Some("old0"));
    assert_eq!(hydrated.order.last().map(String::as_str), Some(&*format!("new{}", N / 2 - 1)));
    assert!(!hydrated.by_id.contains_key(&format!("old{}", N / 2)));
    assert!(elapsed.as_millis() < 1000, "hydration took {elapsed:?}");
}

#[test]
fn removing_most_of_a_large_document_runs_in_linear_time() {
    let loaded = apply(&initial_model(), &Msg::Loaded { annots: annots("a", N) }).0;
    let ids: Vec<Id> = loaded.order.iter().skip(1).cloned().collect();

    let start = Instant::now();
    let removed = apply(&loaded, &Msg::Remove { ids }).0;
    let elapsed = start.elapsed();

    assert_eq!(removed.order.as_slice(), ["a0".to_string()]);
    assert_eq!(removed.by_id.len(), 1);
    assert!(elapsed.as_millis() < 1000, "removal took {elapsed:?}");
}
