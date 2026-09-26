//! `services/scene.ts`: the scene model. The document's item grouping,
//! the layout, the memoized scene (continuous: the whole document; paged: a
//! one-item slice at the origin), and the pure geometry reads every area
//! builds on: fit, alignment points, clamp bounds, anchors. Nothing here
//! dispatches.

use crate::prelude::*;

impl StageCapability {
    // ── state reads ───────────────────────────────────────────────────────────

    pub(crate) fn st(&self) -> Rc<StageState> {
        self.ctx.state()
    }

    pub(crate) fn cam(&self) -> Camera {
        self.st().camera
    }

    pub(crate) fn vp(&self) -> Size {
        self.st().vp
    }

    pub(crate) fn dpr(&self) -> f64 {
        self.st().dpr
    }

    pub(crate) fn pad(&self) -> f64 {
        self.st().settings.padding
    }

    pub(crate) fn paged(&self) -> bool {
        self.st().settings.flow == FlowMode::Paged
    }

    pub(crate) fn is_fit_all(&self) -> bool {
        matches!(self.st().settings.zoom, ZoomSpec::Mode { mode: ZoomMode::FitAll })
    }

    /// `ctx.document()?.pages`, parsed once per registry revision.
    pub(crate) fn registry(&self) -> Rc<Vec<PageLayout>> {
        let Some(doc) = self.ctx.document() else { return Rc::new(Vec::new()) };
        let key = (doc.page_count, doc.revision);
        if let Some((k, pages)) = self.inner.registry_cache.borrow().as_ref()
            && *k == key
        {
            return pages.clone();
        }
        let pages: Vec<PageLayout> = doc
            .pages
            .as_array()
            .map(|pages| {
                pages
                    .iter()
                    .filter_map(|p| serde_json::from_value(without_undefined(p)).ok())
                    .collect()
            })
            .unwrap_or_default();
        let pages = Rc::new(pages);
        *self.inner.registry_cache.borrow_mut() = Some((key, pages.clone()));
        pages
    }

    pub(crate) fn page_count_now(&self) -> usize {
        self.ctx.document().map_or(0, |d| d.page_count as usize)
    }

    // ── the item model (spread grouping) ─────────────────────────────────────

    pub(crate) fn grouping(&self) -> Rc<Grouping> {
        let key = (self.page_count_now(), self.st().settings.spread);
        if let Some(g) = self.inner.grouping_cache.borrow().as_ref()
            && g.key == key
        {
            return g.clone();
        }
        let g = sc::group_pages(key.0, key.1);
        let first_pages = g.iter().map(|item| item[0]).collect();
        let cache =
            Rc::new(Grouping { key, grouping: Rc::new(g), first_pages: Rc::new(first_pages) });
        *self.inner.grouping_cache.borrow_mut() = Some(cache.clone());
        cache
    }

    pub(crate) fn item_index_of_page(&self, page_index: usize) -> usize {
        let g = self.grouping();
        let fp = &g.first_pages;
        let (mut lo, mut hi) = (0, fp.len());
        while lo < hi {
            let mid = (lo + hi) >> 1;
            if fp[mid] <= page_index {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo.saturating_sub(1)
    }

    // ── screen-px settings in world units ─────────────────────────────────────

    pub(crate) fn effective_zoom(&self) -> f64 {
        let st = self.st();
        match st.settings.zoom {
            ZoomSpec::Level { level } => level,
            _ => js::max(st.camera.zoom, 0.0001),
        }
    }

    pub(crate) fn wrap_line_width(&self) -> f64 {
        js::max(1.0, (self.vp().width - 2.0 * self.pad()) / self.effective_zoom())
    }

    pub(crate) fn no_frame(m: &epdf_geometry::PageFrame) -> bool {
        falsy(m.top) && falsy(m.right) && falsy(m.bottom) && falsy(m.left)
    }

    pub(crate) fn world_page_frame(&self) -> epdf_geometry::PageFrame {
        let m = self.st().settings.page_frame;
        if Self::no_frame(&m) {
            return m;
        }
        let ez = self.effective_zoom();
        epdf_geometry::PageFrame {
            top: m.top / ez,
            right: m.right / ez,
            bottom: m.bottom / ez,
            left: m.left / ez,
        }
    }

    pub(crate) fn frame_key(&self) -> String {
        let m = self.st().settings.page_frame;
        if Self::no_frame(&m) {
            return "-".into();
        }
        let w = self.world_page_frame();
        format!(
            "{},{},{},{}",
            num(js::round(w.top)),
            num(js::round(w.right)),
            num(js::round(w.bottom)),
            num(js::round(w.left))
        )
    }

    pub(crate) fn world_gap(&self) -> f64 {
        match self.st().settings.gap {
            Gap::World(g) => g,
            Gap::Px { px } if !falsy(px) => px / self.effective_zoom(),
            Gap::Px { .. } => 0.0,
        }
    }

    pub(crate) fn gap_key(&self) -> String {
        match self.st().settings.gap {
            Gap::World(g) => num(g),
            Gap::Px { .. } => format!("px:{}", num(js::round(self.world_gap()))),
        }
    }

    pub(crate) fn layout_for(&self, groups: &[Vec<usize>]) -> Scene {
        let st = self.st();
        let s = &st.settings;
        // The view-rotation injection point: each page's display rotation
        // is its /Rotate plus this lens's viewRotation.
        let vr = s.view_rotation;
        let pages: Vec<PageGeom> = self
            .registry()
            .iter()
            .map(|p| PageGeom {
                size: p.size,
                rotation: Some(if vr.0 == 0.0 {
                    p.rotation
                } else {
                    add_rotations(p.rotation, vr)
                }),
                user_unit: p.user_unit,
            })
            .collect();
        let page_frame = self.world_page_frame();
        let gap = self.world_gap();
        let vupp = s.view_units_per_point;
        // stage-core fails only on a grouping that names a missing page or
        // a NaN column count; the grouping comes from the same registry.
        let scene = if s.layout == LayoutKind::Grid {
            sc::grid_layout(
                &pages,
                groups,
                sc::GridOptions {
                    gap: Some(gap),
                    columns: match s.columns {
                        GridColumns::Count(n) => Some(n),
                        GridColumns::Policy(_) => None,
                    },
                    line_width: (s.columns == GridColumns::Policy(ColumnsPolicy::Auto))
                        .then(|| self.wrap_line_width()),
                    sizing: Some(s.sizing),
                    direction: Some(s.direction),
                    page_frame: Some(page_frame),
                    view_units_per_point: Some(vupp),
                },
            )
        } else {
            sc::linear_layout(
                &pages,
                groups,
                sc::LinearOptions {
                    gap: Some(gap),
                    axis: Some(if s.layout == LayoutKind::Horizontal {
                        sc::LinearAxis::X
                    } else {
                        sc::LinearAxis::Y
                    }),
                    align: None,
                    sizing: Some(s.sizing),
                    direction: Some(s.direction),
                    page_frame: Some(page_frame),
                    view_units_per_point: Some(vupp),
                },
            )
        };
        scene.unwrap_or_else(|e| panic!("[stage] layout: {}: {e}", e.name()))
    }

    pub(crate) fn columns_key(&self) -> String {
        let st = self.st();
        if st.settings.layout != LayoutKind::Grid {
            return "-".into();
        }
        match st.settings.columns {
            GridColumns::Policy(ColumnsPolicy::Auto) => {
                format!("auto:{}", num(js::round(self.wrap_line_width())))
            }
            GridColumns::Policy(ColumnsPolicy::Square) => "square".into(),
            GridColumns::Count(n) => num(n),
        }
    }

    /// The scene's settings signature, over the `'scene'` rows of the
    /// registry in key order. Primitives key by `String(v)`; the three
    /// screen-px settings quantize.
    pub(crate) fn settings_key(&self) -> String {
        let st = self.st();
        let s = &st.settings;
        let mut parts = Vec::new();
        for key in SETTING_KEYS {
            if settings_effect(key) != SettingEffect::Scene {
                continue;
            }
            parts.push(match key {
                SettingKey::Columns => self.columns_key(),
                SettingKey::Gap => self.gap_key(),
                SettingKey::PageFrame => self.frame_key(),
                SettingKey::Layout => serde_str(&s.layout),
                SettingKey::Spread => serde_str(&s.spread),
                SettingKey::Sizing => serde_str(&s.sizing),
                SettingKey::Direction => serde_str(&s.direction),
                SettingKey::ViewRotation => num(s.view_rotation.0),
                SettingKey::ViewUnitsPerPoint => num(s.view_units_per_point),
                other => unreachable!("{other:?} is not a scene setting"),
            });
        }
        parts.join("|")
    }

    pub(crate) fn doc_key(&self) -> String {
        match self.ctx.document() {
            Some(doc) => format!("{}.{}", doc.page_count, doc.revision),
            None => "0.0".into(),
        }
    }

    /// The scene: the whole document (continuous), or a one-item slice at
    /// the cursor (paged). Cached by key; the same `Rc` means the same
    /// scene, as the TypeScript compares scenes by reference.
    pub(crate) fn build_scene(&self) -> Rc<Scene> {
        let g = self.grouping();
        let groups = &g.grouping;
        let (key, slice) = if self.paged() {
            let idx = if groups.is_empty() {
                0
            } else {
                self.item_index_of_page(self.st().cursor).min(groups.len() - 1)
            };
            (format!("paged|{}|{}|{idx}", self.doc_key(), self.settings_key()), Some(idx))
        } else {
            (format!("cont|{}|{}", self.doc_key(), self.settings_key()), None)
        };
        if let Some((k, scene)) = self.inner.scene_cache.borrow().as_ref()
            && *k == key
        {
            return scene.clone();
        }
        let scene = Rc::new(match slice {
            Some(idx) => {
                self.layout_for(if groups.is_empty() { &[] } else { &groups[idx..idx + 1] })
            }
            None => self.layout_for(groups),
        });
        *self.inner.scene_cache.borrow_mut() = Some((key, scene.clone()));
        scene
    }

    pub(crate) fn scene_cache_key(&self) -> String {
        self.inner.scene_cache.borrow().as_ref().map(|(k, _)| k.clone()).unwrap_or_default()
    }

    pub(crate) fn invalidate_scene(&self) {
        *self.inner.scene_cache.borrow_mut() = None;
    }

    // ── geometry helpers ──────────────────────────────────────────────────────

    pub(crate) fn scene_rect(&self) -> Rect {
        let size = self.build_scene().size;
        Rect { x: 0.0, y: 0.0, width: size.width, height: size.height }
    }

    pub(crate) fn item_rect(it: &SceneItem) -> Rect {
        Rect { x: it.x, y: it.y, width: it.width, height: it.height }
    }

    pub(crate) fn page_box_of(it: &SceneItem, page_index: usize) -> &PageBox {
        it.pages.iter().find(|p| p.page_index == page_index).unwrap_or(&it.pages[0])
    }

    pub(crate) fn page_rect_of(it: &SceneItem, page_index: usize) -> Rect {
        let b = Self::page_box_of(it, page_index);
        Rect { x: b.x, y: b.y, width: b.width, height: b.height }
    }

    /// The item shown for the cursor.
    pub(crate) fn cursor_item(&self) -> Option<SceneItem> {
        let scene = self.build_scene();
        if self.paged() {
            scene.items.first().cloned()
        } else {
            scene.items.get(self.item_index_of_page(self.st().cursor)).cloned()
        }
    }

    /// Content-space rect on a page to a world rect, through the same
    /// quarter-turn matrix `pageRectToScreen` uses.
    pub(crate) fn world_rect_for_content(it: &SceneItem, page_index: usize, rect: Rect) -> Rect {
        let b = Self::page_box_of(it, page_index);
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        let m = rotate_scale_matrix(b.content_scale, content.width, content.height, b.rotation);
        let wr = apply_rect(m, rect);
        Rect { x: b.x + wr.x, y: b.y + wr.y, width: wr.width, height: wr.height }
    }

    /// Does this rect fit the padded viewport at this zoom?
    pub(crate) fn fits(&self, rect: Rect, zoom: f64) -> bool {
        let v = self.vp();
        let p = self.pad();
        let eps = 0.5;
        rect.width * zoom <= v.width - 2.0 * p + eps
            && rect.height * zoom <= v.height - 2.0 * p + eps
    }

    pub(crate) fn align_fraction(a: AlignValue) -> f64 {
        match a {
            AlignValue::Named(Align::Start) => 0.0,
            AlignValue::Named(Align::Center) => 0.5,
            AlignValue::Named(Align::End) => 1.0,
            AlignValue::Fraction(f) => js::min(1.0, js::max(0.0, f)),
        }
    }

    /// An alignment policy to a viewport point, interpolating the padded
    /// range; named x stops are logical under RTL.
    pub(crate) fn align_point(&self, al: AlignmentValue, v: Size) -> Point {
        let rtl = self.st().settings.direction == Direction::Rtl;
        let ax = match al.x {
            AlignValue::Named(Align::Start) if rtl => AlignValue::Named(Align::End),
            AlignValue::Named(Align::End) if rtl => AlignValue::Named(Align::Start),
            x => x,
        };
        let p = self.pad();
        Point {
            x: p + (v.width - 2.0 * p) * Self::align_fraction(ax),
            y: p + (v.height - 2.0 * p) * Self::align_fraction(al.y),
        }
    }

    pub(crate) fn anchor_point(&self) -> Point {
        self.align_point(self.st().settings.anchor_align, self.vp())
    }

    /// The fit box the zoom intent resolves against.
    pub(crate) fn fit_box(&self, item: &SceneItem) -> Size {
        if self.is_fit_all() {
            return self.build_scene().size;
        }
        if self.paged() {
            Size { width: item.width, height: item.height }
        } else {
            self.build_scene().max_item_size
        }
    }

    pub(crate) fn bounds_for(&self, it: &SceneItem) -> Rect {
        if self.paged() { Self::item_rect(it) } else { self.scene_rect() }
    }

    /// Clamp rect for a stay write (pan/zoom). In paged flow over a
    /// document with no pages the slice has no item, and the TypeScript
    /// reads `x` of `undefined`.
    pub(crate) fn stay_bounds(&self) -> Result<Rect, Error> {
        if self.paged() {
            let scene = self.build_scene();
            scene.items.first().map(Self::item_rect).ok_or_else(|| read_of_undefined("x"))
        } else {
            Ok(self.scene_rect())
        }
    }

    pub(crate) fn constraint(&self) -> CameraConstraint {
        let st = self.st();
        CameraConstraint {
            bounded: st.settings.bounded,
            padding: st.settings.padding,
            fit_align: Some(st.settings.fit_align),
            direction: Some(st.settings.direction),
        }
    }

    // ── anchors ───────────────────────────────────────────────────────────────

    pub(crate) fn anchor_at(&self, at: Point) -> Anchor {
        let scene = self.build_scene();
        if scene.item_count > 0 {
            sc::anchor_from_camera(self.cam(), &scene, self.vp(), Some(at))
                .expect("a scene with items has an anchor")
        } else {
            Anchor { page_index: self.st().cursor, fx: 0.5, fy: 0.0 }
        }
    }

    pub(crate) fn current_anchor(&self) -> Anchor {
        self.anchor_at(self.anchor_point())
    }

    /// `indexOfPage(page)`: the page's display index, `None` when the
    /// registry has no such page.
    pub(crate) fn index_of_page(&self, page: PageRef) -> Option<usize> {
        self.registry().iter().position(|p| p.page_ref == page)
    }
}
