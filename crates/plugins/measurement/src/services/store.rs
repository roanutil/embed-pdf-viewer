//! `services/store.ts` and `services/siblings.ts`: slice reads, the page
//! registry, page and PDF point conversion, the liveness and authority
//! guards, the page-target expansion, and the two sibling planes.

use std::cell::Cell;
use std::rc::Rc;

use epdf_annotation_core::{PageRef, PdfPoint, PdfRect, Point};
use epdf_kernel::Error;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::{code, plugin_error};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_interaction::{INTERACTION, InteractionCapability};
use serde_json::Value;

use crate::contract::{LOADING, MeasurementConfig, PageScale, PageTarget};
use crate::controller::MeasurementContext;
use crate::model::MeasurementState;
use crate::services::events::{MeasurementEvents, create_events};

/// What this plugin reads from a page's layout row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageMeta {
    pub crop: PdfRect,
    pub user_unit: f64,
}

impl PageMeta {
    fn of(layout: &Value) -> PageMeta {
        let crop = layout
            .get("boxes")
            .and_then(|b| b.get("crop"))
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or_default();
        // `userUnit = 1` is a default parameter: absence reads 1.
        let user_unit = match layout.get("userUnit") {
            None => 1.0,
            Some(u) => u.as_f64().unwrap_or(f64::NAN),
        };
        PageMeta { crop, user_unit }
    }
}

pub(crate) fn kernel_page(page: &PageRef) -> epdf_kernel::geometry::PageRef {
    epdf_kernel::geometry::PageRef::new(page.page_object_number)
}

/// The services, built once by the controller.
pub struct Services {
    pub cx: MeasurementContext,
    pub config: MeasurementConfig,
    pub events: MeasurementEvents,
    disposed: Rc<Cell<bool>>,
    pub annotation: AnnotationCapability,
    pub interaction: InteractionCapability,
}

impl Services {
    /// `createServices(ctx)`: events, then the store (whose cleanup marks it
    /// disposed), then the siblings.
    pub fn new(cx: MeasurementContext, config: MeasurementConfig) -> Result<Services, Error> {
        let events = create_events(&cx, &config.host);
        let disposed = Rc::new(Cell::new(false));
        let flag = disposed.clone();
        cx.defer(Box::new(move || {
            flag.set(true);
            Ok(None)
        }));
        let annotation = cx.get(&ANNOTATION)?;
        let interaction = cx.get(&INTERACTION)?;
        Ok(Services { cx, config, events, disposed, annotation, interaction })
    }

    pub fn state(&self) -> Rc<MeasurementState> {
        self.cx.state()
    }

    /// The document's layout rows.
    fn layouts(&self) -> Vec<Value> {
        self.cx.document().and_then(|d| d.pages.as_array().cloned()).unwrap_or_default()
    }

    /// Every page of the document, in order.
    pub fn page_refs(&self) -> Vec<PageRef> {
        self.layouts()
            .iter()
            .filter_map(|l| serde_json::from_value(l.get("ref")?.clone()).ok())
            .collect()
    }

    /// `meta(page)`: the page's layout, when it is in the document.
    pub fn meta(&self, page: &PageRef) -> Option<PageMeta> {
        self.layouts().iter().find(|l| kernel_page(page).names(l)).map(PageMeta::of)
    }

    /// `requireMeta(page)`.
    pub fn require_meta(&self, page: &PageRef) -> Result<PageMeta, Error> {
        self.meta(page).ok_or_else(|| not_found("no such page"))
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed.get()
    }

    /// `live()`: the document handle, or `not-ready` once disposed.
    pub fn live(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        match self.cx.doc() {
            Ok(doc) if !self.disposed.get() => Ok(doc),
            _ => Err(plugin_error(code::NOT_READY, "measurement", "document not open", None, None)),
        }
    }

    /// `canCalibrate()`: may write a page scale (`doc.annotate.modify`).
    pub fn can_calibrate(&self) -> bool {
        self.cx.doc().is_ok_and(|doc| doc.security().allows("doc.annotate.modify"))
    }

    /// `assertAllowed()`.
    pub fn assert_allowed(&self) -> Result<(), Error> {
        self.live()?;
        if !self.can_calibrate() {
            return Err(plugin_error(
                code::PERMISSION_DENIED,
                "measurement",
                "changing a scale requires doc.annotate.modify",
                None,
                None,
            ));
        }
        Ok(())
    }

    /// `targets(pages)`.
    pub fn targets(&self, pages: &PageTarget) -> Vec<PageRef> {
        match pages {
            PageTarget::All => self.page_refs(),
            PageTarget::Pages(pages) => pages.clone(),
            PageTarget::Page(page) => vec![*page],
        }
    }

    /// Page space to PDF user space, through the kernel's page geometry.
    pub fn to_pdf(&self, page: &PageRef, point: Point) -> Result<PdfPoint, Error> {
        let space = self.cx.for_page(kernel_page(page))?;
        let p = space.page_to_pdf(epdf_kernel::geometry::Point { x: point.x, y: point.y });
        Ok(PdfPoint { x: p.x, y: p.y })
    }

    /// PDF user space to page space.
    pub fn to_page(&self, page: &PageRef, point: PdfPoint) -> Result<Point, Error> {
        let space = self.cx.for_page(kernel_page(page))?;
        let p = space.pdf_to_page(epdf_kernel::geometry::Point { x: point.x, y: point.y });
        Ok(Point { x: p.x, y: p.y })
    }

    /// `scaleOf(page)`: the page's scale, or the loading scale.
    pub fn scale_of(&self, page: &PageRef) -> PageScale {
        self.state()
            .pages
            .get(&page.page_object_number)
            .map_or(LOADING, |entry| entry.scale.clone())
    }
}

pub(crate) fn not_found(message: &str) -> Error {
    plugin_error(code::NOT_FOUND, "measurement", message, None, None)
}
