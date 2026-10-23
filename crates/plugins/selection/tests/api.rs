//! The typed API beyond what the vectors reach: the capability as the
//! target of a handle drag (the TypeScript passes the capability where a
//! `SelectionHandleTarget` is expected; here that is a trait impl), and a
//! search hit's span selecting with no casts.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use epdf_geometry::{Point, Quad};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt;
use epdf_plugin_selection::*;

use common::{BOTH, kernel, selection};

const PAGE: PageRef = PageRef { page_object_number: 1 };

/// Overlay px are page px shifted by (100, 0).
struct Shifted;

impl SelectionHandleView for Shifted {
    fn to_overlay(&self, _: PageRef, p: Point) -> Option<Point> {
        Some(Point { x: p.x + 100.0, y: p.y })
    }
    fn page_at(&self, o: Point) -> Option<handles::PagePoint> {
        Some(handles::PagePoint { page: PAGE, point: Point { x: o.x - 100.0, y: o.y } })
    }
    fn point_on_page(&self, _: PageRef, o: Point) -> Option<Point> {
        Some(Point { x: o.x - 100.0, y: o.y })
    }
}

#[test]
fn a_handle_drag_drives_the_capability() {
    let (kernel, _) = kernel(1, BOTH, Vec::new());
    let selection = selection(&kernel);
    let _ = selection.ensure_loaded(PAGE);
    rt::run_until_stalled();
    let commits = Rc::new(Cell::new(0));
    let c = commits.clone();
    let _off = selection.on_committed(move |_| {
        c.set(c.get() + 1);
        Ok(())
    });

    // The opposite endpoint is glyph 0's cell; drag toward glyph 2.
    let cell = Quad {
        upper_left: Point { x: 50.0, y: 50.0 },
        upper_right: Point { x: 56.0, y: 50.0 },
        lower_left: Point { x: 50.0, y: 62.0 },
        lower_right: Point { x: 56.0, y: 62.0 },
    };
    let opposite = SelectionHandleEndpoint { page: PAGE, glyph_quad: cell, advance: 1 };
    let session =
        create_selection_handle_drag(Rc::new(selection.clone()), Rc::new(Shifted), &opposite, PAGE);
    session.move_to(Point { x: 165.0, y: 56.0 });
    assert!(selection.is_selecting());
    let range = selection.get_snapshot().range.unwrap();
    assert_eq!((range.start.index, range.end.index), (0, 3));
    session.end();
    assert!(!selection.is_selecting());
    assert_eq!(commits.get(), 1);
}

#[test]
fn an_armed_handle_extends_from_the_opposite_end() {
    let (kernel, _) = kernel(1, BOTH, Vec::new());
    let selection = selection(&kernel);
    let _ = selection.ensure_loaded(PAGE);
    rt::run_until_stalled();
    selection.select(SelectionRangeInput::Span { page: PAGE, start: 0, count: 2 }).unwrap();
    let endpoints = selection_handle_endpoints_of(&selection.get_snapshot()).expect("two ends");
    let armed = arm_selection_handle(
        Rc::new(selection.clone()),
        Rc::new(Shifted),
        &endpoints,
        HandleRole::End,
    )
    .expect("the end handle is laid out");
    // The end handle's bar is glyph 1's trailing edge, x 62, y 50..62.
    assert_eq!(armed.base, Point { x: 162.0, y: 56.0 });
    armed.drag.move_to(Point { x: 165.0, y: 56.0 });
    armed.drag.end();
    let range = selection.get_range().unwrap();
    assert_eq!((range.start.index, range.end.index), (0, 3));
}

#[test]
fn a_search_hits_float_span_selects_without_casts() {
    let (kernel, _) = kernel(1, BOTH, Vec::new());
    let selection = selection(&kernel);
    let _ = selection.ensure_loaded(PAGE);
    rt::run_until_stalled();
    let (char_start, char_count) = (1.0_f64, 2.0_f64); // a SearchHit's fields
    selection.select(SelectionRangeInput::span(PAGE, char_start, char_count)).unwrap();
    let range = selection.get_snapshot().range.unwrap();
    assert_eq!((range.start.index, range.end.index), (1, 3));
    assert_eq!(
        SelectionRangeInput::span(PAGE, f64::NAN, 1e300),
        SelectionRangeInput::Span { page: PAGE, start: 0, count: i64::MAX }
    );
}

#[test]
fn settings_change_for_every_document_and_reset_to_the_registered_ones() {
    let (kernel, _) = kernel(1, BOTH, Vec::new());
    let selection = selection(&kernel);
    assert_eq!(selection.get_settings(), selection_defaults());
    let seen = Rc::new(Cell::new(0));
    let s = seen.clone();
    let _off = selection.on_settings_changed(move |event| {
        assert_eq!(event.changed, vec!["dragThreshold".to_string()]);
        s.set(s.get() + 1);
        Ok(())
    });
    selection.update_settings(&SelectionConfig { drag_threshold: Some(9.0), ..Default::default() });
    assert_eq!(selection.get_settings().drag_threshold, 9.0);
    selection.update_settings(&SelectionConfig { drag_threshold: Some(9.0), ..Default::default() });
    assert_eq!(seen.get(), 1, "a change that changes nothing fires nothing");
    selection.reset_settings();
    assert_eq!(selection.get_settings(), selection_defaults());
}
