//! Printing a document: the host's printer (`HostPrinter`) and what
//! `documents.preparePrint()` hands back (`PreparedPrint`), as `types.ts`
//! declares them. The kernel's half lives with the documents capability
//! (`kernel.ts`, "printing").

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use serde_json::{Value, json};

use crate::Promise;
use crate::geometry::PageRef;
use crate::signal::AbortSignal;

/// `PrintDpi`: the two resolutions a picture print is drawn at.
pub type PrintDpi = u32;

/// `PrintQuality`: how a print comes out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrintQuality {
    /// `{ kind: 'pdf' }`: the printer prints a copy of the pages as they print.
    Pdf,
    /// `{ kind: 'images', dpi }`: pictures of the pages.
    Images { dpi: PrintDpi },
}

/// `PRINT_STANDARD`: pictures at 150 DPI, with `doc.print`.
pub const PRINT_STANDARD: PrintQuality = PrintQuality::Images { dpi: 150 };
/// `PRINT_HIGH`: pictures at 300 DPI, with `doc.print.high`.
pub const PRINT_HIGH: PrintQuality = PrintQuality::Images { dpi: 300 };
/// `PRINT_PDF`: the PDF copy.
pub const PRINT_PDF: PrintQuality = PrintQuality::Pdf;

impl PrintQuality {
    pub fn to_json(self) -> Value {
        match self {
            PrintQuality::Pdf => json!({ "kind": "pdf" }),
            PrintQuality::Images { dpi } => json!({ "kind": "images", "dpi": dpi }),
        }
    }
}

/// `PrintProgress`: how far preparing a print got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrintProgress {
    /// Pages that are ready: one at a time as pictures, all at once as a PDF.
    pub pages_rendered: usize,
    /// Pages the print has.
    pub page_count: usize,
}

/// `PrintOptions`.
#[derive(Clone, Default)]
pub struct PrintOptions {
    /// The pages to print, in this order. Default: every page.
    pub pages: Option<Vec<PageRef>>,
    /// Print the annotations that print. Default: when the user may read
    /// them. Form fields print when the user may read the form.
    pub include_annotations: Option<bool>,
    /// Called each time pages are ready, and once before the first.
    pub on_progress: Option<Rc<dyn Fn(PrintProgress)>>,
    pub signal: Option<AbortSignal>,
}

/// `PrintSheet`: one printed sheet, a picture of a page and the page's size
/// on paper. `image` is the page's PNG as the engine answers
/// `page.render.image` (the TypeScript hands over its `blob()`).
#[derive(Clone, Debug, PartialEq)]
pub struct PrintSheet {
    pub image: Value,
    /// The page's width in points (1/72 inch), as it prints.
    pub width_pt: f64,
    /// The page's height in points.
    pub height_pt: f64,
}

/// What `stage()` and `stagePdf()` get beside the pages: the job's name and
/// the signal that stops staging.
#[derive(Clone)]
pub struct StageOptions {
    pub name: String,
    pub signal: Option<AbortSignal>,
}

/// `StagedPrint`: what a printer got ready.
pub trait StagedPrint {
    /// Open the print dialog. Resolves once the printer has the job.
    fn print(&self) -> Promise<()>;
    /// Free what staging took.
    fn dispose(&self);
}

/// `HostPrinter`: where prints go, the host's print dialog.
pub trait HostPrinter {
    /// `needsTap`: the printer prints only while the user's tap is handled.
    fn needs_tap(&self) -> bool {
        false
    }
    /// Get the sheets ready to print.
    fn stage(&self, sheets: Vec<PrintSheet>, options: StageOptions)
    -> Promise<Rc<dyn StagedPrint>>;
    /// Whether this printer has `stagePdf`: it prints a PDF file on this
    /// device. Without it, prints are pictures.
    fn prints_pdf(&self) -> bool {
        false
    }
    /// `stagePdf(pdf, options)`: get a copy of the pages as they print (the
    /// engine's `pages.extract` answer) ready. Called only when
    /// [`prints_pdf`](Self::prints_pdf) is true.
    fn stage_pdf(&self, pdf: Value, options: StageOptions) -> Promise<Rc<dyn StagedPrint>> {
        let _ = (pdf, options);
        Promise::rejected(crate::error::plugin_error(
            "unsupported",
            "documents",
            "this printer prints no PDF",
            None,
            None,
        ))
    }
}

/// The staged print a prepared print holds, and what it saw.
pub(crate) struct Staged {
    pub(crate) staged: Rc<dyn StagedPrint>,
    pub(crate) quality: PrintQuality,
    pub(crate) changes_seen: u64,
}

/// `PreparedPrint`: a print got ready by `documents.preparePrint()`.
/// Everything slow already happened, so `print()` opens the dialog at once.
#[derive(Clone)]
pub struct PreparedPrint {
    pub(crate) current: Rc<RefCell<Staged>>,
    pub(crate) disposed: Rc<Cell<bool>>,
    pub(crate) page_count: usize,
    pub(crate) needs_tap: bool,
    pub(crate) print: Rc<dyn Fn(Option<AbortSignal>) -> Promise<()>>,
}

impl PreparedPrint {
    /// `quality`: how it prints now (pictures once the printer turned the
    /// PDF copy down).
    pub fn quality(&self) -> PrintQuality {
        self.current.borrow().quality
    }
    /// How many pages it prints.
    pub fn page_count(&self) -> usize {
        self.page_count
    }
    /// Whether `print()` must run inside the user's tap or click.
    pub fn needs_tap(&self) -> bool {
        self.needs_tap
    }
    /// Open the print dialog, inside every `aroundPrint` wrap, preparing it
    /// again first when the document changed since. Rejects `invalid-input`
    /// once disposed, `operation-cancelled` when `signal` fires first.
    pub fn print(&self, signal: Option<AbortSignal>) -> Promise<()> {
        (self.print)(signal)
    }
    /// Free what preparing took. Closing the document does too.
    pub fn dispose(&self) {
        dispose_staged(&self.disposed, &self.current);
    }
}

impl std::fmt::Debug for PreparedPrint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedPrint")
            .field("quality", &self.quality())
            .field("page_count", &self.page_count)
            .field("needs_tap", &self.needs_tap)
            .field("disposed", &self.disposed.get())
            .finish()
    }
}

pub(crate) fn dispose_staged(disposed: &Cell<bool>, current: &RefCell<Staged>) {
    if disposed.replace(true) {
        return;
    }
    let staged = current.borrow().staged.clone();
    staged.dispose();
}
