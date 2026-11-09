//! `snapshot.ts`: the snapshot tool. Drag a box on a page, or click for the
//! whole page, and the box becomes the snapshot, part of the page as a
//! picture to copy or save. The box is state, in page space on the page the
//! press was on; its picture is a one-off render of it (`render_page` with a
//! `rect`), kept beside the state. The pointer comes from the interaction
//! hub when there is one: [`connect_snapshots`] registers the tool and its
//! handler.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use epdf_geometry::{PageRotation, Point, Rect, rect_from_corners};
use epdf_kernel::Error;
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_interaction::{
    INTERACTION, InteractionHandler, PointerSample, RegisterHandlerOptions, RegisterToolOptions,
    Tool, ToolTouch, sample_point_on,
};

use crate::contract::{
    PageArg, PageImage, RenderPageOptions, SNAPSHOT_MAX_PIXELS, SNAPSHOT_TOOL, Snapshot,
    SnapshotChangedEvent,
};
use crate::controller::Core;
use crate::js;
use crate::model::{SnapshotBox, set_snapshot_box};
use crate::paint_plan::RenderFormat;
use crate::policy::EngineRenderPolicy;

const RENDER_SCOPE: &str = "doc.render";
const CONTENT_COPY_SCOPE: &str = "doc.content.copy";
const CAPABILITY: &str = "render";

/// The behavior tag the snapshot tool turns on, which its handler answers
/// to.
const SNAPSHOT_BOX_TAG: &str = "snapshot-box";

/// A drag shorter than this on both axes, in view px, is a click (the zoom
/// tool's threshold).
const BOX_THRESHOLD_PX: f64 = 4.0;

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    js::min(max, js::max(min, value))
}

/// Whether a drag is big enough on screen to be a box, not a click.
fn is_box(dragged: &SnapshotBox) -> bool {
    (dragged.to.x - dragged.from.x).abs() * dragged.scale >= BOX_THRESHOLD_PX
        || (dragged.to.y - dragged.from.y).abs() * dragged.scale >= BOX_THRESHOLD_PX
}

/// The rect a box spans; a side dragged flat is one view pixel, so there's
/// a picture to take.
fn rect_of(dragged: &SnapshotBox) -> Rect {
    let rect = rect_from_corners(dragged.from, dragged.to);
    let least = 1.0 / dragged.scale;
    Rect { width: js::max(rect.width, least), height: js::max(rect.height, least), ..rect }
}

/// `snapshotScale(rect, wanted, policy)`: the scale of a snapshot's
/// picture: the setting, lowered until the picture fits
/// [`SNAPSHOT_MAX_PIXELS`] and the deployment's budget, counting its sides
/// in whole pixels.
pub fn snapshot_scale(rect: Rect, wanted: f64, policy: &EngineRenderPolicy) -> f64 {
    let deployment = match policy {
        EngineRenderPolicy::Lattice(lattice) => lattice.max_render_pixels.unwrap_or(f64::INFINITY),
        EngineRenderPolicy::Continuous => f64::INFINITY,
    };
    let budget = js::min(SNAPSHOT_MAX_PIXELS, deployment);
    let pixels_at = |scale: f64| (rect.width * scale).ceil() * (rect.height * scale).ceil();
    let mut scale = js::min(wanted, (budget / (rect.width * rect.height)).sqrt());
    while pixels_at(scale) > budget {
        scale *= 0.999;
    }
    scale
}

/// The snapshot held now, and the way to stop its picture: set before the
/// box turns held, so a reader woken by the state finds it.
pub(crate) struct Snapshots {
    pub(crate) changed: EventHookSource<SnapshotChangedEvent>,
    held: RefCell<Option<Snapshot>>,
    stop_picture: RefCell<Option<AbortSignal>>,
}

impl Snapshots {
    pub(crate) fn new(changed: EventHookSource<SnapshotChangedEvent>) -> Self {
        Snapshots { changed, held: RefCell::new(None), stop_picture: RefCell::new(None) }
    }
}

impl Core {
    fn snapshot_box(&self) -> Option<SnapshotBox> {
        self.cx.state().snapshot.clone()
    }

    fn set_snapshot_box(&self, snapshot: Option<SnapshotBox>) {
        self.cx.update_state(|state| match set_snapshot_box(&state, snapshot) {
            Some(next) => Rc::new(next),
            None => state,
        });
    }

    fn page_size_of(&self, page: PageRef) -> Option<crate::tiles::PageSizePt> {
        self.find_page(PageArg::Ref(page)).map(|(_, size)| size)
    }

    /// `canSnapshot()`: `doc.render` and `doc.content.copy`.
    pub(crate) fn can_snapshot(&self) -> bool {
        self.cx.allows(RENDER_SCOPE) && self.cx.allows(CONTENT_COPY_SCOPE)
    }

    /// `clearSnapshot()`: let the snapshot go; its picture, if still
    /// rendering, stops.
    pub(crate) fn clear_snapshot(&self) {
        let stop = self.snapshots.stop_picture.borrow_mut().take();
        if let Some(stop) = stop {
            stop.abort(js::dom_abort_error());
        }
        let let_go = self.snapshots.held.borrow_mut().take();
        self.set_snapshot_box(None);
        if let_go.is_some() {
            self.snapshots.changed.emit(&SnapshotChangedEvent { snapshot: None });
        }
    }

    /// `getSnapshot()`: a snapshot of a page that left the document is gone
    /// with it.
    pub(crate) fn get_snapshot(&self) -> Option<Snapshot> {
        let held = self.snapshots.held.borrow().clone()?;
        self.page_size_of(held.page).is_some().then_some(held)
    }

    /// `getSnapshotBox(page)`: the box as a page layer draws it, while it
    /// is dragged far enough to be a box and while it is held.
    pub(crate) fn get_snapshot_box(&self, page: PageRef) -> Option<Rect> {
        let current = self.snapshot_box()?;
        if !(current.held || is_box(&current)) {
            return None;
        }
        (current.page.page_object_number == page.page_object_number).then(|| rect_of(&current))
    }

    /// Hold `rect` of the box's page as the snapshot, and render its picture.
    fn take(self: &Rc<Self>, dragged: SnapshotBox, rect: Rect) {
        let stop = AbortSignal::new();
        *self.snapshots.stop_picture.borrow_mut() = Some(stop.clone());
        let wanted = self.get_settings().snapshot.and_then(|s| s.scale).unwrap_or(f64::NAN);
        let scale = snapshot_scale(rect, wanted, &self.policy());
        // Every change made before the snapshot is in its picture, a stamp
        // placed a moment ago too: the engine has them all first.
        let settled = match self.cx.changes() {
            Ok(changes) => changes.when_settled(),
            Err(error) => Promise::rejected(error),
        };
        let gate = cancellable(CAPABILITY, Some(stop.clone()), settled);
        let this = self.clone();
        let page = dragged.page;
        let rotation = dragged.rotation;
        let image: Promise<PageImage> = Promise::spawn(async move {
            gate.reaction().await?;
            let options = RenderPageOptions {
                rect: Some(rect),
                scale: Some(scale),
                rotation: Some(rotation),
                format: Some(RenderFormat::Png),
                signal: Some(stop),
                ..RenderPageOptions::default()
            };
            this.render_page(PageArg::Ref(page), options).reaction().await
        });
        #[cfg(feature = "uniffi")]
        let display_width = epdf_geometry::display_size(
            epdf_geometry::Size { width: rect.width, height: rect.height },
            rotation,
        )
        .width;
        let held = Snapshot {
            page,
            rect,
            image,
            #[cfg(feature = "uniffi")]
            display_width,
        };
        *self.snapshots.held.borrow_mut() = Some(held.clone());
        self.set_snapshot_box(Some(SnapshotBox {
            from: Point { x: rect.x, y: rect.y },
            to: Point { x: rect.x + rect.width, y: rect.y + rect.height },
            held: true,
            ..dragged
        }));
        self.snapshots.changed.emit(&SnapshotChangedEvent { snapshot: Some(held) });
    }

    // ── the gesture, in page space on the page the press was on ──

    fn begin_snapshot(
        &self,
        page: PageRef,
        point: Point,
        scale: f64,
        rotation: PageRotation,
    ) -> bool {
        if !self.can_snapshot() || self.page_size_of(page).is_none() {
            return false;
        }
        self.clear_snapshot();
        self.set_snapshot_box(Some(SnapshotBox {
            page,
            from: point,
            to: point,
            scale,
            rotation,
            held: false,
        }));
        true
    }

    fn move_snapshot(&self, point: Point) {
        let Some(dragged) = self.snapshot_box().filter(|b| !b.held) else { return };
        let Some(size) = self.page_size_of(dragged.page) else { return };
        let to = Point { x: clamp(point.x, 0.0, size.width), y: clamp(point.y, 0.0, size.height) };
        self.set_snapshot_box(Some(SnapshotBox { to, ..dragged }));
    }

    fn end_snapshot(self: &Rc<Self>) {
        let Some(dragged) = self.snapshot_box().filter(|b| !b.held) else { return };
        let Some(size) = self.page_size_of(dragged.page) else {
            self.set_snapshot_box(None);
            return;
        };
        // A click takes the whole page.
        let rect = if is_box(&dragged) {
            rect_of(&dragged)
        } else {
            Rect { x: 0.0, y: 0.0, width: size.width, height: size.height }
        };
        self.take(dragged, rect);
    }

    fn cancel_snapshot(&self) {
        if self.snapshot_box().is_some_and(|b| !b.held) {
            self.set_snapshot_box(None);
        }
    }
}

/// The tool's pointer handler: live under the `snapshot-box` tag.
struct SnapshotHandler(Weak<Core>);

impl SnapshotHandler {
    /// Where `sample` is on the dragged box's page, if it can say.
    fn point_on_box_page(core: &Core, sample: &PointerSample) -> Option<Point> {
        let dragged = core.snapshot_box()?;
        sample_point_on(sample, dragged.page)
    }
}

impl InteractionHandler for SnapshotHandler {
    fn id(&self) -> &str {
        "render-snapshot"
    }

    fn priority(&self) -> f64 {
        10.0
    }

    fn enabled_for(&self, tool: &Tool) -> bool {
        tool.enables(SNAPSHOT_BOX_TAG)
    }

    /// A press between pages does nothing: a box lives on one page.
    fn on_pointer_down(&self, sample: &PointerSample) -> bool {
        let (Some(core), Some(hit)) = (self.0.upgrade(), sample.page.as_ref()) else {
            return false;
        };
        core.begin_snapshot(
            hit.page,
            hit.point,
            hit.scale.unwrap_or(1.0),
            hit.rotation.unwrap_or(PageRotation(0.0)),
        )
    }

    fn on_pointer_move(&self, sample: &PointerSample) {
        let Some(core) = self.0.upgrade() else { return };
        if let Some(point) = Self::point_on_box_page(&core, sample) {
            core.move_snapshot(point);
        }
    }

    fn on_pointer_up(&self, sample: &PointerSample) {
        let Some(core) = self.0.upgrade() else { return };
        if let Some(point) = Self::point_on_box_page(&core, sample) {
            core.move_snapshot(point);
        }
        core.end_snapshot();
    }

    fn on_pointer_cancel(&self, _sample: &PointerSample) {
        if let Some(core) = self.0.upgrade() {
            core.cancel_snapshot();
        }
    }
}

/// `connect()`: register the tool and its handler with the interaction
/// hub, when there is one. Another tool lets the snapshot go.
pub(crate) fn connect_snapshots(core: &Rc<Core>) -> Result<(), Error> {
    let Some(interaction) = core.cx.try_get(&INTERACTION)? else { return Ok(()) };
    let tool =
        Tool::new(SNAPSHOT_TOOL, "crosshair", [SNAPSHOT_BOX_TAG]).with_touch(ToolTouch::Draw);
    let unregister_tool = interaction.register_tool(tool, RegisterToolOptions::default())?;
    core.cx.cleanup_fn(move || unregister_tool.call());
    let unregister_handler = interaction.register_handler(
        Rc::new(SnapshotHandler(Rc::downgrade(core))),
        RegisterHandlerOptions::default(),
    );
    core.cx.cleanup_fn(move || unregister_handler.call());
    let weak = Rc::downgrade(core);
    core.cx.listen(interaction.on_tool_changed(move |event| {
        if event.tool_id != SNAPSHOT_TOOL
            && let Some(core) = weak.upgrade()
        {
            core.clear_snapshot();
        }
        Ok(())
    }));
    Ok(())
}
