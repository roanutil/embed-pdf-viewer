use epdf_ffi::{
    Modifiers, PageRef, Point, PointerKind, PointerPageHit, PointerPhase, PointerSample,
};

#[test]
fn modifiers_default_to_none_held() {
    assert_eq!(
        Modifiers::default(),
        Modifiers { shift: false, alt: false, ctrl: false, meta: false }
    );
}

#[test]
fn a_sample_over_a_page_keeps_its_page_point() {
    let sample = PointerSample {
        phase: PointerPhase::Down,
        viewport: Point { x: 10.0, y: 20.0 },
        page: Some(PointerPageHit {
            page: PageRef { page_object_number: 4 },
            point: Point { x: 1.0, y: 2.0 },
            scale: Some(2.0),
            rotation: None,
            zoom: None,
        }),
        modifiers: Modifiers::default(),
        click_count: Some(2),
        pointer_type: Some(PointerKind::Touch),
        gesture: None,
        source: None,
    };
    assert_eq!(sample.clone(), sample);
    assert_eq!(sample.page.unwrap().point, Point { x: 1.0, y: 2.0 });
}
