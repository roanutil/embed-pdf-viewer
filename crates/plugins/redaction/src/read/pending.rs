//! `read/pending.ts`: `redact` annotations projected as marks per page,
//! plus the client-side collateral estimate.
//!
//! The TypeScript memoizes each page's marks on the identity of the
//! annotation plane's list for that page, and the whole list on the
//! identity of the per-page results. Nothing can observe that identity
//! through the vectors, and the Rust annotation plane hands out a fresh
//! `Vec` per read, so the port rebuilds the marks on every read.

use std::collections::HashSet;

use epdf_annotation_core::{AnnotationRef, PageRef};
use epdf_geometry::{PdfEdges, Point, Quad, edges_of_quad, edges_overlap};
use epdf_plugin_annotation::repository::seam::annotation_key_of;
use epdf_plugin_annotation::{AnnotationFilter, annotation_key};
use serde_json::Value;

use crate::contract::{RedactionCollateral, RedactionMark, RedactionMarkFilter, RedactionMarkKind};
use crate::controller::RedactionCapability;

fn f64_of(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// A `PdfRect` read as the TypeScript reads its members: a missing or
/// non-numeric member is `NaN` to every comparison.
fn edges_of(v: &Value) -> PdfEdges {
    PdfEdges {
        left: f64_of(v, "left"),
        bottom: f64_of(v, "bottom"),
        right: f64_of(v, "right"),
        top: f64_of(v, "top"),
    }
}

fn point_of(q: &Value, key: &str) -> Point {
    let p = q.get(key).unwrap_or(&Value::Null);
    Point { x: f64_of(p, "x"), y: f64_of(p, "y") }
}

fn quad_points(dto: &Value) -> &[Value] {
    dto.get("quadPoints").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn is_redact(dto: &Value) -> bool {
    dto.get("subtype").and_then(Value::as_str) == Some("redact")
}

/// `annotationKey(dto.ref)` for a DTO still in its wire form.
fn key_of_dto(dto: &Value) -> String {
    dto.get("ref").and_then(annotation_key_of).unwrap_or_default()
}

/// `regionsOf(dto)`: the PDF-space regions a redact mark targets, its
/// quads' boxes, else `/Rect`.
pub fn regions_of(dto: &Value) -> Vec<PdfEdges> {
    let quads = quad_points(dto);
    if quads.is_empty() {
        return vec![edges_of(dto.get("rect").unwrap_or(&Value::Null))];
    }
    quads
        .iter()
        .map(|q| {
            edges_of_quad(Quad {
                p1: point_of(q, "p1"),
                p2: point_of(q, "p2"),
                p3: point_of(q, "p3"),
                p4: point_of(q, "p4"),
            })
        })
        .collect()
}

/// `estimateCollateral(refs)` over the document's pages and the annotation
/// plane's `listRaw({page})`: which non-redact annotations the given marks
/// (by annotation key; `None` for all) would take with them.
pub fn estimate_collateral(
    pages: &[PageRef],
    list_raw: &dyn Fn(&PageRef) -> Vec<Value>,
    wanted: Option<&HashSet<String>>,
) -> RedactionCollateral {
    let mut hits = Vec::new();
    for page in pages {
        let all = list_raw(page);
        let marks: Vec<&Value> = all
            .iter()
            .filter(|a| is_redact(a) && wanted.is_none_or(|w| w.contains(&key_of_dto(a))))
            .collect();
        if marks.is_empty() {
            continue;
        }
        let regions: Vec<PdfEdges> = marks.iter().flat_map(|m| regions_of(m)).collect();
        for other in &all {
            if is_redact(other) {
                continue;
            }
            let rect = edges_of(other.get("rect").unwrap_or(&Value::Null));
            if regions.iter().any(|r| edges_overlap(*r, rect)) {
                hits.push(other.get("ref").cloned().unwrap_or(Value::Null));
            }
        }
    }
    RedactionCollateral { count: hits.len(), refs: hits }
}

impl RedactionCapability {
    /// `marksOn(page)`: the page's marks, from the annotation plane.
    fn marks_on(&self, page: &PageRef) -> Vec<RedactionMark> {
        let annotation = self.annotation();
        let filter = AnnotationFilter {
            page: Some(*page),
            subtype: Some("redact".into()),
            ..AnnotationFilter::default()
        };
        annotation
            .list(Some(&filter))
            .into_iter()
            .filter_map(|record| {
                let raw = annotation.get_raw(&record.ref_).filter(is_redact)?;
                Some(RedactionMark {
                    page: *page,
                    page_index: self.page_index_of(page),
                    kind: if quad_points(&raw).is_empty() {
                        RedactionMarkKind::Area
                    } else {
                        RedactionMarkKind::Text
                    },
                    bounds: record.bounds,
                    overlay_text: raw.get("overlayText").cloned(),
                    ref_: record.ref_,
                })
            })
            .collect()
    }

    /// Pending marks in page order.
    pub fn list_pending(&self, filter: Option<&RedactionMarkFilter>) -> Vec<RedactionMark> {
        if let Some(page) = filter.and_then(|f| f.page.as_ref()) {
            return self.marks_on(page);
        }
        self.pages().iter().flat_map(|page| self.marks_on(page)).collect()
    }

    pub fn get_pending(&self, r: &AnnotationRef) -> Option<RedactionMark> {
        let key = annotation_key(r);
        self.list_pending(None).into_iter().find(|mark| annotation_key(&mark.ref_) == key)
    }

    pub fn get_pending_count(&self, page: Option<&PageRef>) -> usize {
        let filter = page.map(|page| RedactionMarkFilter { page: Some(*page) });
        self.list_pending(filter.as_ref()).len()
    }

    /// Which other annotations applying these marks (default: all) would
    /// destroy.
    pub fn estimate_collateral(&self, refs: Option<&[AnnotationRef]>) -> RedactionCollateral {
        let wanted: Option<HashSet<String>> =
            refs.map(|refs| refs.iter().map(annotation_key).collect());
        let annotation = self.annotation();
        let list_raw = |page: &PageRef| {
            annotation.list_raw(Some(&AnnotationFilter {
                page: Some(*page),
                ..AnnotationFilter::default()
            }))
        };
        estimate_collateral(&self.pages(), &list_raw, wanted.as_ref())
    }
}
