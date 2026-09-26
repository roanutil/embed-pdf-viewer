//! `read/pages.ts`: the page reads. Laid-out pages with their transforms
//! (PDF points to view px to device px), the scroller projection, the
//! visible set (memoized so selectors stay reference-stable), coordinate
//! conversions and the camera, zoom and viewpoint reads.

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
    /// The durable identity for a display index, from the registry
    /// captured at open.
    pub(crate) fn ref_for_index(&self, index: usize) -> PageRef {
        self.registry()
            .get(index)
            .map_or(PageRef::new(index as PageObjectNumber + 1), |p| p.page_ref)
    }

    /// Fallback un-rotated point size from a laid-out box.
    pub(crate) fn unrotated_points(b: &PageBox) -> Size {
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        Size { width: content.width / b.content_scale, height: content.height / b.content_scale }
    }

    pub(crate) fn with_transform(&self, b: &PageBox) -> VisiblePage {
        let registry = self.registry();
        let reg = registry.get(b.page_index);
        let page_size = reg.map_or_else(|| Self::unrotated_points(b), |r| r.size);
        let c = self.cam();
        let ratio = self.dpr();
        let raw_x = (b.x - c.x) * c.zoom;
        let raw_y = (b.y - c.y) * c.zoom;
        let resting = self.st().camera_resting;
        let screen_x = if resting { snap_to_device(raw_x, ratio) } else { raw_x };
        let screen_y = if resting { snap_to_device(raw_y, ratio) } else { raw_y };
        let transform = page_transform(PageTransformInput {
            page_size,
            rotation: b.rotation,
            scale: b.content_scale * c.zoom,
            base_scale: Some(
                self.st().settings.view_units_per_point
                    * reg.and_then(|r| r.user_unit).unwrap_or(1.0),
            ),
            dpr: ratio,
        })
        .expect("a page transform at a positive zoom is invertible");
        let v = self.vp();
        let on_screen = intersect_rects(
            Rect { x: -screen_x, y: -screen_y, width: v.width, height: v.height },
            Rect { x: 0.0, y: 0.0, width: transform.view_width, height: transform.view_height },
        );
        let visible_rect = if on_screen.width > 0.0 && on_screen.height > 0.0 {
            transform.view_to_content_rect(on_screen)
        } else {
            Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }
        };
        VisiblePage {
            page: *b,
            page_ref: self.ref_for_index(b.page_index),
            screen_x,
            screen_y,
            transform,
            visible_rect,
        }
    }

    // ── scroller projection ───────────────────────────────────────────────────

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
        let m = sc::scroll_metrics(self.cam(), bounds, self.vp(), self.pad());
        if let Some(p) = self.inner.scroll_memo.get()
            && p.scroll_left == m.scroll_left
            && p.scroll_top == m.scroll_top
            && p.scroll_width == m.scroll_width
            && p.scroll_height == m.scroll_height
            && p.client_width == m.client_width
            && p.client_height == m.client_height
        {
            return p;
        }
        self.inner.scroll_memo.set(Some(m));
        m
    }

    pub(crate) fn visible_pages_now(&self) -> Rc<Vec<VisiblePage>> {
        if !self.st().placed {
            return Rc::new(Vec::new());
        }
        let c = self.cam();
        let v = self.vp();
        let scene = self.build_scene();
        let items: Vec<&SceneItem> = if self.paged() {
            scene.items.iter().take(1).collect()
        } else {
            scene.query(sc::camera_world_rect(c, v))
        };
        let st = self.st();
        let sig = format!(
            "{}|{}|{},{},{}|{}x{}|{}|r{}",
            self.scene_cache_key(),
            serde_str(&st.settings.flow),
            num(c.x),
            num(c.y),
            num(c.zoom),
            num(v.width),
            num(v.height),
            num(self.dpr()),
            if st.camera_resting { 1 } else { 0 }
        );
        if self.inner.vis.borrow().0 == sig {
            return self.inner.vis.borrow().1.clone();
        }
        let vis: Vec<VisiblePage> =
            items.iter().flat_map(|it| it.pages.iter()).map(|b| self.with_transform(b)).collect();
        let vis = Rc::new(vis);
        *self.inner.vis.borrow_mut() = (sig, vis.clone());
        vis
    }
}

// ── the area's api ────────────────────────────────────────────────────────────

impl StageCapability {
    /// Camera origin (world units) and zoom.
    pub fn get_camera(&self) -> Camera {
        self.cam()
    }

    /// The container size the stage was told about.
    pub fn get_viewport_size(&self) -> Size {
        self.vp()
    }

    /// The camera as a native scroller, in viewport px, over the pan
    /// clamp's travel range. Equal to the previous answer until a field
    /// moves.
    pub fn get_scroll_metrics(&self) -> ScrollMetrics {
        self.scroll_metrics_now()
    }

    /// The laid-out pages on screen (paged: the slice's). The same `Rc`
    /// until the camera, scene, viewport, ratio or rest state changes.
    pub fn list_visible_pages(&self) -> Rc<Vec<VisiblePage>> {
        self.visible_pages_now()
    }

    /// Is any part of the page on screen.
    pub fn is_page_visible(&self, page: PageRef) -> bool {
        self.visible_pages_now().iter().any(|p| p.page_ref == page)
    }

    /// The cursor page's display index.
    pub fn get_current_page_index(&self) -> usize {
        self.st().cursor
    }

    /// The cursor page's registry row, `None` before the document has
    /// pages.
    pub fn get_current_page(&self) -> Option<PageInfo> {
        let cursor = self.st().cursor;
        self.ctx.document()?.pages.as_array()?.get(cursor).cloned()
    }

    /// The registry rows of the current item's pages (one page, or a
    /// spread or grid row).
    pub fn list_current_item_pages(&self) -> Vec<PageInfo> {
        let Some(item) = self.cursor_item() else { return Vec::new() };
        let pages = self.ctx.document().map(|d| d.pages).unwrap_or_default();
        let pages = pages.as_array().map(Vec::as_slice).unwrap_or_default();
        item.page_indexes.iter().filter_map(|&i| pages.get(i).cloned()).collect()
    }

    /// The laid-out box for a page, `None` before placement or for a page
    /// the document does not have.
    pub fn get_page_frame(&self, page: PageRef) -> Option<VisiblePage> {
        if !self.st().placed {
            return None;
        }
        let index = self.index_of_page(page)?;
        let scene = self.build_scene();
        if scene.item_count == 0 {
            return None;
        }
        let item = &scene.items[scene.item_of_page(index)?];
        let b = item.pages.iter().find(|p| p.page_index == index)?;
        Some(self.with_transform(b))
    }

    /// The page under a viewport point and the page-space point on it;
    /// `None` over a gap.
    pub fn get_page_at(&self, point: ViewportPoint) -> Option<PageHit> {
        for p in self.visible_pages_now().iter() {
            let lx = point.x - p.screen_x;
            let ly = point.y - p.screen_y;
            if lx >= 0.0
                && ly >= 0.0
                && lx <= p.transform.view_width
                && ly <= p.transform.view_height
            {
                return Some(PageHit {
                    page_ref: p.page_ref,
                    point: p.transform.view_to_content(Point { x: lx, y: ly }),
                    scale: p.transform.view_scale,
                    rotation: p.page.rotation,
                    zoom: p.transform.zoom,
                });
            }
        }
        None
    }

    /// Project a viewport point onto one page's frame, unclamped; `None`
    /// when the page is not laid out on screen.
    pub fn viewport_to_page(&self, page: PageRef, point: ViewportPoint) -> Option<Point> {
        let vis = self.visible_pages_now();
        let p = vis.iter().find(|v| v.page_ref == page)?;
        Some(
            p.transform.view_to_content(Point { x: point.x - p.screen_x, y: point.y - p.screen_y }),
        )
    }

    /// Page space (intrinsic PDF points) to world space.
    pub fn page_to_world(&self, page: PageRef, point: Point) -> Option<Point> {
        let pr = self.get_page_frame(page)?;
        let b = pr.page;
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        let m = rotate_scale_matrix(b.content_scale, content.width, content.height, b.rotation);
        let offset = apply_point(m, point);
        Some(Point { x: b.x + offset.x, y: b.y + offset.y })
    }

    /// A page-space rect to its viewport-space bounding box.
    pub fn page_rect_to_viewport(&self, page: PageRef, rect: Rect) -> Option<ViewportRect> {
        let pr = self.get_page_frame(page)?;
        let b = pr.page;
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        let m = rotate_scale_matrix(b.content_scale, content.width, content.height, b.rotation);
        let wr = apply_rect(m, rect);
        let c = self.cam();
        let tl = sc::to_screen(c, Point { x: b.x + wr.x, y: b.y + wr.y });
        Some(Rect { x: tl.x, y: tl.y, width: wr.width * c.zoom, height: wr.height * c.zoom })
    }

    pub fn world_to_viewport(&self, world: Point) -> ViewportPoint {
        sc::to_screen(self.cam(), world)
    }

    pub fn viewport_to_world(&self, point: ViewportPoint) -> Point {
        sc::to_world(self.cam(), point)
    }

    /// A page-space point to this stage's viewport.
    pub fn page_to_viewport(&self, page: PageRef, point: Point) -> Option<ViewportPoint> {
        let world = self.page_to_world(page, point)?;
        Some(sc::to_screen(self.cam(), world))
    }

    /// The lens's non-persistent view rotation.
    pub fn get_view_rotation(&self) -> PageRotation {
        self.st().settings.view_rotation
    }

    /// Resolved zoom factor.
    pub fn get_zoom_level(&self) -> f64 {
        self.cam().zoom
    }

    /// The active zoom intent: a fit mode, or custom for a fixed level.
    pub fn get_zoom_mode(&self) -> StageZoomMode {
        StageZoomMode::of(self.st().settings.zoom)
    }

    /// What the viewer is looking at plus the zoom intent, for per-page
    /// view memory.
    pub fn get_viewpoint(&self) -> Viewpoint {
        Viewpoint { anchor: self.current_anchor(), zoom: self.st().settings.zoom }
    }
}
