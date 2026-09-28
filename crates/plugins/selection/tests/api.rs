//! The typed API beyond what the vectors reach: the capability as the
//! target of a handle drag (the TypeScript passes the capability where a
//! `SelectionHandleTarget` is expected; here that is a trait impl).

use std::cell::Cell;
use std::rc::Rc;

use epdf_geometry::{Point, TextQuad};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_selection::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

const PAGE: PageRef = PageRef { page_object_number: 1 };

/// Page 1 with "abc" at x 50..68, y-up 738..750 (content y 50..62).
fn kernel() -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let glyph = |left: f64| json!({ "looseBox": { "left": left, "bottom": 738, "right": left + 6.0, "top": 750 }, "flags": 0 });
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "pages": [{
                "index": 0, "ref": { "kind": "objectNumber", "pageObjectNumber": 1 }, "label": null,
                "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
                "boxes": { "media": box_, "crop": box_ }
            }],
            "security": { "allows": ["doc.text.select"] },
            "responses": [{ "path": "page.geometry.read", "value": { "runs": [{
                "rect": { "left": 50, "bottom": 738, "right": 68, "top": 750 },
                "charStart": 0,
                "glyphs": [glyph(50.0), glyph(56.0), glyph(62.0)]
            }] } }]
        } }
    }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![
            interaction_plugin(InteractionConfig::default()),
            selection_plugin(SelectionConfig::default()),
        ],
        KernelOptions { report: None, on_dispatch: None, release_closed_sessions: false },
    )
    .unwrap();
    let _ = kernel.start();
    rt::run_until_stalled();
    let _ = kernel
        .documents()
        .open(OpenSource::Input(json!({ "kind": "bytes", "id": "a" })), OpenOptions::default());
    rt::run_until_stalled();
    kernel
}

/// Overlay px are content px shifted by (100, 0).
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
    let kernel = kernel();
    let selection = kernel.capability(&SELECTION, Some("a")).unwrap();
    let _ = selection.ensure_loaded(PAGE);
    rt::run_until_stalled();
    let commits = Rc::new(Cell::new(0));
    let c = commits.clone();
    let _off = selection.on_committed(move |_| {
        c.set(c.get() + 1);
        Ok(())
    });

    // The opposite endpoint is glyph 0's cell; drag toward glyph 2.
    let cell = TextQuad {
        upper_start: Point { x: 50.0, y: 50.0 },
        upper_end: Point { x: 56.0, y: 50.0 },
        lower_start: Point { x: 50.0, y: 62.0 },
        lower_end: Point { x: 56.0, y: 62.0 },
    };
    let opposite = SelectionHandleEndpoint { page: PAGE, glyph_quad: cell, advance: 1 };
    let session =
        create_selection_handle_drag(Rc::new(selection.clone()), Rc::new(Shifted), &opposite, PAGE);
    session.move_to(Point { x: 165.0, y: 56.0 });
    assert!(selection.is_gesture_active());
    let range = selection.get_snapshot().range.unwrap();
    assert_eq!((range.start.index, range.end.index), (0, 3));
    session.end();
    assert!(!selection.is_gesture_active());
    assert_eq!(commits.get(), 1);
}
