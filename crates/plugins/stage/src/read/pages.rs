//! `read/pages.ts`: the page reads. Laid-out pages with their transforms
//! (PDF points to view px to device px), the scroller projection, the
//! visible set (memoized so reads stay reference-stable), coordinate
//! conversions and the camera, zoom, viewpoint and destination reads.

use epdf_kernel::geometry::PageRef;

use crate::controller::{JsNumber, VisibleInputs};
use crate::prelude::*;

const EMPTY_SCROLL_METRICS: ScrollMetrics = ScrollMetrics {
    scroll_left: 0.0,
    scroll_top: 0.0,
    scroll_width: 0.0,
    scroll_height: 0.0,
    client_width: 0.0,
    client_height: 0.0,
    scrollable_x: false,
    scrollable_y: false,
};

impl StageCapability {
    /// The durable identity of the page at a display index, from the
    /// registry.
    pub(crate) fn page_ref_at(&self, index: usize) -> PageRef {
        let rows = self.registry_rows();
        rows.as_array()
            .and_then(|pages| pages.get(index))
            .and_then(|page| page.get("ref"))
            .and_then(PageRef::from_json)
            .unwrap_or(PageRef::new(index as u32 + 1))
    }

    /// The un-rotated point size of a laid-out box, for when the registry
    /// entry is momentarily absent.
    fn unrotated_points(b: &PageBox) -> Size {
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        Size { width: content.width / b.content_scale, height: content.height / b.content_scale }
    }

    /// Attach durable identity and the per-page transform. `None` where the
    /// TypeScript throws `Mat2D.invert: non-invertible matrix`: a view scale
    /// whose square underflows (a huge device ratio over a tiny unbounded
    /// zoom). No read here returns a `Result`, so the page is left out of
    /// the visible list and `get_page_frame` answers `None` (a Rust-only
    /// divergence, registered with the crate's other kept ones).
    pub(crate) fn with_transform(&self, b: &PageBox) -> Option<VisiblePage> {
        let registry = self.registry();
        let registered = registry.get(b.page_index).and_then(Option::as_ref);
        let page_size = registered.map_or_else(|| Self::unrotated_points(b), |r| r.size);
        let current = self.camera();
        let ratio = self.dpr();
        // The footprint's top-left, camera-resolved. Device-snapped at rest;
        // while the zoom moves it places fractionally.
        let raw_x = (b.x - current.x) * current.zoom;
        let raw_y = (b.y - current.y) * current.zoom;
        let resting = self.st().camera_resting;
        let screen_x = if resting { snap_to_device(raw_x, ratio) } else { raw_x };
        let screen_y = if resting { snap_to_device(raw_y, ratio) } else { raw_y };
        let transform = page_transform(PageTransformInput {
            page_size,
            rotation: b.rotation,
            scale: b.content_scale * current.zoom,
            // The page's physical 100%: the platform unit factor times its
            // /UserUnit.
            base_scale: Some(
                self.st().settings.view_units_per_point
                    * registered.and_then(|r| r.user_unit).unwrap_or(1.0),
            ),
            dpr: ratio,
        })
        .ok()?;
        // The on-screen page region in points: viewport ∩ footprint in view
        // space, inverted through the transform.
        let size = self.viewport();
        let on_screen = intersect_rects(
            Rect { x: -screen_x, y: -screen_y, width: size.width, height: size.height },
            Rect { x: 0.0, y: 0.0, width: transform.view_width, height: transform.view_height },
        );
        let visible_rect = if on_screen.width > 0.0 && on_screen.height > 0.0 {
            transform.view_to_page_rect(on_screen)
        } else {
            Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }
        };
        Some(VisiblePage {
            page: *b,
            page_ref: self.page_ref_at(b.page_index),
            screen_x,
            screen_y,
            transform,
            visible_rect,
        })
    }

    // ── the scroller projection ──

    /// The camera in native DOM vocabulary, against the bounds the pan clamp
    /// uses. The same value until a field moves.
    pub(crate) fn scroll_metrics_now(&self) -> ScrollMetrics {
        if !self.st().placed {
            return EMPTY_SCROLL_METRICS;
        }
        let scene = self.build_scene();
        let bounds = if scene.item_count > 0 {
            self.stay_bounds().expect("a scene with items has bounds")
        } else {
            Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }
        };
        let metrics = sc::scroll_metrics(self.camera(), bounds, self.viewport(), self.padding());
        if let Some(previous) = self.inner.last_metrics.get()
            && previous.scroll_left == metrics.scroll_left
            && previous.scroll_top == metrics.scroll_top
            && previous.scroll_width == metrics.scroll_width
            && previous.scroll_height == metrics.scroll_height
            && previous.client_width == metrics.client_width
            && previous.client_height == metrics.client_height
        {
            return previous;
        }
        self.inner.last_metrics.set(Some(metrics));
        metrics
    }

    /// The visible pages, memoized on everything they depend on (compared
    /// with `Object.is`), so the read is reference-stable. Paged flow
    /// renders only the slice's item; continuous flow the camera's window.
    pub(crate) fn visible_pages(&self) -> Rc<Vec<VisiblePage>> {
        if !self.st().placed {
            return self.inner.no_visible_pages.clone();
        }
        let current = self.camera();
        let size = self.viewport();
        self.build_scene(); // keys the scene cache that `scene_cache_key` reads
        let state = self.st();
        let inputs = VisibleInputs {
            scene_key: self.scene_cache_key(),
            flow: state.settings.flow,
            numbers: [
                JsNumber(current.x),
                JsNumber(current.y),
                JsNumber(current.zoom),
                JsNumber(size.width),
                JsNumber(size.height),
                JsNumber(self.dpr()),
            ],
            resting: state.camera_resting,
        };
        if let Some((last, pages)) = self.inner.visible_memo.borrow().as_ref()
            && *last == inputs
        {
            return pages.clone();
        }
        let scene = self.build_scene();
        let items: Vec<&SceneItem> = if self.paged() {
            scene.items.iter().take(1).collect()
        } else {
            scene.query(sc::camera_world_rect(current, size))
        };
        let pages: Vec<VisiblePage> = items
            .iter()
            .flat_map(|item| item.pages.iter())
            .filter_map(|b| self.with_transform(b))
            .collect();
        let pages = Rc::new(pages);
        *self.inner.visible_memo.borrow_mut() = Some((inputs, pages.clone()));
        pages
    }

    /// The visible entry of a page given by its ref or its index; `None`
    /// when it is off screen or not in the document.
    fn visible_page_of(&self, page: PageTarget) -> Option<VisiblePage> {
        let page = self.get_page(page)?.get("ref").and_then(PageRef::from_json)?;
        self.visible_pages().iter().find(|visible| visible.page_ref == page).cloned()
    }
}

// ── the area's api ──

impl StageCapability {
    /// Camera origin (world units) and zoom.
    pub fn get_camera(&self) -> Camera {
        self.camera()
    }

    /// The container size the stage was told about.
    pub fn get_viewport_size(&self) -> Size {
        self.viewport()
    }

    /// The camera as a native scroller, in viewport px. Equal to the
    /// previous answer until a field moves.
    pub fn get_scroll_metrics(&self) -> ScrollMetrics {
        self.scroll_metrics_now()
    }

    /// Pages on screen, with their viewport rects and visible page-space
    /// rect. The same `Rc` until the camera, scene, viewport, ratio or rest
    /// state changes.
    pub fn list_visible_pages(&self) -> Rc<Vec<VisiblePage>> {
        self.visible_pages()
    }

    /// Is any part of the page (its ref or its index) on screen. False for
    /// a page that isn't in the document.
    pub fn is_page_visible(&self, page: impl Into<PageTarget>) -> bool {
        self.visible_page_of(page.into()).is_some()
    }

    /// The current page's display index, from 0.
    pub fn get_current_page_index(&self) -> usize {
        self.st().cursor
    }

    /// The current page's ref, `None` before the document has pages.
    pub fn get_current_page(&self) -> Option<PageRef> {
        let cursor = self.st().cursor;
        let rows = self.registry_rows();
        rows.as_array()?.get(cursor)?.get("ref").and_then(PageRef::from_json)
    }

    /// How many pages the document has.
    pub fn get_page_count(&self) -> usize {
        self.page_count()
    }

    /// The registry rows of the current item's pages (one page, or a
    /// spread or grid row).
    pub fn list_current_item_pages(&self) -> Vec<PageInfo> {
        let Some(item) = self.cursor_item() else { return Vec::new() };
        let rows = self.registry_rows();
        let pages = rows.as_array().map(Vec::as_slice).unwrap_or_default();
        item.page_indexes.iter().filter_map(|&i| pages.get(i).cloned()).collect()
    }

    /// The laid-out box for a page (its ref or its index), on screen or
    /// not; `None` before placement or for a page the document lacks.
    pub fn get_page_frame(&self, page: impl Into<PageTarget>) -> Option<VisiblePage> {
        if !self.st().placed {
            return None;
        }
        let index = self.index_of(page.into())?;
        let scene = self.build_scene();
        if scene.item_count == 0 {
            return None;
        }
        // `scene.items[scene.itemOfPage(index)]`: an index past the scene's
        // pages reads `undefined` there and throws; here it answers `None`.
        let item = &scene.items[scene.item_of_page(index)?];
        let b = item.pages.iter().find(|p| p.page_index == index)?;
        self.with_transform(b)
    }

    /// The page under a viewport point and the page-space point on it;
    /// `None` over a gap.
    pub fn get_page_at(&self, point: ViewportPoint) -> Option<PageHit> {
        for page in self.visible_pages().iter() {
            let local_x = point.x - page.screen_x;
            let local_y = point.y - page.screen_y;
            if local_x >= 0.0
                && local_y >= 0.0
                && local_x <= page.transform.view_width
                && local_y <= page.transform.view_height
            {
                return Some(PageHit {
                    page_ref: page.page_ref,
                    point: page.transform.view_to_page(Point { x: local_x, y: local_y }),
                    scale: page.transform.view_scale,
                    rotation: page.page.rotation,
                    zoom: page.transform.zoom,
                });
            }
        }
        None
    }

    /// Project a viewport point onto one page (its ref or its index),
    /// unclamped; `None` when the page is not on screen or not in the
    /// document.
    pub fn viewport_to_page(
        &self,
        page: impl Into<PageTarget>,
        point: ViewportPoint,
    ) -> Option<Point> {
        let target = self.visible_page_of(page.into())?;
        Some(
            target
                .transform
                .view_to_page(Point { x: point.x - target.screen_x, y: point.y - target.screen_y }),
        )
    }

    /// A page-space point to world space; `None` when the page is not laid
    /// out.
    pub fn page_to_world(&self, page: impl Into<PageTarget>, point: Point) -> Option<Point> {
        let frame = self.get_page_frame(page)?;
        let b = frame.page;
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        let matrix =
            rotate_scale_matrix(b.content_scale, content.width, content.height, b.rotation);
        let offset = apply_point(matrix, point);
        Some(Point { x: b.x + offset.x, y: b.y + offset.y })
    }

    /// A page-space rect to its viewport-space bounding box; `None` when the
    /// page is not laid out.
    pub fn page_rect_to_viewport(
        &self,
        page: impl Into<PageTarget>,
        rect: Rect,
    ) -> Option<ViewportRect> {
        let frame = self.get_page_frame(page)?;
        let b = frame.page;
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        let matrix =
            rotate_scale_matrix(b.content_scale, content.width, content.height, b.rotation);
        let world = apply_rect(matrix, rect);
        let current = self.camera();
        let top_left = sc::to_screen(current, Point { x: b.x + world.x, y: b.y + world.y });
        Some(Rect {
            x: top_left.x,
            y: top_left.y,
            width: world.width * current.zoom,
            height: world.height * current.zoom,
        })
    }

    pub fn world_to_viewport(&self, world: Point) -> ViewportPoint {
        sc::to_screen(self.camera(), world)
    }

    pub fn viewport_to_world(&self, point: ViewportPoint) -> Point {
        sc::to_world(self.camera(), point)
    }

    /// A page-space point to this stage's viewport; `None` when the page is
    /// not laid out.
    pub fn page_to_viewport(
        &self,
        page: impl Into<PageTarget>,
        point: Point,
    ) -> Option<ViewportPoint> {
        let world = self.page_to_world(page, point)?;
        Some(sc::to_screen(self.camera(), world))
    }

    /// The lens's view rotation.
    pub fn get_view_rotation(&self) -> PageRotation {
        self.st().settings.view_rotation
    }

    /// Resolved zoom factor.
    pub fn get_zoom_level(&self) -> f64 {
        self.camera().zoom
    }

    /// The active zoom intent: a fit mode, or custom for a fixed level.
    pub fn get_zoom_mode(&self) -> StageZoomMode {
        StageZoomMode::of(self.st().settings.zoom)
    }

    /// What the viewer is looking at plus the zoom intent, for per-page
    /// view memory. `Err` where the TypeScript throws: no item under the
    /// anchor point.
    pub fn get_viewpoint(&self) -> Result<Viewpoint, Error> {
        Ok(Viewpoint { anchor: self.current_anchor()?, zoom: self.st().settings.zoom })
    }

    /// Where the view is, as a destination: the first page on screen, the
    /// point of it at the viewport's top-left corner (a padding in), and no
    /// zoom. `None` before a page is on screen.
    pub fn get_current_destination(&self) -> Option<PageDestination> {
        let inset = self.padding();
        let current = self.camera();
        let visible = self.visible_pages();
        // `.filter(...).sort((l, r) => l.pageIndex - r.pageIndex)[0]`: the
        // first of the lowest index, a stable sort.
        let first = visible
            .iter()
            .filter(|page| {
                let corner = sc::to_screen(
                    current,
                    Point { x: page.page.x + page.page.width, y: page.page.y + page.page.height },
                );
                corner.x > inset && corner.y > inset
            })
            .fold(None::<&VisiblePage>, |best, page| match best {
                Some(best) if best.page.page_index <= page.page.page_index => Some(best),
                _ => Some(page),
            })?;
        let row = self.get_page(PageTarget::Ref(first.page_ref))?;
        let size: Size = serde_json::from_value(row.get("size")?.clone()).ok()?;
        let corner = first
            .transform
            .view_to_page(Point { x: inset - first.screen_x, y: inset - first.screen_y });
        let clamp =
            |value: f64, max: f64| js::round(js::min(js::max(value, 0.0), max) * 100.0) / 100.0;
        Some(PageDestination::Xyz {
            page: first.page_ref,
            x: Some(clamp(corner.x, size.width)),
            y: Some(clamp(corner.y, size.height)),
            zoom: None,
        })
    }
}
