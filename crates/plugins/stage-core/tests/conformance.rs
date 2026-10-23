//! `vectors/core/stage-*.json`, generated from `packages/core/stage`.

use epdf_stage_core::harness::functions;
use plugin_conformance::{Registry, assert_suite};

fn replay(file: &str) {
    assert_suite(file, &Registry::new(), &functions());
}

#[test]
fn camera_matches_the_typescript() {
    replay("core/stage-camera.json");
}

#[test]
fn layout_matches_the_typescript() {
    replay("core/stage-layout.json");
}

#[test]
fn anchor_matches_the_typescript() {
    replay("core/stage-anchor.json");
}

// Edge inputs the stage plugin feeds this crate. Each expected value is what
// `packages/core/stage/src/index.ts` answers for the same input under Node.
// The stage plugin's empty-scene guard (review findings M27 and R5, in
// `crates/plugins/stage/src/services/scene.rs`) relies on the three layout
// facts pinned here: an empty linear layout succeeds, a grouping that names
// a missing page fails, and a NaN column count fails, each without a panic.
mod edge_inputs {
    use epdf_stage_core::*;

    const PAGE: PageGeom =
        PageGeom { size: Size { width: 100.0, height: 200.0 }, rotation: None, user_unit: None };
    const NAN_PAGE: PageGeom = PageGeom {
        size: Size { width: f64::NAN, height: f64::NAN },
        rotation: None,
        user_unit: None,
    };
    const VP: Size = Size { width: 800.0, height: 600.0 };
    const VP_ZERO: Size = Size { width: 0.0, height: 0.0 };
    const VP_NAN: Size = Size { width: f64::NAN, height: f64::NAN };
    const ORIGIN: Camera = Camera { x: 0.0, y: 0.0, zoom: 1.0 };

    fn singles(n: usize) -> Vec<Vec<usize>> {
        (0..n).map(|i| vec![i]).collect()
    }

    fn grid(pages: &[PageGeom], opts: GridOptions) -> Result<Scene, StageError> {
        grid_layout(pages, &singles(pages.len()), opts)
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect { x, y, width, height }
    }

    fn assert_nan(value: f64, what: &str) {
        assert!(value.is_nan(), "{what}: expected NaN, got {value}");
    }

    #[test]
    fn an_empty_scene_lays_out_and_answers_nothing() {
        let linear = linear_layout(&[], &[], LinearOptions::default()).expect("empty linear");
        assert_eq!(linear.size, Size { width: 0.0, height: 0.0 });
        let grid = grid_layout(&[], &[], GridOptions::default()).expect("empty grid");
        assert_eq!(grid.size, Size { width: 1.0, height: 1.0 });
        for scene in [&linear, &grid] {
            assert_eq!(scene.item_count, 0);
            assert!(scene.query(rect(0.0, 0.0, 10.0, 10.0)).is_empty());
            assert!(scene.nearest_item(Point { x: 0.0, y: 0.0 }).is_none());
            assert_eq!(scene.item_of_page(0), None);
            let pages = StageError::ReadOfUndefined("pages");
            assert_eq!(anchor_from_camera(ORIGIN, scene, VP_ZERO, None), Err(pages));
            let anchor = Anchor { page_index: 0, fx: 0.0, fy: 0.0 };
            assert_eq!(camera_from_anchor(anchor, scene, VP_ZERO, 1.0, None), Err(pages));
        }
    }

    #[test]
    fn a_grouping_that_names_a_missing_page_fails_without_a_panic() {
        let missing = StageError::ReadOfUndefined("userUnit");
        assert_eq!(
            linear_layout(&[PAGE], &[vec![0], vec![1]], LinearOptions::default()),
            Err(missing)
        );
        assert_eq!(grid_layout(&[PAGE], &[vec![0, 1]], GridOptions::default()), Err(missing));
        assert_eq!(missing.name(), "TypeError");
        assert_eq!(missing.to_string(), "Cannot read properties of undefined (reading 'userUnit')");
    }

    #[test]
    fn a_nan_column_count_fails_without_a_panic() {
        for opts in [
            GridOptions { columns: Some(f64::NAN), ..GridOptions::default() },
            GridOptions { line_width: Some(f64::NAN), ..GridOptions::default() },
        ] {
            assert_eq!(grid(&[PAGE], opts), Err(StageError::InvalidArrayLength));
        }
        // A NaN page size makes the cell width NaN, so an auto wrap's column
        // count is NaN too; a fixed count still lays out, at NaN size.
        let auto = GridOptions { line_width: Some(500.0), ..GridOptions::default() };
        assert_eq!(grid(&[NAN_PAGE, PAGE], auto), Err(StageError::InvalidArrayLength));
        let fixed =
            grid(&[NAN_PAGE, PAGE], GridOptions { columns: Some(2.0), ..GridOptions::default() })
                .expect("fixed columns over a NaN page");
        assert_nan(fixed.size.width, "fixed grid width");
        assert_nan(fixed.size.height, "fixed grid height");
        let err = StageError::InvalidArrayLength;
        assert_eq!((err.name(), err.to_string().as_str()), ("RangeError", "Invalid array length"));
    }

    #[test]
    fn infinite_column_counts_clamp_to_the_item_count() {
        let three = [PAGE, PAGE, PAGE];
        for opts in [
            GridOptions { columns: Some(f64::INFINITY), ..GridOptions::default() },
            GridOptions { line_width: Some(f64::INFINITY), ..GridOptions::default() },
        ] {
            assert_eq!(grid(&three, opts).unwrap().size, Size { width: 396.0, height: 200.0 });
        }
        let one_column = GridOptions { columns: Some(f64::NEG_INFINITY), ..GridOptions::default() };
        assert_eq!(grid(&three, one_column).unwrap().size, Size { width: 100.0, height: 696.0 });
    }

    #[test]
    fn nan_points_on_a_grid_find_no_item() {
        let scene =
            grid(&[PAGE, PAGE, PAGE], GridOptions { columns: Some(2.0), ..GridOptions::default() })
                .unwrap();
        assert!(scene.nearest_item(Point { x: f64::NAN, y: f64::NAN }).is_none());
        assert!(scene.query(rect(f64::NAN, f64::NAN, f64::NAN, f64::NAN)).is_empty());
        assert_eq!(
            anchor_from_camera(ORIGIN, &scene, VP_NAN, None),
            Err(StageError::ReadOfUndefined("pages"))
        );
        let at_zero_zoom = anchor_from_camera(Camera { zoom: 0.0, ..ORIGIN }, &scene, VP, None)
            .expect("an infinite point clamps to the last item");
        assert_eq!(at_zero_zoom.page_index, 2);
        assert_eq!((at_zero_zoom.fx, at_zero_zoom.fy), (f64::INFINITY, f64::INFINITY));
    }

    #[test]
    fn a_nan_page_size_on_a_linear_layout_still_answers() {
        let scene =
            linear_layout(&[NAN_PAGE, PAGE], &singles(2), LinearOptions::default()).unwrap();
        assert_nan(scene.size.width, "linear width");
        assert_nan(scene.size.height, "linear height");
        assert_eq!(scene.nearest_item(Point { x: 0.0, y: 10.0 }).map(|it| it.index), Some(0));
    }

    #[test]
    fn singular_and_nan_viewports_answer_numbers() {
        let fit_page = ZoomSpec::Mode { mode: ZoomMode::FitPage };
        let page = Size { width: 100.0, height: 200.0 };
        assert_eq!(resolve_zoom(fit_page, page, VP_ZERO, 0.0), ZOOM_MIN);
        assert_nan(resolve_zoom(fit_page, page, VP_NAN, 0.0), "fit-page zoom in a NaN viewport");
        let fit_width = ZoomSpec::Mode { mode: ZoomMode::FitWidth };
        assert_eq!(resolve_zoom(fit_width, Size { width: 0.0, height: 0.0 }, VP, 0.0), ZOOM_MAX);

        let world = camera_world_rect(Camera { x: 1.0, y: 2.0, zoom: 0.0 }, VP);
        assert_eq!(world, rect(1.0, 2.0, f64::INFINITY, f64::INFINITY));

        let centered = center_on_world(Point { x: 50.0, y: 50.0 }, VP_NAN, 1.0);
        assert_eq!(centered.zoom, 1.0);
        assert_nan(centered.x, "centered x");
        assert_nan(centered.y, "centered y");

        let bounds = rect(0.0, 0.0, 100.0, 100.0);
        let metrics = scroll_metrics(ORIGIN, bounds, VP_NAN, 0.0);
        assert_eq!((metrics.scroll_left, metrics.scroll_top), (0.0, 0.0));
        assert_nan(metrics.scroll_width, "scroll width");
        assert_nan(metrics.client_height, "client height");
        assert!(!metrics.scrollable_x && !metrics.scrollable_y);

        let bounded =
            CameraConstraint { bounded: true, padding: 0.0, fit_align: None, direction: None };
        let held = clamp_camera(Camera { x: 5.0, y: 5.0, zoom: 1.0 }, bounds, VP_ZERO, bounded);
        assert_eq!(held, Camera { x: 5.0, y: 5.0, zoom: 1.0 });

        let placed = place_camera(
            rect(0.0, 0.0, 10.0, 10.0),
            VP,
            0.0,
            0.0,
            AlignmentValue::default(),
            Direction::Ltr,
        );
        assert_eq!(placed.zoom, 0.0);
        assert_nan(placed.x, "placed x at zero zoom");

        let revealed = reveal_camera(ORIGIN, rect(0.0, 0.0, 10.0, 10.0), VP_NAN, 0.0);
        assert_nan(revealed.x, "revealed x");
        assert_nan(revealed.y, "revealed y");
    }

    #[test]
    fn a_linear_scene_anchors_through_a_singular_camera() {
        let scene = linear_layout(&[PAGE, PAGE], &singles(2), LinearOptions::default()).unwrap();
        let anchor = anchor_from_camera(ORIGIN, &scene, VP_NAN, None).unwrap();
        assert_eq!(anchor.page_index, 0);
        assert_nan(anchor.fx, "anchor fx");
        let camera =
            camera_from_anchor(Anchor { page_index: 1, fx: 0.5, fy: 0.5 }, &scene, VP, 0.0, None)
                .unwrap();
        assert_eq!(camera, Camera { zoom: 0.0, x: f64::NEG_INFINITY, y: f64::NEG_INFINITY });
    }
}
