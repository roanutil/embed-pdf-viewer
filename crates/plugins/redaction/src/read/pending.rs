//! `read/pending.ts`: the pending view, `redact` annotations projected as
//! marks per page and memoized on the annotation plane's own
//! reference-stable lists, plus the client-side collateral estimate.
//!
//! The memos are the TypeScript's: one page's marks are the same `Rc` while
//! the plane's list for that page (`annotation.list({pages: [page]})`, an
//! `Rc` the annotation plugin memoizes per model) and the page's display
//! index hold; the document's marks are the same `Rc` while no page's marks
//! changed. A page or a document without marks reads one shared empty list.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use epdf_annotation_core::engine::geom::{mirrored_quad, unmirrored_box};
use epdf_annotation_core::json::number;
use epdf_annotation_core::record::annotation_key;
use epdf_geometry::pdf::pdf_quad_bounds;
use epdf_geometry::{Point, Quad, Rect, rects_overlap};
use epdf_kernel::Error;
use epdf_kernel::geometry::PageRef;
use epdf_plugin_annotation::read::annotations::AnnotationFilter;
use serde_json::Value;

use crate::contract::{
    PageTarget, RedactionCollateral, RedactionMark, RedactionMarkFilter, RedactionMarkKind,
};
use crate::controller::RedactionCapability;

/// A member read as JavaScript reads a number: anything else is `NaN` to
/// every comparison.
fn num(value: Option<&Value>) -> f64 {
    value.and_then(number).unwrap_or(f64::NAN)
}

/// A record's `rect` (page space).
pub fn rect_of(value: Option<&Value>) -> Rect {
    let member = |key: &str| num(value.and_then(|v| v.get(key)));
    Rect { x: member("x"), y: member("y"), width: member("width"), height: member("height") }
}

fn point_of(value: Option<&Value>) -> Point {
    Point { x: num(value.and_then(|p| p.get("x"))), y: num(value.and_then(|p| p.get("y"))) }
}

fn quad_of(value: &Value) -> Quad {
    Quad {
        upper_left: point_of(value.get("upperLeft")),
        upper_right: point_of(value.get("upperRight")),
        lower_left: point_of(value.get("lowerLeft")),
        lower_right: point_of(value.get("lowerRight")),
    }
}

/// `pageQuadBounds(quad)`: a page-space quad's upright bounds, through the
/// engine's y-up bounds as `pageSpace/helpers.ts` takes them.
pub fn page_quad_bounds(quad: Quad) -> Rect {
    unmirrored_box(pdf_quad_bounds(mirrored_quad(quad)))
}

fn quad_points(record: &Value) -> &[Value] {
    record.get("quadPoints").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn is_redact(record: &Value) -> bool {
    record.get("subtype").and_then(Value::as_str) == Some("redact")
}

/// `regionsOf(dto)`: the regions a redact mark targets, its quads' boxes,
/// else its `rect`.
pub fn regions_of(record: &Value) -> Vec<Rect> {
    let quads = quad_points(record);
    if quads.is_empty() {
        return vec![rect_of(record.get("rect"))];
    }
    quads.iter().map(|q| page_quad_bounds(quad_of(q))).collect()
}

/// `markOf(record, pageIndex)`: a `redact` annotation as a mark. `Err` for
/// a record whose `ref.page` is not a page ref, which the typed mark cannot
/// hold (an engine record always has one).
pub fn mark_of(record: &Value, page_index: i64) -> Result<RedactionMark, Error> {
    let reference = record.get("ref").cloned().unwrap_or(Value::Null);
    let page = reference
        .get("page")
        .and_then(PageRef::from_json)
        .ok_or_else(|| Error::named("TypeError", "a redact record's ref names no page"))?;
    Ok(RedactionMark {
        page,
        page_index,
        kind: if quad_points(record).is_empty() {
            RedactionMarkKind::Area
        } else {
            RedactionMarkKind::Text
        },
        bounds: rect_of(record.get("rect")),
        overlay_text: record.get("overlayText").and_then(Value::as_str).map(str::to_string),
        repeat: record.get("repeat").and_then(Value::as_bool).unwrap_or(false),
        ref_: reference,
    })
}

/// `estimateCollateral(refs)` over the document's pages and the annotation
/// plane's one-page lists: which non-mark annotations the given marks (all
/// marks for `None`) would take with them.
pub fn estimate_collateral(
    pages: &[PageRef],
    list: &dyn Fn(PageRef) -> Rc<Vec<Value>>,
    refs: Option<&[Value]>,
) -> RedactionCollateral {
    let wanted: Option<HashSet<String>> =
        refs.map(|refs| refs.iter().filter_map(annotation_key).collect());
    let mut hits = Vec::new();
    for &page in pages {
        let on_page = list(page);
        let marks: Vec<&Value> = on_page
            .iter()
            .filter(|record| {
                is_redact(record)
                    && wanted.as_ref().is_none_or(|wanted| {
                        record
                            .get("ref")
                            .and_then(annotation_key)
                            .is_some_and(|k| wanted.contains(&k))
                    })
            })
            .collect();
        if marks.is_empty() {
            continue;
        }
        let regions: Vec<Rect> = marks.iter().flat_map(|mark| regions_of(mark)).collect();
        for other in on_page.iter() {
            if is_redact(other) {
                continue;
            }
            let rect = rect_of(other.get("rect"));
            if regions.iter().any(|region| rects_overlap(*region, rect)) {
                hits.push(other.get("ref").cloned().unwrap_or(Value::Null));
            }
        }
    }
    RedactionCollateral { count: hits.len(), refs: hits }
}

type Marks = Rc<Vec<RedactionMark>>;

/// One page's marks and what they were built from.
struct PageMarks {
    records: Rc<Vec<Value>>,
    page_index: i64,
    marks: Marks,
}

/// The pending view's memos.
pub(crate) struct PendingMemo {
    empty: Marks,
    pages: RefCell<HashMap<u32, PageMarks>>,
    all: RefCell<Option<(Vec<Marks>, Marks)>>,
}

impl Default for PendingMemo {
    fn default() -> Self {
        PendingMemo {
            empty: Rc::new(Vec::new()),
            pages: RefCell::default(),
            all: RefCell::default(),
        }
    }
}

impl RedactionCapability {
    /// `annotation.list({ pages: [page] })`.
    pub(crate) fn list_page(&self, page: PageRef) -> Rc<Vec<Value>> {
        let filter = AnnotationFilter { pages: Some(vec![page.to_json()]), subtype: None };
        self.core().siblings.annotation.list(Some(&filter))
    }

    /// A record as the mark it is, `None` when it is not a `redact`
    /// annotation. A record the typed mark cannot hold goes to the kernel's
    /// report.
    fn mark_or_report(&self, record: &Value, page_index: i64) -> Option<RedactionMark> {
        if !is_redact(record) {
            return None;
        }
        match mark_of(record, page_index) {
            Ok(mark) => Some(mark),
            Err(error) => {
                self.core().cx.report_error(&error);
                None
            }
        }
    }

    /// `marksOn(pageObjectNumber)`: one page's marks, the same `Rc` while
    /// the plane's list and the page's position hold.
    fn marks_on(&self, page: PageRef) -> Marks {
        let records = self.list_page(page);
        let page_index = self.core().store.page_index_of(page);
        let memo = &self.core().pending;
        if let Some(held) = memo.pages.borrow().get(&page.page_object_number)
            && Rc::ptr_eq(&held.records, &records)
            && held.page_index == page_index
        {
            return held.marks.clone();
        }
        let marks: Vec<RedactionMark> =
            records.iter().filter_map(|r| self.mark_or_report(r, page_index)).collect();
        let marks = if marks.is_empty() { memo.empty.clone() } else { Rc::new(marks) };
        memo.pages.borrow_mut().insert(
            page.page_object_number,
            PageMarks { records, page_index, marks: marks.clone() },
        );
        marks
    }

    /// `allMarks()`: the whole document's marks, the same `Rc` while no
    /// page's marks changed.
    fn all_marks(&self) -> Marks {
        let per_page: Vec<Marks> =
            self.core().store.pages().into_iter().map(|page| self.marks_on(page)).collect();
        let memo = &self.core().pending;
        if let Some((held, marks)) = &*memo.all.borrow()
            && held.len() == per_page.len()
            && held.iter().zip(&per_page).all(|(a, b)| Rc::ptr_eq(a, b))
        {
            return marks.clone();
        }
        let marks = if per_page.iter().any(|marks| !marks.is_empty()) {
            Rc::new(per_page.iter().flat_map(|marks| marks.iter().cloned()).collect())
        } else {
            memo.empty.clone()
        };
        *memo.all.borrow_mut() = Some((per_page, marks.clone()));
        marks
    }

    /// Pending marks in page order, or one page's (none for a page that
    /// isn't there). Reference-stable while unchanged.
    pub fn list_pending(&self, filter: Option<RedactionMarkFilter>) -> Rc<Vec<RedactionMark>> {
        let Some(page) = filter.and_then(|f| f.page) else { return self.all_marks() };
        match self
            .core()
            .store
            .get_page(page)
            .and_then(|l| l.get("ref").and_then(PageRef::from_json))
        {
            Some(page) => self.marks_on(page),
            None => self.core().pending.empty.clone(),
        }
    }

    /// `markFor(record)`: a record the annotation plugin made or changed, as
    /// the mark it is.
    pub(crate) fn mark_for(&self, record: &Value) -> Option<RedactionMark> {
        let page = record.get("ref").and_then(|r| r.get("page")).and_then(PageRef::from_json);
        let index = page.map_or(-1, |page| self.core().store.page_index_of(page));
        self.mark_or_report(record, index)
    }

    pub fn get_pending(&self, reference: &Value) -> Option<RedactionMark> {
        let key = annotation_key(reference)?;
        self.list_pending(None)
            .iter()
            .find(|mark| annotation_key(&mark.ref_).as_deref() == Some(key.as_str()))
            .cloned()
    }

    /// How many marks wait to be applied, in the document or on one page.
    pub fn get_pending_count(&self, page: Option<PageTarget>) -> usize {
        self.list_pending(page.map(|page| RedactionMarkFilter { page: Some(page) })).len()
    }

    /// Which other annotations applying these marks (default: all) would
    /// destroy.
    pub fn estimate_collateral(&self, refs: Option<&[Value]>) -> RedactionCollateral {
        let pages = self.core().store.pages();
        estimate_collateral(&pages, &|page| self.list_page(page), refs)
    }
}
