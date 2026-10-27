//! `services/scene.ts`: the scene model. The document's item grouping, the
//! layout, the memoized scene (continuous flow lays out the whole document;
//! paged flow a one-item slice at the origin), and the pure geometry reads
//! every area builds on: fit, alignment points, clamp bounds and anchors.
//! Reads only; nothing here changes state.

use serde::Deserialize;
use serde_json::Value;

use crate::prelude::*;

/// What the stage reads of a registry row (`PageInfo`): its size and
/// rotation for the layout, its `/UserUnit` for the page transform.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageRow {
    pub(crate) size: Size,
    #[serde(default)]
    pub(crate) rotation: PageRotation,
    #[serde(default)]
    pub(crate) user_unit: Option<f64>,
}

/// Drop the members a JSON value carries as `undefined` (the kernel's
/// `{"$undefined": true}`), as a JavaScript object read would see them:
/// absent. Registry rows reach the stage as JSON and may carry the marker.
pub(crate) fn without_undefined(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(_, v)| !epdf_kernel::is_undefined(v))
                .map(|(k, v)| (k.clone(), without_undefined(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_undefined).collect()),
        other => other.clone(),
    }
}

impl StageCapability {
    // ── state reads ──

    pub(crate) fn camera(&self) -> Camera {
        self.st().camera
    }

    pub(crate) fn viewport(&self) -> Size {
        self.st().viewport
    }

    pub(crate) fn dpr(&self) -> f64 {
        self.st().dpr
    }

    pub(crate) fn padding(&self) -> f64 {
        self.st().settings.padding
    }

    pub(crate) fn paged(&self) -> bool {
        self.st().settings.flow == FlowMode::Paged
    }

    pub(crate) fn is_fit_all(&self) -> bool {
        matches!(self.st().settings.zoom, ZoomSpec::Mode { mode: ZoomMode::FitAll })
    }

    /// `ctx.document()?.pageCount ?? 0`.
    pub(crate) fn page_count(&self) -> usize {
        self.ctx.document().map_or(0, |d| d.page_count as usize)
    }

    /// `ctx.document()?.pages ?? []`, as JSON rows.
    pub(crate) fn registry_rows(&self) -> Rc<Value> {
        self.ctx.document().map_or_else(|| Rc::new(Value::Array(Vec::new())), |d| d.pages)
    }

    /// The registry rows, parsed once per registry. A row that does not
    /// parse stays in place as `None`, so every other row keeps its display
    /// index.
    pub(crate) fn registry(&self) -> Rc<Vec<Option<PageRow>>> {
        let rows = self.registry_rows();
        if let Some((key, pages)) = self.inner.registry_cache.borrow().as_ref()
            && Rc::ptr_eq(key, &rows)
        {
            return pages.clone();
        }
        let pages: Vec<Option<PageRow>> = rows
            .as_array()
            .map(|pages| {
                pages.iter().map(|p| serde_json::from_value(without_undefined(p)).ok()).collect()
            })
            .unwrap_or_default();
        let pages = Rc::new(pages);
        *self.inner.registry_cache.borrow_mut() = Some((rows, pages.clone()));
        pages
    }

    /// `ctx.getPage(page)`: the registry row a ref or an index names.
    pub(crate) fn get_page(&self, page: PageTarget) -> Option<Value> {
        let rows = self.registry_rows();
        let pages = rows.as_array()?;
        match page {
            PageTarget::Index(index) => {
                (index >= 0.0 && index.fract() == 0.0).then(|| pages.get(index as usize)).flatten()
            }
            PageTarget::Ref(page) => pages.iter().find(|row| page.names(row)),
        }
        .cloned()
    }

    /// `ctx.getPage(page)?.index ?? -1`, as an option.
    pub(crate) fn index_of(&self, page: PageTarget) -> Option<usize> {
        let row = self.get_page(page)?;
        let index = row.get("index")?.as_f64()?;
        (index >= 0.0 && index.fract() == 0.0).then_some(index as usize)
    }

    // ── the item model (spread grouping) ──

    pub(crate) fn grouping(&self) -> Rc<Grouping> {
        let key = (self.ctx.document().map_or(0, |d| d.page_count), self.st().settings.spread);
        if let Some((k, g)) = self.inner.grouping_memo.borrow().as_ref()
            && *k == key
        {
            return g.clone();
        }
        let groups = sc::group_pages(key.0 as usize, key.1);
        let first_pages = groups.iter().map(|item| item[0]).collect();
        let grouping =
            Rc::new(Grouping { grouping: Rc::new(groups), first_pages: Rc::new(first_pages) });
        *self.inner.grouping_memo.borrow_mut() = Some((key, grouping.clone()));
        grouping
    }

    pub(crate) fn item_index_of_page(&self, page_index: usize) -> usize {
        let grouping = self.grouping();
        let first_pages = &grouping.first_pages;
        let (mut low, mut high) = (0, first_pages.len());
        while low < high {
            let middle = (low + high) >> 1;
            if first_pages[middle] <= page_index {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low.saturating_sub(1)
    }

    // ── screen-px settings in world units ──

    pub(crate) fn effective_zoom(&self) -> f64 {
        let state = self.st();
        match state.settings.zoom {
            ZoomSpec::Level { level } => level,
            _ => js::max(state.camera.zoom, 0.0001),
        }
    }

    pub(crate) fn wrap_line_width(&self) -> f64 {
        js::max(1.0, (self.viewport().width - 2.0 * self.padding()) / self.effective_zoom())
    }

    fn no_frame(frame: &epdf_geometry::PageFrame) -> bool {
        falsy(frame.top) && falsy(frame.right) && falsy(frame.bottom) && falsy(frame.left)
    }

    pub(crate) fn world_page_frame(&self) -> epdf_geometry::PageFrame {
        let frame = self.st().settings.page_frame;
        if Self::no_frame(&frame) {
            return frame;
        }
        let zoom = self.effective_zoom();
        epdf_geometry::PageFrame {
            top: frame.top / zoom,
            right: frame.right / zoom,
            bottom: frame.bottom / zoom,
            left: frame.left / zoom,
        }
    }

    fn frame_key(&self) -> String {
        let frame = self.st().settings.page_frame;
        if Self::no_frame(&frame) {
            return "-".into();
        }
        let world = self.world_page_frame();
        format!(
            "{},{},{},{}",
            num(js::round(world.top)),
            num(js::round(world.right)),
            num(js::round(world.bottom)),
            num(js::round(world.left))
        )
    }

    pub(crate) fn world_gap(&self) -> f64 {
        match self.st().settings.gap {
            Gap::World(gap) => gap,
            Gap::Px { px } if !falsy(px) => px / self.effective_zoom(),
            Gap::Px { .. } => 0.0,
        }
    }

    fn gap_key(&self) -> String {
        match self.st().settings.gap {
            Gap::World(gap) => num(gap),
            Gap::Px { .. } => format!("px:{}", num(js::round(self.world_gap()))),
        }
    }

    pub(crate) fn layout_for(&self, groups: &[Vec<usize>]) -> Scene {
        let state = self.st();
        let settings = &state.settings;
        // The view rotation is composed here, and only here: each page's
        // display rotation is its /Rotate plus this lens's viewRotation.
        let view_rotation = settings.view_rotation;
        // A row that does not parse (a page with no size, say) makes the
        // TypeScript throw wherever the scene is built. Here the pages stop
        // short of it instead, so the grouping names a missing page and the
        // scene below comes out empty: a kept Rust-only divergence (R5), as
        // for a NaN column count (M27).
        let pages: Vec<PageGeom> = self
            .registry()
            .iter()
            .map_while(Option::as_ref)
            .map(|page| PageGeom {
                size: page.size,
                rotation: Some(if view_rotation.0 == 0.0 {
                    page.rotation
                } else {
                    add_rotations(page.rotation, view_rotation)
                }),
                user_unit: page.user_unit,
            })
            .collect();
        let page_frame = self.world_page_frame();
        let gap = self.world_gap();
        let view_units_per_point = settings.view_units_per_point;
        // stage-core fails only on a grouping that names a missing page or
        // a NaN column count (a NaN `columns`, or an auto wrap at a NaN
        // zoom); the grouping comes from the same registry.
        let scene = if settings.layout == LayoutKind::Grid {
            sc::grid_layout(
                &pages,
                groups,
                sc::GridOptions {
                    gap: Some(gap),
                    columns: match settings.columns {
                        GridColumns::Count(n) => Some(n),
                        GridColumns::Policy(_) => None,
                    },
                    line_width: (settings.columns == GridColumns::Policy(ColumnsPolicy::Auto))
                        .then(|| self.wrap_line_width()),
                    sizing: Some(settings.sizing),
                    direction: Some(settings.direction),
                    page_frame: Some(page_frame),
                    view_units_per_point: Some(view_units_per_point),
                },
            )
        } else {
            sc::linear_layout(
                &pages,
                groups,
                sc::LinearOptions {
                    gap: Some(gap),
                    axis: Some(if settings.layout == LayoutKind::Horizontal {
                        sc::LinearAxis::X
                    } else {
                        sc::LinearAxis::Y
                    }),
                    align: None,
                    sizing: Some(settings.sizing),
                    direction: Some(settings.direction),
                    page_frame: Some(page_frame),
                    view_units_per_point: Some(view_units_per_point),
                },
            )
        };
        // The TypeScript throws (a RangeError or a TypeError) wherever the
        // scene is built, and the stage stays broken until the input
        // changes. Here the scene is empty instead, so every verb is a
        // no-op until then.
        scene.unwrap_or_else(|_| {
            sc::linear_layout(&[], &[], sc::LinearOptions::default())
                .expect("an empty linear layout cannot fail")
        })
    }

    fn columns_key(&self) -> String {
        let state = self.st();
        if state.settings.layout != LayoutKind::Grid {
            return "-".into();
        }
        match state.settings.columns {
            GridColumns::Policy(ColumnsPolicy::Auto) => {
                format!("auto:{}", num(js::round(self.wrap_line_width())))
            }
            GridColumns::Policy(ColumnsPolicy::Square) => "square".into(),
            GridColumns::Count(n) => num(n),
        }
    }

    /// The scene's settings signature, over the `'scene'` rows of the
    /// registry in key order. Primitives key by `String(value)`; the three
    /// screen-px settings quantize.
    fn settings_key(&self) -> String {
        let state = self.st();
        let settings = &state.settings;
        let mut parts = Vec::new();
        for key in SETTING_KEYS {
            if settings_effect(key) != SettingEffect::Scene {
                continue;
            }
            parts.push(match key {
                SettingKey::Columns => self.columns_key(),
                SettingKey::Gap => self.gap_key(),
                SettingKey::PageFrame => self.frame_key(),
                SettingKey::Layout => serde_str(&settings.layout),
                SettingKey::Spread => serde_str(&settings.spread),
                SettingKey::Sizing => serde_str(&settings.sizing),
                SettingKey::Direction => serde_str(&settings.direction),
                SettingKey::ViewRotation => num(settings.view_rotation.0),
                SettingKey::ViewUnitsPerPoint => num(settings.view_units_per_point),
                other => unreachable!("{other:?} is not a scene setting"),
            });
        }
        parts.join("|")
    }

    fn document_key(&self) -> String {
        match self.ctx.document() {
            Some(document) => format!("{}.{}", document.page_count, document.revision),
            None => "0.0".into(),
        }
    }

    /// The scene: the whole document (continuous), or a one-item slice at
    /// the cursor (paged). Cached by key; the same `Rc` means the same
    /// scene, as the TypeScript compares scenes by reference.
    pub(crate) fn build_scene(&self) -> Rc<Scene> {
        let grouping = self.grouping();
        let groups = &grouping.grouping;
        let (key, slice) = if self.paged() {
            let index = if groups.is_empty() {
                0
            } else {
                self.item_index_of_page(self.st().cursor).min(groups.len() - 1)
            };
            (format!("paged|{}|{}|{index}", self.document_key(), self.settings_key()), Some(index))
        } else {
            (format!("continuous|{}|{}", self.document_key(), self.settings_key()), None)
        };
        if let Some((k, scene)) = self.inner.scene_cache.borrow().as_ref()
            && *k == key
        {
            return scene.clone();
        }
        let scene = Rc::new(match slice {
            Some(index) => {
                self.layout_for(if groups.is_empty() { &[] } else { &groups[index..index + 1] })
            }
            None => self.layout_for(groups),
        });
        *self.inner.scene_cache.borrow_mut() = Some((key, scene.clone()));
        scene
    }

    /// The current scene's cache key, part of the visible-pages signature.
    pub(crate) fn scene_cache_key(&self) -> String {
        self.inner.scene_cache.borrow().as_ref().map(|(k, _)| k.clone()).unwrap_or_default()
    }

    /// Drop the memoized scene (a 'scene' or 'reflow' setting changed, or a
    /// view was restored).
    pub(crate) fn invalidate_scene(&self) {
        *self.inner.scene_cache.borrow_mut() = None;
    }

    // ── geometry helpers ──

    pub(crate) fn scene_rect(&self) -> Rect {
        let size = self.build_scene().size;
        Rect { x: 0.0, y: 0.0, width: size.width, height: size.height }
    }

    pub(crate) fn item_rect(item: &SceneItem) -> Rect {
        Rect { x: item.x, y: item.y, width: item.width, height: item.height }
    }

    pub(crate) fn page_box_of(item: &SceneItem, page_index: usize) -> &PageBox {
        item.pages.iter().find(|p| p.page_index == page_index).unwrap_or(&item.pages[0])
    }

    pub(crate) fn page_rect_of(item: &SceneItem, page_index: usize) -> Rect {
        let b = Self::page_box_of(item, page_index);
        Rect { x: b.x, y: b.y, width: b.width, height: b.height }
    }

    /// The item shown for the cursor: the slice's only item (paged) or the
    /// full-scene item; `None` where the TypeScript reads `undefined`.
    pub(crate) fn cursor_item(&self) -> Option<SceneItem> {
        let scene = self.build_scene();
        if self.paged() {
            scene.items.first().cloned()
        } else {
            scene.items.get(self.item_index_of_page(self.st().cursor)).cloned()
        }
    }

    /// A page-space rect on a page to a world rect, through the same
    /// quarter-turn matrix `pageRectToViewport` uses.
    pub(crate) fn world_rect_for_content(item: &SceneItem, page_index: usize, rect: Rect) -> Rect {
        let b = Self::page_box_of(item, page_index);
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        let matrix =
            rotate_scale_matrix(b.content_scale, content.width, content.height, b.rotation);
        let world = apply_rect(matrix, rect);
        Rect { x: b.x + world.x, y: b.y + world.y, width: world.width, height: world.height }
    }

    /// Does this rect fit the padded viewport at this zoom? It decides the
    /// navigation step size and the arrival subject, never the alignment.
    pub(crate) fn fits(&self, rect: Rect, zoom: f64) -> bool {
        let size = self.viewport();
        let inset = self.padding();
        let tolerance = 0.5;
        rect.width * zoom <= size.width - 2.0 * inset + tolerance
            && rect.height * zoom <= size.height - 2.0 * inset + tolerance
    }

    pub(crate) fn align_fraction(value: AlignValue) -> f64 {
        match value {
            AlignValue::Named(Align::Start) => 0.0,
            AlignValue::Named(Align::Center) => 0.5,
            AlignValue::Named(Align::End) => 1.0,
            AlignValue::Fraction(f) => js::min(1.0, js::max(0.0, f)),
        }
    }

    /// An alignment policy to a viewport point, a fraction of the padded
    /// range per axis; named x stops are logical under RTL.
    pub(crate) fn align_point(&self, alignment: AlignmentValue, size: Size) -> Point {
        let rtl = self.st().settings.direction == Direction::Rtl;
        let align_x = match alignment.x {
            AlignValue::Named(Align::Start) if rtl => AlignValue::Named(Align::End),
            AlignValue::Named(Align::End) if rtl => AlignValue::Named(Align::Start),
            x => x,
        };
        let inset = self.padding();
        Point {
            x: inset + (size.width - 2.0 * inset) * Self::align_fraction(align_x),
            y: inset + (size.height - 2.0 * inset) * Self::align_fraction(alignment.y),
        }
    }

    pub(crate) fn anchor_point(&self) -> Point {
        self.align_point(self.st().settings.anchor_align, self.viewport())
    }

    /// The box the zoom intent fits: the whole scene (fit-all), the current
    /// item (paged), or the document maximum (continuous). Only paged flow
    /// reads the item, and there the slice always has one.
    pub(crate) fn fit_box(&self, item: Option<&SceneItem>) -> Size {
        if self.is_fit_all() {
            return self.build_scene().size;
        }
        if self.paged() {
            item.map_or(Size { width: 0.0, height: 0.0 }, |item| Size {
                width: item.width,
                height: item.height,
            })
        } else {
            self.build_scene().max_item_size
        }
    }

    /// The clamp rect for a camera write targeting this item.
    pub(crate) fn bounds_for(&self, item: &SceneItem) -> Rect {
        if self.paged() { Self::item_rect(item) } else { self.scene_rect() }
    }

    /// The clamp rect for a write that stays put (pan, zoom): the slice
    /// item (paged) or the scene. In paged flow over a document with no
    /// pages the slice has no item, and the TypeScript reads `x` of
    /// `undefined` (`scene.ts` `itemRect`); here that is the same error,
    /// returned rather than a panic.
    pub(crate) fn stay_bounds(&self) -> Result<Rect, Error> {
        if self.paged() {
            let scene = self.build_scene();
            scene.items.first().map(Self::item_rect).ok_or_else(|| read_of_undefined("x"))
        } else {
            Ok(self.scene_rect())
        }
    }

    pub(crate) fn constraint(&self) -> CameraConstraint {
        let state = self.st();
        CameraConstraint {
            bounded: state.settings.bounded,
            padding: state.settings.padding,
            fit_align: Some(state.settings.fit_align),
            direction: Some(state.settings.direction),
        }
    }

    // ── anchors ──

    /// `Err` where the TypeScript throws: no item under the point.
    pub(crate) fn anchor_at(&self, at: Point) -> Result<Anchor, Error> {
        let scene = self.build_scene();
        if scene.item_count > 0 {
            sc::anchor_from_camera(self.camera(), &scene, self.viewport(), Some(at))
                .map_err(stage_error)
        } else {
            Ok(Anchor { page_index: self.st().cursor, fx: 0.5, fy: 0.0 })
        }
    }

    pub(crate) fn current_anchor(&self) -> Result<Anchor, Error> {
        self.anchor_at(self.anchor_point())
    }

    /// `indexOfTarget(page)`: a page argument as a display index, or `None`
    /// when it names no page (a ref not in the document, or a number that
    /// isn't whole). An index past either end is kept; arrivals clamp it.
    pub(crate) fn index_of_target(&self, page: PageTarget) -> Option<isize> {
        match page {
            PageTarget::Index(index) => {
                (index.is_finite() && index.fract() == 0.0).then_some(index as isize)
            }
            PageTarget::Ref(_) => self.index_of(page).map(|index| index as isize),
        }
    }
}
